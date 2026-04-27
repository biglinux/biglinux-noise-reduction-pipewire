"""Offline emulator of the BigLinux mic filter chain.

The biquad/EQ stages mirror the PipeWire builtin math exactly (RBJ
cookbook formulas, identical to `bq_highpass`/`bq_peaking`). The
GTCRN stage calls the same ONNX model the LADSPA plugin uses, so
denoiser scoring is the live model's behaviour. Compressor and gate
are functional approximations: useful for trend analysis, not for
matching SC4 sample-perfect.

Inputs/outputs: float32 mono PCM. Sample rate flows through unchanged.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from scipy.signal import sosfilt


# ── Biquad math (RBJ cookbook, matches PipeWire builtins) ────────────


def _hp_sos(freq_hz: float, q: float, sr: int) -> np.ndarray:
    """Single biquad high-pass. Returns SOS row [b0,b1,b2, a0,a1,a2]."""
    w0 = 2 * np.pi * freq_hz / sr
    cosw = np.cos(w0)
    alpha = np.sin(w0) / (2 * q)
    b0 = (1 + cosw) / 2
    b1 = -(1 + cosw)
    b2 = (1 + cosw) / 2
    a0 = 1 + alpha
    a1 = -2 * cosw
    a2 = 1 - alpha
    return np.array([b0, b1, b2, a0, a1, a2]) / a0


def _peaking_sos(freq_hz: float, q: float, gain_db: float, sr: int) -> np.ndarray:
    """Peaking EQ biquad. `gain_db` is the boost/cut at `freq_hz`."""
    a = 10 ** (gain_db / 40)
    w0 = 2 * np.pi * freq_hz / sr
    cosw = np.cos(w0)
    alpha = np.sin(w0) / (2 * q)
    b0 = 1 + alpha * a
    b1 = -2 * cosw
    b2 = 1 - alpha * a
    a0 = 1 + alpha / a
    a1 = -2 * cosw
    a2 = 1 - alpha / a
    return np.array([b0, b1, b2, a0, a1, a2]) / a0


def _apply_sos(x: np.ndarray, sos_rows: list[np.ndarray]) -> np.ndarray:
    if not sos_rows:
        return x
    sos = np.stack(sos_rows)
    # scipy expects (n_sections, 6) — already the right shape.
    return sosfilt(sos, x).astype(np.float32)


# ── Stage signatures ────────────────────────────────────────────────


def apply_hpf(x: np.ndarray, sr: int, freq_hz: float, cascaded: bool = True) -> np.ndarray:
    """High-pass cascade. `cascaded=True` matches the production chain
    (Linkwitz-Riley 4th order, two Q=0.707 biquads). `cascaded=False`
    is a single-biquad legacy mode for A/B comparison."""
    sections = [_hp_sos(freq_hz, 0.707, sr)]
    if cascaded:
        sections.append(_hp_sos(freq_hz, 0.707, sr))
    return _apply_sos(x, sections)


# `EQ_BANDS_HZ` mirrors `src/config/paths.rs` so config and emulator
# stay aligned without an extra build step. Update both together.
EQ_BANDS_HZ: tuple[int, ...] = (31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)


def apply_eq(x: np.ndarray, sr: int, gains_db: list[float], q: float = 1.41) -> np.ndarray:
    """Ten cascaded `bq_peaking` biquads at `EQ_BANDS_HZ`. `q=1.41`
    matches the production param_eq node."""
    if len(gains_db) != len(EQ_BANDS_HZ):
        raise ValueError(f"gains length {len(gains_db)} != {len(EQ_BANDS_HZ)}")
    sections = []
    for freq, gain in zip(EQ_BANDS_HZ, gains_db):
        if abs(gain) < 1e-3:
            continue  # 0 dB peaking is a no-op; skip for speed
        sections.append(_peaking_sos(float(freq), q, float(gain), sr))
    return _apply_sos(x, sections)


# ── Compressor (SC4-equivalent feed-forward, RMS detection) ──────────


def apply_compressor(
    x: np.ndarray,
    sr: int,
    threshold_db: float,
    ratio: float,
    attack_ms: float,
    release_ms: float,
    makeup_gain_db: float,
    knee_db: float = 6.0,
) -> np.ndarray:
    """Soft-knee feed-forward compressor. Approximation of SC4 mono."""
    eps = 1e-9
    att = float(np.exp(-1.0 / (sr * attack_ms / 1000.0 + eps)))
    rel = float(np.exp(-1.0 / (sr * release_ms / 1000.0 + eps)))
    env = 0.0
    out = np.empty_like(x)
    makeup = 10 ** (makeup_gain_db / 20)
    for i, s in enumerate(x):
        # RMS-style detector
        sq = s * s
        coeff = att if sq > env else rel
        env = coeff * env + (1 - coeff) * sq
        rms_db = 10 * np.log10(env + eps)
        # Soft-knee gain reduction
        over = rms_db - threshold_db
        if over <= -knee_db / 2:
            gr_db = 0.0
        elif over >= knee_db / 2:
            gr_db = over * (1.0 / ratio - 1.0)
        else:
            x_knee = over + knee_db / 2
            gr_db = (1.0 / ratio - 1.0) * x_knee * x_knee / (2 * knee_db)
        out[i] = s * (10 ** (gr_db / 20)) * makeup
    return out.astype(np.float32)


# ── GTCRN ONNX wrapper ───────────────────────────────────────────────

# GTCRN works on 16 kHz mono with a 512-point STFT, hop 256, 257 bins.
# These constants are pinned to match the LADSPA wrapper's framing.
GTCRN_SR = 16000
GTCRN_NFFT = 512
GTCRN_HOP = 256
GTCRN_BINS = GTCRN_NFFT // 2 + 1


@dataclass
class GtcrnSession:
    """Lazy-init ONNX session. Pass `model_path=None` for passthrough."""

    model_path: Path | None
    _session: object | None = field(default=None, init=False, repr=False)

    def ensure_loaded(self) -> object | None:
        if self.model_path is None:
            return None
        if self._session is None:
            import onnxruntime as ort

            self._session = ort.InferenceSession(
                str(self.model_path),
                providers=["CPUExecutionProvider"],
            )
        return self._session


def apply_gtcrn(
    x: np.ndarray,
    sr: int,
    session: GtcrnSession,
    strength: float = 1.0,
) -> np.ndarray:
    """Run GTCRN denoiser on `x`. `strength` blends dry/wet (1.0=full).

    The shipped ONNX is the **streaming** GTCRN: it processes one STFT
    frame per call and carries three recurrent caches (`conv_cache`,
    `tra_cache`, `inter_cache`) between calls. Cache shapes match the
    LADSPA wrapper at `ladspa/src/model.rs`."""
    sess = session.ensure_loaded()
    if sess is None:
        return x.astype(np.float32)
    if sr != GTCRN_SR:
        from scipy.signal import resample_poly

        x16 = resample_poly(x, GTCRN_SR, sr).astype(np.float32)
    else:
        x16 = x.astype(np.float32)

    win = np.hanning(GTCRN_NFFT).astype(np.float32)
    n_frames = 1 + max(0, (len(x16) - GTCRN_NFFT) // GTCRN_HOP)
    if n_frames <= 0:
        return x.astype(np.float32)

    # Initial caches — zero state, shapes mirror LADSPA `model.rs`.
    conv_cache = np.zeros((2, 1, 16, 16, 33), dtype=np.float32)
    tra_cache = np.zeros((2, 3, 1, 1, 16), dtype=np.float32)
    inter_cache = np.zeros((2, 1, 33, 16), dtype=np.float32)

    # Resolve input names once. Order in the model file: mix, conv,
    # tra, inter — but we look them up to be defensive.
    in_names = [i.name for i in sess.get_inputs()]
    name_mix = next(n for n in in_names if "mix" in n.lower() or n == in_names[0])
    name_conv = next(n for n in in_names if "conv" in n.lower())
    name_tra = next(n for n in in_names if "tra" in n.lower())
    name_inter = next(n for n in in_names if "inter" in n.lower())

    enhanced = np.zeros_like(x16)
    norm = np.zeros_like(x16) + 1e-9

    for f in range(n_frames):
        seg = x16[f * GTCRN_HOP : f * GTCRN_HOP + GTCRN_NFFT] * win
        S = np.fft.rfft(seg, n=GTCRN_NFFT).astype(np.complex64)
        spec_in = np.zeros((1, GTCRN_BINS, 1, 2), dtype=np.float32)
        spec_in[0, :, 0, 0] = S.real
        spec_in[0, :, 0, 1] = S.imag

        outputs = sess.run(
            None,
            {
                name_mix: spec_in,
                name_conv: conv_cache,
                name_tra: tra_cache,
                name_inter: inter_cache,
            },
        )
        # Output 0 is the enhanced spectrum; 1..3 are updated caches.
        enh = outputs[0]
        conv_cache = outputs[1]
        tra_cache = outputs[2]
        inter_cache = outputs[3]

        S_enh = enh[0, :, 0, 0] + 1j * enh[0, :, 0, 1]
        time = np.fft.irfft(S_enh, n=GTCRN_NFFT).astype(np.float32) * win
        enhanced[f * GTCRN_HOP : f * GTCRN_HOP + GTCRN_NFFT] += time
        norm[f * GTCRN_HOP : f * GTCRN_HOP + GTCRN_NFFT] += win * win

    enhanced = enhanced / norm

    if sr != GTCRN_SR:
        from scipy.signal import resample_poly

        enhanced = resample_poly(enhanced, sr, GTCRN_SR).astype(np.float32)
    n = min(len(enhanced), len(x))
    enhanced = enhanced[:n]
    dry = x[:n].astype(np.float32)
    wet = float(np.clip(strength, 0.0, 1.0))
    return ((1.0 - wet) * dry + wet * enhanced).astype(np.float32)


# ── Full chain composition ──────────────────────────────────────────


@dataclass
class ChainSettings:
    """Settings snapshot for one offline run. Mirrors the prod struct
    fields we actually exercise in calibration sweeps."""

    hpf_enabled: bool = False
    hpf_freq_hz: float = 80.0
    hpf_cascaded: bool = True

    gtcrn_enabled: bool = True
    gtcrn_strength: float = 1.0
    gtcrn_model: Path | None = None

    eq_enabled: bool = False
    eq_gains_db: tuple[float, ...] = (0.0,) * len(EQ_BANDS_HZ)

    compressor_enabled: bool = False
    compressor_threshold_db: float = -18.0
    compressor_ratio: float = 3.0
    compressor_attack_ms: float = 10.0
    compressor_release_ms: float = 100.0
    compressor_makeup_db: float = 6.0


def apply_chain(x: np.ndarray, sr: int, s: ChainSettings) -> np.ndarray:
    """Run `x` through the full chain in production order: HPF → GTCRN
    → compressor → EQ. Mirrors `src/pipeline/mic.rs`."""
    y = x
    if s.hpf_enabled:
        y = apply_hpf(y, sr, s.hpf_freq_hz, cascaded=s.hpf_cascaded)
    if s.gtcrn_enabled and s.gtcrn_model is not None:
        sess = GtcrnSession(s.gtcrn_model)
        y = apply_gtcrn(y, sr, sess, strength=s.gtcrn_strength)
    if s.compressor_enabled:
        y = apply_compressor(
            y,
            sr,
            s.compressor_threshold_db,
            s.compressor_ratio,
            s.compressor_attack_ms,
            s.compressor_release_ms,
            s.compressor_makeup_db,
        )
    if s.eq_enabled:
        y = apply_eq(y, sr, list(s.eq_gains_db))
    return y.astype(np.float32)
