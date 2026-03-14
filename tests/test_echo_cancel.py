"""Tests for echo cancellation integration."""

from biglinux_microphone.audio.filter_chain import (
    FilterChainConfig,
    FilterChainGenerator,
)
from biglinux_microphone.config import (
    AppSettings,
    EchoCancelConfig,
    StereoMode,
)


class TestEchoCancelConfig:
    def test_defaults(self):
        ec = EchoCancelConfig()
        assert ec.enabled is True

    def test_settings_serialization(self):
        settings = AppSettings()
        settings.echo_cancel = EchoCancelConfig(enabled=True)
        data = settings.to_dict()
        assert data["echo_cancel"]["enabled"] is True

    def test_settings_deserialization(self):
        data = {"echo_cancel": {"enabled": True}}
        settings = AppSettings.from_dict(data)
        assert settings.echo_cancel.enabled is True

    def test_settings_deserialization_missing(self):
        settings = AppSettings.from_dict({})
        assert settings.echo_cancel.enabled is True


class TestEchoCancelFilterChain:
    def test_disabled_no_aec_module(self):
        config = FilterChainConfig(echo_cancel_enabled=False)
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "libpipewire-module-echo-cancel" not in output
        assert "big-aec-source" not in output

    def test_enabled_mono_has_aec_module(self):
        config = FilterChainConfig(echo_cancel_enabled=True, source_node_name="test-mic")
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "libpipewire-module-echo-cancel" in output
        assert "aec/libspa-aec-webrtc" in output
        assert "monitor.mode = true" in output
        assert "big-aec-source" in output
        assert 'target.object = "big-aec-source"' in output
        assert 'target.object = "test-mic"' in output

    def test_enabled_passthrough_has_aec(self):
        config = FilterChainConfig(
            echo_cancel_enabled=True,
            source_node_name="test-mic",
            noise_reduction_enabled=False,
            hpf_enabled=False,
            compressor_enabled=False,
            gate_enabled=False,
            eq_enabled=False,
        )
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "libpipewire-module-echo-cancel" in output
        assert 'target.object = "big-aec-source"' in output

    def test_enabled_stereo_has_aec(self):
        config = FilterChainConfig(
            echo_cancel_enabled=True,
            source_node_name="test-mic",
            stereo_mode=StereoMode.DUAL_MONO,
        )
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "libpipewire-module-echo-cancel" in output
        assert 'target.object = "big-aec-source"' in output

    def test_enabled_voice_changer_has_aec(self):
        config = FilterChainConfig(
            echo_cancel_enabled=True,
            source_node_name="test-mic",
            stereo_mode=StereoMode.VOICE_CHANGER,
            stereo_width=0.5,
        )
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "libpipewire-module-echo-cancel" in output
        assert 'target.object = "big-aec-source"' in output

    def test_fixed_latency_in_config(self):
        config = FilterChainConfig(echo_cancel_enabled=True, source_node_name="test-mic")
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "node.latency = 1024/48000" not in output
        assert "960/48000" in output

    def test_aec_mono_channels(self):
        """AEC must force mono to avoid stereo/mono routing mismatch."""
        config = FilterChainConfig(echo_cancel_enabled=True, source_node_name="test-mic")
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "audio.channels = 1" in output

    def test_disabled_no_target_in_capture(self):
        config = FilterChainConfig(echo_cancel_enabled=False)
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "target.object" not in output

    def test_aec_before_filter_chain(self):
        config = FilterChainConfig(echo_cancel_enabled=True)
        gen = FilterChainGenerator(config)
        output = gen.generate()
        aec_pos = output.index("libpipewire-module-echo-cancel")
        fc_pos = output.index("libpipewire-module-filter-chain")
        assert aec_pos < fc_pos

    def test_no_filter_smart_with_aec(self):
        config = FilterChainConfig(echo_cancel_enabled=True)
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "filter.smart" not in output
        assert 'node.name = "big-noise-canceling-output"' in output

    def test_filter_smart_without_aec(self):
        config = FilterChainConfig(echo_cancel_enabled=False)
        gen = FilterChainGenerator(config)
        output = gen.generate()
        assert "filter.smart = true" in output
