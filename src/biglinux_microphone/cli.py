#!/usr/bin/env python3
"""
Command-line interface for noise reduction filter control.

This module provides a CLI for managing the PipeWire noise reduction
filter chain, including starting/stopping the filter process.
"""

import argparse
import re
import signal
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
                    try:
                        pids.append(int(parts[1]))
                    except ValueError:
                        pass
        return pids
    except Exception:
        return []


def kill_filter_processes() -> None:
    """Kill all running pipewire filter-chain processes."""
    for pid in find_filter_pids():
        try:
            subprocess.run(["kill", str(pid)], capture_output=True, check=False)
        except Exception:
            pass


def start_filter_process() -> bool:
    """Start the pipewire filter-chain process."""
    try:
        subprocess.Popen(
            ["/usr/bin/pipewire", "-c", "filter-chain.conf"],
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True
    except Exception as e:
        print(f"Error starting pipewire: {e}", file=sys.stderr)
        return False


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


def generate_config() -> bool:
    """Generate the filter chain configuration from user settings."""
    from biglinux_microphone.audio.filter_chain import (
        FilterChainConfig,
        FilterChainGenerator,
    )
    from biglinux_microphone.config import load_settings

    try:
        settings = load_settings()

        config = FilterChainConfig(
            noise_reduction_enabled=True,
            noise_reduction_model=settings.noise_reduction.model,
            noise_reduction_strength=settings.noise_reduction.strength,
            gate_enabled=settings.gate.enabled,
            gate_threshold_db=settings.gate.threshold_db,
            gate_range_db=settings.gate.range_db,
            gate_attack_ms=settings.gate.attack_ms,
            gate_hold_ms=settings.gate.hold_ms,
            gate_release_ms=settings.gate.release_ms,
            stereo_mode=settings.stereo.mode if settings.stereo.enabled else "mono",
            stereo_width=settings.stereo.width,
            eq_enabled=settings.equalizer.enabled,
            eq_bands=settings.equalizer.bands,
            transient_enabled=settings.transient.enabled,
            transient_attack=settings.transient.attack,
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
    return CONFIG_PATH.exists()


# =============================================================================
# Commands
# =============================================================================


def cmd_start() -> int:
    """Start noise reduction filter."""
    # Kill any existing filter processes
    kill_filter_processes()

    # Generate config if it doesn't exist
    if not CONFIG_PATH.exists():
        if not generate_config():
            return 1

    # Start the filter process
    if not start_filter_process():
        return 1

    # Wait for filter to initialize and configure source
    time.sleep(2)
    configure_filter_source()

    # Retry configuration for stability
    time.sleep(2)
    configure_filter_source()

    return 0


def cmd_stop() -> int:
    """Stop noise reduction filter."""
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
