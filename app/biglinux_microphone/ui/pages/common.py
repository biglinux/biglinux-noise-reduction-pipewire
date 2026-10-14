from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, GObject, Gtk


def action_button(
    label: str, icon: str | None = None, *, primary: bool = False
) -> Gtk.Button:
    content = Gtk.Box(spacing=8, halign=Gtk.Align.CENTER)
    if icon:
        content.append(Gtk.Image.new_from_icon_name(icon))
    content.append(Gtk.Label(label=label, wrap=True, justify=Gtk.Justification.CENTER))
    button = Gtk.Button(child=content)
    button.update_property([Gtk.AccessibleProperty.LABEL], [label])
    button.add_css_class("pill")
    if primary:
        button.add_css_class("suggested-action")
    return button


def adaptive_button_row(*buttons: Gtk.Widget) -> Gtk.FlowBox:
    """Wrap action buttons instead of clipping at narrow widths or large text."""
    flow = Gtk.FlowBox(
        selection_mode=Gtk.SelectionMode.NONE,
        homogeneous=True,
        column_spacing=8,
        row_spacing=8,
        min_children_per_line=1,
        max_children_per_line=max(1, len(buttons)),
    )
    flow.set_activate_on_single_click(False)
    for button in buttons:
        if hasattr(button, "set_hexpand"):
            button.set_hexpand(True)
        flow.insert(button, -1)
        wrapper = button.get_parent()
        wrapper.set_focusable(False)
        button.bind_property(
            "visible", wrapper, "visible", GObject.BindingFlags.SYNC_CREATE
        )
    return flow


def attach_chevron(row: Adw.ActionRow) -> None:
    row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
    row.set_activatable(True)
