"""
UI package for BigLinux Microphone Settings.

Contains all GTK4/Libadwaita UI components.
"""

from biglinux_microphone.ui.components import (
    create_action_row_with_scale,
    create_action_row_with_switch,
    create_combo_row,
    create_compact_eq_slider,
    create_expander_row_with_switch,
    create_preferences_group,
)
from biglinux_microphone.ui.main_view import MainView
from biglinux_microphone.ui.spectrum_widget import SpectrumAnalyzerWidget

__all__ = [
    # Component factories
    "create_action_row_with_switch",
    "create_action_row_with_scale",
    "create_combo_row",
    "create_preferences_group",
    "create_expander_row_with_switch",
    "create_compact_eq_slider",
    # Main view (unified)
    "MainView",
    # Visualizer widgets
    "SpectrumAnalyzerWidget",
]
