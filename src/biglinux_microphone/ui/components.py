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


def create_spin_row(
    title: str,
    subtitle: str | None = None,
    min_value: float = 0.0,
    max_value: float = 100.0,
    value: float = 50.0,
    step: float = 1.0,
    digits: int = 0,
    on_changed: Callable[[float], None] | None = None,
) -> Adw.SpinRow:
    """
    Create a spin row with numeric input.

    Args:
        title: Row title
        subtitle: Optional subtitle
        min_value: Minimum value
        max_value: Maximum value
        value: Initial value
        step: Step increment
        digits: Decimal digits
        on_changed: Callback when value changes

    Returns:
        Adw.SpinRow: Configured spin row
    """
    adjustment = Gtk.Adjustment(
        value=value,
        lower=min_value,
        upper=max_value,
        step_increment=step,
        page_increment=step * 10,
    )

    row = Adw.SpinRow()
    row.set_title(title)
    row.set_adjustment(adjustment)
    row.set_digits(digits)

    if subtitle:
        row.set_subtitle(subtitle)

    if on_changed:
        row.connect("notify::value", lambda r, _: on_changed(r.get_value()))

    return row


def create_expander_row(
    title: str,
    subtitle: str | None = None,
    icon_name: str | None = None,
    enable_switch: bool = False,
    expanded: bool = False,
) -> Adw.ExpanderRow:
    """
    Create an expander row with optional switch.

    Args:
        title: Row title
        subtitle: Optional subtitle
        icon_name: Optional icon name
        enable_switch: Show enable switch
        expanded: Initially expanded state

    Returns:
        Adw.ExpanderRow: Configured expander row
    """
    row = Adw.ExpanderRow()
    row.set_title(title)
    row.set_expanded(expanded)
    row.set_enable_expansion(True)
    row.set_show_enable_switch(enable_switch)

    if subtitle:
        row.set_subtitle(subtitle)

    if icon_name:
        row.set_icon_name(icon_name)

    return row


def create_button_row(
    label: str,
    style_class: str | None = None,
    on_clicked: Callable[[], None] | None = None,
) -> Gtk.Button:
    """
    Create a styled button.

    Args:
        label: Button label
        style_class: Optional CSS class (e.g., 'suggested-action', 'destructive-action')
        on_clicked: Click callback

    Returns:
        Gtk.Button: Configured button
    """
    button = Gtk.Button(label=label)
    button.set_valign(Gtk.Align.CENTER)

    if style_class:
        button.add_css_class(style_class)

    if on_clicked:
        button.connect("clicked", lambda _: on_clicked())

    return button


def create_icon_button(
    icon_name: str,
    tooltip: str | None = None,
    style_class: str | None = None,
    on_clicked: Callable[[], None] | None = None,
) -> Gtk.Button:
    """
    Create an icon-only button.

    Args:
        icon_name: Icon name
        tooltip: Tooltip text
        style_class: Optional CSS class
        on_clicked: Click callback

    Returns:
        Gtk.Button: Configured icon button
    """
    button = Gtk.Button.new_from_icon_name(icon_name)
    button.set_valign(Gtk.Align.CENTER)

    if tooltip:
        button.set_tooltip_text(tooltip)

    if style_class:
        button.add_css_class(style_class)
    else:
        button.add_css_class("flat")

    if on_clicked:
        button.connect("clicked", lambda _: on_clicked())

    return button


def show_toast(
    overlay: Adw.ToastOverlay,
    message: str,
    timeout: int = 3,
) -> None:
    """
    Show a toast notification.

    Args:
        overlay: Toast overlay widget
        message: Toast message
        timeout: Duration in seconds
    """
    toast = Adw.Toast(title=message)
    toast.set_timeout(timeout)
    overlay.add_toast(toast)


def create_status_page(
    icon_name: str,
    title: str,
    description: str | None = None,
) -> Adw.StatusPage:
    """
    Create a status page with icon, title, and description.

    Args:
        icon_name: Icon name
        title: Page title
        description: Page description

    Returns:
        Adw.StatusPage: Configured status page
    """
    page = Adw.StatusPage()
    page.set_icon_name(icon_name)
    page.set_title(title)

    if description:
        page.set_description(description)

    return page


def create_level_bar(
    min_value: float = 0.0,
    max_value: float = 1.0,
    value: float = 0.0,
) -> Gtk.LevelBar:
    """
    Create a level bar for audio visualization.

    Args:
        min_value: Minimum value
        max_value: Maximum value
        value: Initial value

    Returns:
        Gtk.LevelBar: Configured level bar
    """
    level_bar = Gtk.LevelBar()
    level_bar.set_min_value(min_value)
    level_bar.set_max_value(max_value)
    level_bar.set_value(value)
    level_bar.set_hexpand(True)

    # Add standard offset marks
    level_bar.add_offset_value("low", 0.3)
    level_bar.add_offset_value("high", 0.6)
    level_bar.add_offset_value("full", 0.9)

    return level_bar


def create_action_button(
    label: str,
    icon_name: str | None = None,
    style: str = "suggested-action",
    on_clicked: Callable[[], None] | None = None,
    tooltip: str | None = None,
) -> Gtk.Button:
    """
    Create an action button with consistent styling.

    Use this factory for primary action buttons in dialogs and views.

    Args:
        label: Button text
        icon_name: Optional icon name
        style: CSS class ('suggested-action', 'destructive-action', 'flat', 'pill')
        on_clicked: Click callback
        tooltip: Optional tooltip text

    Returns:
        Gtk.Button: Styled action button
    """
    if icon_name:
        button = Gtk.Button()
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        box.set_halign(Gtk.Align.CENTER)

        icon = Gtk.Image.new_from_icon_name(icon_name)
        box.append(icon)

        lbl = Gtk.Label(label=label)
        box.append(lbl)

        button.set_child(box)
    else:
        button = Gtk.Button(label=label)

    button.set_valign(Gtk.Align.CENTER)

    if style:
        button.add_css_class(style)

    if tooltip:
        button.set_tooltip_text(tooltip)

    if on_clicked:
        button.connect("clicked", lambda _: on_clicked())

    return button


def create_navigation_button(
    label: str,
    icon_name: str = "go-next-symbolic",
    subtitle: str | None = None,
    on_clicked: Callable[[], None] | None = None,
) -> Adw.ActionRow:
    """
    Create a navigation row that looks like a button.

    Use this factory for navigation to sub-views/pages.

    Args:
        label: Row title
        icon_name: Arrow icon (default: go-next-symbolic)
        subtitle: Optional description
        on_clicked: Navigation callback

    Returns:
        Adw.ActionRow: Clickable navigation row
    """
    row = Adw.ActionRow()
    row.set_title(label)
    row.set_activatable(True)

    if subtitle:
        row.set_subtitle(subtitle)

    # Add navigation arrow
    arrow = Gtk.Image.new_from_icon_name(icon_name)
    arrow.add_css_class("dim-label")
    row.add_suffix(arrow)

    if on_clicked:
        row.connect("activated", lambda _: on_clicked())

    return row


def create_header_bar(
    title: str | None = None,
    show_back: bool = False,
    on_back: Callable[[], None] | None = None,
) -> Adw.HeaderBar:
    """
    Create a header bar with optional back button.

    Args:
        title: Optional title widget text
        show_back: Show back button
        on_back: Back button callback

    Returns:
        Adw.HeaderBar: Configured header bar
    """
    header = Adw.HeaderBar()

    if title:
        title_label = Gtk.Label(label=title)
        title_label.add_css_class("title")
        header.set_title_widget(title_label)

    if show_back:
        back_btn = Gtk.Button.new_from_icon_name("go-previous-symbolic")
        back_btn.set_tooltip_text("Go Back")
        if on_back:
            back_btn.connect("clicked", lambda _: on_back())
        header.pack_start(back_btn)

    return header


def create_card(
    child: Gtk.Widget,
    margin: int = 12,
) -> Gtk.Frame:
    """
    Create a card-style container.

    Args:
        child: Widget to contain
        margin: Internal margin

    Returns:
        Gtk.Frame: Card frame
    """
    frame = Gtk.Frame()
    frame.add_css_class("card")

    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
    box.set_margin_top(margin)
    box.set_margin_bottom(margin)
    box.set_margin_start(margin)
    box.set_margin_end(margin)
    box.append(child)

    frame.set_child(box)
    return frame


def create_toggle_group(
    options: list[str],
    selected: int = 0,
    on_selected: Callable[[int], None] | None = None,
) -> Gtk.Box:
    """
    Create a toggle button group (mutually exclusive).

    Args:
        options: List of option labels
        selected: Initially selected index
        on_selected: Selection callback

    Returns:
        Gtk.Box: Container with toggle buttons
    """
    box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
    box.add_css_class("linked")

    buttons: list[Gtk.ToggleButton] = []

    for i, option in enumerate(options):
        btn = Gtk.ToggleButton(label=option)
        btn.set_active(i == selected)

        def make_handler(index: int) -> Callable[[Gtk.ToggleButton], None]:
            def handler(button: Gtk.ToggleButton) -> None:
                if button.get_active():
                    # Deactivate others
                    for j, other in enumerate(buttons):
                        if j != index:
                            other.set_active(False)
                    if on_selected:
                        on_selected(index)

            return handler

        btn.connect("toggled", make_handler(i))
        buttons.append(btn)
        box.append(btn)

    return box


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
