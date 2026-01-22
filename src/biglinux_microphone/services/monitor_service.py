#!/usr/bin/env python3
"""
Monitor service for headphone monitoring.

Provides microphone monitoring through headphones using PulseAudio's module-loopback.
This method is preferred over native pw-loopback for consistent volume levels.
"""

from __future__ import annotations

import logging
import subprocess

logger = logging.getLogger(__name__)


class MonitorService:
    """
    Service for managing headphone monitoring using native pw-loopback.

    Routes microphone input to headphones with configurable delay using PipeWire's
    loopback module in a separate process.
    """

    def __init__(self) -> None:
        """Initialize the monitor service."""
        self._process: subprocess.Popen | None = None
        self._current_delay_ms: int = 0
        self._current_source: str | None = None
        self._current_channels: int = 0
        logger.debug("MonitorService initialized (pw-loopback)")

    def is_active(self) -> bool:
        """Check if monitoring is currently active."""
        if self._process is None:
            return False

        # Check if process is still running
        if self._process.poll() is not None:
            # Process ended unexpectedly
            self._process = None
            return False

        return True

    def start_monitor(self, source: str, delay_ms: int = 0, channels: int = 0) -> bool:
        """
        Start headphone monitoring (pw-loopback).

        Args:
            source: Audio source to monitor (name or serial)
            delay_ms: Delay in milliseconds (0-5000)
            channels: Number of channels (0=auto, 1=mono/downmix, 2=stereo)

        Returns:
            bool: True if started successfully
        """
        if self.is_active():
            self.stop_monitor()

        try:
            # Fixed low latency for responsiveness, use delay arg for user delay
            latency_prop = "100ms"
            delay_seconds = float(delay_ms) / 1000.0

            # Build pw-loopback command
            cmd = [
                "/usr/bin/pw-loopback",
                "--capture-props=media.class=Stream/Input/Audio",
                "--playback-props=media.class=Stream/Output/Audio",
                f"--latency={latency_prop}",
                f"--delay={delay_seconds}",
                f"--capture-props=node.target={source}",
                # Ensure we have a distinct name to identify later if needed
                "--playback-props=media.name=BigLinuxMicMonitor",
                "--playback-props=node.name=biglinux-mic-monitor",
                # Force volume to 100% (1.0)
                "--playback-props=audio.volume=1.0",
            ]

            # Channel configuration
            if channels == 1:
                logger.info("Starting Mono Loopback (pw-loopback)")
                # Force mono capture and stereo playback (FL,FR) mixing center
                # Capture: Mono
                cmd.append("--capture-props=audio.channels=1")
                # Playback: Stereo, copying capture to both FL and FR
                cmd.append("--playback-props=audio.channels=2")
                cmd.append("--playback-props=audio.position=[FL,FR]")
                # Mix matrix: In -> FL, In -> FR
                # 1 input channel, 2 output channels
                # [ 1.0, 1.0 ] maps input 1 to FL and FR
                # Not strictly needed if we just let upmix happen, but explicit is safer
                # BUT pw-loopback argument parsing for mix-matrix is tricky.
                # Let's rely on standard upmix channel map mismatch behavior first
                # or just 'FL,FR' output position usually duplicates mono input.

            else:
                # Standard Stereo or Auto
                logger.debug("Starting Standard Loopback (pw-loopback)")
                # Let it autodetect or pass through

            # Start process
            self._process = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,  # Detach
            )

            self._current_delay_ms = delay_ms
            self._current_source = source
            self._current_channels = channels

            logger.info("Monitor started. PID: %s", self._process.pid)
            return True

        except Exception:
            logger.exception("Error starting monitor")
            self._process = None
            return False

    def stop_monitor(self) -> bool:
        """Stop headphone monitoring."""
        if self._process is None:
            return True

        try:
            self._process.terminate()
            try:
                self._process.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait()

            self._process = None
            self._current_source = None
            return True
        except Exception:
            logger.exception("Error stopping monitor")
            self._process = None
            return False

    def set_delay(self, delay_ms: int) -> bool:
        """Update delay (restarts monitor)."""
        if self._current_source:
            return self.start_monitor(
                self._current_source, delay_ms, self._current_channels
            )
        return False

    def get_delay(self) -> int:
        return self._current_delay_ms

    def _force_volume_100(self):
        """No-op, volume forced in arguments."""
        pass
