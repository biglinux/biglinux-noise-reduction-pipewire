#!/usr/bin/env python3
"""
Value validation and conversion utilities.

Provides helpers for value validation, clamping, and audio conversions.
"""

from __future__ import annotations

import math
from typing import TypeVar

T = TypeVar("T", int, float)


def validate_range(
    value: T,
    min_val: T,
    max_val: T,
    name: str = "value",
) -> T:
    """
    Validate that a value is within range.

    Args:
        value: Value to validate
        min_val: Minimum allowed value
        max_val: Maximum allowed value
        name: Name for error messages

    Returns:
        The validated value

    Raises:
        ValueError: If value is out of range
    """
    if value < min_val or value > max_val:
        raise ValueError(f"{name} must be between {min_val} and {max_val}, got {value}")
    return value


def clamp(value: T, min_val: T, max_val: T) -> T:
    """
    Clamp a value to a range.

    Args:
        value: Value to clamp
        min_val: Minimum value
        max_val: Maximum value

    Returns:
        Clamped value
    """
    return max(min_val, min(max_val, value))


def normalize(
    value: float,
    min_val: float,
    max_val: float,
) -> float:
    """
    Normalize a value to 0.0-1.0 range.

    Args:
        value: Value to normalize
        min_val: Original minimum
        max_val: Original maximum

    Returns:
        Normalized value (0.0 to 1.0)
    """
    if max_val == min_val:
        return 0.0
    return clamp((value - min_val) / (max_val - min_val), 0.0, 1.0)


def denormalize(
    value: float,
    min_val: float,
    max_val: float,
) -> float:
    """
    Denormalize a value from 0.0-1.0 range.

    Args:
        value: Normalized value (0.0 to 1.0)
        min_val: Target minimum
        max_val: Target maximum

    Returns:
        Denormalized value
    """
    return min_val + clamp(value, 0.0, 1.0) * (max_val - min_val)


def db_to_linear(db: float) -> float:
    """
    Convert decibels to linear scale.

    Args:
        db: Value in decibels

    Returns:
        Linear value (0.0 to ...)
    """
    return pow(10.0, db / 20.0)


def linear_to_db(linear: float, min_db: float = -96.0) -> float:
    """
    Convert linear scale to decibels.

    Args:
        linear: Linear value (0.0 to ...)
        min_db: Minimum dB value (for zero/negative input)

    Returns:
        Value in decibels
    """
    if linear <= 0:
        return min_db
    return max(min_db, 20.0 * math.log10(linear))


def percentage_to_db(percentage: float, range_db: float = 12.0) -> float:
    """
    Convert percentage (0-100) to decibels.

    Args:
        percentage: Value from 0 to 100
        range_db: Maximum dB range (±range)

    Returns:
        Value in decibels
    """
    # 50% = 0dB, 0% = -range_db, 100% = +range_db
    normalized = (percentage - 50.0) / 50.0
    return normalized * range_db


def db_to_percentage(db: float, range_db: float = 12.0) -> float:
    """
    Convert decibels to percentage (0-100).

    Args:
        db: Value in decibels
        range_db: Maximum dB range (±range)

    Returns:
        Percentage value (0-100)
    """
    normalized = clamp(db / range_db, -1.0, 1.0)
    return (normalized * 50.0) + 50.0


def frequency_to_log_position(
    freq: float,
    min_freq: float = 20.0,
    max_freq: float = 20000.0,
) -> float:
    """
    Convert frequency to logarithmic position (0-1).

    Used for equalizer band positioning.

    Args:
        freq: Frequency in Hz
        min_freq: Minimum frequency
        max_freq: Maximum frequency

    Returns:
        Position from 0.0 to 1.0
    """
    if freq <= min_freq:
        return 0.0
    if freq >= max_freq:
        return 1.0

    log_min = math.log10(min_freq)
    log_max = math.log10(max_freq)
    log_freq = math.log10(freq)

    return (log_freq - log_min) / (log_max - log_min)


def log_position_to_frequency(
    position: float,
    min_freq: float = 20.0,
    max_freq: float = 20000.0,
) -> float:
    """
    Convert logarithmic position to frequency.

    Args:
        position: Position from 0.0 to 1.0
        min_freq: Minimum frequency
        max_freq: Maximum frequency

    Returns:
        Frequency in Hz
    """
    position = clamp(position, 0.0, 1.0)

    log_min = math.log10(min_freq)
    log_max = math.log10(max_freq)

    log_freq = log_min + position * (log_max - log_min)
    return pow(10.0, log_freq)


def validate_filter_chain_param(
    name: str,
    value: float,
) -> float:
    """
    Validate a PipeWire filter-chain parameter.

    Args:
        name: Parameter name
        value: Parameter value

    Returns:
        Validated value

    Raises:
        ValueError: If value is invalid
    """
    # Parameter ranges
    ranges = {
        "strength": (0.0, 1.0),
        "threshold": (-96.0, 0.0),
        "attack": (0.1, 500.0),
        "release": (1.0, 5000.0),
        "width": (0.0, 1.0),
        "crossfeed": (0.0, 1.0),
        "gain": (-24.0, 24.0),
    }

    if name in ranges:
        min_val, max_val = ranges[name]
        return clamp(value, min_val, max_val)

    return value


def format_db(value: float, precision: int = 1) -> str:
    """
    Format a dB value for display.

    Args:
        value: Value in decibels
        precision: Decimal places

    Returns:
        Formatted string like "+3.5 dB" or "-12.0 dB"
    """
    sign = "+" if value >= 0 else ""
    return f"{sign}{value:.{precision}f} dB"


def format_frequency(freq: float) -> str:
    """
    Format a frequency value for display.

    Args:
        freq: Frequency in Hz

    Returns:
        Formatted string like "440 Hz" or "2.5 kHz"
    """
    if freq >= 1000:
        return f"{freq / 1000:.1f} kHz"
    return f"{int(freq)} Hz"


def format_time_ms(ms: float) -> str:
    """
    Format a time value in milliseconds.

    Args:
        ms: Time in milliseconds

    Returns:
        Formatted string like "50 ms" or "1.5 s"
    """
    if ms >= 1000:
        return f"{ms / 1000:.1f} s"
    return f"{int(ms)} ms"


def format_percentage(value: float, precision: int = 0) -> str:
    """
    Format a percentage value.

    Args:
        value: Value from 0.0 to 1.0 or 0-100
        precision: Decimal places

    Returns:
        Formatted string like "75%"
    """
    # Handle 0-1 range
    if value <= 1.0:
        value *= 100

    if precision == 0:
        return f"{int(value)}%"
    return f"{value:.{precision}f}%"


def is_valid_source_name(name: str) -> bool:
    """
    Check if a string is a valid PipeWire source name.

    Args:
        name: Source name to validate

    Returns:
        True if valid
    """
    if not name:
        return False

    # Basic validation
    invalid_chars = set('<>:"|?*')
    return not any(c in name for c in invalid_chars)


def sanitize_profile_name(name: str) -> str:
    """
    Sanitize a profile name for storage.

    Args:
        name: Raw profile name

    Returns:
        Sanitized name
    """
    # Remove or replace invalid characters
    invalid = '<>:"/\\|?*'
    result = name
    for char in invalid:
        result = result.replace(char, "_")

    # Trim whitespace and limit length
    result = result.strip()[:64]

    return result or "Untitled"
