"""
Tests for config_persistence module.

Tests configuration persistence for systemd autostart.
"""

from __future__ import annotations

import tempfile
from pathlib import Path

import pytest

from biglinux_microphone.config import NoiseModel, StereoMode
from biglinux_microphone.services.config_persistence import (
    ConfigPersistence,
    FilterChainState,
)


class TestFilterChainState:
    """Tests for FilterChainState dataclass."""

    def test_default_values(self) -> None:
        """Test default state values."""
        state = FilterChainState()

        assert state.noise_reduction_enabled is True
        assert state.noise_reduction_model == NoiseModel.GTCRN_LOW_LATENCY
        assert state.noise_reduction_strength == 1.0
        assert state.gate_enabled is True
        assert state.gate_threshold_db == -36
        assert state.gate_range_db == -6
        assert state.stereo_mode == StereoMode.MONO
        assert state.stereo_width == 0.7

        assert state.crossfeed_enabled is False
        assert state.eq_enabled is False
        assert len(state.eq_bands) == 10
        assert all(b == 0.0 for b in state.eq_bands)

    def test_custom_values(self) -> None:
        """Test custom state values."""
        eq_bands = [1.0, 2.0, 3.0, 4.0, 5.0, -1.0, -2.0, -3.0, -4.0, -5.0]
        state = FilterChainState(
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
            noise_reduction_strength=0.8,
            gate_threshold_db=-40,
            stereo_mode=StereoMode.RADIO,
            eq_enabled=True,
            eq_bands=eq_bands,
        )

        assert state.noise_reduction_model == NoiseModel.GTCRN_FULL_QUALITY
        assert state.noise_reduction_strength == 0.8
        assert state.gate_threshold_db == -40
        assert state.stereo_mode == StereoMode.RADIO
        assert state.eq_enabled is True
        assert state.eq_bands == eq_bands

    def test_to_filter_config(self) -> None:
        """Test conversion to FilterChainConfig."""
        state = FilterChainState(
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
            noise_reduction_strength=0.5,
            gate_enabled=False,
            stereo_mode=StereoMode.DUAL_MONO,
        )

        config = state.to_filter_config()

        assert config.noise_reduction_model == NoiseModel.GTCRN_FULL_QUALITY
        assert config.noise_reduction_strength == 0.5
        assert config.gate_enabled is False
        assert config.stereo_mode == StereoMode.DUAL_MONO


class TestConfigPersistence:
    """Tests for ConfigPersistence class."""

    @pytest.fixture
    def temp_config_dir(self) -> Path:
        """Create temporary config directory."""
        with tempfile.TemporaryDirectory() as tmpdir:
            yield Path(tmpdir)

    def test_init_default_dir(self) -> None:
        """Test initialization with default directory."""
        persistence = ConfigPersistence()

        assert persistence.config_path.name == "source-gtcrn-smart.conf"
        assert "filter-chain.conf.d" in str(persistence.config_path)

    def test_init_custom_dir(self, temp_config_dir: Path) -> None:
        """Test initialization with custom directory."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)

        assert persistence.config_path.parent == temp_config_dir

    def test_config_exists_false(self, temp_config_dir: Path) -> None:
        """Test config_exists returns False when no file."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)

        assert persistence.config_exists() is False

    def test_save_creates_file(self, temp_config_dir: Path) -> None:
        """Test save creates config file."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)
        state = FilterChainState()

        result = persistence.save(state)

        assert result.exists()
        assert persistence.config_exists() is True

    def test_save_content(self, temp_config_dir: Path) -> None:
        """Test saved config content."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)
        state = FilterChainState(
            noise_reduction_strength=0.75,
            gate_threshold_db=-42,
        )

        result = persistence.save(state)
        content = result.read_text()

        # Check for key configuration elements
        assert "filter.graph" in content
        assert "gtcrn_mono" in content  # LADSPA plugin label
        assert "Strength" in content
        assert "0.75" in content

    def test_save_from_service_state(self, temp_config_dir: Path) -> None:
        """Test save_from_service_state convenience method."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)

        result = persistence.save_from_service_state(
            noise_reduction_model=NoiseModel.GTCRN_FULL_QUALITY,
            noise_reduction_strength=0.9,
            gate_enabled=True,
            gate_threshold_db=-30,
            gate_range_db=-10,
            stereo_mode=StereoMode.MONO,
            stereo_width=0.5,
            crossfeed_enabled=False,
            eq_enabled=False,
            eq_bands=[0.0] * 10,
        )

        assert result.exists()
        content = result.read_text()
        assert "0.9" in content  # strength value

    def test_delete_removes_file(self, temp_config_dir: Path) -> None:
        """Test delete removes config file."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)
        state = FilterChainState()
        persistence.save(state)

        assert persistence.config_exists() is True

        result = persistence.delete()

        assert result is True
        assert persistence.config_exists() is False

    def test_delete_nonexistent_returns_false(self, temp_config_dir: Path) -> None:
        """Test delete returns False for nonexistent file."""
        persistence = ConfigPersistence(config_dir=temp_config_dir)

        result = persistence.delete()

        assert result is False

    def test_ensure_config_dir_creates(self, temp_config_dir: Path) -> None:
        """Test ensure_config_dir creates directory."""
        subdir = temp_config_dir / "nested" / "path"
        persistence = ConfigPersistence(config_dir=subdir)

        result = persistence.ensure_config_dir()

        assert result.exists()
        assert result.is_dir()
