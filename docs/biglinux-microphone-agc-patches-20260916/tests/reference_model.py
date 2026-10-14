#!/usr/bin/env python3
"""Independent Python numerical model of the proposed *software-only* controller.

This does NOT execute Rust, PipeWire, audio VAD, model inference or hardware ACKs.
Its purpose is falsification of the level logic and reproducible control scenarios.
All inputs are synthetic sample-peak sequences, not an acoustic speech corpus.
"""
from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import math
import struct


def f32(x: float) -> float:
    return struct.unpack("f", struct.pack("f", x))[0]


def clamp(x: float, low: float, high: float) -> float:
    return min(max(x, low), high)


def volume_db(v: float) -> float:
    return 60 * math.log10(max(f32(v), f32(1e-4)))


def knob(db: float) -> float:
    return clamp(f32(10 ** (db / 60)), 0, 3)


def changed(a: float, b: float) -> bool:
    return abs(f32(a) - f32(b)) > 0.005


MAX_DB = 60 * math.log10(3)
MIN_DB = -40.0
DT = 0.05


@dataclass
class Features:
    speech: float
    noise: float
    p: float = 0.0
    snr: float = 0.0
    muted: bool = False
    discontinuity: bool = False


class Extractor:
    def __init__(self) -> None:
        self.clock = 0.0
        self.floor: float | None = None
        self.background = 0.0
        self.primary: float | None = None
        self.evidence: deque[tuple[float, float, float]] = deque()
        self.flat_level: float | None = None
        self.flat_time = 0.0

    def interrupt(self) -> None:
        self.floor = None
        self.background = 0
        self.evidence.clear()
        self.flat_level = None
        self.flat_time = 0

    def rebase(self, delta: float) -> None:
        for name in ("floor", "primary", "flat_level"):
            old = getattr(self, name)
            if old is not None:
                setattr(self, name, old + delta)
        self.evidence = deque((t, x + delta, w) for t, x, w in self.evidence)

    def empty(self, muted: bool = False, discontinuity: bool = False) -> Features:
        floor = self.floor if self.floor is not None else -120.0
        return Features(self.primary if self.primary is not None else floor,
                        floor, muted=muted, discontinuity=discontinuity)

    def observe(self, measured: float, gain: float = 0.0, dt: float = DT) -> Features:
        if not all(map(math.isfinite, (measured, gain, dt))) or not 0 < dt <= 0.1:
            self.interrupt()
            return self.empty(discontinuity=True)
        self.clock += dt
        while self.evidence and self.clock - self.evidence[0][0] > 2.4:
            self.evidence.popleft()
        if measured <= -120:
            bound = -120 - gain
            self.floor = bound if self.floor is None else max(self.floor, bound)
            self.background = min(self.background + dt, 0.5)
            self.flat_level, self.flat_time = None, 0
            return self.empty()
        peak = measured - gain
        if not math.isfinite(peak):
            self.interrupt()
            return self.empty(discontinuity=True)
        while self.evidence and self.clock - self.evidence[0][0] > 2.4:
            self.evidence.popleft()
        floor = self.floor if self.floor is not None else peak
        if peak < floor:
            floor += (peak - floor) * (1 - math.exp(-dt / 0.15))
        contrast = peak - floor
        if contrast < 12:
            self.background = min(self.background + dt, 0.5)
        if self.flat_level is not None and abs(peak - self.flat_level) <= 1:
            self.flat_time += dt
        else:
            self.flat_level, self.flat_time = peak, dt
        if peak > floor and (contrast < 12 or self.flat_time >= 2.4):
            floor += (peak - floor) * (1 - math.exp(-dt / 10))
        self.floor = floor
        contrast = peak - floor
        eligible = (self.background >= 0.5 - 1e-9
                    and contrast >= (12 if self.primary is not None else 18)
                    and (self.primary is None or peak >= self.primary - 10))
        if eligible:
            if len(self.evidence) == 256:
                self.evidence.popleft()
            self.evidence.append((self.clock, peak, dt))
        values = sorted((x, w) for _, x, w in self.evidence)
        duration = sum(w for _, w in values)

        def quantile(fraction: float) -> float | None:
            total = 0.0
            for x, weight in values:
                total += weight
                if total >= duration * fraction:
                    return x
            return values[-1][0] if values else None

        low, high = quantile(0.2), quantile(0.8)
        shaped = (duration >= 1.2 - 1e-9 and low is not None
                  and high is not None and high - low >= 3)
        trusted = eligible and shaped
        if trusted:
            self.primary = high if self.primary is None else max(self.primary, high)
        return Features(self.primary if self.primary is not None else peak,
                        floor, float(trusted), contrast)


class Policy:
    def __init__(self, target: float = -12.0, minimum: float = -40,
                 maximum: float = MAX_DB, hold: float = 0.1) -> None:
        self.target, self.minimum, self.maximum = target, minimum, maximum
        self.hold = hold
        self.gain = self.time = self.speech_time = 0.0
        self.manual = False
        self.settle_until = 0.0

    def synchronize(self, gain: float) -> None:
        if not math.isfinite(gain) or not self.minimum - 0.01 <= gain <= self.maximum + 0.01:
            raise ValueError("invalid external gain")
        self.gain = clamp(gain, self.minimum, self.maximum)

    def update(self, f: Features, dt: float = DT, peak: float | None = None,
               ceiling: float = -3.0) -> str:
        if not math.isfinite(dt) or not 0 < dt <= 0.1:
            raise ValueError("invalid dt")
        if not all(map(math.isfinite, (f.speech, f.noise, f.p, f.snr))):
            raise ValueError("invalid features")
        self.time += dt
        if f.muted or f.discontinuity:
            self.speech_time = 0
            self.settle_until = self.time + 0.5
            return "muted" if f.muted else "settling"
        if self.manual:
            return "manual_hold"
        if self.time < self.settle_until:
            return "settling"
        trusted = f.p >= 0.8 and f.snr >= 10
        self.speech_time = self.speech_time + dt if trusted else 0
        if peak is not None:
            desired = clamp(ceiling - peak, self.minimum, self.maximum)
            excess = self.gain - desired
            if excess > 0:
                u = clamp(excess / 15, 0, 1)
                rate = 0.6 + (14 - 0.6) * u * u
                self.gain -= min(rate * dt, excess)
                return "peak_safety"
        if not trusted or self.speech_time < self.hold:
            return "frozen_no_trusted_speech"
        requested = min(self.target - f.speech, -50 - f.noise)
        desired = clamp(requested, self.minimum, self.maximum)
        delta = desired - self.gain
        excess = abs(requested - self.gain) - 4
        if excess > 0 and delta != 0:
            lo, hi = (0.4, 8) if delta > 0 else (0.6, 14)
            rate = lo + (hi - lo) * clamp(excess / 15, 0, 1) ** 2
            self.gain += math.copysign(min(rate * dt, abs(delta)), delta)
        return "tracking_speech"


class Controller:
    def __init__(self, volume: float = 1.0, target: float = -12.0) -> None:
        if not math.isfinite(volume) or not 0 <= volume <= 3:
            raise ValueError("invalid volume")
        self.volume = f32(volume)
        self.base = self.applied = volume_db(volume)
        self.target = target
        self.fx = Extractor()
        self.policy = Policy(target, min(MIN_DB - self.base, 0), max(MAX_DB - self.base, 0))
        self.faulted = False
        self.last_request: float | None = None

    def manual_change(self, volume: float) -> None:
        new_base = volume_db(volume)
        self.fx.rebase(new_base - self.base)
        self.base = self.applied = new_base
        self.volume = f32(volume)
        self.policy = Policy(self.target, min(MIN_DB - self.base, 0), max(MAX_DB - self.base, 0))
        self.policy.manual = True
        self.last_request = None

    def fault(self) -> None:
        self.faulted = self.policy.manual = True
        self.fx.interrupt()

    def ack(self, requested: float, actual: float) -> None:
        requested, actual = f32(requested), f32(actual)
        if not all(map(math.isfinite, (requested, actual))) or not 0 < actual <= 3:
            self.fault()
            raise ValueError("invalid readback")
        db = volume_db(actual)
        if changed(actual, self.volume):
            try:
                self.policy.synchronize(db - self.base)
            except ValueError:
                self.fault()
                raise
        self.last_request, self.applied, self.volume = requested, db, actual

    def observe(self, peak: float, dt: float = DT, muted: bool = False) -> dict:
        applied_gain = self.applied - self.base
        if muted or self.volume == 0:
            self.fx.interrupt()
            f = self.fx.empty(muted=True)
        else:
            f = self.fx.observe(peak, applied_gain, dt)
        ceiling = min(self.target + 4, -3)
        if not f.muted and not f.discontinuity and peak > ceiling and self.policy.gain > applied_gain:
            self.policy.synchronize(applied_gain)
        state = self.policy.update(f, dt, peak - applied_gain if math.isfinite(peak) else None, ceiling)
        if self.faulted and not f.muted:
            state = "actuator_fault"
        lower = min(MIN_DB, self.base)
        target_db = clamp(self.base + self.policy.gain, lower, MAX_DB)
        at_limit = abs(target_db - lower) < 1e-8 or abs(target_db - MAX_DB) < 1e-8
        may_write = (not self.faulted and state in ("tracking_speech", "peak_safety")
                     and (abs(target_db - self.applied) >= 0.4 or at_limit))
        minimum_knob = f32(math.ceil(f32(knob(lower) * 100)) / 100)
        quantized = f32(clamp(f32(math.floor(f32(knob(target_db) * 100) + 0.5) / 100), minimum_knob, 3))
        new_request = self.last_request is None or changed(quantized, self.last_request)
        request = quantized if may_write and changed(quantized, self.volume) and new_request else None
        if at_limit and not new_request and changed(quantized, self.volume) and state == "tracking_speech":
            state = "actuator_limited"
        return dict(features=f, state=state, request=request, gain=self.policy.gain, target_db=target_db)


def voice(i: int, high: float) -> float:
    return high + (-6.0, -3.0, 0.0, -3.0)[(i // 3) % 4]


def tick(c: Controller, unity_peak: float, write: bool = True, coarse: float | None = None,
         cap: float = 3.0, measured_override: float | None = None) -> dict:
    measured = unity_peak + c.applied if measured_override is None else measured_override
    result = c.observe(measured)
    v = result["request"]
    if write and v is not None:
        actual = v
        if coarse:
            actual = math.floor(v / coarse + 0.5) * coarse
        c.ack(v, clamp(actual, 0.01, cap))
    return result


def background(c: Controller, floor: float, count: int = 40) -> None:
    for _ in range(count):
        tick(c, floor)
