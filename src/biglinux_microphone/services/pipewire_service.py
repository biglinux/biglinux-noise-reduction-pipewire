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

        # Live update queue
        self._update_queue = Queue()
        self._cached_node_id: str | None = None

        # Start worker thread
        self._worker_thread = threading.Thread(target=self._worker_loop, daemon=True)
        self._worker_thread.start()

        # Internal state tracking
        self._noise_reduction_enabled = True
        self._noise_reduction_model = NoiseModel.GTCRN_LOW_LATENCY
        self._noise_reduction_strength = 1.0
        self._gate_enabled = True
        self._gate_threshold_db = -36
        self._gate_range_db = -60
        self._gate_attack_ms = 10.0
        self._gate_hold_ms = 300.0
        self._gate_release_ms = 150.0
        self._stereo_mode = StereoMode.MONO
        self._stereo_width = 0.7

        self._crossfeed_enabled = False
        self._eq_enabled = False
        self._eq_bands = [0.0] * 10

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

        self._eq_bands = list(settings.equalizer.bands)
        logger.debug("Synchronized state from settings")

    # =========================================================================
    # Status Methods
    # =========================================================================

    def is_enabled(self) -> bool:
        """
        Check if noise reduction filter-chain is currently active.

        Checks multiple indicators:
        1. The separate pipewire filter-chain process
        2. The filter-chain source exists in PipeWire
        3. Config file exists as fallback
        """
        # Method 1: Check for pipewire filter-chain process
        try:
            result = subprocess.run(
                ["pgrep", "-f", "/usr/bin/pipewire -c filter-chain.conf"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                return True
        except Exception:
            pass

        # Method 2: Check if filter source exists in PipeWire
        try:
            result = subprocess.run(
                ["pactl", "list", "sources", "short"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                output = result.stdout.lower()
                if "big.filter-microphone" in output or "filter-chain" in output:
                    return True
        except Exception:
            pass

        # Fallback: check if config file exists
        return self._config_file.exists()

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
            # First, stop any existing filter-chain process
            await self._stop_filter_chain()

            # Sync from settings if provided
            if settings:
                self.sync_from_settings(settings)

            # Ensure config directory exists and generate config file
            self._config_file.parent.mkdir(parents=True, exist_ok=True)
            self._ensure_daemon_config()
            self._generate_config()

            # Start filter-chain as a separate background process
            # This does NOT restart the main PipeWire daemon
            # Using subprocess.Popen for background process
            subprocess.Popen(
                ["/usr/bin/pipewire", "-c", "filter-chain.conf"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,  # Detach from parent
            )

            # Wait briefly for the filter-chain to initialize
            await asyncio.sleep(0.5)

            # Verify startup (retry for up to 3 seconds)
            for _ in range(6):
                await asyncio.sleep(0.5)
                if self.is_enabled():
                    logger.info("Noise reduction started via filter-chain process")
                    # Invalidate cache on new start
                    self._cached_node_id = None
                    # Ensure filter source is available
                    await self._configure_filter_source()
                    return True

            logger.error(
                "Failed to start noise reduction - process not found after timeout"
            )
            return False

        except Exception:
            logger.exception("Error starting noise reduction")
            return False
        finally:
            self._is_updating = False

    async def _configure_filter_source(self) -> None:
        """Configure the filter source after startup."""
        # The filter source auto-connects in PipeWire
        logger.debug("Filter source configuration complete")

    def _ensure_daemon_config(self) -> None:
        """
        Ensure ~/.config/pipewire/filter-chain.conf exists with correct realtime priority.

        This main config file is required by the separate 'pipewire -c filter-chain.conf'
        process to ensure it runs with correct RT priority (83), preventing choppy audio.
        """
        config_path = Path.home() / ".config" / "pipewire" / "filter-chain.conf"

        # Check if file exists and has correct priority
        valid = False
        if config_path.exists():
            try:
                content = config_path.read_text()
                if "rt.prio" in content and "83" in content:
                    valid = True
            except OSError:
                pass

        if not valid:
            logger.info("Updating %s with correct RT priority", config_path)
            content = """# BigLinux Microphone Filter Chain RT Config
# Required for correct process priority to avoid audio stuttering

context.properties = {
    ## Properties for the PipeWire daemon.
    #module.x11.bell = false
}

context.modules = [
    { name = libpipewire-module-rt
        args = {
            rt.prio      = 83
            nice.level   = -11
            rt.time.soft = -1
            rt.time.hard = -1
        }
        flags = [ ifexists nofail ]
    },
    { name = libpipewire-module-protocol-native },
    { name = libpipewire-module-client-node },
    { name = libpipewire-module-adapter }
]
"""
            try:
                config_path.parent.mkdir(parents=True, exist_ok=True)
                config_path.write_text(content)
            except OSError:
                logger.error("Failed to write RT config to %s", config_path)

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

        This does NOT restart the main PipeWire daemon.
        """
        # Find and kill any running pipewire filter-chain processes
        try:
            result = subprocess.run(
                ["pgrep", "-f", "/usr/bin/pipewire -c filter-chain.conf"],
                capture_output=True,
                text=True,
            )
            if result.returncode == 0 and result.stdout.strip():
                for pid in result.stdout.strip().split("\n"):
                    if pid:
                        try:
                            subprocess.run(
                                ["kill", pid],
                                capture_output=True,
                                timeout=5,
                            )
                            logger.debug("Killed filter-chain process: %s", pid)
                        except Exception:
                            logger.warning("Failed to kill process %s", pid)
        except Exception:
            logger.warning("Error finding filter-chain processes")

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
        """Get the filter-chain node ID for live parameter updates."""
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
                    name = props.get("node.name", "")

                    if "big.filter-microphone" in name or "filter-chain" in name:
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
            param_name: Full parameter name (e.g., "gtcrn:Strength")
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
            noise_reduction_enabled=settings.noise_reduction.enabled,
            noise_reduction_model=settings.noise_reduction.model,
            noise_reduction_strength=settings.noise_reduction.strength,
            gate_enabled=settings.gate.enabled,
            gate_threshold_db=settings.gate.threshold_db,
            gate_range_db=settings.gate.range_db,
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

        if self.is_enabled():
            self._run_restart_in_thread(on_complete)
        elif on_complete:
            # If not enabled, run callback immediately (or next tick)
            from gi.repository import GLib

            GLib.idle_add(on_complete)

    def _run_restart_in_thread(
        self, on_complete: Callable[[], None] | None = None
    ) -> None:
        """Run restart in a separate thread to avoid blocking GTK."""

        def _do_restart() -> None:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(self._restart())
            finally:
                loop.close()

            if on_complete:
                try:
                    from gi.repository import GLib

                    GLib.idle_add(on_complete)
                except ImportError:
                    pass

        thread = threading.Thread(target=_do_restart, daemon=True)
        thread.start()

    async def _restart(self) -> None:
        """Restart the filter chain with new configuration."""
        await self.stop()
        await asyncio.sleep(0.5)
        await self.start()
