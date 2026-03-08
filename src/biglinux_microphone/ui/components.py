#!/usr/bin/env python3
"""
Reusable UI components for BigLinux Microphone Settings.

Factory functions for creating consistent Adwaita widgets.
"""

from __future__ import annotations

from collections.abc import Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk


def create_preferences_group(
    title: str,
    description: str | None = None,
) -> Adw.PreferencesGroup:
    """
    Create a preferences group with title and optional description.

    Args:
        title: Group title
        description: Optional description text

    Returns:
        Adw.PreferencesGroup: Configured preferences group
    """
    group = Adw.PreferencesGroup()
    group.set_title(title)

    if description:
        group.set_description(description)

    return group


def create_action_row_with_switch(
    title: str,
    subtitle: str | None = None,
    active: bool = False,
    on_toggled: Callable[[bool], None] | None = None,
) -> tuple[Adw.ActionRow, Gtk.Switch]:
    """
    Create an action row with a switch.

    Args:
        title: Row title
        subtitle: Optional subtitle
        active: Initial switch state
        on_toggled: Callback when switch is toggled

    Returns:
        tuple: (Adw.ActionRow, Gtk.Switch)
    """
    row = Adw.ActionRow()
    row.set_title(title)

    if subtitle:
        row.set_subtitle(subtitle)

    switch = Gtk.Switch()
    switch.set_active(active)
    switch.set_valign(Gtk.Align.CENTER)
    switch.update_property(
        [Gtk.AccessibleProperty.LABEL],
        [title],
    )

    if on_toggled:
        switch.connect("notify::active", lambda s, _: on_toggled(s.get_active()))

    row.add_suffix(switch)
    row.set_activatable_widget(switch)

    return row, switch


def create_action_row_with_scale(
    title: str,
    subtitle: str | None = None,
    min_value: float = 0.0,
    max_value: float = 1.0,
    value: float = 0.5,
    step: float = 0.1,
    digits: int = 1,
    on_changed: Callable[[float], None] | None = None,
    marks: list[tuple[float, str]] | None = None,
) -> tuple[Adw.ActionRow, Gtk.Scale]:
    """
    Create an action row with a horizontal scale slider.

    Args:
        title: Row title
        subtitle: Optional subtitle
        min_value: Minimum scale value
        max_value: Maximum scale value
        value: Initial value
        step: Step increment
        digits: Number of decimal digits
        on_changed: Callback when value changes
        marks: Optional list of (value, label) tuples for scale marks

    Returns:
        tuple: (Adw.ActionRow, Gtk.Scale)
    """
    row = Adw.ActionRow()
    row.set_title(title)

    if subtitle:
        row.set_subtitle(subtitle)

    # Create scale
    adjustment = Gtk.Adjustment(
        value=value,
        lower=min_value,
        upper=max_value,
        step_increment=step,
        page_increment=step * 10,
    )

    scale = Gtk.Scale(
        orientation=Gtk.Orientation.HORIZONTAL,
        adjustment=adjustment,
    )
    scale.set_digits(digits)
    scale.set_hexpand(True)
    scale.set_size_request(200, -1)
    scale.set_valign(Gtk.Align.CENTER)
    scale.update_property(
        [Gtk.AccessibleProperty.LABEL],
        [title],
    )

    # Add marks if provided
    if marks:
        for mark_value, mark_label in marks:
            scale.add_mark(mark_value, Gtk.PositionType.BOTTOM, mark_label)

    if on_changed:

        def _on_value_changed(s: Gtk.Scale) -> None:
            value = s.get_value()
            on_changed(value)

        scale.connect("value-changed", _on_value_changed)

    row.add_suffix(scale)

    return row, scale


def create_combo_row(
    title: str,
    subtitle: str | None = None,
    options: list[str] | None = None,
    selected_index: int = 0,
    on_selected: Callable[[int], None] | None = None,
) -> Adw.ComboRow:
    """
    Create a combo row with dropdown options.

    Args:
        title: Row title
        subtitle: Optional subtitle
        options: List of option strings
        selected_index: Initially selected index
        on_selected: Callback when selection changes

    Returns:
        Adw.ComboRow: Configured combo row
    """
    row = Adw.ComboRow()
    row.set_title(title)

    if subtitle:
        row.set_subtitle(subtitle)

    if options:
        model = Gtk.StringList.new(options)
        row.set_model(model)
        row.set_selected(selected_index)

    if on_selected:
        row.connect("notify::selected", lambda r, _: on_selected(r.get_selected()))

    return row


def create_expander_row_with_switch(
    title: str,
    subtitle: str | None = None,
    icon_name: str | None = None,
    active: bool = False,
    expanded: bool = False,
    on_toggled: Callable[[bool], None] | None = None,
) -> tuple[Adw.ExpanderRow, Gtk.Switch]:
    """
    Create an expander row with an integrated enable switch in the header.

    The switch controls both the feature state and provides visual feedback.
    The expander allows collapsing detailed settings.

    Args:
        title: Row title
        subtitle: Optional subtitle
        icon_name: Optional icon name
        active: Initial switch state
        expanded: Initial expanded state
        on_toggled: Callback when switch is toggled

    Returns:
        tuple: (Adw.ExpanderRow, Gtk.Switch)
    """
    row = Adw.ExpanderRow()
    row.set_title(title)
    row.set_expanded(expanded)
    row.set_enable_expansion(True)

    if subtitle:
        row.set_subtitle(subtitle)

    if icon_name:
        row.set_icon_name(icon_name)

    # Create the switch for the header
    switch = Gtk.Switch()
    switch.set_active(active)
    switch.set_valign(Gtk.Align.CENTER)
    switch.update_property(
        [Gtk.AccessibleProperty.LABEL],
        [title],
    )

    if on_toggled:
        switch.connect("notify::active", lambda s, _: on_toggled(s.get_active()))

    row.add_suffix(switch)

    return row, switch


def create_compact_eq_slider(
    index: int,
    freq: int,
    value: float = 0.0,
    on_changed: Callable[[int, float], None] | None = None,
) -> Gtk.Box:
    """
    Create a compact vertical EQ slider with frequency label.

    Args:
        index: Band index (0-9)
        freq: Frequency in Hz
        value: Initial value in dB
        on_changed: Callback with (index, value) when changed

    Returns:
        Gtk.Box: Vertical box with slider and label
    """
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
    box.set_size_request(40, -1)

    # Value label
    value_label = Gtk.Label(label=f"{value:+.0f}")
    value_label.add_css_class("caption")
    value_label.add_css_class("numeric")
    box.append(value_label)

    # Vertical slider
    adjustment = Gtk.Adjustment(
        value=value,
        lower=-40.0,
        upper=40.0,
        step_increment=0.5,
        page_increment=2.0,
    )

    scale = Gtk.Scale(
        orientation=Gtk.Orientation.VERTICAL,
        adjustment=adjustment,
    )
    scale.set_inverted(True)  # High values at top
    scale.set_draw_value(False)
    scale.set_size_request(-1, 100)
    scale.set_vexpand(True)
    freq_str = f"{freq // 1000}k Hz" if freq >= 1000 else f"{freq} Hz"
    scale.update_property(
        [Gtk.AccessibleProperty.LABEL],
        [f"EQ {freq_str}"],
    )

    # Add center mark
    scale.add_mark(0.0, Gtk.PositionType.RIGHT, None)

    def _on_value_changed(s: Gtk.Scale) -> None:
        val = s.get_value()
        value_label.set_text(f"{val:+.0f}")
        if on_changed:
            on_changed(index, val)

    scale.connect("value-changed", _on_value_changed)
    box.append(scale)

    # Frequency label
    freq_label = Gtk.Label()
    if freq >= 1000:
        freq_label.set_text(f"{freq // 1000}k")
    else:
        freq_label.set_text(str(freq))
    freq_label.add_css_class("caption")
    freq_label.add_css_class("dim-label")
    box.append(freq_label)

    return box
