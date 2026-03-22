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
import contextlib
import logging
import os
import signal
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
def _safe_parse_pw_dump(raw: str) -> list | None:
    """Parse pw-dump JSON output, handling truncated/malformed data.

    PipeWire's pw-dump can produce invalid JSON when the graph changes
    during the dump. This function attempts a full parse first, then
    falls back to truncation at the last complete array element.

    Returns:
        Parsed list of objects, or None on failure.
    """
    import json

    try:
        return json.loads(raw)
    except json.JSONDecodeError as e:
        # Try to recover partial data by truncating at the last complete entry
        truncated = raw[: e.pos]
        last_brace = truncated.rfind("}")
        if last_brace > 0:
            try:
                return json.loads(truncated[: last_brace + 1] + "\n]")
            except json.JSONDecodeError:
                pass
        logger.debug("pw-dump produced unparseable JSON (pos=%d)", e.pos)
        return None
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
        self._restart_lock = threading.Lock()

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
        self._noise_reduction_lookahead_ms = 50
        self._noise_reduction_model_blending = 0.0
        self._noise_reduction_voice_recovery = 0.75
        self._gate_enabled = True
        self._gate_threshold_db = -30
        self._gate_range_db = -60
        self._gate_attack_ms = 10.0
        self._gate_hold_ms = 400.0
        self._gate_release_ms = 200.0
        self._stereo_mode = StereoMode.MONO
        self._stereo_width = 0.7

        self._crossfeed_enabled = False
        self._hpf_enabled = True
        self._hpf_frequency = 80.0
        self._eq_enabled = True
        self._eq_bands = [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0]

        # Echo cancellation state
        self._echo_cancel_enabled = False
        self._current_hw_source: str = ""
        self._source_monitor_id: int = 0
        self._gain_monitor_id: int = 0

        # AGC (filter chain LADSPA compressor node)
        self._agc_enabled = True
        self._agc_target_level_dbfs = 70
        self._agc_process: subprocess.Popen | None = None

        # Filter-chain process tracking
        self._filter_chain_process: subprocess.Popen | None = None

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
        self._noise_reduction_speech_strength = settings.noise_reduction.speech_strength
        self._noise_reduction_lookahead_ms = settings.noise_reduction.lookahead_ms
        self._noise_reduction_model_blending = settings.noise_reduction.model_blending
        self._noise_reduction_voice_recovery = settings.noise_reduction.voice_recovery
        self._gate_enabled = settings.gate.enabled
        self._gate_threshold_db = settings.gate.threshold_db
        self._gate_range_db = settings.gate.range_db
        self._gate_attack_ms = settings.gate.attack_ms
        self._gate_hold_ms = settings.gate.hold_ms
        self._gate_release_ms = settings.gate.release_ms
        self._stereo_mode = settings.stereo.mode
        self._stereo_width = settings.stereo.width

        self._crossfeed_enabled = settings.stereo.crossfeed_enabled
        self._hpf_enabled = settings.hpf.enabled
        self._hpf_frequency = settings.hpf.frequency
        self._eq_enabled = settings.equalizer.enabled
        self._eq_bands = list(settings.equalizer.bands)

        # Echo cancellation
        self._echo_cancel_enabled = settings.echo_cancel.enabled

        # AGC settings (filter chain node)
        self._agc_enabled = settings.agc.enabled
        self._agc_target_level_dbfs = settings.agc.target_level_dbfs

        logger.debug("Synchronized state from settings")

    # =========================================================================
    # Status Methods
    # =========================================================================

    def is_enabled(self) -> bool:
        """
        Check if noise reduction filter-chain is currently active.

        Uses pw-cli to query PipeWire graph for our specific filter node
        identified by stable internal names (filter.smart.name or node.name).
        Falls back to pw-dump with filter.smart.name check.
        """
        # Primary: lightweight check via pw-cli list-objects
        try:
            result = subprocess.run(
                ["pw-cli", "list-objects"],
                capture_output=True,
                text=True,
                timeout=2,
            )
            if result.returncode == 0:
                out = result.stdout
                return (
                    "big.filter-microphone" in out
                    or "big-noise-canceling-output" in out
                )
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
                data = _safe_parse_pw_dump(result.stdout)
                if data is not None:
                    for obj in data:
                        if obj.get("type") == "PipeWire:Interface:Node":
                            props = obj.get("info", {}).get("props", {})
                            if (
                                props.get("filter.smart.name")
                                == "big.filter-microphone"
                            ):
                                return True
                return False
        except Exception:
            pass

        return False

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
        with self._restart_lock:
            if self._is_updating:
                logger.warning("Already updating, ignoring start request")
                return False
            self._is_updating = True
        try:
            if settings:
                self.sync_from_settings(settings)
            return await self._start_filter_chain_process(settings)
        finally:
            with self._restart_lock:
                self._is_updating = False

    async def _start_filter_chain_process(self, settings=None) -> bool:
        """Internal: start the filter chain process and verify it's running.

        Args:
            settings: Optional AppSettings to use for config generation.
                      When provided, avoids loading from disk (prevents race conditions).

        Does NOT check _is_updating — the caller is responsible for that.
        """
        import time as _time

        try:
            t0 = _time.monotonic()
            # First, stop any existing filter-chain process
            await self._stop_filter_chain()
            t1 = _time.monotonic()
            logger.info("[TIMING] stop_filter_chain: %.3fs", t1 - t0)

            # Ensure config directory exists and generate config file
            self._config_file.parent.mkdir(parents=True, exist_ok=True)
            ensure_daemon_config()
            self._generate_config(settings)
            t2 = _time.monotonic()
            logger.info("[TIMING] generate_config: %.3fs", t2 - t1)

            # Start filter-chain as a separate background process, capture PID
            try:
                proc = subprocess.Popen(
                    ["/usr/bin/pipewire", "-c", "filter-chain.conf"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
            except FileNotFoundError:
                logger.error("pipewire binary not found at /usr/bin/pipewire")
                return False
            self._filter_chain_process = proc
            pid = proc.pid
            t3 = _time.monotonic()
            logger.info("[TIMING] popen: %.3fs", t3 - t2)

            # Wait for filter to register in PipeWire graph.
            # Uses process-aware polling: if the process dies, fail immediately
            # instead of waiting the full timeout.
            max_wait = 5.0
            poll_interval = 0.15
            elapsed = 0.0
            await asyncio.sleep(0.1)
            elapsed += 0.1

            while elapsed < max_wait:
                # Check if process is still alive
                try:
                    os.kill(pid, 0)
                except OSError:
                    logger.error(
                        "Filter-chain process (PID %d) died during startup", pid
                    )
                    break

                tp = _time.monotonic()
                enabled = self.is_enabled()
                tp2 = _time.monotonic()
                logger.debug("[TIMING] is_enabled() = %s (%.3fs)", enabled, tp2 - tp)

                if enabled:
                    t4 = _time.monotonic()
                    logger.info(
                        "[TIMING] filter detected after %.3fs total (poll: %.3fs)",
                        t4 - t0,
                        elapsed,
                    )
                    self._cached_node_id = None
                    await self._configure_filter_source()
                    t5 = _time.monotonic()
                    logger.info(
                        "[TIMING] configure_filter_source: %.3fs, TOTAL: %.3fs",
                        t5 - t4,
                        t5 - t0,
                    )
                    return True

                await asyncio.sleep(poll_interval)
                elapsed += poll_interval

            logger.error(
                "Failed to start noise reduction - not detected after %.1fs", elapsed
            )
            if self._config_file.exists():
                self._config_file.unlink()
            return False

        except Exception:
            logger.exception("Error starting noise reduction")
            if self._config_file.exists():
                self._config_file.unlink()
            return False

    async def _configure_filter_source(self) -> None:
        """Configure the filter source after startup.

        When echo cancellation is active, filter.smart is disabled so the
        node must be explicitly set as the default audio source.
        Also sets force-quantum=960 to align with WebRTC AEC frame size.
        Limits ALSA mic boost and capture volume to prevent ADC clipping.
        Starts monitoring for default source changes.
        """
        self._optimize_mic_gain()
        self._start_gain_monitor()
        if self._echo_cancel_enabled:
            await self._set_default_source("big-noise-canceling-output")
            self._set_pipewire_quantum(960)
            self._current_hw_source = self._detect_hardware_source()
            self._start_source_monitor()
        if self._agc_enabled:
            self._start_agc_process()
        logger.debug("Filter source configuration complete")

    def ensure_gain_monitoring(self) -> None:
        """Ensure gain monitoring is active when filter is already running.

        Called by the UI when it detects the filter is already enabled
        at startup (e.g., started by CLI/systemd before GUI opened).
        """
        if self.is_enabled() and not self._gain_monitor_id:
            self._optimize_mic_gain()
            self._start_gain_monitor()

    def _start_source_monitor(self) -> None:
        """Start periodic check for default source changes.

        When AEC is active, polls every 3 seconds to detect if the user
        switched the default microphone. If changed, triggers a pipeline
        restart so the AEC re-targets the new hardware source.
        """
        self._stop_source_monitor()
        try:
            from gi.repository import GLib

            self._source_monitor_id = GLib.timeout_add_seconds(
                3, self._check_source_changed
            )
        except ImportError:
            pass

    def _stop_source_monitor(self) -> None:
        """Stop the source change monitor."""
        if self._source_monitor_id:
            try:
                from gi.repository import GLib

                GLib.source_remove(self._source_monitor_id)
            except ImportError:
                pass
            self._source_monitor_id = 0

    def _start_gain_monitor(self) -> None:
        """Start periodic mic boost check (every 30s)."""
        self._stop_gain_monitor()
        try:
            from gi.repository import GLib

            self._gain_monitor_id = GLib.timeout_add_seconds(
                30, self._gain_monitor_tick
            )
        except ImportError:
            pass

    def _stop_gain_monitor(self) -> None:
        """Stop the gain monitor timer."""
        if self._gain_monitor_id:
            try:
                from gi.repository import GLib

                GLib.source_remove(self._gain_monitor_id)
            except ImportError:
                pass
            self._gain_monitor_id = 0

    def _gain_monitor_tick(self) -> bool:
        """Periodic mic boost check.

        Verifies mic boost hasn't been changed externally.
        Capture volume is managed by the WebRTC AGC pipeline.
        Returns True to keep the timer running.
        """
        if not self.is_enabled():
            self._gain_monitor_id = 0
            return False

        def _work() -> None:
            self._optimize_mic_gain()

        thread = threading.Thread(target=_work, daemon=True)
        thread.start()
        return True

    def _check_source_changed(self) -> bool:
        """Check if the default hardware source changed.

        Called periodically by GLib timer. Runs detection in a thread
        to avoid blocking the UI.

        Returns:
            True to keep the timer running, False to stop.
        """
        if not self._echo_cancel_enabled:
            self._source_monitor_id = 0
            return False

        def _check() -> None:
            current = self._detect_hardware_source()
            if not current or current == self._current_hw_source:
                return

            logger.info(
                "Default source changed: %s -> %s, restarting pipeline",
                self._current_hw_source,
                current,
            )
            self._current_hw_source = current

            from biglinux_microphone.config import load_settings

            settings = load_settings()
            self.apply_config(settings)

        thread = threading.Thread(target=_check, daemon=True)
        thread.start()
        return True

    def _detect_hardware_source(self) -> str:
        """Detect the hardware microphone node name via pactl.

        Returns the current default source name (before our filter chain
        replaces it). This is used as target.object for the AEC capture
        to ensure it reads from the physical mic, not the filter output.
        """
        try:
            result = subprocess.run(
                ["pactl", "get-default-source"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                name = result.stdout.strip()
                if name and not name.startswith("big-"):
                    logger.info("Detected hardware source: %s", name)
                    return name
        except Exception:
            logger.warning("Failed to detect hardware source")
        return ""

    async def _set_default_source(self, node_name: str) -> None:
        """Set a node as the default audio source by name.

        Uses pw-cli ls Node (fast text parsing) instead of pw-dump (slow JSON).
        """
        try:
            result = subprocess.run(
                ["pw-cli", "ls", "Node"],
                capture_output=True,
                text=True,
                timeout=3,
            )
            if result.returncode != 0:
                logger.warning("pw-cli ls Node failed when setting default source")
                return

            # Parse pw-cli output: lines like "	id 42, type ..." and properties
            current_id = ""
            for line in result.stdout.splitlines():
                stripped = line.strip()
                if stripped.startswith("id "):
                    # Extract node ID: "id 42, type PipeWire:Interface:Node/3"
                    parts = stripped.split(",", 1)
                    current_id = parts[0].replace("id ", "").strip()
                elif "node.name" in stripped and f'"{node_name}"' in stripped:
                    if current_id:
                        subprocess.run(
                            ["wpctl", "set-default", current_id],
                            capture_output=True,
                            timeout=3,
                        )
                        logger.info(
                            "Set default source to %s (id=%s)",
                            node_name,
                            current_id,
                        )
                        return
            logger.warning("Could not find node %s to set as default", node_name)
        except Exception:
            logger.exception("Error setting default source")

    @staticmethod
    def _find_alsa_input_card() -> str | None:
        """Find the ALSA card number for the hardware input source.

        Uses pw-cli ls Node (fast) to search for ALSA input source
        and extract its card number from the properties.
        """
        try:
            result = subprocess.run(
                ["pw-cli", "ls", "Node"],
                capture_output=True,
                text=True,
                timeout=2,
            )
            if result.returncode != 0:
                return None

            current_id = ""
            for line in result.stdout.splitlines():
                stripped = line.strip()
                if stripped.startswith("id "):
                    parts = stripped.split(",", 1)
                    current_id = parts[0].replace("id ", "").strip()
                elif "alsa_input" in stripped and "node.name" in stripped:
                    # Found an ALSA input node, query its properties
                    if current_id:
                        info = subprocess.run(
                            ["pw-cli", "info", current_id],
                            capture_output=True,
                            text=True,
                            timeout=2,
                        )
                        if info.returncode == 0:
                            for prop_line in info.stdout.splitlines():
                                if "api.alsa.pcm.card" in prop_line:
                                    # Format: '  *    api.alsa.pcm.card = "0"'
                                    parts = prop_line.split("=", 1)
                                    if len(parts) == 2:
                                        val = parts[1].strip().strip('"')
                                        if val.isdigit():
                                            return val
        except Exception:
            pass
        return None

    @staticmethod
    def _find_alsa_input_name() -> str | None:
        """Find the ALSA input node name from PipeWire (e.g. alsa_input.pci-...).

        Uses pw-cli ls Node (fast) instead of pw-dump.
        """
        try:
            result = subprocess.run(
                ["pw-cli", "ls", "Node"],
                capture_output=True,
                text=True,
                timeout=2,
            )
            if result.returncode != 0:
                return None
            for line in result.stdout.splitlines():
                stripped = line.strip()
                if "node.name" in stripped and "alsa_input" in stripped:
                    # Format: 'node.name = "alsa_input.pci-0000_00_1f.3..."'
                    parts = stripped.split("=", 1)
                    if len(parts) == 2:
                        val = parts[1].strip().strip('"')
                        if val.startswith("alsa_input"):
                            return val
        except Exception:
            pass
        return None

    @staticmethod
    def _optimize_mic_gain(
        max_boost: int = 1,
        capture_min: int = 30,
        capture_max: int = 100,
    ) -> None:
        """Optimize ALSA mic gain staging to prevent ADC clipping.

        Internal laptop mics with high boost (+20/+30dB) combined with
        max capture gain (+30dB) can clip the 16-bit ADC, producing
        robotic/distorted audio through the filter chain.

        Detects the ALSA card from PipeWire and:
        1. Caps any 'Mic Boost' controls to max_boost (default=1 = +10dB)
        2. Clamps capture volume to [capture_min, capture_max] range.
           Only adjusts if capture is outside this range — leaves the
           user's preferred level untouched otherwise.
        """
        try:
            card_num = PipeWireService._find_alsa_input_card()
            if card_num is None:
                return

            # Use scontrols for simple mixer names (usable with sget/sset)
            result = subprocess.run(
                ["amixer", "-c", card_num, "scontrols"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode != 0:
                return

            for ctrl_line in result.stdout.splitlines():
                # Format: Simple mixer control 'Name',index
                start = ctrl_line.find("'")
                end = ctrl_line.rfind("'")
                if start < 0 or end <= start:
                    continue
                ctrl_name = ctrl_line[start + 1 : end]

                # Limit mic boost controls
                if "Mic Boost" in ctrl_name:
                    result2 = subprocess.run(
                        ["amixer", "-c", card_num, "sget", ctrl_name],
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    if result2.returncode != 0:
                        continue
                    for val_line in result2.stdout.splitlines():
                        if "Front Left:" in val_line:
                            parts = val_line.split()
                            try:
                                idx = parts.index("Left:")
                                current = int(parts[idx + 1])
                                if current > max_boost:
                                    subprocess.run(
                                        [
                                            "amixer",
                                            "-c",
                                            card_num,
                                            "sset",
                                            ctrl_name,
                                            str(max_boost),
                                        ],
                                        capture_output=True,
                                        timeout=5,
                                    )
                                    logger.info(
                                        "Limited %s from %d to %d on card %s",
                                        ctrl_name,
                                        current,
                                        max_boost,
                                        card_num,
                                    )
                            except (ValueError, IndexError):
                                pass
                            break

                # Clamp capture volume to [capture_min, capture_max]
                elif ctrl_name == "Capture":
                    result2 = subprocess.run(
                        ["amixer", "-c", card_num, "sget", "Capture"],
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    if result2.returncode != 0:
                        continue
                    # Parse current percentage
                    current_pct = -1
                    for val_line in result2.stdout.splitlines():
                        if "Front Left:" in val_line:
                            pct_start = val_line.find("[")
                            pct_end = val_line.find("%]")
                            if pct_start >= 0 and pct_end > pct_start:
                                with contextlib.suppress(ValueError):
                                    current_pct = int(val_line[pct_start + 1 : pct_end])
                            break
                    if current_pct < capture_min:
                        subprocess.run(
                            [
                                "amixer",
                                "-c",
                                card_num,
                                "sset",
                                "Capture",
                                f"{capture_min}%",
                            ],
                            capture_output=True,
                            timeout=5,
                        )
                        logger.info(
                            "Raised Capture volume from %d%% to %d%% on card %s",
                            current_pct,
                            capture_min,
                            card_num,
                        )
                    elif current_pct > capture_max:
                        subprocess.run(
                            [
                                "amixer",
                                "-c",
                                card_num,
                                "sset",
                                "Capture",
                                f"{capture_max}%",
                            ],
                            capture_output=True,
                            timeout=5,
                        )
                        logger.info(
                            "Lowered Capture volume from %d%% to %d%% on card %s",
                            current_pct,
                            capture_max,
                            card_num,
                        )

        except Exception:
            logger.debug("Could not optimize mic gain", exc_info=True)

    def _set_pipewire_quantum(self, quantum: int) -> None:
        """Set PipeWire force-quantum via pw-metadata.

        When echo cancellation is active, forces quantum=960 to align with
        WebRTC AEC's 480-sample frame size (960 = 2×480). This prevents
        buffer underruns caused by quantum mismatch (e.g. 4096 % 960 ≠ 0).
        Pass 0 to restore automatic quantum selection.
        """
        try:
            subprocess.run(
                [
                    "pw-metadata",
                    "-n",
                    "settings",
                    "0",
                    "clock.force-quantum",
                    str(quantum),
                ],
                capture_output=True,
                timeout=3,
            )
            logger.info("Set PipeWire force-quantum to %d", quantum)
        except Exception:
            logger.warning("Failed to set PipeWire force-quantum to %d", quantum)

    async def stop(self) -> bool:
        """
        Stop the noise reduction filter chain.

        Kills the filter-chain process and removes the config file.

        Returns:
            bool: True if stopped successfully
        """
        with self._restart_lock:
            if self._is_updating:
                logger.warning("Already updating, ignoring stop request")
                return False
            self._is_updating = True
        try:
            self._stop_agc_process()
            await self._stop_filter_chain()
            return not self.is_enabled()

        except Exception:
            logger.exception("Error stopping noise reduction")
            return False
        finally:
            with self._restart_lock:
                self._is_updating = False

    async def _stop_filter_chain(self) -> None:
        """
        Internal method to stop filter chain by killing the process and removing config.

        Uses three methods in order:
        1. Kill the tracked process reference (most reliable)
        2. Kill any orphaned 'pipewire -c filter-chain.conf' processes via pgrep
        3. Remove the config file
        """
        # Method 1: Kill tracked process
        proc = self._filter_chain_process
        if proc is not None:
            try:
                proc.terminate()
                try:
                    proc.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=1)
                logger.debug("Killed tracked filter-chain process (PID %d)", proc.pid)
            except OSError:
                pass
            self._filter_chain_process = None

        # Method 2: Clean up any orphaned filter-chain processes
        # (from previous runs, crashes, or races)
        try:
            result = subprocess.run(
                ["pgrep", "-f", "pipewire.*filter-chain.conf"],
                capture_output=True,
                text=True,
                timeout=1,
            )
            if result.returncode == 0 and result.stdout.strip():
                for pid in result.stdout.strip().split("\n"):
                    pid = pid.strip()
                    if pid:
                        try:
                            os.kill(int(pid), signal.SIGTERM)
                            logger.debug(
                                "Killed orphaned filter-chain process: %s", pid
                            )
                        except (OSError, ValueError):
                            pass
        except Exception:
            logger.warning("Error cleaning up orphaned filter-chain processes")

        # Remove the configuration file
        if self._config_file.exists():
            self._config_file.unlink()
            logger.debug("Removed config file: %s", self._config_file)

        # Invalidate cache
        self._cached_node_id = None

        # Stop monitoring for source changes
        self._stop_source_monitor()
        self._stop_gain_monitor()

        # Restore default quantum (AEC may have forced it to 960)
        self._set_pipewire_quantum(0)

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

    def get_model_blending(self) -> float:
        return self._noise_reduction_model_blending

    def set_model_blending(self, value: float) -> bool:
        self._noise_reduction_model_blending = value
        if self.is_enabled():
            return self._update_param_live("ai:ModelBlend", value)
        return True

    def get_voice_recovery(self) -> float:
        """Get voice recovery level (HF band reconstruction)."""
        return self._noise_reduction_voice_recovery

    def set_voice_recovery(self, value: float) -> bool:
        """Set voice recovery level with live update."""
        value = max(0.0, min(1.0, value))
        self._noise_reduction_voice_recovery = value
        if self.is_enabled():
            return self._update_param_live("ai:VoiceRecovery", value)
        return True

    # =========================================================================
    # High-Pass Filter Parameters
    # =========================================================================

    def set_hpf_enabled(self, enabled: bool) -> bool:
        """Enable or disable HPF via live update.

        HPF node is always present in the filter chain.
        When disabled, frequency is set to 5 Hz (inaudible passthrough).
        When enabled, current frequency is restored.
        """
        self._hpf_enabled = enabled
        if self.is_enabled():
            freq = self._hpf_frequency if enabled else 5.0
            return self._update_param_live("hpf:Freq", freq)
        return True

    def set_hpf_frequency(self, freq: float) -> bool:
        """Set HPF frequency with live update."""
        self._hpf_frequency = freq
        if self.is_enabled() and self._hpf_enabled:
            return self._update_param_live("hpf:Freq", freq)
        return True

    # =========================================================================
    # Compressor Parameters
    # =========================================================================

    def set_compressor_enabled(self, enabled: bool) -> bool:  # noqa: ARG002
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
            "compressor:Attack time (ms)": comp.attack_ms,
            "compressor:Release time (ms)": comp.release_ms,
            "compressor:Knee radius (dB)": comp.knee_db,
            "compressor:RMS/peak": comp.rms_peak,
            "compressor:Makeup gain (dB)": comp.makeup_gain_db,
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
        - VOICE_CHANGER: Updates Pitch co-efficient and gain.
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

        if mode == StereoMode.VOICE_CHANGER:
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
        Enable or disable equalizer via live update.

        EQ node is always present in the filter chain (PipeWire 1.6.1 workaround).
        When disabled, all bands are set to 0 dB (passthrough) via pw-cli.
        When enabled, current band values are restored.

        Args:
            enabled: True to enable EQ

        Returns:
            bool: True if update successful
        """
        self._eq_enabled = enabled

        if self.is_enabled():
            success = True
            for i, value in enumerate(self._eq_bands):
                mbeq_index = EQ_BAND_TO_MBEQ_INDEX[i]
                param_name = f"eq:{MBEQ_PARAM_NAMES[mbeq_index]}"
                target = value if enabled else 0.0
                if not self._update_param_live(param_name, target):
                    success = False
            return success
        return True

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
    # Automatic Gain Control Parameters
    # =========================================================================

    _AGC_BINARY = "biglinux-mic-agc"

    def _start_agc_process(self) -> None:
        """Start the AGC process if not already running.

        Tries systemd service first; if unavailable, runs the binary directly.
        """
        if getattr(self, "_agc_process", None) is not None and self._agc_process.poll() is None:
                return  # already running

        # Try systemd service first
        try:
            result = subprocess.run(
                [
                    "systemctl",
                    "--user",
                    "is-active",
                    "--quiet",
                    "biglinux-mic-agc.service",
                ],
                timeout=3,
            )
            if result.returncode == 0:
                return  # systemd service already running
            subprocess.run(
                ["systemctl", "--user", "start", "biglinux-mic-agc.service"],
                timeout=5,
                capture_output=True,
            )
            # Check if it actually started
            result = subprocess.run(
                [
                    "systemctl",
                    "--user",
                    "is-active",
                    "--quiet",
                    "biglinux-mic-agc.service",
                ],
                timeout=3,
            )
            if result.returncode == 0:
                self._agc_process = None
                logger.info("Started AGC via systemd service")
                return
        except Exception:
            pass

        # Fallback: run binary directly
        import shutil

        binary = shutil.which(self._AGC_BINARY)
        if binary is None:
            logger.warning("AGC binary '%s' not found in PATH", self._AGC_BINARY)
            return
        try:
            self._agc_process = subprocess.Popen(
                [binary],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            logger.info("Started AGC process (pid %d)", self._agc_process.pid)
        except Exception:
            logger.warning("Failed to start AGC process", exc_info=True)

    def _stop_agc_process(self) -> None:
        """Stop the AGC process if running."""
        # Try systemd first
        try:
            result = subprocess.run(
                [
                    "systemctl",
                    "--user",
                    "is-active",
                    "--quiet",
                    "biglinux-mic-agc.service",
                ],
                timeout=3,
            )
            if result.returncode == 0:
                subprocess.run(
                    ["systemctl", "--user", "stop", "biglinux-mic-agc.service"],
                    timeout=5,
                    capture_output=True,
                )
                logger.info("Stopped AGC via systemd service")
                return
        except Exception:
            pass

        # Stop direct process
        proc = getattr(self, "_agc_process", None)
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
            logger.info("Stopped AGC process")
        self._agc_process = None

    def restart_agc(self) -> None:
        """Restart the AGC service/process."""
        if self._agc_enabled:
            self._stop_agc_process()
            self._start_agc_process()

    def set_agc_enabled(self, enabled: bool) -> None:
        """Enable or disable AGC. Starts/stops the Rust service accordingly."""
        self._agc_enabled = enabled
        if enabled:
            self._start_agc_process()
        else:
            self._stop_agc_process()

    def set_agc_target_level(self, value: int) -> None:
        """Set AGC target level (saved to config for the Rust service)."""
        self._agc_target_level_dbfs = value

    # =========================================================================
    # Echo Cancellation Parameters
    # =========================================================================

    def set_echo_cancel_enabled(self, enabled: bool) -> bool:
        """Enable or disable echo cancellation (requires pipeline restart)."""
        self._echo_cancel_enabled = enabled
        return self._requires_restart()

    # =========================================================================
    # Private Methods
    # =========================================================================

    def _get_filter_chain_node_id(self) -> str | None:
        """Get the filter-chain input node ID for live parameter updates.

        Searches for a node with 'input.' prefix that belongs to our
        filter chain, since LADSPA control params are on the input side.
        Uses pw-cli ls Node (fast) for discovery.
        """
        if self._cached_node_id:
            # Validate cache: ensure the node still exists
            try:
                result = subprocess.run(
                    ["pw-cli", "info", self._cached_node_id],
                    capture_output=True,
                    text=True,
                    timeout=2,
                )
                out = result.stdout if result.returncode == 0 else ""
                if "input.filter-chain" not in out and "input.big" not in out:
                    self._cached_node_id = None
            except Exception:
                self._cached_node_id = None

        if self._cached_node_id:
            return self._cached_node_id

        try:
            result = subprocess.run(
                ["pw-cli", "ls", "Node"],
                capture_output=True,
                text=True,
                timeout=2,
            )
            if result.returncode != 0:
                return None

            current_id = ""
            for line in result.stdout.splitlines():
                stripped = line.strip()
                if stripped.startswith("id "):
                    parts = stripped.split(",", 1)
                    current_id = parts[0].replace("id ", "").strip()
                elif "node.name" in stripped:
                    # Match input node of our filter chain:
                    # AEC mode: "input.filter-chain-XXXXX-Y"
                    # Smart mode: "input.big.filter-microphone" or similar
                    val = stripped.split("=", 1)[-1].strip().strip('"')
                    if val.startswith("input.") and (
                        "filter-chain" in val or "big" in val
                    ):
                        self._cached_node_id = current_id
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
            logger.debug("Could not find filter-chain node for live update")
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
            noise_reduction_model_blending=settings.noise_reduction.model_blending,
            noise_reduction_voice_recovery=settings.noise_reduction.voice_recovery,
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
            echo_cancel_enabled=settings.echo_cancel.enabled,
            source_node_name=self._detect_hardware_source()
            if settings.echo_cancel.enabled
            else "",
            echo_cancel_gain_control=settings.echo_cancel.gain_control,
            echo_cancel_noise_suppression=settings.echo_cancel.noise_suppression,
            echo_cancel_voice_detection=settings.echo_cancel.voice_detection,
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
            do_start = False
            with self._restart_lock:
                if self._is_updating:
                    self._restart_pending = True
                    self._pending_settings = settings
                    self._pending_on_complete = on_complete
                    logger.debug("Restart already in progress, queuing pending restart")
                else:
                    self._is_updating = True
                    self._restart_pending = False
                    do_start = True
            if do_start:
                self._run_restart_in_thread(on_complete, settings)
        elif on_complete:
            from gi.repository import GLib

            GLib.idle_add(on_complete)

    def _run_restart_in_thread(
        self,
        on_complete: Callable[[], None] | None = None,
        settings=None,
    ) -> None:
        """Run restart in a separate thread to avoid blocking GTK.

        Caller MUST set _is_updating = True under _restart_lock before calling.

        Args:
            on_complete: Optional callback to run after restart finishes
            settings: Optional AppSettings to pass through the restart chain
        """

        def _do_restart() -> None:
            try:
                loop = asyncio.new_event_loop()
                asyncio.set_event_loop(loop)
                try:
                    loop.run_until_complete(self._restart(settings))
                finally:
                    loop.close()

                # Check if another restart was requested while we were restarting
                with self._restart_lock:
                    if self._restart_pending:
                        pending_settings = self._pending_settings
                        pending_callback = self._pending_on_complete
                        self._restart_pending = False
                        self._pending_settings = None
                        self._pending_on_complete = None
                    else:
                        self._is_updating = False
                        pending_settings = None
                        pending_callback = None

                if pending_settings is not None or pending_callback is not None:
                    logger.debug("Executing pending restart after previous completed")
                    self._generate_config(pending_settings)
                    self._run_restart_in_thread(pending_callback, pending_settings)
                    return

            except Exception:
                logger.exception("Error in restart thread")
                with self._restart_lock:
                    self._is_updating = False
                    self._restart_pending = False

            if on_complete:
                try:
                    from gi.repository import GLib

                    GLib.idle_add(on_complete)
                except ImportError:
                    pass

        thread = threading.Thread(target=_do_restart, daemon=True)
        thread.start()

    async def _restart(self, settings=None) -> None:
        """Restart the filter chain with new configuration.

        Args:
            settings: Optional AppSettings to use for config generation.
                      When provided, avoids loading from disk (prevents race conditions).

        Uses internal methods directly to avoid _is_updating guard,
        since the caller (_run_restart_in_thread) manages that flag.
        """
        await self._start_filter_chain_process(settings)
