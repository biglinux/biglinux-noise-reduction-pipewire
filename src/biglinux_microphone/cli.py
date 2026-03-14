#!/usr/bin/env python3
"""
Command-line interface for noise reduction filter control.

This module provides a CLI for managing the PipeWire noise reduction
filter chain, including starting/stopping the filter process.
"""

import argparse
import contextlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path

# =============================================================================
# Configuration Paths
# =============================================================================

CONFIG_PATH = (
    Path.home() / ".config/pipewire/filter-chain.conf.d/source-gtcrn-smart.conf"
)
LEGACY_PATHS = [
    Path.home() / ".config/pipewire/filter-chain.conf.d/source-rnnoise-config.conf",
    Path.home() / ".config/pipewire/filter-chain.conf.d/source-rnnoise-smart.conf",
]


# =============================================================================
# Process Management
# =============================================================================


def find_filter_pids() -> list[int]:
    """Find PIDs of running pipewire filter-chain processes."""
    try:
        result = subprocess.run(
            ["ps", "-aux"], capture_output=True, text=True, check=False
        )
        pids = []
        for line in result.stdout.splitlines():
            if "/usr/bin/pipewire -c filter-chain.conf" in line:
                parts = line.split()
                if len(parts) > 1:
                    with contextlib.suppress(ValueError):
                        pids.append(int(parts[1]))
        return pids
    except OSError:
        return []


def kill_filter_processes() -> None:
    """Kill all running pipewire filter-chain processes."""
    for pid in find_filter_pids():
        with contextlib.suppress(OSError):
            subprocess.run(["kill", str(pid)], capture_output=True, check=False)


# =============================================================================
# Filter Source Configuration
# =============================================================================


def get_filter_source() -> str | None:
    """Find the filter-chain source name in PulseAudio/PipeWire."""
    patterns = [
        r"output\.filter-chain[\w.-]*",
        r"big\.filter-microphone[\w.-]*",
    ]

    try:
        result = subprocess.run(
            ["pactl", "list", "sources", "short"],
            capture_output=True,
            text=True,
            check=False,
        )

        for pattern in patterns:
            match = re.search(pattern, result.stdout)
            if match:
                return match.group(0)

        # Try finding by filter-chain in any source
        for line in result.stdout.splitlines():
            if "filter-chain" in line:
                parts = line.split()
                if len(parts) > 1:
                    return parts[1]

        # Try finding by description
        result = subprocess.run(
            ["pactl", "list", "sources"], capture_output=True, text=True, check=False
        )
        lines = result.stdout.splitlines()
        for i, line in enumerate(lines):
            if "Noise Canceling Microphone" in line:
                # Look backwards for "Name:"
                for j in range(max(0, i - 5), i):
                    if "Name:" in lines[j]:
                        return lines[j].split(":", 1)[1].strip()

        return None

    except Exception:
        return None


def configure_filter_source() -> bool:
    """Configure the filter source (unmute and set volume)."""
    source_name = get_filter_source()
    if not source_name:
        print("Warning: Could not find filter-chain source", file=sys.stderr)
        return False

    print(f"Found filter source: {source_name}", file=sys.stderr)

    # Unmute and set volume
    subprocess.run(
        ["pactl", "set-source-mute", source_name, "0"], capture_output=True, check=False
    )
    subprocess.run(
        ["pactl", "set-source-volume", source_name, "100%"],
        capture_output=True,
        check=False,
    )

    return True


# =============================================================================
# Config Generation
# =============================================================================


def detect_hardware_source() -> str:
    """Detect the hardware microphone source name.

    Returns the default source excluding our own filter nodes.
    """
    try:
        result = subprocess.run(
            ["pactl", "get-default-source"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        if result.returncode == 0:
            name = result.stdout.strip()
            if name and not name.startswith("big-"):
                return name
    except Exception:
        pass
    return ""


def set_pipewire_quantum(quantum: int) -> None:
    """Set PipeWire force-quantum via pw-metadata."""
    with contextlib.suppress(Exception):
        subprocess.run(
            ["pw-metadata", "-n", "settings", "0",
             "clock.force-quantum", str(quantum)],
            capture_output=True,
            timeout=3,
            check=False,
        )


def generate_config() -> bool:
    """Generate the filter chain configuration from user settings."""
    from biglinux_microphone.audio.filter_chain import (
        FilterChainConfig,
        FilterChainGenerator,
    )
    from biglinux_microphone.config import StereoMode, load_settings

    try:
        settings = load_settings()

        stereo_mode = (
            settings.stereo.mode if settings.stereo.enabled else StereoMode.MONO
        )

        source_node_name = ""
        if settings.echo_cancel.enabled:
            source_node_name = detect_hardware_source()

        config = FilterChainConfig(
            hpf_enabled=settings.hpf.enabled,
            hpf_frequency=settings.hpf.frequency,
            noise_reduction_enabled=True,
            noise_reduction_model=settings.noise_reduction.model,
            noise_reduction_strength=settings.noise_reduction.strength,
            noise_reduction_speech_strength=settings.noise_reduction.speech_strength,
            noise_reduction_lookahead_ms=settings.noise_reduction.lookahead_ms,
            noise_reduction_voice_enhance=settings.noise_reduction.voice_enhance,
            noise_reduction_model_blending=settings.noise_reduction.model_blending,
            noise_reduction_noise_gate=settings.noise_reduction.noise_gate,
            compressor_enabled=settings.compressor.enabled,
            compressor_threshold_db=settings.compressor.threshold_db,
            compressor_ratio=settings.compressor.ratio,
            compressor_attack_ms=settings.compressor.attack_ms,
            compressor_release_ms=settings.compressor.release_ms,
            compressor_makeup_gain_db=settings.compressor.makeup_gain_db,
            compressor_knee_db=settings.compressor.knee_db,
            compressor_rms_peak=settings.compressor.rms_peak,
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
            echo_cancel_enabled=settings.echo_cancel.enabled,
            source_node_name=source_node_name,
            echo_cancel_gain_control=settings.echo_cancel.gain_control,
            echo_cancel_noise_suppression=settings.echo_cancel.noise_suppression,
            echo_cancel_voice_detection=settings.echo_cancel.voice_detection,
        )

        generator = FilterChainGenerator(config)
        generator.save()
        print("Config generated successfully")
        return True

    except Exception as e:
        print(f"Error generating config: {e}", file=sys.stderr)
        return False


def remove_config() -> None:
    """Remove the filter chain configuration files."""
    # Remove current config
    if CONFIG_PATH.exists():
        CONFIG_PATH.unlink()

    # Remove legacy configs
    for path in LEGACY_PATHS:
        if path.exists():
            path.unlink()


def check_status() -> bool:
    """Check if the noise reduction is enabled."""
    if CONFIG_PATH.exists():
        return True
    # Also detect legacy GTCRN config (pre-migration state)
    return any(path.exists() for path in LEGACY_PATHS)


# =============================================================================
# Commands
# =============================================================================


def cmd_start() -> int:
    """Start noise reduction filter."""
    from biglinux_microphone.config import load_settings

    # Kill any existing filter processes
    kill_filter_processes()

    # Remove legacy configs so PipeWire doesn't load old filters
    for path in LEGACY_PATHS:
        if path.exists():
            path.unlink()

    # Always regenerate config from current settings to stay in sync
    if not generate_config():
        return 1

    # Set force-quantum for AEC if enabled (must be before starting filter)
    try:
        settings = load_settings()
        if settings.echo_cancel.enabled:
            set_pipewire_quantum(960)
    except Exception:
        pass

    # Ensure main filter-chain config exists (with RT priority)
    # This must be imported inside the function to avoid circular imports if placed at top level
    from biglinux_microphone.audio.filter_chain import ensure_daemon_config

    ensure_daemon_config()

    # Start the filter process and monitor its PID
    try:
        proc = subprocess.Popen(
            ["/usr/bin/pipewire", "-c", "filter-chain.conf"],
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError as e:
        print(f"Error starting pipewire: {e}", file=sys.stderr)
        return 1

    # Wait for filter to register in PipeWire graph.
    # Process-aware polling: if the process dies, fail immediately.
    pid = proc.pid
    max_wait = 10.0
    poll_interval = 0.5
    elapsed = 0.0
    time.sleep(poll_interval)
    elapsed += poll_interval
    started = False

    while elapsed < max_wait:
        # Check if process is still alive
        try:
            os.kill(pid, 0)
        except OSError:
            print(
                f"Filter-chain process (PID {pid}) died during startup",
                file=sys.stderr,
            )
            break

        try:
            result = subprocess.run(
                ["pw-cli", "list-objects"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0 and "Noise Canceling Microphone" in result.stdout:
                started = True
                break
        except Exception:
            pass

        time.sleep(poll_interval)
        elapsed += poll_interval

    if not started:
        print(f"Warning: filter not detected after {elapsed:.1f}s", file=sys.stderr)

    configure_filter_source()

    # Brief retry for stability
    time.sleep(1)
    configure_filter_source()

    return 0


def cmd_stop() -> int:
    """Stop noise reduction filter."""
    # Restore default quantum
    set_pipewire_quantum(0)

    # Remove config files
    remove_config()

    # Kill filter processes
    kill_filter_processes()

    return 0


def cmd_status() -> int:
    """Check status of noise reduction filter."""
    if check_status():
        print("enabled")
        return 0
    else:
        print("disabled")
        return 1


def cmd_generate() -> int:
    """Generate config file only (for internal use)."""
    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    return 0 if generate_config() else 1


def cmd_remove() -> int:
    """Remove config file only (for internal use)."""
    remove_config()
    return 0


# =============================================================================
# Main
# =============================================================================


def main() -> int:
    """Main entry point for the CLI."""
    parser = argparse.ArgumentParser(
        description="Control the PipeWire noise reduction filter."
    )
    parser.add_argument(
        "command",
        choices=["start", "stop", "status", "generate", "remove"],
        help="Command to execute",
    )

    args = parser.parse_args()

    commands = {
        "start": cmd_start,
        "stop": cmd_stop,
        "status": cmd_status,
        "generate": cmd_generate,
        "remove": cmd_remove,
    }

    return commands[args.command]()


if __name__ == "__main__":
    sys.exit(main())
