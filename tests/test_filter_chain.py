"""
Tests for filter_chain module.

Tests PipeWire filter-chain configuration generation.
"""

from __future__ import annotations

import tempfile
from pathlib import Path
from unittest.mock import patch

from biglinux_microphone.audio.filter_chain import (
    CONFIG_DIR,
    CONFIG_FILE,
    EQ_BAND_TO_MBEQ_INDEX,
    GATE_LABEL,
    GATE_LIBRARY,
    GTCRN_LABEL,
    GTCRN_LIBRARY,
    MBEQ_BANDS,
    MBEQ_LABEL,
    MBEQ_LIBRARY,
    MBEQ_PARAM_NAMES,
    FilterChainConfig,
    FilterChainGenerator,
    generate_config_for_settings,
)
from biglinux_microphone.config import NoiseModel, StereoMode


class TestFilterChainConfig:
    """Tests for FilterChainConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default configuration values."""
        config = FilterChainConfig()
        assert config.noise_reduction_enabled is True
        assert config.noise_reduction_model == NoiseModel.GTCRN_FULL_QUALITY
        assert config.noise_reduction_strength == 1.0
        assert config.gate_enabled is True
        assert config.gate_threshold_db == -40
        assert config.gate_range_db == -60
        assert config.stereo_mode == StereoMode.MONO
        assert config.eq_enabled is False

    def test_custom_values(self) -> None:
        """Test custom configuration values."""
        config = FilterChainConfig(
            noise_reduction_enabled=False,
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
            gate_threshold_db=-45,
            stereo_mode=StereoMode.DUAL_MONO,
        )
        assert config.noise_reduction_enabled is False
        assert config.noise_reduction_model == NoiseModel.GTCRN_FULL_QUALITY
        assert config.gate_threshold_db == -45
        assert config.stereo_mode == StereoMode.DUAL_MONO

    def test_eq_bands_default(self) -> None:
        """Test default EQ bands."""
        config = FilterChainConfig()
        assert len(config.eq_bands) == 10
        assert all(b == 0.0 for b in config.eq_bands)


class TestFilterChainGenerator:
    """Tests for FilterChainGenerator."""

    def test_generate_mono_config(self) -> None:
        """Test generating mono configuration."""
        config = FilterChainConfig(stereo_mode=StereoMode.MONO)
        generator = FilterChainGenerator(config)
        content = generator.generate()

        # Should contain basic structure
        assert "context.modules" in content
        assert "libpipewire-module-filter-chain" in content
        assert "filter.graph" in content

        # Should have gate and gtcrn nodes
        assert GATE_LIBRARY in content
        assert GATE_LABEL in content
        assert GTCRN_LIBRARY in content
        assert GTCRN_LABEL in content

        # Mono config uses MONO position for playback
        assert "audio.position = [ MONO ]" in content
        # Should NOT have stereo (FL FR) output
        assert "audio.position = [ FL FR ]" not in content
        assert "audio.channels = 1" in content

    def test_generate_stereo_config(self) -> None:
        """Test generating stereo configuration with dual mono mode."""
        config = FilterChainConfig(
            stereo_mode=StereoMode.DUAL_MONO,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        # Should contain basic structure
        assert "context.modules" in content
        assert "libpipewire-module-filter-chain" in content

        # Should have stereo output configuration
        assert "audio.position = [ MONO ]" in content
        assert "audio.position = [ FL FR ]" in content

        # Should have copy nodes for stereo duplication
        assert 'name = "copy_tee"' in content
        assert 'name = "copy_left"' in content
        assert 'name = "copy_right"' in content

    def test_generate_dual_mono_config(self) -> None:
        """Test generating dual mono configuration (no delay)."""
        config = FilterChainConfig(
            stereo_mode=StereoMode.DUAL_MONO,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        # Audio position check - should be stereo output
        assert "audio.position = [ FL FR ]" in content
        # DUAL_MONO should NOT have delay - uses simple copy instead
        assert '"Delay Time (s)"' not in content
        assert 'name = "delay_right"' not in content
        # Should have copy node for right channel
        assert 'name = "copy_right"' in content

    def test_generate_config_with_eq(self) -> None:
        """Test generating configuration with EQ enabled."""
        eq_bands = [2.0, 1.0, 0.0, -1.0, -2.0, 0.0, 1.0, 2.0, 0.0, -1.0]
        config = FilterChainConfig(
            stereo_mode=StereoMode.MONO,
            eq_enabled=True,
            eq_bands=eq_bands,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        # Should have EQ node
        assert MBEQ_LIBRARY in content
        assert MBEQ_LABEL in content
        assert 'name = "eq"' in content

        # Should have EQ band values
        assert "50Hz gain" in content
        assert "100Hz gain" in content

    def test_generate_stereo_with_eq(self) -> None:
        """Test generating stereo configuration with EQ."""
        config = FilterChainConfig(
            stereo_mode=StereoMode.DUAL_MONO,
            eq_enabled=True,
            eq_bands=[1.0] * 10,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        # Should have both EQ and stereo
        assert MBEQ_LIBRARY in content
        assert "audio.position = [ FL FR ]" in content

    def test_gate_parameters(self) -> None:
        """Test gate filter parameters are correctly applied."""
        config = FilterChainConfig(
            gate_threshold_db=-42,
            gate_range_db=-12,
            gate_attack_ms=5.0,
            gate_hold_ms=100.0,
            gate_release_ms=30.0,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        assert '"Threshold (dB)" = -42' in content
        assert '"Range (dB)" = -12' in content
        assert '"Attack (ms)" = 5.0' in content
        assert '"Hold (ms)" = 100.0' in content
        assert '"Decay (ms)" = 30.0' in content

    def test_noise_reduction_parameters(self) -> None:
        """Test noise reduction parameters are correctly applied."""
        config = FilterChainConfig(
            noise_reduction_strength=0.75,
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
        )
        generator = FilterChainGenerator(config)
        content = generator.generate()

        assert '"Strength" = 0.75' in content
        assert '"Model" = 1' in content  # FULL_QUALITY = 1

    def test_save_config(self) -> None:
        """Test saving configuration to file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir) / "test.conf"

            config = FilterChainConfig()
            generator = FilterChainGenerator(config)
            saved_path = generator.save(tmp_path)

            assert saved_path == tmp_path
            assert tmp_path.exists()

            content = tmp_path.read_text()
            assert "context.modules" in content

    def test_save_config_default_path(self) -> None:
        """Test saving to default path."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_config_dir = Path(tmpdir)

            with patch(
                "biglinux_microphone.audio.filter_chain.CONFIG_DIR", tmp_config_dir
            ):
                config = FilterChainConfig()
                generator = FilterChainGenerator(config)
                saved_path = generator.save()

                assert saved_path == tmp_config_dir / CONFIG_FILE
                assert saved_path.exists()

    def test_delete_config(self) -> None:
        """Test deleting configuration file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir) / "test.conf"

            config = FilterChainConfig()
            generator = FilterChainGenerator(config)

            # Save first
            generator.save(tmp_path)
            assert tmp_path.exists()

            # Delete
            result = generator.delete(tmp_path)
            assert result is True
            assert not tmp_path.exists()

    def test_delete_nonexistent_config(self) -> None:
        """Test deleting non-existent file returns False."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir) / "nonexistent.conf"

            config = FilterChainConfig()
            generator = FilterChainGenerator(config)

            result = generator.delete(tmp_path)
            assert result is False


class TestGenerateConfigForSettings:
    """Tests for generate_config_for_settings convenience function."""

    def test_basic_generation(self) -> None:
        """Test basic config generation."""
        content = generate_config_for_settings()

        assert "context.modules" in content
        assert GTCRN_LIBRARY in content
        assert GATE_LIBRARY in content

    def test_stereo_mode(self) -> None:
        """Test stereo mode generation."""
        content = generate_config_for_settings(
            stereo_mode=StereoMode.DUAL_MONO,
        )

        assert "audio.position = [ FL FR ]" in content
        # DUAL_MONO mode should have copy nodes for stereo duplication
        assert 'name = "copy_right"' in content
        assert 'name = "copy_left"' in content

    def test_custom_parameters(self) -> None:
        """Test custom parameter application."""
        content = generate_config_for_settings(
            noise_reduction_strength=0.5,
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
            gate_threshold_db=-50,
        )

        assert '"Strength" = 0.5' in content
        assert '"Model" = 1' in content
        assert '"Threshold (dB)" = -50' in content


class TestLADSPAConstants:
    """Tests for LADSPA plugin constants."""

    def test_library_paths_defined(self) -> None:
        """Test all required LADSPA library paths are defined."""
        assert GTCRN_LIBRARY == "/usr/lib/ladspa/libgtcrn_ladspa.so"
        assert GATE_LIBRARY == "/usr/lib/ladspa/gate_1410.so"
        assert MBEQ_LIBRARY == "/usr/lib/ladspa/mbeq_1197.so"

    def test_labels_defined(self) -> None:
        """Test all required LADSPA labels are defined."""
        assert GTCRN_LABEL == "gtcrn_mono"
        assert GATE_LABEL == "gate"
        assert MBEQ_LABEL == "mbeq"

    def test_mbeq_bands(self) -> None:
        """Test MBEQ frequency bands are correct."""
        assert len(MBEQ_BANDS) == 15
        assert MBEQ_BANDS[0] == 50
        assert MBEQ_BANDS[-1] == 20000

    def test_eq_band_mapping(self) -> None:
        """Test EQ band to MBEQ index mapping."""
        assert len(EQ_BAND_TO_MBEQ_INDEX) == 10
        # First UI band (31Hz) maps to MBEQ band 0 (50Hz)
        assert EQ_BAND_TO_MBEQ_INDEX[0] == 0
        # Last UI band (16000Hz) maps to MBEQ band 14 (20000Hz)
        assert EQ_BAND_TO_MBEQ_INDEX[9] == 14

    def test_mbeq_param_names(self) -> None:
        """Test MBEQ parameter names for live updates."""
        assert len(MBEQ_PARAM_NAMES) == 15
        # First band is a shelving filter
        assert "shelving" in MBEQ_PARAM_NAMES[0].lower()
        assert "50hz" in MBEQ_PARAM_NAMES[0].lower()
        # All names contain 'gain'
        assert all("gain" in name.lower() for name in MBEQ_PARAM_NAMES)


class TestConfigPath:
    """Tests for configuration path constants."""

    def test_config_dir(self) -> None:
        """Test config directory path."""
        assert (
            Path.home() / ".config" / "pipewire" / "filter-chain.conf.d" == CONFIG_DIR
        )

    def test_config_file(self) -> None:
        """Test config filename."""
        assert CONFIG_FILE == "source-gtcrn-smart.conf"
