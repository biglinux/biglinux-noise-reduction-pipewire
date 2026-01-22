import pytest
from biglinux_microphone.audio.filter_chain import (
    FilterChainConfig,
    FilterChainGenerator,
)
from biglinux_microphone.config import StereoMode


class TestFilterChainEffects:
    def test_voice_changer_mode_generation(self):
        # 1. Normal (0.5 width -> 1.0 pitch)
        config = FilterChainConfig(
            stereo_mode=StereoMode.VOICE_CHANGER,
            stereo_width=0.5,
            noise_reduction_enabled=False,
        )
        generator = FilterChainGenerator(config)
        output = generator.generate()

        assert "pitchScale" in output
        assert "pitch_scale" in output
        assert "1.00" in output
        assert "Voice Change" in output

        # 2. Squirrel (1.0 width -> 2.0 pitch)
        c2 = FilterChainConfig(
            stereo_mode=StereoMode.VOICE_CHANGER,
            stereo_width=1.0,
            noise_reduction_enabled=False,
        )
        gen2 = FilterChainGenerator(c2)
        out2 = gen2.generate()
        assert "2.00" in out2

        # 3. Deep (0.0 width -> 0.5 pitch)
        c3 = FilterChainConfig(
            stereo_mode=StereoMode.VOICE_CHANGER,
            stereo_width=0.0,
            noise_reduction_enabled=False,
        )
        gen3 = FilterChainGenerator(c3)
        out3 = gen3.generate()
        assert "0.50" in out3
        # Check for gain compensation (Deep voice -> gain boost)
        assert "amp" in out3
        # 0.5 pitch -> 15dB gain (5.0 base + 10.0 boost)
        assert "15.00" in out3

    def test_radio_mode_generation(self):
        config = FilterChainConfig(
            stereo_mode=StereoMode.RADIO,
            stereo_width=0.5,
            noise_reduction_enabled=False,
        )
        generator = FilterChainGenerator(config)
        output = generator.generate()

        assert "sc4m" in output
        assert "Ratio (1:n)" in output
        # Radio params check
        # Ratio: 4.0 + (0.5 * 6.0) = 7.0
        assert "7.0" in output
