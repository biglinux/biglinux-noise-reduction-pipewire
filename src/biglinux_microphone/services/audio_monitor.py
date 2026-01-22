#!/usr/bin/env python3
"""
Audio monitoring service for BigLinux Microphone Settings.

Provides real-time audio level monitoring using GStreamer + PipeWire.
Uses NumPy FFT for accurate spectrum analysis.
"""

from __future__ import annotations

import logging
import subprocess
from collections.abc import Callable
from dataclasses import dataclass, field

import gi
import numpy as np

gi.require_version("Gst", "1.0")

from gi.repository import GLib, Gst

logger = logging.getLogger(__name__)

# Initialize GStreamer
Gst.init(None)

# Constants for audio monitoring
NUM_SPECTRUM_BANDS = 32  # Match spectrum_widget.py for premium visualization
SPECTRUM_INTERVAL_MS = 50
SPECTRUM_INTERVAL_NS = SPECTRUM_INTERVAL_MS * 1_000_000
SPECTRUM_THRESHOLD_DB = -80

# FFT parameters for NumPy-based spectrum analysis
SAMPLE_RATE = 32000
FFT_SIZE = 2048  # ~46ms window at 44100Hz
FREQ_MIN = 20.0
FREQ_MAX = 16000.0


@dataclass
class AudioLevels:
    """Audio level measurements."""

    input_level: float = 0.0  # 0.0 to 1.0
    output_level: float = 0.0  # 0.0 to 1.0
    input_peak: float = 0.0
    output_peak: float = 0.0
    spectrum_bands: list[float] = field(
        default_factory=lambda: [0.0] * NUM_SPECTRUM_BANDS
    )


class AudioMonitor:
    """
    Real-time audio level and spectrum monitoring using GStreamer.

    Uses pipewiresrc for audio capture and NumPy FFT for spectrum analysis.
    Provides frequency band data for spectrum visualization.
    """

    def __init__(
        self,
        num_bands: int = NUM_SPECTRUM_BANDS,
        interval_ms: int = SPECTRUM_INTERVAL_MS,
    ) -> None:
        """
        Initialize the audio monitor.

        Args:
            num_bands: Number of frequency bands for spectrum
            interval_ms: Update interval in milliseconds
        """
        self._num_bands = num_bands
        self._interval_ms = interval_ms

        self._pipeline: Gst.Pipeline | None = None
        self._is_running = False
        self._callback: Callable[[AudioLevels], None] | None = None

        self._current_levels = AudioLevels()
        self._peak_decay = 0.95

        self._current_source_name: str | None = None
        self._source_check_timer: int | None = None

        # FFT analysis buffers and precomputed data
        self._audio_buffer: list[float] = []
        self._fft_window = np.hanning(FFT_SIZE)
        self._freq_resolution = SAMPLE_RATE / FFT_SIZE

        # Precompute logarithmic frequency band boundaries
        log_min = np.log10(FREQ_MIN)
        log_max = np.log10(FREQ_MAX)
        self._band_edges = np.logspace(log_min, log_max, num_bands + 1)

        # Precompute FFT bin ranges for each band
        self._band_bin_ranges: list[tuple[int, int]] = []
        for i in range(num_bands):
            start_bin = max(1, int(self._band_edges[i] / self._freq_resolution))
            end_bin = min(
                FFT_SIZE // 2, int(self._band_edges[i + 1] / self._freq_resolution)
            )
            self._band_bin_ranges.append((start_bin, end_bin))

        # Precompute frequency compensation weights (boost high frequencies)
        # Compensates for natural ~6dB/octave rolloff in voice/music
        self._freq_weights: list[float] = []
        for i in range(num_bands):
            center_freq = (self._band_edges[i] + self._band_edges[i + 1]) / 2
            weight = (center_freq / 200) ** 0.5  # sqrt compensation
            self._freq_weights.append(min(weight, 5.0))  # Cap at 5x boost

        # Update timing
        self._last_update_time = 0.0
        self._update_interval_samples = int(SAMPLE_RATE * interval_ms / 1000)

        logger.debug(
            "GStreamer AudioMonitor initialized with %d bands (NumPy FFT)", num_bands
        )

    def _check_source_update(self) -> bool:
        """
        Periodically check for the best audio source and switch if needed.

        If the noise reduction filter source is available, it is preferred.
        Otherwise, falls back to the default system source.

        Returns:
            bool: True to keep the timer running.
        """
        if not self._is_running:
            return False

        # Prefer filtered source if available
        new_source = self._detect_filtered_source()
        if not new_source:
            new_source = self._detect_default_source()

        # Switch if source changed
        if new_source and new_source != self._current_source_name:
            logger.info(
                "Auto-switching audio source: %s -> %s",
                self._current_source_name,
                new_source,
            )
            self.set_input_source(new_source)
            self._current_source_name = new_source

        return True

    def start(self, callback: Callable[[AudioLevels], None]) -> bool:
        """
        Start audio monitoring.

        Args:
            callback: Function to call with updated levels

        Returns:
            bool: True if started successfully
        """
        if self._is_running:
            logger.warning("AudioMonitor already running")
            return True

        self._callback = callback
        self._audio_buffer = []  # Reset buffer

        # Initial source detection
        source_name = self._detect_filtered_source()
        if not source_name:
            logger.debug("Filtered source not found initially, using default")
            source_name = self._detect_default_source()

        self._current_source_name = source_name

        if source_name:
            logger.debug("Using initial audio source: %s", source_name)
        else:
            logger.warning("No audio source detected, using automatic detection")

        # Build target option if source detected
        target_opt = f'target-object="{source_name}"' if source_name else ""

        # Build GStreamer pipeline with appsink for raw audio
        # pipewiresrc -> audioconvert -> appsink (for NumPy FFT processing)
        pipeline_desc = (
            f"pipewiresrc name=src {target_opt} ! "
            f"audio/x-raw,format=F32LE,channels=1,rate={SAMPLE_RATE} ! "
            f"audioconvert ! "
            f"appsink name=sink emit-signals=true sync=false"
        )

        try:
            self._pipeline = Gst.parse_launch(pipeline_desc)

            if self._pipeline is None:
                logger.error("Failed to create GStreamer pipeline")
                return False

            # Connect to appsink for raw audio samples
            sink = self._pipeline.get_by_name("sink")
            if sink:
                sink.connect("new-sample", self._on_new_sample)

            # Connect to bus for error messages
            bus = self._pipeline.get_bus()
            bus.add_signal_watch()
            bus.connect("message::error", self._on_error_message)

            # Start pipeline
            ret = self._pipeline.set_state(Gst.State.PLAYING)
            if ret == Gst.StateChangeReturn.FAILURE:
                logger.error("Failed to start GStreamer pipeline")
                self._cleanup()
                return False

            self._is_running = True
            self._start_watchdog()

            # Start 1-second interval check for source switching
            self._source_check_timer = GLib.timeout_add(1000, self._check_source_update)

            logger.info("AudioMonitor started with GStreamer (NumPy FFT)")
            return True

        except Exception:
            logger.exception("Failed to create GStreamer pipeline")
            self._cleanup()
            return False

    def stop(self) -> None:
        """Stop audio monitoring."""
        if not self._is_running:
            return

        self._is_running = False

        if self._source_check_timer:
            GLib.source_remove(self._source_check_timer)
            self._source_check_timer = None

        self._stop_watchdog()
        self._cleanup()
        self._callback = None
        self._current_levels = AudioLevels()

        logger.info("AudioMonitor stopped")

    def restart(self) -> None:
        """Restart monitoring with current callback."""
        if self._callback:
            callback = self._callback
            self.stop()

            # Brief delay to allow cleanup and for filter chain to stabilize
            def _restart_delayed():
                self.start(callback)
                return False

            GLib.timeout_add(500, _restart_delayed)

    def _cleanup(self) -> None:
        """Clean up GStreamer resources."""
        if self._pipeline:
            self._pipeline.set_state(Gst.State.NULL)
            self._pipeline = None

    def is_running(self) -> bool:
        """Check if monitoring is active."""
        return self._is_running

    def get_levels(self) -> AudioLevels:
        """Get current audio levels."""
        return self._current_levels

    def _start_watchdog(self) -> None:
        """Start the watchdog timer to detect freezes."""
        self._last_sample_time = GLib.get_monotonic_time()
        # Check every 2 seconds
        self._watchdog_timer = GLib.timeout_add(2000, self._check_watchdog)

    def _stop_watchdog(self) -> None:
        """Stop the watchdog timer."""
        if hasattr(self, "_watchdog_timer") and self._watchdog_timer:
            GLib.source_remove(self._watchdog_timer)
            self._watchdog_timer = None

    def _check_watchdog(self) -> bool:
        """
        Check if we are receiving samples.

        Returns:
            bool: True to keep timer running
        """
        if not self._is_running:
            return False

        # Check time since last sample (microseconds)
        current_time = GLib.get_monotonic_time()
        elapsed = (current_time - self._last_sample_time) / 1_000_000

        # If no samples for > 3 seconds, restart
        if elapsed > 3.0:
            logger.warning(
                "AudioMonitor watchdog: No samples for %.1fs, restarting...", elapsed
            )
            callback = self._callback
            self.stop()
            if callback:
                self.start(callback)
            return False  # Stop this timer (start() will create a new one)

        return True

    def _detect_default_source(self) -> str | None:
        """Detect the default input source."""
        try:
            result = subprocess.run(
                ["pactl", "get-default-source"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                return result.stdout.strip()
        except Exception:
            pass
        return None

    def _detect_filtered_source(self) -> str | None:
        """
        Detect the noise-filtered microphone source using pw-dump.

        More robust than parsing pactl output. Matches the specific
        filter.smart.name property set in the filter-chain config.

        Returns:
            str: Node serial number as string, or None if not found.
        """
        import json

        try:
            # pw-dump is more efficient and reliable than parsing pactl text
            # We look for the source node created by our filter chain
            result = subprocess.run(
                ["pw-dump"],
                capture_output=True,
                text=True,
                timeout=2,
            )

            if result.returncode != 0:
                return None

            data = json.loads(result.stdout)

            for obj in data:
                # We are looking for a Node
                if obj.get("type") != "PipeWire:Interface:Node":
                    continue

                props = obj.get("info", {}).get("props", {})

                # Check for our specific smart filter name
                # This is defined in filter_chain.py and set in the config
                if props.get("filter.smart.name") == "big.filter-microphone":
                    # We found our filter chain!
                    # Return object.serial (most reliable persistent ID)
                    # serial is an int, convert to str for GStreamer
                    serial = props.get("object.serial")
                    if serial:
                        logger.debug(
                            "Found filtered source via pw-dump: serial=%s", serial
                        )
                        return str(serial)

        except Exception:
            logger.exception("Error detecting filtered source")

        return None

    def _on_new_sample(self, sink: Gst.Element) -> Gst.FlowReturn:
        """
        Handle new audio samples from appsink.

        Args:
            sink: The appsink element

        Returns:
            Gst.FlowReturn.OK to continue processing
        """
        # Update watchdog timestamp
        self._last_sample_time = GLib.get_monotonic_time()

        sample = sink.emit("pull-sample")
        if not sample:
            return Gst.FlowReturn.OK

        buf = sample.get_buffer()
        success, map_info = buf.map(Gst.MapFlags.READ)
        if not success:
            return Gst.FlowReturn.OK

        try:
            # Get audio samples as float32
            data = np.frombuffer(map_info.data, dtype=np.float32)
            self._audio_buffer.extend(data.tolist())

            # Process when we have enough samples for FFT
            while len(self._audio_buffer) >= FFT_SIZE:
                chunk = np.array(self._audio_buffer[:FFT_SIZE])
                self._audio_buffer = self._audio_buffer[FFT_SIZE // 2 :]  # 50% overlap
                self._process_fft_chunk(chunk)

        except Exception:
            logger.exception("Error processing audio samples")
        finally:
            buf.unmap(map_info)

        return Gst.FlowReturn.OK

    def _process_fft_chunk(self, chunk: np.ndarray) -> None:
        """Process a single FFT chunk and update levels."""
        # 1. Calculate ACCURATE time-domain peak / RMS for the main meter
        # This fixes the discrepancy with OBS/PipeWire meters
        peak_amplitude = np.max(np.abs(chunk))
        if peak_amplitude > 1e-10:
            peak_db = 20.0 * np.log10(peak_amplitude)
        else:
            peak_db = SPECTRUM_THRESHOLD_DB

        # Map dB to 0-1 range for the level meter
        meter_level = (peak_db - SPECTRUM_THRESHOLD_DB) / (-SPECTRUM_THRESHOLD_DB)
        meter_level = max(0.0, min(1.0, meter_level))

        # 2. Process FFT for visualization
        # Apply Hanning window
        windowed = chunk * self._fft_window
        fft_data = np.abs(np.fft.rfft(windowed))

        # Compensation factors
        # 1. FFT Normalization: ref_magnitude = FFT_SIZE / 2
        # 2. Window Coherent Gain: Hanning reduces peak by 0.5 (-6dB). We multiply by 2.0 to compensate.
        ref_magnitude = (FFT_SIZE / 2.0) * 0.5

        # Compute band magnitudes
        bands: list[float] = []
        for i, (start_bin, end_bin) in enumerate(self._band_bin_ranges):
            if start_bin < end_bin:
                # Use MAX instead of MEAN to capture pure tones/peaks better in the band
                magnitude = float(np.max(fft_data[start_bin:end_bin]))
            else:
                idx = min(start_bin, len(fft_data) - 1)
                magnitude = float(fft_data[idx])

            # Apply frequency compensation weight
            magnitude *= self._freq_weights[i]

            # Convert to dB
            if magnitude > 1e-10:
                db = 20.0 * np.log10(magnitude / ref_magnitude)
            else:
                db = SPECTRUM_THRESHOLD_DB

            # Map dB to 0-1 range
            normalized = (db - SPECTRUM_THRESHOLD_DB) / (-SPECTRUM_THRESHOLD_DB)
            normalized = max(0.0, min(1.0, normalized))
            bands.append(normalized)

        # Update peaks with decay
        input_peak = max(
            meter_level, self._current_levels.input_peak * self._peak_decay
        )

        # Update current levels
        self._current_levels = AudioLevels(
            input_level=meter_level,
            output_level=meter_level,
            input_peak=input_peak,
            output_peak=input_peak,
            spectrum_bands=bands[:30],
        )

        # Notify callback on main thread
        if self._callback:
            GLib.idle_add(self._callback, self._current_levels)

    def _on_error_message(
        self,
        _bus: Gst.Bus,
        message: Gst.Message,
    ) -> None:
        """Handle error messages from GStreamer bus."""
        err, debug = message.parse_error()
        logger.error("GStreamer error: %s (debug: %s)", err.message, debug)

        # Try to restart
        if self._is_running:
            logger.info("Attempting to restart audio monitoring...")
            callback = self._callback
            self.stop()
            if callback:
                GLib.timeout_add(1000, lambda: self._delayed_restart(callback))

    def _delayed_restart(self, callback: Callable[[AudioLevels], None]) -> bool:
        """Delayed restart after error."""
        if not self._is_running:
            self.start(callback)
        return False  # Don't repeat

    def set_input_source(self, source: str) -> None:
        """
        Set the input source to monitor.

        Args:
            source: Source name or serial ID
        """
        logger.debug("Input source set to: %s", source)

        # Force a clean restart of the pipeline.
        # Hot-swapping 'target-object' on a running pipewiresrc is unreliable
        # and often leads to data flow freezes (watchdog timeouts).
        callback = self._callback
        if self._is_running:
            self.stop()

        if callback:
            # Small delay to ensure clean state
            GLib.timeout_add(100, lambda: self._delayed_start(callback, source))

    def _delayed_start(
        self, callback: Callable[[AudioLevels], None], source_name: str | None = None
    ) -> bool:
        """Helper to start detection with a specific source preference."""
        # Update current source name preference
        if source_name:
            self._current_source_name = source_name

        self.start(callback)
        return False


class SpectrumAnalyzer:
    """
    Audio spectrum analysis for visualization.

    Provides frequency band analysis for audio visualization widgets.
    Note: This is a simplified implementation for fallback.
    The GStreamer spectrum element is preferred.
    """

    def __init__(self, num_bands: int = 10) -> None:
        """
        Initialize the spectrum analyzer.

        Args:
            num_bands: Number of frequency bands
        """
        self._num_bands = num_bands
        self._bands = [0.0] * num_bands
        self._decay = 0.85

        logger.debug("Spectrum analyzer initialized with %d bands", num_bands)

    def get_bands(self) -> list[float]:
        """
        Get current spectrum band values.

        Returns:
            list[float]: Band values (0.0 to 1.0)
        """
        return self._bands.copy()

    def update_from_level(self, level: float) -> list[float]:
        """
        Update spectrum from a single level value.

        This is a simplified visualization that creates
        pseudo-spectrum data from a single level.

        Args:
            level: Input level (0.0 to 1.0)

        Returns:
            list[float]: Updated band values
        """
        import random

        for i in range(self._num_bands):
            # Create variation based on level
            variation = random.uniform(0.7, 1.3)
            # Lower bands tend to have more energy
            band_weight = 1.0 - (i / (self._num_bands * 2))

            target = level * variation * band_weight

            # Smooth transition with decay
            self._bands[i] = max(target, self._bands[i] * self._decay)
            self._bands[i] = min(1.0, self._bands[i])

        return self._bands.copy()

    def reset(self) -> None:
        """Reset all bands to zero."""
        self._bands = [0.0] * self._num_bands
