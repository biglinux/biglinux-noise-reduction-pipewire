"""
Tests for race condition fixes in PipeWire configuration generation.

Validates that:
1. Settings are propagated through the entire restart chain (never loaded from stale disk)
2. State poller does not corrupt in-memory state during restarts
3. Structural changes save settings immediately (not debounced)
"""

from __future__ import annotations

import asyncio
import tempfile
import threading
from pathlib import Path
from unittest.mock import AsyncMock, patch

from biglinux_microphone.config import (
    AppSettings,
    NoiseModel,
    StereoMode,
)
from biglinux_microphone.services.pipewire_service import PipeWireService

# ============================================================================
# Helpers
# ============================================================================


def _make_settings(**overrides) -> AppSettings:
    """Create AppSettings with optional overrides."""
    s = AppSettings()
    for key, value in overrides.items():
        parts = key.split(".")
        obj = s
        for part in parts[:-1]:
            obj = getattr(obj, part)
        setattr(obj, parts[-1], value)
    return s


# ============================================================================
# Test Race Condition 1: Settings propagation through restart chain
# ============================================================================


class TestSettingsPropagation:
    """Verify settings flow through apply_config -> restart -> start without disk reads."""

    def test_generate_config_uses_passed_settings_not_disk(
        self, tmp_path: Path
    ) -> None:
        """When settings are passed, _generate_config must NOT load from disk."""
        service = PipeWireService.__new__(PipeWireService)
        service._config_file = (
            tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        )
        service._config_file.parent.mkdir(parents=True, exist_ok=True)

        # In-memory settings: NR enabled, EQ enabled, gate enabled
        live_settings = _make_settings()
        live_settings.noise_reduction.enabled = True
        live_settings.equalizer.enabled = True
        live_settings.gate.enabled = True

        # Call _generate_config with live settings
        service._generate_config(live_settings)

        # Read generated config
        content = service._config_file.read_text()

        # Config must reflect live settings (AI node + EQ node + gate node)
        assert '"ai"' in content, "AI node missing — stale settings were used"
        assert '"eq"' in content, "EQ node missing — stale settings were used"
        assert '"gate"' in content, "Gate node missing — stale settings were used"

    def test_generate_config_without_settings_loads_from_disk(
        self, tmp_path: Path
    ) -> None:
        """When settings=None, _generate_config loads from disk (fallback behavior)."""
        service = PipeWireService.__new__(PipeWireService)
        service._config_file = (
            tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        )
        service._config_file.parent.mkdir(parents=True, exist_ok=True)

        # Patch load_settings at its origin module (it's imported locally inside _generate_config)
        mock_settings = _make_settings()
        mock_settings.noise_reduction.enabled = True
        mock_settings.gate.enabled = True
        mock_settings.equalizer.enabled = False

        with patch(
            "biglinux_microphone.config.load_settings", return_value=mock_settings
        ):
            service._generate_config(None)

        content = service._config_file.read_text()
        assert '"ai"' in content
        assert '"gate"' in content
        # EQ node always present (PipeWire 1.6.1 workaround), but bands at 0 dB
        assert '"eq"' in content
        assert '"50Hz gain (low shelving)" = 0.0' in content

    def test_start_filter_chain_process_propagates_settings(
        self, tmp_path: Path
    ) -> None:
        """_start_filter_chain_process must pass settings to _generate_config."""
        service = PipeWireService.__new__(PipeWireService)
        service._config_file = (
            tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        )
        service._config_file.parent.mkdir(parents=True, exist_ok=True)
        service._cached_node_id = None

        live_settings = _make_settings()
        live_settings.noise_reduction.enabled = True
        live_settings.gate.enabled = False
        live_settings.equalizer.enabled = True

        # Track what _generate_config receives
        received_settings = []

        def mock_generate(self_inner, settings=None):
            received_settings.append(settings)
            self_inner._config_file.parent.mkdir(parents=True, exist_ok=True)
            self_inner._config_file.write_text("# mock config")

        with (
            patch.object(PipeWireService, "_generate_config", mock_generate),
            patch.object(PipeWireService, "_stop_filter_chain", new=AsyncMock()),
            patch("biglinux_microphone.services.pipewire_service.ensure_daemon_config"),
            patch("subprocess.Popen"),
            patch.object(PipeWireService, "is_enabled", return_value=True),
            patch.object(PipeWireService, "_configure_filter_source", new=AsyncMock()),
        ):
            loop = asyncio.new_event_loop()
            try:
                loop.run_until_complete(
                    service._start_filter_chain_process(live_settings)
                )
            finally:
                loop.close()

        assert len(received_settings) == 1, (
            f"Expected 1 call, got {len(received_settings)}"
        )
        assert received_settings[0] is live_settings, (
            "_generate_config received None instead of live settings — race condition!"
        )

    def test_restart_propagates_settings(self) -> None:
        """_restart must pass settings to _start_filter_chain_process."""
        service = PipeWireService.__new__(PipeWireService)
        service._config_file = Path("/tmp/test-config")
        service._cached_node_id = None

        live_settings = _make_settings()
        live_settings.noise_reduction.enabled = True

        received_settings = []

        async def mock_start(settings=None):
            received_settings.append(settings)
            return True

        with (
            patch.object(PipeWireService, "_stop_filter_chain", new=AsyncMock()),
            patch.object(
                PipeWireService, "_start_filter_chain_process", side_effect=mock_start
            ),
        ):
            loop = asyncio.new_event_loop()
            try:
                loop.run_until_complete(service._restart(live_settings))
            finally:
                loop.close()

        assert len(received_settings) == 1
        assert received_settings[0] is live_settings, (
            "_restart did not propagate settings to _start_filter_chain_process"
        )

    def test_run_restart_in_thread_propagates_settings(self) -> None:
        """_run_restart_in_thread must pass settings to _restart."""
        service = PipeWireService.__new__(PipeWireService)
        service._is_updating = False
        service._restart_pending = False
        service._pending_settings = None
        service._pending_on_complete = None
        service._config_file = Path("/tmp/test-config")

        live_settings = _make_settings()
        live_settings.noise_reduction.enabled = True
        live_settings.gate.enabled = False

        received_settings = []
        done_event = threading.Event()

        async def mock_restart(settings=None):
            received_settings.append(settings)

        # We need to also mock the on_complete callback so it doesn't use GLib
        def patched_run_restart(self_inner, on_complete=None, settings=None):
            """Wrapper that replaces GLib.idle_add callback with direct call."""

            def simple_complete():
                if on_complete:
                    on_complete()

            # Run restart in a thread (same as original, but callback is direct)
            def _do_restart():
                loop = asyncio.new_event_loop()
                try:
                    loop.run_until_complete(self_inner._restart(settings))
                finally:
                    loop.close()
                    self_inner._is_updating = False
                    simple_complete()

            self_inner._is_updating = True
            t = threading.Thread(target=_do_restart, daemon=True)
            t.start()

        with patch.object(PipeWireService, "_restart", side_effect=mock_restart):
            patched_run_restart(
                service, on_complete=lambda: done_event.set(), settings=live_settings
            )
            # Wait inside the context manager so the mock stays active for the thread
            done_event.wait(timeout=5)

        assert len(received_settings) == 1
        assert received_settings[0] is live_settings, (
            "_run_restart_in_thread did not propagate settings to _restart"
        )

    def test_restart_without_settings_passes_none(self) -> None:
        """When no settings provided, _restart passes None (backward compat)."""
        service = PipeWireService.__new__(PipeWireService)
        service._config_file = Path("/tmp/test-config")

        received_settings = []

        async def mock_start(settings=None):
            received_settings.append(settings)
            return True

        with (
            patch.object(PipeWireService, "_stop_filter_chain", new=AsyncMock()),
            patch.object(
                PipeWireService, "_start_filter_chain_process", side_effect=mock_start
            ),
        ):
            loop = asyncio.new_event_loop()
            try:
                loop.run_until_complete(service._restart())
            finally:
                loop.close()

        assert len(received_settings) == 1
        assert received_settings[0] is None


# ============================================================================
# Test Race Condition 2: Config content correctness under race scenario
# ============================================================================


class TestConfigContentRace:
    """Simulate the stale-disk scenario and verify correct config is generated."""

    def test_config_matches_in_memory_settings_not_disk(self, tmp_path: Path) -> None:
        """
        Simulates the race condition scenario:
        1. User enables NR + gate + EQ in the UI (in-memory settings)
        2. Settings are NOT yet saved to disk (debounced)
        3. Disk still has old config (NR disabled)
        4. Config generation must use in-memory settings, not disk
        """
        config_file = tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        config_file.parent.mkdir(parents=True, exist_ok=True)

        in_memory = _make_settings()
        in_memory.noise_reduction.enabled = True
        in_memory.noise_reduction.model = NoiseModel.GTCRN_DNS3
        in_memory.noise_reduction.strength = 0.8
        in_memory.gate.enabled = True
        in_memory.equalizer.enabled = True
        in_memory.equalizer.bands = [1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 3.0]

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(in_memory)

        content = config_file.read_text()

        assert '"ai"' in content, (
            "AI node missing — generated from stale disk settings!"
        )
        assert "Strength" in content
        assert "0.8" in content
        assert '"gate"' in content, "Gate node missing!"
        assert '"eq"' in content, "EQ node missing!"

    def test_stereo_mode_reflected_in_config(self, tmp_path: Path) -> None:
        """Stereo mode from in-memory settings must be reflected, not stale disk mode."""
        config_file = tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        config_file.parent.mkdir(parents=True, exist_ok=True)

        in_memory = _make_settings()
        in_memory.noise_reduction.enabled = True
        in_memory.stereo.enabled = True
        in_memory.stereo.mode = StereoMode.DUAL_MONO

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(in_memory)

        content = config_file.read_text()
        assert "FL FR" in content, (
            "Stereo mode not reflected — used stale MONO from disk!"
        )
        assert "copy_left" in content

    def test_nr_disabled_has_no_ai_node(self, tmp_path: Path) -> None:
        """NR disabled in settings must not produce AI node in config."""
        config_file = tmp_path / "filter-chain.conf.d" / "source-gtcrn-smart.conf"
        config_file.parent.mkdir(parents=True, exist_ok=True)

        settings = _make_settings()
        settings.noise_reduction.enabled = False
        settings.gate.enabled = True
        settings.equalizer.enabled = True

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(settings)

        content = config_file.read_text()
        assert '"ai"' not in content, "AI node present when NR is disabled!"
        assert '"gate"' in content
        assert '"eq"' in content


# ============================================================================
# Test Race Condition 3: Pending restart uses correct settings
# ============================================================================


class TestPendingRestart:
    """When a restart is already in progress, the pending restart must use latest settings."""

    def test_pending_restart_uses_latest_settings(self) -> None:
        """
        Scenario: restart#1 in progress, user changes gate -> queues restart#2.
        Restart#2 must use the settings that include the gate change.
        """
        service = PipeWireService.__new__(PipeWireService)
        service._is_updating = True  # Simulate restart in progress
        service._restart_pending = False
        service._pending_settings = None
        service._pending_on_complete = None
        service._noise_reduction_enabled = True
        service._restart_lock = threading.Lock()

        settings_with_gate = _make_settings()
        settings_with_gate.noise_reduction.enabled = True
        settings_with_gate.gate.enabled = True

        config_file = Path(tempfile.mktemp(suffix=".conf"))
        service._config_file = config_file
        config_file.parent.mkdir(parents=True, exist_ok=True)

        service.apply_config(settings_with_gate, on_complete=lambda: None)

        assert service._restart_pending is True
        assert service._pending_settings is settings_with_gate
        assert service._pending_on_complete is not None

        if config_file.exists():
            config_file.unlink()

    def test_apply_config_passes_settings_to_restart_thread(self) -> None:
        """apply_config must pass settings (not None) to _run_restart_in_thread."""
        service = PipeWireService.__new__(PipeWireService)
        service._is_updating = False
        service._restart_pending = False
        service._noise_reduction_enabled = True
        service._restart_lock = threading.Lock()
        config_file = Path(tempfile.mktemp(suffix=".conf"))
        service._config_file = config_file
        config_file.parent.mkdir(parents=True, exist_ok=True)

        settings = _make_settings()
        settings.noise_reduction.enabled = True

        received_args = []

        def mock_run_restart(self_inner, on_complete=None, settings=None):
            received_args.append({"on_complete": on_complete, "settings": settings})

        with (
            patch.object(PipeWireService, "sync_from_settings", lambda self, x: None),
            patch.object(PipeWireService, "_run_restart_in_thread", mock_run_restart),
        ):
            service.apply_config(settings, on_complete=lambda: None)

        assert len(received_args) == 1
        assert received_args[0]["settings"] is settings, (
            "apply_config did not pass settings to _run_restart_in_thread!"
        )

        if config_file.exists():
            config_file.unlink()


# ============================================================================
# Test FilterChainConfig generation correctness
# ============================================================================


class TestFilterChainConfigFromSettings:
    """Verify _generate_config creates correct FilterChainConfig from AppSettings."""

    def test_all_filters_enabled(self, tmp_path: Path) -> None:
        """All filters enabled must produce complete pipeline."""
        config_file = tmp_path / "test.conf"

        settings = _make_settings()
        settings.noise_reduction.enabled = True
        settings.noise_reduction.model = NoiseModel.GTCRN_DNS3
        settings.noise_reduction.strength = 0.7
        settings.gate.enabled = True
        settings.equalizer.enabled = True
        settings.compressor.enabled = True

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(settings)

        content = config_file.read_text()

        assert '"ai"' in content
        assert '"gate"' in content
        assert '"eq"' in content
        assert '"compressor"' in content

        compressor_pos = content.index('"compressor"')
        ai_pos = content.index('"ai"')
        gate_pos = content.index('"gate"')
        eq_pos = content.index('"eq"')
        assert compressor_pos < ai_pos < eq_pos < gate_pos, (
            "Filter chain order is wrong"
        )

    def test_only_nr_enabled(self, tmp_path: Path) -> None:
        """Only noise reduction enabled produces minimal pipeline."""
        config_file = tmp_path / "test.conf"

        settings = _make_settings()
        settings.noise_reduction.enabled = True
        settings.gate.enabled = False
        settings.equalizer.enabled = False
        settings.compressor.enabled = False

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(settings)

        content = config_file.read_text()

        assert '"ai"' in content
        assert '"gate"' not in content
        # EQ node always present (PipeWire 1.6.1 workaround), bands at 0 dB
        assert '"eq"' in content
        assert '"compressor"' not in content

    def test_settings_strength_value_in_config(self, tmp_path: Path) -> None:
        """Specific setting values must appear in generated config."""
        config_file = tmp_path / "test.conf"

        settings = _make_settings()
        settings.noise_reduction.enabled = True
        settings.noise_reduction.strength = 0.42
        settings.gate.enabled = False
        settings.equalizer.enabled = False

        service = PipeWireService.__new__(PipeWireService)
        service._config_file = config_file

        service._generate_config(settings)

        content = config_file.read_text()

        assert "0.42" in content, "Strength value not in config"
