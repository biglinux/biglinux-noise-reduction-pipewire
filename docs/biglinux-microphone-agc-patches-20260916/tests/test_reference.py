#!/usr/bin/env python3
"""Numerical falsification suite. This is not a substitute for the Rust tests."""
import math
import unittest
from collections import deque
from reference_model import Controller, Extractor, Features, Policy, DT, MAX_DB, background, tick, voice, volume_db


class ExtractorTests(unittest.TestCase):
    def calibrated(self, high=-35):
        fx = Extractor()
        for _ in range(40): fx.observe(-95)
        for i in range(100): f = fx.observe(voice(i, high))
        self.assertEqual(f.p, 1)
        return fx

    def test_single_transient_does_not_enroll(self):
        fx = Extractor()
        for _ in range(40): fx.observe(-90)
        self.assertEqual(fx.observe(-20).p, 0)
        self.assertIsNone(fx.primary)

    def test_modulated_sustained_input_enrolls(self):
        self.assertEqual(self.calibrated().primary, -35)

    def test_ten_minutes_far_speech_cannot_replace_anchor(self):
        fx = self.calibrated(-25)
        for i in range(12000):
            self.assertEqual(fx.observe(voice(i, -50)).p, 0)
        self.assertEqual(fx.primary, -25)

    def test_constant_noise_and_level_step_do_not_enroll(self):
        fx = Extractor()
        for level, n in [(-80, 200), (-35, 12000)]:
            for _ in range(n): self.assertEqual(fx.observe(level).p, 0)
        self.assertIsNone(fx.primary)

    def test_small_stationary_jitter_does_not_enroll(self):
        fx = Extractor()
        for _ in range(40): fx.observe(-90)
        for i in range(4000): self.assertEqual(fx.observe(-45 + (-0.7, 0.3, 0.7, -0.2)[i % 4]).p, 0)

    def test_pause_is_not_fresh_speech(self):
        fx = self.calibrated()
        gap = fx.observe(-95)
        self.assertEqual(gap.p, 0)
        self.assertLess(gap.snr, 1)
        self.assertEqual(fx.primary, -35)

    def test_sensitivity_offset_invariance(self):
        for offset in (-20, 0, 20):
            fx = Extractor()
            for _ in range(40): fx.observe(-80 + offset)
            for i in range(100): f = fx.observe(voice(i, -40 + offset))
            self.assertEqual(f.p, 1)
            self.assertAlmostEqual(f.speech, -40 + offset)

    def test_gain_changes_are_normalized_before_history(self):
        a, b = Extractor(), Extractor()
        for i in range(600):
            gain = 0 if i < 150 else 28 if i < 350 else -5
            x = -90 if i < 40 or i % 80 > 65 else voice(i, -40)
            fa, fb = a.observe(x), b.observe(x + gain, gain)
            self.assertEqual(fa.p, fb.p)
            self.assertAlmostEqual(fa.noise, fb.noise)
            self.assertAlmostEqual(fa.speech, fb.speech)

    def test_censored_noise_is_finite_and_zero_is_not_speech(self):
        fx = Extractor()
        for _ in range(400):
            f = fx.observe(-120)
            self.assertFalse(f.muted)
            self.assertEqual(f.p, 0)
            self.assertEqual(f.noise, -120)
        self.assertIsNone(fx.primary)
        for i in range(100): f = fx.observe(voice(i, -65))
        self.assertEqual(f.p, 1)

    def test_normalized_valid_signal_can_be_below_minus_120(self):
        f = Extractor().observe(-110, 28)
        self.assertFalse(f.muted)
        self.assertEqual(f.noise, -138)

    def test_interrupt_rebase_and_far_speech(self):
        fx = self.calibrated()
        fx.interrupt()
        fx.rebase(8)
        self.assertEqual(fx.primary, -27)
        for _ in range(40): fx.observe(-87)
        for i in range(1200): self.assertEqual(fx.observe(voice(i, -52)).p, 0)

    def test_invalid_values_are_fail_closed(self):
        fx = Extractor()
        for p, gain, dt in [(math.nan, 0, DT), (-40, math.inf, DT), (-40, 0, 5), (-40, 0, 0), (-40, 0, math.nan)]:
            f = fx.observe(p, gain, dt)
            self.assertTrue(f.discontinuity)
            self.assertEqual(f.p, 0)

    def test_history_is_bounded(self):
        fx = Extractor()
        for _ in range(40): fx.observe(-90)
        for i in range(20000): fx.observe(voice(i, -35), 0, 0.001)
        self.assertLessEqual(len(fx.evidence), 256)

    def test_known_cold_start_identifiability_limit(self):
        owner, television = self.calibrated(-35), self.calibrated(-35)
        self.assertEqual(owner.primary, television.primary)
        for i in range(200):
            self.assertEqual(owner.observe(voice(i, -35)), television.observe(voice(i, -35)))


class PolicyTests(unittest.TestCase):
    def test_physical_limit_not_deadband_below_limit(self):
        p = Policy()
        f = Features(-55, -100, 1, 45)
        for _ in range(4000): p.update(f)
        self.assertAlmostEqual(p.gain, MAX_DB)

    def test_fast_untrusted_cut_is_rate_bounded(self):
        p, previous = Policy(minimum=-50), 0.0
        for _ in range(40):
            p.update(Features(-55, -100), peak=30)
            self.assertLessEqual(p.gain, previous)
            self.assertLessEqual(previous - p.gain, 14 * DT + 1e-8)
            previous = p.gain
        self.assertLess(previous, -20)

    def test_manual_and_mute_override_peak_guard(self):
        p = Policy()
        p.manual = True
        self.assertEqual(p.update(Features(-55, -100), peak=30), "manual_hold")
        self.assertEqual(p.gain, 0)
        self.assertEqual(p.update(Features(-55, -100, muted=True), peak=30), "muted")
        self.assertEqual(p.gain, 0)

    def test_no_speech_no_upward_gain(self):
        p = Policy()
        for _ in range(4000): p.update(Features(-55, -100), peak=-55)
        self.assertEqual(p.gain, 0)

    def test_noise_ceiling_still_limits_desired_gain(self):
        p = Policy()
        for _ in range(4000): p.update(Features(-35, -40, 1, 20))
        self.assertLessEqual(p.gain, -5.9)

    def test_positive_and_negative_slew_bounds(self):
        for level in (-80, 20):
            p = Policy(minimum=-60, maximum=60)
            for _ in range(1000):
                old = p.gain
                p.update(Features(level, -110, 1, 80))
                self.assertLessEqual(p.gain - old, 8 * DT + 1e-8)
                self.assertLessEqual(old - p.gain, 14 * DT + 1e-8)


class ClosedLoopTests(unittest.TestCase):
    def calibrated(self, high=-35, count=2000, volume=1.0):
        c = Controller(volume)
        background(c, -100)
        for i in range(count): tick(c, voice(i, high))
        return c

    def test_quiet_mic_target_zone(self):
        c = self.calibrated(-32, 3000)
        self.assertGreaterEqual(-32 + c.applied, -16.6)
        self.assertLessEqual(-32 + c.applied, -7.4)
        self.assertGreater(c.volume, 1.5)

    def test_weak_mic_reaches_300_percent(self):
        self.assertEqual(self.calibrated(-48, 4000).volume, 3)

    def test_ten_minutes_distant_only_gain_is_frozen(self):
        c = self.calibrated()
        before = c.volume
        for i in range(12000):
            result = tick(c, voice(i, -60))
            self.assertEqual(result["features"].p, 0)
            self.assertIsNone(result["request"])
        self.assertEqual(c.volume, before)

    def test_no_startup_rescue_on_silence_noise_or_zero(self):
        for v in (0, 0.1, 0.3, 1, 3):
            c = Controller(v)
            for _ in range(400): self.assertIsNone(c.observe(-70)["request"])
            self.assertAlmostEqual(c.volume, v, places=6)

    def test_mute_never_writes(self):
        c = Controller()
        for i in range(400):
            result = c.observe(voice(i, 10), muted=True)
            self.assertEqual(result["state"], "muted")
            self.assertIsNone(result["request"])

    def test_fast_reduce_from_300_percent(self):
        c = Controller(3)
        before = c.applied
        for _ in range(40): tick(c, 5)
        self.assertGreater(before - c.applied, 20)

    def test_manual_rebaseline_zeroes_pending_correction(self):
        c = self.calibrated(-40, 500)
        self.assertGreater(c.policy.gain, 5)
        c.manual_change(0.7)
        self.assertEqual(c.policy.gain, 0)
        c.fx.interrupt()
        for i in range(1200):
            result = c.observe(voice(i, 20))
            self.assertEqual(result["state"], "manual_hold")
            self.assertIsNone(result["request"])

    def test_fault_is_not_rearmed_by_speech(self):
        c = Controller()
        c.fault()
        for i in range(400):
            result = c.observe(voice(i, 30))
            self.assertEqual(result["state"], "actuator_fault")
            self.assertIsNone(result["request"])

    def test_coarse_hardware_does_not_stall(self):
        c = Controller()
        background(c, -100)
        for i in range(4000): tick(c, voice(i, -48), coarse=0.1)
        self.assertEqual(c.volume, 3)

    def test_backend_clamp_does_not_cause_endless_identical_writes(self):
        c = Controller()
        background(c, -100)
        late = 0
        for i in range(4000):
            result = tick(c, voice(i, -48), cap=1.5)
            late += int(i > 3000 and result["request"] is not None)
        self.assertEqual(late, 0)
        self.assertEqual(c.volume, 1.5)

    def test_coalesced_writes_preserve_300_percent(self):
        c = Controller()
        background(c, -100)
        for i in range(4000): tick(c, voice(i, -48), write=i % 4 == 0)
        self.assertEqual(c.volume, 3)

    def test_pending_gain_does_not_write_in_a_pause(self):
        c = Controller()
        background(c, -100)
        for i in range(50): tick(c, voice(i, -45), write=False)
        self.assertGreater(c.policy.gain, 0)
        for _ in range(100): self.assertIsNone(tick(c, -100)["request"])
        self.assertEqual(c.volume, 1)

    def test_quiet_start_censored_noise_and_natural_word_gaps(self):
        c = Controller(0.3)
        background(c, -95)
        self.assertAlmostEqual(c.volume, 0.3, places=6)
        for i in range(4000):
            tick(c, -95 if i % 80 > 65 else voice(i, -32))
        self.assertGreater(c.volume, 1.5)
        self.assertGreaterEqual(-32 + c.applied, -16.6)
        self.assertLessEqual(-32 + c.applied, -7.4)

    def test_gain_delay_stress_no_background_takeover(self):
        # A 150 ms gain-readback/audio mismatch stress case. This is not a claim
        # of known hardware latency; real capture timestamps are not exposed.
        c = Controller()
        background(c, -100)
        gains = deque([0.0] * 4, maxlen=4)
        for i in range(4000):
            old = gains[0]
            tick(c, voice(i, -40), write=i % 4 == 0,
                 measured_override=voice(i, -40) + old)
            gains.append(c.applied)
        before = c.volume
        for i in range(12000):
            tick(c, voice(i, -65), measured_override=voice(i, -65) + gains[0])
            gains.append(c.applied)
        self.assertEqual(c.volume, before)
        self.assertTrue(math.isfinite(c.applied))


if __name__ == "__main__":
    unittest.main(verbosity=2)
