"""
Tests for config module.

Tests configuration dataclasses, serialization, and settings management.
"""

from __future__ import annotations

import json
import tempfile
from pathlib import Path
from unittest.mock import patch

from biglinux_microphone.config import (
    APP_ID,
    APP_NAME,
    EQ_BAND_COUNT,
    EQ_BAND_DEFAULT,
    EQ_PRESETS,
    GATE_RANGE_DEFAULT,
    GATE_THRESHOLD_DEFAULT,
    STEREO_WIDTH_DEFAULT,
    STRENGTH_DEFAULT,
    AppSettings,
    BluetoothConfig,
    EqualizerConfig,
    GateConfig,
    NoiseModel,
    NoiseReductionConfig,
    StereoConfig,
    StereoMode,
    UIConfig,
    VisualizerStyle,
    WindowConfig,
    load_settings,
    save_settings,
)


class TestNoiseReductionConfig:
    """Tests for NoiseReductionConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default configuration values."""
        config = NoiseReductionConfig()
        assert config.enabled is True
        assert config.model == NoiseModel.GTCRN_FULL_QUALITY
        assert config.strength == STRENGTH_DEFAULT

    def test_custom_values(self) -> None:
        """Test custom configuration values."""
        config = NoiseReductionConfig(
            enabled=False,
            model=NoiseModel.GTCRN_LOW_LATENCY,
            strength=0.5,
        )
        assert config.enabled is False
        assert config.model == NoiseModel.GTCRN_LOW_LATENCY
        assert config.strength == 0.5


class TestGateConfig:
    """Tests for GateConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default gate configuration."""
        config = GateConfig()
        assert config.enabled is True
        assert config.threshold_db == GATE_THRESHOLD_DEFAULT
        assert config.range_db == GATE_RANGE_DEFAULT

    def test_custom_values(self) -> None:
        """Test custom gate configuration."""
        config = GateConfig(
            enabled=False,
            threshold_db=-50,
            range_db=-20,
        )
        assert config.enabled is False
        assert config.threshold_db == -50
        assert config.range_db == -20


class TestStereoConfig:
    """Tests for StereoConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default stereo configuration."""
        config = StereoConfig()
        assert config.enabled is True
        assert config.mode == StereoMode.DUAL_MONO
        assert config.width == STEREO_WIDTH_DEFAULT

    def test_radio_mode(self) -> None:
        """Test RADIO stereo mode configuration."""
        config = StereoConfig(
            enabled=True,
            mode=StereoMode.RADIO,
        )
        assert config.mode == StereoMode.RADIO


class TestEqualizerConfig:
    """Tests for EqualizerConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default equalizer configuration."""
        config = EqualizerConfig()
        assert config.enabled is False
        assert len(config.bands) == EQ_BAND_COUNT
        assert all(b == EQ_BAND_DEFAULT for b in config.bands)
        assert config.preset == "flat"

    def test_custom_bands(self) -> None:
        """Test custom EQ bands."""
        bands = [1.0, 2.0, 3.0, 4.0, 5.0, 4.0, 3.0, 2.0, 1.0, 0.0]
        config = EqualizerConfig(enabled=True, bands=bands, preset="custom")
        assert config.bands == bands


class TestWindowConfig:
    """Tests for WindowConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default window configuration."""
        config = WindowConfig()
        assert config.width == 720
        assert config.height == 700
        assert config.maximized is False


class TestUIConfig:
    """Tests for UIConfig dataclass."""

    def test_default_values(self) -> None:
        """Test default UI configuration."""
        config = UIConfig()
        assert config.visualizer_style == VisualizerStyle.MODERN_WAVES
        assert config.show_advanced is False


class TestAppSettings:
    """Tests for AppSettings dataclass."""

    def test_default_settings(self) -> None:
        """Test default application settings."""
        settings = AppSettings()
        assert isinstance(settings.noise_reduction, NoiseReductionConfig)
        assert isinstance(settings.gate, GateConfig)
        assert isinstance(settings.stereo, StereoConfig)
        assert isinstance(settings.equalizer, EqualizerConfig)
        assert isinstance(settings.window, WindowConfig)
        assert isinstance(settings.ui, UIConfig)
        assert isinstance(settings.bluetooth, BluetoothConfig)

    def test_to_dict(self) -> None:
        """Test settings serialization to dictionary."""
        settings = AppSettings()
        data = settings.to_dict()

        assert "noise_reduction" in data
        assert "gate" in data
        assert "stereo" in data
        assert "equalizer" in data
        assert "window" in data
        assert "ui" in data
        assert "bluetooth" in data

        # Verify enums are converted to values (GTCRN_FULL_QUALITY = 1)
        assert data["noise_reduction"]["model"] == 1
        assert data["stereo"]["mode"] == "dual_mono"
        assert data["ui"]["visualizer_style"] == 0

    def test_from_dict(self) -> None:
        """Test settings deserialization from dictionary."""
        data = {
            "noise_reduction": {"enabled": False, "model": 1, "strength": 0.8},
            "gate": {"enabled": False, "threshold_db": -45, "range_db": -10},
            "stereo": {"enabled": True, "mode": "radio"},
            "equalizer": {"enabled": True, "preset": "voice_boost"},
            "window": {"width": 800, "height": 600},
            "ui": {"visualizer_style": 1},
            "bluetooth": {"auto_switch_headset": True},
        }

        settings = AppSettings.from_dict(data)

        assert settings.noise_reduction.enabled is False
        assert settings.noise_reduction.model == NoiseModel.GTCRN_FULL_QUALITY
        assert settings.noise_reduction.strength == 0.8

        assert settings.gate.enabled is False
        assert settings.gate.threshold_db == -45

        assert settings.stereo.enabled is True
        assert settings.stereo.mode == StereoMode.RADIO

        assert settings.equalizer.enabled is True
        assert settings.equalizer.preset == "voice_boost"

        assert settings.window.width == 800
        assert settings.window.height == 600

        assert settings.ui.visualizer_style == VisualizerStyle.RETRO_BARS

        assert settings.bluetooth.auto_switch_headset is True

    def test_roundtrip(self) -> None:
        """Test serialization roundtrip."""
        original = AppSettings(
            noise_reduction=NoiseReductionConfig(
                enabled=True,
                model=NoiseModel.GTCRN_FULL_QUALITY,
                strength=0.9,
            ),
            stereo=StereoConfig(
                enabled=True,
                mode=StereoMode.RADIO,
                width=0.8,
            ),
        )

        data = original.to_dict()
        restored = AppSettings.from_dict(data)

        assert restored.noise_reduction.enabled == original.noise_reduction.enabled
        assert restored.noise_reduction.model == original.noise_reduction.model
        assert restored.noise_reduction.strength == original.noise_reduction.strength
        assert restored.stereo.mode == original.stereo.mode


class TestSettingsIO:
    """Tests for settings load/save functions."""

    def test_load_missing_file(self) -> None:
        """Test loading settings when file doesn't exist."""
        with patch.object(Path, "exists", return_value=False):
            settings = load_settings()
            assert isinstance(settings, AppSettings)

    def test_save_and_load(self) -> None:
        """Test save and load roundtrip."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_config_dir = Path(tmpdir)
            tmp_settings_file = tmp_config_dir / "settings.json"

            with (
                patch("biglinux_microphone.config.CONFIG_DIR", tmp_config_dir),
                patch("biglinux_microphone.config.SETTINGS_FILE", tmp_settings_file),
            ):
                settings = AppSettings(
                    noise_reduction=NoiseReductionConfig(strength=0.75),
                    gate=GateConfig(threshold_db=-42),
                )

                # Save settings
                result = save_settings(settings)
                assert result is True
                assert tmp_settings_file.exists()

                # Verify file contents
                with open(tmp_settings_file) as f:
                    data = json.load(f)
                    assert data["noise_reduction"]["strength"] == 0.75
                    assert data["gate"]["threshold_db"] == -42

    def test_load_corrupted_json(self) -> None:
        """Test loading corrupted JSON file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_settings_file = Path(tmpdir) / "settings.json"
            tmp_settings_file.write_text("not valid json {{{")

            with patch("biglinux_microphone.config.SETTINGS_FILE", tmp_settings_file):
                settings = load_settings()
                # Should return defaults on error
                assert isinstance(settings, AppSettings)


class TestEnumerations:
    """Tests for enumeration types."""

    def test_noise_model_values(self) -> None:
        """Test NoiseModel enumeration values."""
        assert NoiseModel.GTCRN_LOW_LATENCY == 0
        assert NoiseModel.GTCRN_FULL_QUALITY == 1

    def test_stereo_mode_values(self) -> None:
        """Test StereoMode enumeration values."""
        assert StereoMode.MONO.value == "mono"
        assert StereoMode.DUAL_MONO.value == "dual_mono"
        assert StereoMode.RADIO.value == "radio"
        assert StereoMode.VOICE_CHANGER.value == "voice_changer"

    def test_visualizer_style_values(self) -> None:
        """Test VisualizerStyle enumeration values."""
        assert VisualizerStyle.MODERN_WAVES == 0
        assert VisualizerStyle.RETRO_BARS == 1


class TestConstants:
    """Tests for module constants."""

    def test_app_constants(self) -> None:
        """Test application constants are defined."""
        assert APP_ID == "br.com.biglinux.microphone"
        assert APP_NAME == "Microphone Settings"

    def test_eq_presets_exist(self) -> None:
        """Test EQ presets are defined."""
        assert "flat" in EQ_PRESETS
        assert "voice_boost" in EQ_PRESETS
        assert "podcast" in EQ_PRESETS
        assert "warm" in EQ_PRESETS

        # Each preset should have bands
        for preset in EQ_PRESETS.values():
            assert "bands" in preset
            assert len(preset["bands"]) == EQ_BAND_COUNT
