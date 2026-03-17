#!/usr/bin/env python3
"""
Configuration persistence service.

Handles saving and loading filter chain configuration for systemd autostart.
This separates persistence concerns from runtime PipeWire operations.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path

from biglinux_microphone.audio.filter_chain import (
    CONFIG_DIR,
    CONFIG_FILE,
    FilterChainConfig,
    FilterChainGenerator,
)
from biglinux_microphone.config import (
    CompressorConfig,
    GateConfig,
    NoiseModel,
    StereoMode,
)

logger = logging.getLogger(__name__)


@dataclass
class FilterChainState:
    """
    In-memory state of the filter chain configuration.

    This is the single source of truth for current settings.
    Can be serialized to disk for systemd autostart.
    """

    # Noise reduction settings
    noise_reduction_enabled: bool = True
    noise_reduction_model: NoiseModel = NoiseModel.GTCRN_DNS3
    noise_reduction_strength: float = 1.0
    noise_reduction_speech_strength: float = 1.0
    noise_reduction_lookahead_ms: int = 50
    noise_reduction_model_blending: float = 0.0
    noise_reduction_voice_recovery: float = 0.75

    # HPF settings
    hpf_enabled: bool = True
    hpf_frequency: float = 80.0

    # Compressor settings
    compressor_enabled: bool = True
    compressor_intensity: float = 0.25

    # Gate settings
    gate_enabled: bool = True
    gate_intensity: float = 0.25

    # Stereo settings
    stereo_mode: StereoMode = StereoMode.MONO
    stereo_width: float = 0.7

    # Crossfeed settings
    crossfeed_enabled: bool = False

    # Equalizer settings
    eq_enabled: bool = True
    eq_bands: list[float] | None = None

    # Echo cancellation settings
    echo_cancel_enabled: bool = False

    def __post_init__(self) -> None:
        """Initialize default EQ bands if not provided."""
        if self.eq_bands is None:
            self.eq_bands = [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0]

    @property
    def _compressor(self) -> CompressorConfig:
        """Derive SC4m parameters from intensity."""
        return CompressorConfig(
            enabled=self.compressor_enabled,
            intensity=self.compressor_intensity,
        )

    @property
    def _gate(self) -> GateConfig:
        """Derive gate parameters from intensity."""
        return GateConfig(
            enabled=self.gate_enabled,
            intensity=self.gate_intensity,
        )

    def to_filter_config(self) -> FilterChainConfig:
        """Convert state to FilterChainConfig for generation."""
        return FilterChainConfig(
            hpf_enabled=self.hpf_enabled,
            hpf_frequency=self.hpf_frequency,
            noise_reduction_enabled=self.noise_reduction_enabled,
            noise_reduction_model=self.noise_reduction_model,
            noise_reduction_strength=self.noise_reduction_strength,
            noise_reduction_speech_strength=self.noise_reduction_speech_strength,
            noise_reduction_lookahead_ms=self.noise_reduction_lookahead_ms,
            noise_reduction_model_blending=self.noise_reduction_model_blending,
            noise_reduction_voice_recovery=self.noise_reduction_voice_recovery,
            compressor_enabled=self.compressor_enabled,
            compressor_threshold_db=self._compressor.threshold_db,
            compressor_ratio=self._compressor.ratio,
            compressor_attack_ms=self._compressor.attack_ms,
            compressor_release_ms=self._compressor.release_ms,
            compressor_makeup_gain_db=self._compressor.makeup_gain_db,
            compressor_knee_db=self._compressor.knee_db,
            compressor_rms_peak=self._compressor.rms_peak,
            gate_enabled=self.gate_enabled,
            gate_threshold_db=int(self._gate.threshold_db),
            gate_range_db=int(self._gate.range_db),
            gate_attack_ms=self._gate.attack_ms,
            gate_hold_ms=self._gate.hold_ms,
            gate_release_ms=self._gate.release_ms,
            stereo_mode=self.stereo_mode,
            stereo_width=self.stereo_width,
            crossfeed_enabled=self.crossfeed_enabled,
            eq_enabled=self.eq_enabled,
            eq_bands=list(self.eq_bands) if self.eq_bands else [0.0] * 10,
            echo_cancel_enabled=self.echo_cancel_enabled,
        )


class ConfigPersistence:
    """
    Handles saving filter chain configuration for systemd autostart.

    Only writes files when:
    - User explicitly saves settings
    - Application closes cleanly
    - User enables "start at login"

    Does NOT write files during:
    - Normal parameter changes
    - Live adjustments (handled by PipeWireService)
    """

    def __init__(self, config_dir: Path | None = None) -> None:
        """
        Initialize the persistence service.

        Args:
            config_dir: Directory for config files. Defaults to ~/.config/pipewire/pipewire.conf.d/
        """
        self._config_dir = config_dir or CONFIG_DIR
        self._config_file = self._config_dir / CONFIG_FILE

    @property
    def config_path(self) -> Path:
        """Get the full path to the config file."""
        return self._config_file

    def config_exists(self) -> bool:
        """Check if config file exists (autostart enabled)."""
        return self._config_file.exists()

    def save(self, state: FilterChainState) -> Path:
        """
        Save current state to PipeWire config format.

        This file is read by systemd service on login/boot.

        Args:
            state: Current filter chain state

        Returns:
            Path to the saved config file
        """
        config = state.to_filter_config()
        generator = FilterChainGenerator(config)
        generator.save(self._config_file)

        logger.info("Config saved to: %s", self._config_file)
        return self._config_file

    def save_from_service_state(
        self,
        *,
        noise_reduction_model: NoiseModel,
        noise_reduction_strength: float,
        gate_enabled: bool,
        gate_intensity: float,
        stereo_mode: StereoMode,
        stereo_width: float,
        crossfeed_enabled: bool,
        eq_enabled: bool,
        eq_bands: list[float],
    ) -> Path:
        """
        Save config from individual service state values.

        This is a convenience method for PipeWireService to persist
        its current state without creating FilterChainState directly.

        Returns:
            Path to the saved config file
        """
        state = FilterChainState(
            noise_reduction_enabled=True,  # Always enabled when saving
            noise_reduction_model=noise_reduction_model,
            noise_reduction_strength=noise_reduction_strength,
            gate_enabled=gate_enabled,
            gate_intensity=gate_intensity,
            stereo_mode=stereo_mode,
            stereo_width=stereo_width,
            crossfeed_enabled=crossfeed_enabled,
            eq_enabled=eq_enabled,
            eq_bands=eq_bands.copy(),
        )
        return self.save(state)

    def delete(self) -> bool:
        """
        Remove config file (disables autostart).

        Returns:
            True if file was deleted, False if it didn't exist
        """
        if self._config_file.exists():
            self._config_file.unlink()
            logger.info("Config deleted: %s", self._config_file)
            return True
        return False

    def ensure_config_dir(self) -> Path:
        """
        Ensure the config directory exists.

        Returns:
            Path to the config directory
        """
        self._config_dir.mkdir(parents=True, exist_ok=True)
        return self._config_dir
