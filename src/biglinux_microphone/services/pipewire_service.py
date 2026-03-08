#!/usr/bin/env python3
"""
PipeWire service for managing audio filter chains.

Handles all interactions with PipeWire, including:
- Starting/stopping noise reduction
- Configuring filter parameters
- Live parameter updates via pw-cli
- Dynamic filter chain configuration generation
"""

from __future__ import annotations

import asyncio
import logging
import subprocess
import threading
from collections.abc import Callable
from pathlib import Path
from queue import Queue

from biglinux_microphone.audio.filter_chain import (
    CONFIG_DIR,
    CONFIG_FILE,
    EQ_BAND_TO_MBEQ_INDEX,
    MBEQ_PARAM_NAMES,
    FilterChainConfig,
    FilterChainGenerator,
    ensure_daemon_config,
)
from biglinux_microphone.config import (
    NoiseModel,
    StereoMode,
)

logger = logging.getLogger(__name__)

# Constants
CONFIG_PATH = CONFIG_DIR
SETTINGS_FILE = Path.home() / ".config" / "biglinux-microphone" / "settings.json"
SERVICE_NAME = "noise-reduction-pipewire"


class PipeWireService:
    """
    Service for managing PipeWire noise reduction and audio processing.

    Provides methods for:
    - Starting/stopping the filter chain
    - Getting/setting noise reduction parameters
    - Live parameter updates without audio interruption
    - Stereo enhancement configuration
    - Equalizer configuration
    - Dynamic configuration generation
    """

    def __init__(self) -> None:
        """Initialize the PipeWire service."""
        self._is_updating = False
        self._restart_pending = False
        self._pending_settings = None
        self._pending_on_complete: Callable[[], None] | None = None

        # Live update queue
        self._update_queue = Queue()
        self._cached_node_id: str | None = None

        # Start worker thread
        self._worker_thread = threading.Thread(target=self._worker_loop, daemon=True)
        self._worker_thread.start()

        # Internal state tracking
        self._noise_reduction_enabled = True
        self._noise_reduction_model = NoiseModel.GTCRN_DNS3
        self._noise_reduction_strength = 1.0
        self._noise_reduction_speech_strength = 1.0
        self._noise_reduction_lookahead_ms = 0
        self._noise_reduction_voice_enhance = 0.50
        self._noise_reduction_model_blending = False
        self._gate_enabled = True
        self._gate_threshold_db = -30
        self._gate_range_db = -60
        self._gate_attack_ms = 10.0
        self._gate_hold_ms = 400.0
        self._gate_release_ms = 200.0
        self._stereo_mode = StereoMode.MONO
        self._stereo_width = 0.7

        self._crossfeed_enabled = False
        self._eq_enabled = True
        self._eq_bands = [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0]

        # Paths
        self._config_file = CONFIG_PATH / CONFIG_FILE

        # Clean up legacy configurations from older versions
        self._cleanup_legacy_configs()

        logger.debug("PipeWire service initialized")

    def _cleanup_legacy_configs(self) -> None:
        """Remove legacy configuration files from older versions."""
        legacy_paths = [
            # Old pipewire.conf.d location (caused issues with main PipeWire restart)
            Path.home()
            / ".config"
            / "pipewire"
            / "pipewire.conf.d"
            / "10-biglinux-microphone.conf",
            # Old rnnoise configs
            Path.home()
            / ".config"
            / "pipewire"
            / "filter-chain.conf.d"
            / "source-rnnoise.conf",
            Path.home()
            / ".config"
            / "pipewire"
            / "filter-chain.conf.d"
            / "source-rnnoise-smart.conf",
            # Old GTCRN config
            Path.home()
            / ".config"
            / "pipewire"
            / "filter-chain.conf.d"
            / "source-gtcrn-smart.conf",
        ]

        for path in legacy_paths:
            if path.exists():
                try:
                    path.unlink()
                    logger.info("Removed legacy config: %s", path)
                except OSError:
                    logger.warning("Failed to remove legacy config: %s", path)

    def sync_from_settings(self, settings) -> None:
        """
        Synchronize internal state from AppSettings.

        Args:
            settings: AppSettings instance to sync from
        """
        self._noise_reduction_enabled = settings.noise_reduction.enabled
        self._noise_reduction_model = settings.noise_reduction.model
        self._noise_reduction_strength = settings.noise_reduction.strength
        self._noise_reduction_speech_strength = settings.noise_reduction.speech_strength
        self._noise_reduction_lookahead_ms = settings.noise_reduction.lookahead_ms
        self._noise_reduction_voice_enhance = settings.noise_reduction.voice_enhance
        self._noise_reduction_model_blending = settings.noise_reduction.model_blending
        self._gate_enabled = settings.gate.enabled
        self._gate_threshold_db = settings.gate.threshold_db
        self._gate_range_db = settings.gate.range_db
        self._gate_attack_ms = settings.gate.attack_ms
        self._gate_hold_ms = settings.gate.hold_ms
        self._gate_release_ms = settings.gate.release_ms
        self._stereo_mode = settings.stereo.mode
        self._stereo_width = settings.stereo.width

        self._crossfeed_enabled = settings.stereo.crossfeed_enabled
        self._eq_enabled = settings.equalizer.enabled
        self._eq_bands = list(settings.equalizer.bands)
        logger.debug("Synchronized state from settings")

    # =========================================================================
    # Status Methods
    # =========================================================================

    def is_enabled(self) -> bool:
        """
        Check if noise reduction filter-chain is currently active.

        Uses pw-cli to query PipeWire graph for our specific filter node
        identified by its description 'Noise Canceling Microphone'.
        Falls back to pw-dump with filter.smart.name check.
        """
        # Primary: lightweight check via pw-cli list-objects
        try:
            result = subprocess.run(
                ["pw-cli", "list-objects"],
                capture_output=True,
                text=True,
                timeout=3,
            )
            if result.returncode == 0:
                return "Noise Canceling Microphone" in result.stdout
        except Exception:
            pass

        # Fallback: pw-dump with precise filter.smart.name check
        try:
            result = subprocess.run(
                ["pw-dump"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                import json

                data = json.loads(result.stdout)
                for obj in data:
                    if obj.get("type") == "PipeWire:Interface:Node":
                        props = obj.get("info", {}).get("props", {})
                        if props.get("filter.smart.name") == "big.filter-microphone":
                            return True
                return False
        except Exception:
            pass

        return False

    async def get_status_async(self) -> str:
        """
        Get noise reduction status asynchronously.

        Returns:
            str: 'enabled' if active, 'disabled' otherwise
        """
        return "enabled" if self.is_enabled() else "disabled"

    def get_status(self) -> str:
        """Get noise reduction status synchronously."""
        return "enabled" if self.is_enabled() else "disabled"

    # =========================================================================
    # Start/Stop Methods
    # =========================================================================

    async def start(self, settings=None) -> bool:
        """
        Start the noise reduction filter chain as a separate process.

        This runs 'pipewire -c filter-chain.conf' which loads the filter-chain
        configuration from ~/.config/pipewire/filter-chain.conf.d/ without
        interrupting the main PipeWire daemon.

        Args:
            settings: Optional AppSettings to sync from before starting

        Returns:
            bool: True if started successfully
        """
        if self._is_updating:
            logger.warning("Already updating, ignoring start request")
            return False

        self._is_updating = True
        try:
            if settings:
                self.sync_from_settings(settings)
            return await self._start_filter_chain_process()
        finally:
            self._is_updating = False

    async def _start_filter_chain_process(self) -> bool:
        """Internal: start the filter chain process and verify it's running.

        Does NOT check _is_updating — the caller is responsible for that.
        """
        try:
            # First, stop any existing filter-chain process
            await self._stop_filter_chain()

            # Ensure config directory exists and generate config file
            self._config_file.parent.mkdir(parents=True, exist_ok=True)
            ensure_daemon_config()
            self._generate_config()

            # Start filter-chain as a separate background process
            subprocess.Popen(
                ["/usr/bin/pipewire", "-c", "filter-chain.conf"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )

            # Wait briefly for the filter-chain to initialize
            await asyncio.sleep(0.5)

            # Verify startup (retry for up to 3 seconds)
            for _ in range(6):
                await asyncio.sleep(0.5)
                if self.is_enabled():
                    logger.info("Noise reduction started via filter-chain process")
                    self._cached_node_id = None
                    await self._configure_filter_source()
                    return True

            logger.error(
                "Failed to start noise reduction - process not found after timeout"
            )
            if self._config_file.exists():
                self._config_file.unlink()
            return False

        except Exception:
            logger.exception("Error starting noise reduction")
            if self._config_file.exists():
                self._config_file.unlink()
            return

    async def _configure_filter_source(self) -> None:
        """Configure the filter source after startup."""
        # The filter source auto-connects in PipeWire
        logger.debug("Filter source configuration complete")

    async def stop(self) -> bool:
        """
        Stop the noise reduction filter chain.

        Kills the filter-chain process and removes the config file.

        Returns:
            bool: True if stopped successfully
        """
        if self._is_updating:
            logger.warning("Already updating, ignoring stop request")
            return False

        self._is_updating = True
        try:
            await self._stop_filter_chain()
            return not self.is_enabled()

        except Exception:
            logger.exception("Error stopping noise reduction")
            return False
        finally:
            self._is_updating = False

    async def _stop_filter_chain(self) -> None:
        """
        Internal method to stop filter chain by killing the process and removing config.

        Identifies our specific process via pw-dump node.name pattern
        (input.filter-chain-PID-N) to avoid killing unrelated filter chains.
        Falls back to config-dir matching if pw-dump fails.
        """
        pids_to_kill: set[str] = set()

        # Method 1: Extract PID from our node's name in PipeWire graph
        try:
            result = subprocess.run(
                ["pw-dump"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                import json
                import re

                data = json.loads(result.stdout)
                for obj in data:
                    if obj.get("type") == "PipeWire:Interface:Node":
                        props = obj.get("info", {}).get("props", {})
                        desc = props.get("node.description", "")
                        name = props.get("node.name", "")

                        if (
                            desc == "Noise Canceling Microphone"
                            and "filter-chain" in name
                        ):
                            # Extract PID from node.name like "input.filter-chain-80250-8"
                            match = re.search(r"filter-chain-(\d+)", name)
                            if match:
                                pids_to_kill.add(match.group(1))
        except Exception:
            logger.warning("pw-dump failed during stop, trying fallback")

        # Method 2: Fallback - find pipewire processes in our config dir
        if not pids_to_kill:
            try:
                result = subprocess.run(
                    ["pgrep", "-f", "pipewire.*filter-chain"],
                    capture_output=True,
                    text=True,
                    timeout=3,
                )
                if result.returncode == 0 and result.stdout.strip():
                    config_dir = str(self._config_file.parent)
                    for pid in result.stdout.strip().split("\n"):
                        pid = pid.strip()
                        if not pid:
                            continue
                        # Verify the process cwd points to our config dir
                        try:
                            cwd = Path(f"/proc/{pid}/cwd").resolve()
                            if config_dir in str(cwd):
                                pids_to_kill.add(pid)
                        except (OSError, PermissionError):
                            pass
            except Exception:
                logger.warning("Error finding filter-chain processes")

        for pid in pids_to_kill:
            try:
                subprocess.run(
                    ["kill", pid],
                    capture_output=True,
                    timeout=5,
                )
                logger.debug("Killed filter-chain process: %s", pid)
            except Exception:
                logger.warning("Failed to kill process %s", pid)

        # Remove the configuration file
        if self._config_file.exists():
            self._config_file.unlink()
            logger.debug("Removed config file: %s", self._config_file)

        # Invalidate cache
        self._cached_node_id = None

        # Brief wait for process termination
        await asyncio.sleep(0.2)
        logger.info("Noise reduction stopped")

    # =========================================================================
    # Noise Reduction Parameters
    # =========================================================================

    def get_model(self) -> NoiseModel:
        """Get current AI model."""
        return self._noise_reduction_model

    def set_model(self, model: NoiseModel) -> bool:
        """
        Set AI model with live update.

        Args:
            model: NoiseModel to use

        Returns:
            bool: True if update successful
        """
        self._noise_reduction_model = model

        if self.is_enabled():
            return self._update_param_live("ai:Model", float(model.value))
        return True

    def get_strength(self) -> float:
        """Get current noise reduction strength."""
        return self._noise_reduction_strength

    def set_strength(self, strength: float) -> bool:
        """
        Set noise reduction strength with live update.

        Args:
            strength: Value between 0.0 and 1.0

        Returns:
            bool: True if update successful
        """
        strength = max(0.0, min(1.0, strength))
        self._noise_reduction_strength = strength

        if self.is_enabled():
            return self._update_param_live("ai:Strength", strength)
        return True

    def get_speech_strength(self) -> float:
        """Get current speech strength."""
        return self._noise_reduction_speech_strength

    def set_speech_strength(self, value: float) -> bool:
        """Set speech strength with live update."""
        value = max(0.0, min(1.0, value))
        self._noise_reduction_speech_strength = value
        if self.is_enabled():
            return self._update_param_live("ai:SpeechStrength", value)
        return True

    def get_lookahead_ms(self) -> int:
        """Get current lookahead in ms."""
        return self._noise_reduction_lookahead_ms

    def set_lookahead_ms(self, value: int) -> bool:
        """Set lookahead with live update."""
        value = max(0, min(200, value))
        self._noise_reduction_lookahead_ms = value
        if self.is_enabled():
            return self._update_param_live("ai:LookaheadMs", float(value))
        return True

    def get_voice_enhance(self) -> float:
        """Get voice enhancement level."""
        return self._noise_reduction_voice_enhance

    def set_voice_enhance(self, value: float) -> bool:
        """Set voice enhancement level with live update."""
        value = max(0.0, min(1.0, value))
        self._noise_reduction_voice_enhance = value
        if self.is_enabled():
            return self._update_param_live("ai:VoiceEnhance", value)
        return True

    def get_model_blending(self) -> bool:
        return self._noise_reduction_model_blending

    def set_model_blending(self, enabled: bool) -> bool:
        self._noise_reduction_model_blending = enabled
        if self.is_enabled():
            return self._update_param_live("ai:ModelBlend", 1.0 if enabled else 0.0)
        return True

    # =========================================================================
    # Compressor Parameters
    # =========================================================================

    def set_compressor_enabled(self, enabled: bool) -> bool:
        """Enable or disable compressor (requires pipeline restart)."""
        return self._requires_restart()

    def set_compressor_intensity(self, intensity: float) -> bool:
        """Set compressor intensity with live parameter update."""
        from biglinux_microphone.config import CompressorConfig

        intensity = max(0.0, min(1.0, intensity))
        comp = CompressorConfig(enabled=True, intensity=intensity)

        if not self.is_enabled():
            return True

        success = True
        updates = {
            "compressor:Threshold level (dB)": comp.threshold_db,
            "compressor:Ratio (1:n)": comp.ratio,
            "compressor:Makeup gain (dB)": comp.makeup_gain_db,
            "compressor:Attack time (ms)": comp.attack_ms,
            "compressor:Release time (ms)": comp.release_ms,
            "compressor:Knee radius (dB)": comp.knee_db,
            "compressor:RMS/peak": comp.rms_peak,
        }
        for param, val in updates.items():
            if not self._update_param_live(param, val):
                success = False
        return success

    # =========================================================================
    # Gate Filter Parameters
    # =========================================================================

    def get_gate_enabled(self) -> bool:
        """Check if gate filter is enabled."""
        return self._gate_enabled

    def set_gate_enabled(self, enabled: bool) -> bool:
        """
        Enable or disable gate filter.

        Args:
            enabled: True to enable gate

        Returns:
            bool: True if update successful
        """
        self._gate_enabled = enabled
        return self._requires_restart()

    def get_gate_threshold(self) -> int:
        """Get current gate threshold in dB."""
        return self._gate_threshold_db

    def set_gate_threshold(self, threshold_db: int) -> bool:
        """
        Set gate threshold with live update.

        Args:
            threshold_db: Threshold in dB (-60 to 0)

        Returns:
            bool: True if update successful
        """
        threshold_db = max(-60, min(0, threshold_db))
        self._gate_threshold_db = threshold_db

        if self.is_enabled():
            return self._update_param_live("gate:Threshold (dB)", threshold_db)
        return True

    def get_gate_range(self) -> int:
        """Get current gate range in dB."""
        return self._gate_range_db

    def set_gate_range(self, range_db: int) -> bool:
        """
        Set gate range with live update.

        Args:
            range_db: Range in dB (-90 to 0)

        Returns:
            bool: True if update successful
        """
        range_db = max(-90, min(0, range_db))
        self._gate_range_db = range_db

        if self.is_enabled():
            return self._update_param_live("gate:Range (dB)", range_db)
        return True

    def get_gate_attack(self) -> float:
        """Get current gate attack in ms."""
        return self._gate_attack_ms

    def set_gate_attack(self, ms: float) -> bool:
        """Set gate attack with live update."""
        ms = max(0.1, min(500.0, ms))
        self._gate_attack_ms = ms
        if self.is_enabled():
            return self._update_param_live("gate:Attack (ms)", ms)
        return True

    def get_gate_hold(self) -> float:
        """Get current gate hold in ms."""
        return self._gate_hold_ms

    def set_gate_hold(self, ms: float) -> bool:
        """Set gate hold with live update."""
        ms = max(0.0, min(1000.0, ms))
        self._gate_hold_ms = ms
        if self.is_enabled():
            return self._update_param_live("gate:Hold (ms)", ms)
        return True

    def get_gate_release(self) -> float:
        """Get current gate release in ms."""
        return self._gate_release_ms

    def set_gate_release(self, ms: float) -> bool:
        """Set gate release with live update."""
        ms = max(0.0, min(1000.0, ms))
        self._gate_release_ms = ms
        if self.is_enabled():
            return self._update_param_live("gate:Decay (ms)", ms)
        return True

    def set_gate_intensity(self, intensity: float) -> bool:
        """Set gate intensity, updating all derived gate parameters live."""
        from biglinux_microphone.config import GateConfig

        gate = GateConfig(intensity=intensity)
        ok = True
        ok = self.set_gate_threshold(int(gate.threshold_db)) and ok
        ok = self.set_gate_range(int(gate.range_db)) and ok
        ok = self.set_gate_attack(gate.attack_ms) and ok
        ok = self.set_gate_hold(gate.hold_ms) and ok
        ok = self.set_gate_release(gate.release_ms) and ok
        return ok

    # =========================================================================
    # Stereo Enhancement Parameters
    # =========================================================================

    def get_stereo_enabled(self) -> bool:
        """Check if stereo enhancement is enabled."""
        return self._stereo_mode != StereoMode.MONO

    def set_stereo_enabled(self, enabled: bool) -> bool:
        """
        Enable or disable stereo enhancement.

        Args:
            enabled: True to enable stereo

        Returns:
            bool: True if update successful
        """
        if enabled:
            if self._stereo_mode == StereoMode.MONO:
                self._stereo_mode = StereoMode.DUAL_MONO
        else:
            self._stereo_mode = StereoMode.MONO

        # Stereo changes require restart
        return self._requires_restart()

    def get_stereo_mode(self) -> StereoMode:
        """Get current stereo mode."""
        return self._stereo_mode

    def set_stereo_mode(self, mode: StereoMode) -> bool:
        """
        Set stereo enhancement mode.

        Args:
            mode: StereoMode to use

        Returns:
            bool: True if update successful
        """
        self._stereo_mode = mode
        return self._requires_restart()

    def get_stereo_width(self) -> float:
        """Get stereo width (0-1)."""
        return self._stereo_width

    def set_stereo_width(self, width: float) -> bool:
        """
        Set stereo width with live update.

        Updates dependent parameters based on current mode:
        - SPATIAL: Updates GVerb room size, reverb time, damping, etc.
        - RADIO: Updates Compressor ratio, release, threshold, etc.
        - HAAS/DUAL_MONO: Updates Delay time (if applicable).

        Args:
            width: Width between 0.0 and 1.0

        Returns:
            bool: True if update successful
        """
        width = max(0.0, min(1.0, width))
        self._stereo_width = width

        if not self.is_enabled() or not self.get_stereo_enabled():
            return True

        mode = self._stereo_mode
        success = True

        if mode == StereoMode.RADIO:
            # Re-calculate Compressor params matching filter_chain.py
            ratio = 4.0 + (width * 6.0)
            threshold = -15.0 - (width * 10.0)
            release = 100.0 + (width * 100.0)
            makeup_gain = 6.0 + (width * 6.0)

            updates = {
                "compressor:Ratio (1:n)": ratio,
                "compressor:Threshold level (dB)": threshold,
                "compressor:Release time (ms)": release,
                "compressor:Makeup gain (dB)": makeup_gain,
            }

            for param, val in updates.items():
                if not self._update_param_live(param, val):
                    success = False

        elif mode == StereoMode.VOICE_CHANGER:
            # Pitch mapping: [0.0, 1.0] -> [0.5, 2.0]
            # P = 0.5 * 4^width
            pitch_coeff = 0.5 * (4.0**width)
            pitch_coeff = max(0.5, min(2.0, pitch_coeff))

            # Gain Compensation (mirrors filter_chain.py)
            gain_db = 5.0
            if pitch_coeff < 1.0:
                gain_db += (1.0 - pitch_coeff) * 20.0

            updates = {
                "pitch:Pitch co-efficient": pitch_coeff,
                "pitch_gain:Amps gain (dB)": gain_db,
            }

            for param, val in updates.items():
                if not self._update_param_live(param, val):
                    success = False

        elif mode == StereoMode.DUAL_MONO:
            # DUAL_MONO uses simple channel duplication without delay
            # No live parameters to update for this mode
            pass

        return success

    def get_crossfeed_enabled(self) -> bool:
        """Check if crossfeed is enabled."""
        return self._crossfeed_enabled

    def set_crossfeed_enabled(self, enabled: bool) -> bool:
        """
        Enable or disable crossfeed.

        Args:
            enabled: True to enable crossfeed

        Returns:
            bool: True if update successful
        """
        self._crossfeed_enabled = enabled
        return self._requires_restart()

    # =========================================================================
    # Equalizer Parameters
    # =========================================================================

    def get_eq_enabled(self) -> bool:
        """Check if equalizer is enabled."""
        return self._eq_enabled

    def set_eq_enabled(self, enabled: bool) -> bool:
        """
        Enable or disable equalizer.

        Args:
            enabled: True to enable EQ

        Returns:
            bool: True if update successful
        """
        self._eq_enabled = enabled
        return self._requires_restart()

    def get_eq_bands(self) -> list[float]:
        """Get current EQ band values."""
        return self._eq_bands.copy()

    def set_eq_bands(self, bands: list[float]) -> bool:
        """
        Set all EQ band values with live update.

        Args:
            bands: List of 10 band values in dB (-12 to +12)

        Returns:
            bool: True if update successful
        """
        if len(bands) != 10:
            logger.error("EQ requires exactly 10 bands")
            return False

        self._eq_bands = [max(-12.0, min(12.0, b)) for b in bands]

        # Live update if EQ is enabled and filter chain is running
        if self.is_enabled() and self._eq_enabled:
            success = True
            for i, value in enumerate(self._eq_bands):
                mbeq_index = EQ_BAND_TO_MBEQ_INDEX[i]
                param_name = f"eq:{MBEQ_PARAM_NAMES[mbeq_index]}"
                if not self._update_param_live(param_name, value):
                    success = False
            return success

        return True

    def set_eq_band(self, band_index: int, value_db: float) -> bool:
        """
        Set a single EQ band value with live update.

        Args:
            band_index: Band index (0-9)
            value_db: Value in dB (-12 to +12)

        Returns:
            bool: True if update successful
        """
        if not 0 <= band_index < 10:
            logger.error("Band index must be 0-9")
            return False

        value_db = max(-12.0, min(12.0, value_db))
        self._eq_bands[band_index] = value_db

        # Live update if EQ is enabled and filter chain is running
        if self.is_enabled() and self._eq_enabled:
            mbeq_index = EQ_BAND_TO_MBEQ_INDEX[band_index]
            param_name = f"eq:{MBEQ_PARAM_NAMES[mbeq_index]}"
            return self._update_param_live(param_name, value_db)

        return True

    # =========================================================================
    # Bluetooth Configuration
    # =========================================================================

    def get_bluetooth_autoswitch(self) -> bool:
        """Check if Bluetooth auto-switch to headset is enabled."""
        policy_file = (
            Path.home()
            / ".config"
            / "wireplumber"
            / "wireplumber.conf.d"
            / "11-bluetooth-policy.conf"
        )

        if not policy_file.exists():
            return False  # Default is disabled (no policy file)

        try:
            content = policy_file.read_text()
            return "false" not in content
        except OSError:
            return False

    def set_bluetooth_autoswitch(self, enabled: bool) -> bool:
        """
        Enable or disable Bluetooth auto-switch to headset profile.

        Args:
            enabled: True to enable auto-switch

        Returns:
            bool: True if update successful
        """
        policy_dir = Path.home() / ".config" / "wireplumber" / "wireplumber.conf.d"
        policy_file = policy_dir / "11-bluetooth-policy.conf"

        try:
            policy_dir.mkdir(parents=True, exist_ok=True)

            value = "true" if enabled else "false"
            content = f"""wireplumber.settings = {{
    bluetooth.autoswitch-to-headset-profile = {value}
}}
"""
            policy_file.write_text(content)

            # WirePlumber automatically monitors wireplumber.conf.d/ directory
            # and reloads configuration files without requiring a restart
            logger.info("Bluetooth autoswitch set to: %s (no restart needed)", enabled)
            return True

        except Exception:
            logger.exception("Error setting Bluetooth autoswitch")
            return False

    # =========================================================================
    # Private Methods
    # =========================================================================

    def _get_filter_chain_node_id(self) -> str | None:
        """Get the filter-chain input node ID for live parameter updates.

        Searches for a node with our description and 'input.' prefix,
        since LADSPA control params are on the input side.
        """
        if self._cached_node_id:
            # Validate cache: ensure the node still exists
            try:
                result = subprocess.run(
                    ["pw-cli", "info", self._cached_node_id],
                    capture_output=True,
                    text=True,
                    timeout=3,
                )
                if (
                    result.returncode != 0
                    or "Noise Canceling Microphone" not in result.stdout
                ):
                    self._cached_node_id = None
            except Exception:
                self._cached_node_id = None

        if self._cached_node_id:
            return self._cached_node_id

        try:
            result = subprocess.run(
                ["pw-dump"],
                capture_output=True,
                text=True,
                timeout=5,
            )

            if result.returncode != 0:
                return None

            import json

            data = json.loads(result.stdout)

            for obj in data:
                if obj.get("type") == "PipeWire:Interface:Node":
                    props = obj.get("info", {}).get("props", {})
                    desc = props.get("node.description", "")
                    name = props.get("node.name", "")

                    # Input node has LADSPA controls; match by our description
                    if desc == "Noise Canceling Microphone" and name.startswith(
                        "input."
                    ):
                        self._cached_node_id = str(props.get("object.id", ""))
                        return self._cached_node_id

            return None

        except Exception:
            logger.exception("Error getting filter-chain node ID")
            return None

    def _worker_loop(self) -> None:
        """Worker loop for processing live updates."""
        while True:
            try:
                # Get item with blocking
                item = self._update_queue.get()

                # Simple debouncing/conflation:
                # If there are more items in the queue for the same parameter,
                # skip this one and take the latest.
                # Note: This is a simple heuristic. Ideally we'd map param -> latest_val

                param_name, value = item

                # Check if there are other updates for the same param awaiting
                # This requires peeking or draining, which Queue doesn't support easily for random access.
                # A simpler approach: process it.
                # To implement true conflation, we'd need a custom queue or logic here.
                # For now, let's just process. The queue naturally serializes.
                # But to avoid lag with many scroll events:

                # Optimization: Clear duplicate pending updates for same param
                # We can't easily iterate the Queue.
                # Instead, we perform the update.

                self._perform_live_update(param_name, value)
                self._update_queue.task_done()

            except Exception:
                logger.exception("Error in worker loop")

    def _update_param_live(self, param_name: str, value: float | int) -> bool:
        """
        Update a parameter live without restarting the filter chain.
        Now queues the update to non-blocking worker thread.

        Args:
            param_name: Full parameter name (e.g., "ai:Strength")
            value: New value

        Returns:
            bool: True (assumed successful due to async nature)
        """
        self._update_queue.put((param_name, value))
        return True

    def _perform_live_update(self, param_name: str, value: float | int) -> bool:
        """
        Actual implementation of live update (runs in worker thread).
        """
        node_id = self._get_filter_chain_node_id()

        if not node_id:
            logger.warning("Could not find filter-chain node for live update")
            return False

        try:
            # Force float for all number values to ensure correct type parsing in pw-cli
            val_float = float(value)
            params_str = f'{{ params = [ "{param_name}" {val_float} ] }}'

            result = subprocess.run(
                ["pw-cli", "set-param", node_id, "Props", params_str],
                capture_output=True,
                text=True,
                timeout=5,
            )

            if result.returncode == 0:
                logger.debug("Live update: %s = %s", param_name, value)
                return True
            else:
                logger.warning("Live update failed: %s", result.stderr)
                # If failed, maybe node ID changed? Invalidate cache
                self._cached_node_id = None
                return False

        except Exception:
            logger.exception("Error in live parameter update")
            return False

    def _requires_restart(self) -> bool:
        """
        Indicate that changes require filter chain restart.

        Returns:
            bool: Always True (caller should restart)
        """
        logger.info("Configuration change requires restart")
        return True

    def _generate_config(self, settings=None) -> None:
        """Generate and save the filter chain configuration file.

        Args:
            settings: Optional AppSettings to use. If None, loads from file.
        """
        # CRITICAL: Always load latest settings from file to ensure consistency
        if settings is None:
            from biglinux_microphone.config import load_settings

            settings = load_settings()
            logger.debug("Loaded settings from file for config generation")

        # Determine stereo mode - use MONO if stereo enhancement is disabled
        stereo_mode = (
            settings.stereo.mode if settings.stereo.enabled else StereoMode.MONO
        )
        logger.info(
            "Generating config: stereo.enabled=%s, stereo.mode=%s, effective_mode=%s",
            settings.stereo.enabled,
            settings.stereo.mode,
            stereo_mode,
        )

        # Use settings directly instead of internal state
        config = FilterChainConfig(
            hpf_enabled=settings.hpf.enabled,
            hpf_frequency=settings.hpf.frequency,
            noise_reduction_enabled=settings.noise_reduction.enabled,
            noise_reduction_model=settings.noise_reduction.model,
            noise_reduction_strength=settings.noise_reduction.strength,
            noise_reduction_speech_strength=settings.noise_reduction.speech_strength,
            noise_reduction_lookahead_ms=settings.noise_reduction.lookahead_ms,
            noise_reduction_voice_enhance=settings.noise_reduction.voice_enhance,
            noise_reduction_model_blending=settings.noise_reduction.model_blending,
            compressor_enabled=settings.compressor.enabled,
            compressor_threshold_db=settings.compressor.threshold_db,
            compressor_ratio=settings.compressor.ratio,
            compressor_attack_ms=settings.compressor.attack_ms,
            compressor_release_ms=settings.compressor.release_ms,
            compressor_makeup_gain_db=settings.compressor.makeup_gain_db,
            compressor_knee_db=settings.compressor.knee_db,
            compressor_rms_peak=settings.compressor.rms_peak,
            gate_enabled=settings.gate.enabled,
            gate_threshold_db=int(settings.gate.threshold_db),
            gate_range_db=int(settings.gate.range_db),
            gate_attack_ms=settings.gate.attack_ms,
            gate_hold_ms=settings.gate.hold_ms,
            gate_release_ms=settings.gate.release_ms,
            stereo_mode=stereo_mode,
            stereo_width=settings.stereo.width,
            crossfeed_enabled=settings.stereo.crossfeed_enabled,
            eq_enabled=settings.equalizer.enabled,
            eq_bands=list(settings.equalizer.bands),
        )

        generator = FilterChainGenerator(config)
        generator.save(self._config_file)
        logger.info("Generated filter chain config: %s", self._config_file)

    def apply_config(
        self, settings=None, on_complete: Callable[[], None] | None = None
    ) -> None:
        """Apply current configuration (requires restart).

        Args:
            settings: Optional AppSettings to sync from before applying
            on_complete: Optional callback to run after restart finishes
        """
        # Sync from settings if provided
        if settings:
            self.sync_from_settings(settings)

        # Generate new config file
        self._generate_config(settings)

        # Always restart if noise reduction should be active.
        # We check settings (desired state) instead of is_enabled() (runtime state)
        # to avoid race conditions during pipeline transitions.
        should_be_active = (
            settings.noise_reduction.enabled
            if settings
            else self._noise_reduction_enabled
        )

        if should_be_active:
            if self._is_updating:
                # A restart is already in progress; mark pending so the
                # current restart's callback will trigger another one.
                self._restart_pending = True
                self._pending_settings = settings
                self._pending_on_complete = on_complete
                logger.debug("Restart already in progress, queuing pending restart")
            else:
                self._run_restart_in_thread(on_complete)
        elif on_complete:
            from gi.repository import GLib

            GLib.idle_add(on_complete)

    def _run_restart_in_thread(
        self, on_complete: Callable[[], None] | None = None
    ) -> None:
        """Run restart in a separate thread to avoid blocking GTK."""
        self._is_updating = True
        self._restart_pending = False

        def _do_restart() -> None:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(self._restart())
            finally:
                loop.close()
                self._is_updating = False

            # Check if another restart was requested while we were restarting
            if self._restart_pending:
                pending_settings = getattr(self, "_pending_settings", None)
                pending_callback = getattr(self, "_pending_on_complete", None)
                self._restart_pending = False
                self._pending_settings = None
                self._pending_on_complete = None
                logger.debug("Executing pending restart after previous completed")
                # Regenerate config with latest settings and restart again
                self._generate_config(pending_settings)
                self._run_restart_in_thread(pending_callback)
                return  # Don't call on_complete for the superseded restart

            if on_complete:
                try:
                    from gi.repository import GLib

                    GLib.idle_add(on_complete)
                except ImportError:
                    pass

        thread = threading.Thread(target=_do_restart, daemon=True)
        thread.start()

    async def _restart(self) -> None:
        """Restart the filter chain with new configuration.

        Uses internal methods directly to avoid _is_updating guard,
        since the caller (_run_restart_in_thread) manages that flag.
        """
        await self._stop_filter_chain()
        await asyncio.sleep(0.5)
        await self._start_filter_chain_process()
