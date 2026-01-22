"""
UI package for BigLinux Microphone Settings.

Contains all GTK4/Libadwaita UI components.
"""

from biglinux_microphone.ui.base_view import BaseView, PreferencesView, ScrollableView
from biglinux_microphone.ui.components import (
    create_action_button,
    create_action_row_with_scale,
    create_action_row_with_switch,
    create_card,
    create_combo_row,
    create_compact_eq_slider,
    create_expander_row_with_switch,
    create_header_bar,
    create_navigation_button,
    create_preferences_group,
    create_spin_row,
    create_toggle_group,
)
from biglinux_microphone.ui.main_view import MainView
from biglinux_microphone.ui.spectrum_widget import SpectrumAnalyzerWidget
from biglinux_microphone.ui.visualizer_widget import (
    DualLevelMeter,
    LevelBar,
    SpectrumVisualizerWidget,
    WaveformWidget,
)

__all__ = [
    # Component factories (required by PLANNING.md)
    "create_action_button",
    "create_navigation_button",
    # Other component factories
    "create_action_row_with_switch",
    "create_action_row_with_scale",
    "create_combo_row",
    "create_spin_row",
    "create_preferences_group",
    "create_header_bar",
    "create_card",
    "create_toggle_group",
    "create_expander_row_with_switch",
    "create_compact_eq_slider",
    # Base views
    "BaseView",
    "ScrollableView",
    "PreferencesView",
    # Main view (unified)
    "MainView",
    # Visualizer widgets
    "SpectrumAnalyzerWidget",
    "LevelBar",
    "SpectrumVisualizerWidget",
    "DualLevelMeter",
    "WaveformWidget",
]
