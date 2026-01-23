#!/usr/bin/env python3
"""
Configuration module for BigLinux Microphone Settings.

Contains all constants, dataclasses, and configuration management
following strict typing and PEP standards.
"""

from __future__ import annotations

import json
import logging
from dataclasses import asdict, dataclass, field
from enum import Enum, IntEnum
from pathlib import Path
from typing import Any

from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)

# =============================================================================
# Application Constants
# =============================================================================

APP_ID = "br.com.biglinux.microphone"
APP_NAME = "Microphone Settings"
APP_VERSION = "5.0.0"
APP_DEVELOPER = "BigLinux Team"
APP_WEBSITE = "https://github.com/biglinux/biglinux-noise-reduction-pipewire"
APP_ISSUE_URL = f"{APP_WEBSITE}/issues"

# =============================================================================
# Window Configuration
# =============================================================================

WINDOW_WIDTH_DEFAULT = 720
WINDOW_HEIGHT_DEFAULT = 700
WINDOW_WIDTH_MIN = 400
WINDOW_HEIGHT_MIN = 500

# =============================================================================
# UI Spacing Constants
# =============================================================================

MARGIN_SMALL = 6
MARGIN_DEFAULT = 12
MARGIN_LARGE = 24
SPACING_SMALL = 6
SPACING_DEFAULT = 12
SPACING_LARGE = 24

# =============================================================================
# Path Configuration
# =============================================================================

CONFIG_DIR = Path.home() / ".config" / "biglinux-microphone"
SETTINGS_FILE = CONFIG_DIR / "settings.json"
PROFILES_DIR = CONFIG_DIR / "profiles"

# System paths
SCRIPTS_DIR = Path("/usr/share/biglinux/microphone/scripts")
ICONS_DIR = Path("/usr/share/icons/hicolor/scalable")
ICON_APP = ICONS_DIR / "apps" / "biglinux-noise-reduction-pipewire.svg"
ICON_ON = ICONS_DIR / "status" / "big-noise-reduction-on.svg"
ICON_OFF = ICONS_DIR / "status" / "big-noise-reduction-off.svg"

# LADSPA Plugin paths
LADSPA_DIR = Path("/usr/lib/ladspa")
GTCRN_PLUGIN = LADSPA_DIR / "libgtcrn_ladspa.so"
PITCH_SCALE_PLUGIN = LADSPA_DIR / "pitch_scale_1193.so"
GVERB_PLUGIN = LADSPA_DIR / "gverb_1216.so"
SC4_PLUGIN = LADSPA_DIR / "sc4m_1916.so"


# =============================================================================
# Enumerations
# =============================================================================


class AIPlugin(Enum):
    """Available AI noise reduction plugins."""

    GTCRN = "gtcrn"


class NoiseModel(IntEnum):
    """AI model types for noise reduction.

    Models are organized by plugin type:
    - 0-9: GTCRN models
    - 10-19: FastEnhancer models
    """

    # GTCRN models (model control: 0 or 1)
    GTCRN_LOW_LATENCY = 0  # GTCRN simple model - lower latency
    GTCRN_FULL_QUALITY = 1  # GTCRN full model - best quality


class StereoMode(Enum):
    """Stereo enhancement modes."""

    MONO = "mono"
    DUAL_MONO = "dual_mono"
    RADIO = "radio"  # Professional radio voice with compression
    VOICE_CHANGER = "voice_changer"  # Unified pitch shift


class VisualizerStyle(IntEnum):
    """Audio visualizer display styles."""

    MODERN_WAVES = 0
    RETRO_BARS = 1
    CIRCULAR = 2


# =============================================================================
# Model Display Configuration
# =============================================================================

MODEL_NAMES: dict[NoiseModel, str] = {
    # GTCRN models
    NoiseModel.GTCRN_LOW_LATENCY: "GTCRN - Low Latency",
    NoiseModel.GTCRN_FULL_QUALITY: "GTCRN - Full Quality",
}

MODEL_DESCRIPTIONS: dict[NoiseModel, str] = {
    # GTCRN models
    NoiseModel.GTCRN_LOW_LATENCY: "GTCRN simple model - Lower latency, suitable for real-time communication",
    NoiseModel.GTCRN_FULL_QUALITY: "GTCRN full model - Best noise reduction quality, slightly higher CPU usage",
}

# Plugin to model mapping
PLUGIN_MODELS: dict[AIPlugin, list[NoiseModel]] = {
    AIPlugin.GTCRN: [
        NoiseModel.GTCRN_LOW_LATENCY,
        NoiseModel.GTCRN_FULL_QUALITY,
    ],
}

STEREO_MODE_NAMES: dict[StereoMode, str] = {
    StereoMode.MONO: "Mono (Original)",
    StereoMode.DUAL_MONO: "Dual Mono",
    StereoMode.RADIO: "Radio Voice",
    StereoMode.VOICE_CHANGER: "Voice Changer",
}

STEREO_MODE_DESCRIPTIONS: dict[StereoMode, str] = {
    StereoMode.MONO: "No stereo processing, original mono signal",
    StereoMode.DUAL_MONO: "Simple copy to both channels",
    StereoMode.RADIO: "Professional radio voice with compression",
    StereoMode.VOICE_CHANGER: "Adjust Pitch: 0% (Deep) to 100% (High)",
}

# =============================================================================
# Audio Processing Limits
# =============================================================================

# Noise Reduction
STRENGTH_MIN = 0.0
STRENGTH_MAX = 1.0
STRENGTH_DEFAULT = 1.0
STRENGTH_STEP = 0.05

# Gate Filter
GATE_THRESHOLD_MIN = -60
GATE_THRESHOLD_MAX = 0
GATE_THRESHOLD_DEFAULT = -30
GATE_THRESHOLD_STEP = 1

GATE_RANGE_MIN = -90
GATE_RANGE_MAX = 0
GATE_RANGE_DEFAULT = -60
GATE_RANGE_STEP = 1

GATE_ATTACK_MIN = 0.1
GATE_ATTACK_MAX = 500.0
GATE_ATTACK_DEFAULT = 20.0
GATE_ATTACK_STEP = 1.0

GATE_HOLD_MIN = 0.0
GATE_HOLD_MAX = 1000.0
GATE_HOLD_DEFAULT = 300.0
GATE_HOLD_STEP = 10.0

GATE_RELEASE_MIN = 0.0
GATE_RELEASE_MAX = 1000.0
GATE_RELEASE_DEFAULT = 150.0
GATE_RELEASE_STEP = 10.0

# Transient Suppressor (for click removal)
TRANSIENT_ATTACK_MIN = -1.0
TRANSIENT_ATTACK_MAX = 0.0
TRANSIENT_ATTACK_DEFAULT = -0.5  # Moderate suppression
TRANSIENT_ATTACK_STEP = 0.1

# Stereo Enhancement
STEREO_WIDTH_MIN = 0.0
STEREO_WIDTH_MAX = 1.0
STEREO_WIDTH_DEFAULT = 0.75
STEREO_WIDTH_STEP = 0.05


CROSSFEED_MIN = 0.0
CROSSFEED_MAX = 1.0
CROSSFEED_DEFAULT = 0.45
CROSSFEED_STEP = 0.05

# Equalizer (10-band)
EQ_BAND_MIN = -40.0
EQ_BAND_MAX = 40.0
EQ_BAND_DEFAULT = 0.0
EQ_BAND_STEP = 0.5
EQ_BAND_COUNT = 10

# Frequency centers for 10-band EQ (Hz)
EQ_BANDS = [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
EQ_FREQUENCIES = EQ_BANDS  # Alias for compatibility


# =============================================================================

# =============================================================================
# Dataclasses for Configuration
# =============================================================================


@dataclass
class NoiseReductionConfig:
    """Configuration for AI noise reduction."""

    enabled: bool = True
    model: NoiseModel = NoiseModel.GTCRN_FULL_QUALITY
    strength: float = STRENGTH_DEFAULT


@dataclass
class GateConfig:
    """Configuration for gate filter."""

    enabled: bool = True
    threshold_db: int = GATE_THRESHOLD_DEFAULT
    range_db: int = GATE_RANGE_DEFAULT
    attack_ms: float = GATE_ATTACK_DEFAULT
    hold_ms: float = GATE_HOLD_DEFAULT
    release_ms: float = GATE_RELEASE_DEFAULT


@dataclass
class TransientConfig:
    """Configuration for transient suppressor (click removal)."""

    enabled: bool = False
    attack: float = TRANSIENT_ATTACK_DEFAULT


@dataclass
class StereoConfig:
    """Configuration for stereo enhancement."""

    enabled: bool = True
    mode: StereoMode = StereoMode.DUAL_MONO
    width: float = STEREO_WIDTH_DEFAULT

    crossfeed_enabled: bool = False
    crossfeed_level: float = CROSSFEED_DEFAULT


@dataclass
class EqualizerConfig:
    """Configuration for parametric equalizer."""

    enabled: bool = False
    bands: list[float] = field(
        default_factory=lambda: [EQ_BAND_DEFAULT] * EQ_BAND_COUNT
    )
    preset: str = "flat"


@dataclass
class WindowConfig:
    """Window state configuration."""

    width: int = WINDOW_WIDTH_DEFAULT
    height: int = WINDOW_HEIGHT_DEFAULT
    maximized: bool = False


@dataclass
class UIConfig:
    """UI preferences configuration."""

    visualizer_style: VisualizerStyle = VisualizerStyle.MODERN_WAVES
    show_advanced: bool = False


@dataclass
class BluetoothConfig:
    """Bluetooth audio configuration."""

    auto_switch_headset: bool = False


@dataclass
class MonitorConfig:
    """Headphone monitor configuration."""

    enabled: bool = False
    delay_ms: int = 0  # 0-5000ms
    volume: float = 1.0  # 0.0-2.0 (200%)


@dataclass
class AppSettings:
    """Complete application settings."""

    noise_reduction: NoiseReductionConfig = field(default_factory=NoiseReductionConfig)
    gate: GateConfig = field(default_factory=GateConfig)
    stereo: StereoConfig = field(default_factory=StereoConfig)
    equalizer: EqualizerConfig = field(default_factory=EqualizerConfig)
    transient: TransientConfig = field(default_factory=TransientConfig)
    window: WindowConfig = field(default_factory=WindowConfig)
    ui: UIConfig = field(default_factory=UIConfig)
    bluetooth: BluetoothConfig = field(default_factory=BluetoothConfig)
    monitor: MonitorConfig = field(default_factory=MonitorConfig)

    def to_dict(self) -> dict[str, Any]:
        """Convert settings to dictionary for JSON serialization."""
        data = asdict(self)
        # Convert enums to their values (handle both enum and already-converted values)
        model = self.noise_reduction.model
        data["noise_reduction"]["model"] = (
            model.value if hasattr(model, "value") else model
        )

        mode = self.stereo.mode
        data["stereo"]["mode"] = mode.value if hasattr(mode, "value") else mode

        vis_style = self.ui.visualizer_style
        data["ui"]["visualizer_style"] = (
            vis_style.value if hasattr(vis_style, "value") else vis_style
        )

        return data

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> AppSettings:
        """Create settings from dictionary."""
        settings = cls()

        if "noise_reduction" in data:
            nr = data["noise_reduction"]
            settings.noise_reduction = NoiseReductionConfig(
                enabled=nr.get("enabled", True),
                model=NoiseModel(nr.get("model", 0)),
                strength=nr.get("strength", STRENGTH_DEFAULT),
            )

        if "gate" in data:
            g = data["gate"]
            settings.gate = GateConfig(
                enabled=g.get("enabled", True),
                threshold_db=g.get("threshold_db", GATE_THRESHOLD_DEFAULT),
                range_db=g.get("range_db", GATE_RANGE_DEFAULT),
                attack_ms=g.get("attack_ms", GATE_ATTACK_DEFAULT),
                hold_ms=g.get("hold_ms", GATE_HOLD_DEFAULT),
                release_ms=g.get("release_ms", GATE_RELEASE_DEFAULT),
            )

        if "stereo" in data:
            s = data["stereo"]
            settings.stereo = StereoConfig(
                enabled=s.get("enabled", False),
                mode=StereoMode(
                    "voice_changer"
                    if s.get("mode") in ("squirrel", "hidden")
                    else "dual_mono"
                    if s.get("mode") in ("spatial", "extra_stereo")
                    else s.get("mode", "dual_mono")
                ),
                width=s.get("width", STEREO_WIDTH_DEFAULT),
                crossfeed_enabled=s.get("crossfeed_enabled", False),
                crossfeed_level=s.get("crossfeed_level", CROSSFEED_DEFAULT),
            )

        if "equalizer" in data:
            eq = data["equalizer"]
            settings.equalizer = EqualizerConfig(
                enabled=eq.get("enabled", False),
                bands=eq.get("bands", [EQ_BAND_DEFAULT] * EQ_BAND_COUNT),
                preset=eq.get("preset", "flat"),
            )

        if "transient" in data:
            tr = data["transient"]
            settings.transient = TransientConfig(
                enabled=tr.get("enabled", False),
                attack=tr.get("attack", TRANSIENT_ATTACK_DEFAULT),
            )

        if "window" in data:
            w = data["window"]
            settings.window = WindowConfig(
                width=w.get("width", WINDOW_WIDTH_DEFAULT),
                height=w.get("height", WINDOW_HEIGHT_DEFAULT),
                maximized=w.get("maximized", False),
            )

        if "ui" in data:
            ui = data["ui"]
            settings.ui = UIConfig(
                visualizer_style=VisualizerStyle(ui.get("visualizer_style", 0)),
                show_advanced=ui.get("show_advanced", False),
            )

        if "bluetooth" in data:
            bt = data["bluetooth"]
            settings.bluetooth = BluetoothConfig(
                auto_switch_headset=bt.get("auto_switch_headset", False),
            )

        if "monitor" in data:
            mon = data["monitor"]
            settings.monitor = MonitorConfig(
                enabled=mon.get("enabled", False),
                delay_ms=mon.get("delay_ms", 0),
                volume=mon.get("volume", 1.0),
            )

        return settings


# =============================================================================
# Settings Management Functions
# =============================================================================


def load_settings() -> AppSettings:
    """
    Load settings from the user configuration file.

    Returns:
        AppSettings: Complete application settings (defaults used for missing values)
    """
    if not SETTINGS_FILE.exists():
        logger.info("No settings file found, using defaults")
        return AppSettings()

    try:
        with open(SETTINGS_FILE, encoding="utf-8") as f:
            data = json.load(f)
            return AppSettings.from_dict(data)
    except json.JSONDecodeError as e:
        logger.error("Error parsing settings file: %s", e)
        return AppSettings()
    except OSError as e:
        logger.error("Error reading settings file: %s", e)
        return AppSettings()


def save_settings(settings: AppSettings) -> bool:
    """
    Save settings to the user configuration file.

    Args:
        settings: Application settings to save

    Returns:
        bool: True if save was successful
    """
    try:
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        data = settings.to_dict()
        logger.info(
            "Saving settings: eq.enabled=%s, file=%s",
            data.get("equalizer", {}).get("enabled"),
            SETTINGS_FILE,
        )
        with open(SETTINGS_FILE, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=4)
        logger.debug("Settings saved successfully")
        return True
    except OSError as e:
        logger.error("Error saving settings: %s", e)
        return False


# =============================================================================
# AI Plugin Detection
# =============================================================================


def get_installed_plugins() -> list[AIPlugin]:
    """
    Detect which AI noise reduction plugins are installed.

    Returns:
        List of installed AIPlugin enums
    """
    installed = []

    if GTCRN_PLUGIN.exists():
        installed.append(AIPlugin.GTCRN)

    return installed


def get_available_models() -> list[NoiseModel]:
    """
    Get list of available AI models based on installed plugins.

    Returns:
        List of available NoiseModel enums
    """
    available_models = []
    installed_plugins = get_installed_plugins()

    for plugin in installed_plugins:
        available_models.extend(PLUGIN_MODELS[plugin])

    return available_models


def get_plugin_for_model(model: NoiseModel) -> AIPlugin | None:
    """
    Get the plugin type for a given model.

    Args:
        model: The NoiseModel to check

    Returns:
        AIPlugin enum or None if not found
    """
    for plugin, models in PLUGIN_MODELS.items():
        if model in models:
            return plugin
    return None


def get_model_control_value(model: NoiseModel) -> int:
    """
    Get the LADSPA control value for a model within its plugin.

    Args:
        model: The NoiseModel to get control value for

    Returns:
        Control value (0-based index within plugin's model list)
    """
    plugin = get_plugin_for_model(model)
    if plugin is None:
        return 0

    models = PLUGIN_MODELS[plugin]
    try:
        return models.index(model)
    except ValueError:
        return 0


# =============================================================================
# Equalizer Presets
# =============================================================================

EQ_PRESETS: dict[str, dict[str, Any]] = {
    "flat": {
        "name": _("Natural (No Effects)"),
        "description": _("Original sound without changes."),
        "bands": [0.0] * EQ_BAND_COUNT,
    },
    "voice_boost": {
        "name": _("Crystal Voice"),
        "description": _("Focuses on clarity and intelligibility."),
        "bands": [-10.0, -5.0, 0.0, 5.0, 15.0, 20.0, 15.0, 10.0, 5.0, 0.0],
    },
    "podcast": {
        "name": _("Radio Host"),
        "description": _("Full and deep voice (Broadcast)."),
        "bands": [5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -5.0],
    },
    "warm": {
        "name": _("Velvet Voice"),
        "description": _("Warm and pleasant tone."),
        "bands": [10.0, 15.0, 10.0, 5.0, 0.0, -5.0, -10.0, -15.0, -15.0, -20.0],
    },
    "bright": {
        "name": _("Extra Brightness"),
        "description": _("Enhance treble and details."),
        "bands": [-10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 20.0, 15.0],
    },
    "de_esser": {
        "name": _("Soften 'S' (De-esser)"),
        "description": _("Reduces sibilance and annoying high-pitched sounds."),
        "bands": [0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -15.0, -25.0, -20.0, -10.0],
    },
    "bass_cut": {
        "name": _("Remove Rumble"),
        "description": _("Eliminates low-frequency noise (trucks, bumps)."),
        "bands": [-40.0, -35.0, -25.0, -15.0, -5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    "presence": {
        "name": _("Present Voice"),
        "description": _("Brings the voice to the 'front' of the mix."),
        "bands": [-5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 15.0, 10.0, 5.0, 0.0],
    },
    "custom": {
        "name": _("Custom"),
        "description": _("Manual adjustment."),
        "bands": [0.0] * EQ_BAND_COUNT,
    },
}
