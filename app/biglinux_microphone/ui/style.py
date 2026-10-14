"""The CSS for the visualisations and the sidebar's setting cards.

Ordinary controls use libadwaita defaults, so the application inherits
light/dark/high-contrast themes; the cards are the ones of the BigLinux
converters.
"""

from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gtk

CSS = """
.activity-card { padding: 12px 14px 8px; border-radius: 12px; }
.activity-meter { color: @success_color; }
.eq-bands { padding: 14px 10px; }
.numeric { font-variant-numeric: tabular-nums; }
.pass-progress trough { min-height: 10px; border-radius: 6px; }
.pass-progress progress { min-height: 10px; border-radius: 6px; }
.preset-button { padding: 4px 8px; }
.live-panel { padding: 0 24px 18px; }
.live-card { padding: 16px 18px; }
.status-badge {
    min-width: 56px;
    min-height: 56px;
    border-radius: 9999px;
    background-color: alpha(currentColor, 0.08);
}
/* Filled while the reduction works: the state shows in a shape too. */
.status-badge.working {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
}

/* Sidebar settings: rows that show what they apply, each opening a card.
   The ring and tint come and go at once: Adwaita would fade them. */
row.setting-row { padding: 6px 12px; min-height: 48px; transition: none; }
/* The row under the pointer takes a rounded ring in the accent, 3 px inside
   its edges. Once its card opens, a bridge as tall as the ring joins the row
   to the card in the card's surface, the ring running straight along it
   into the card's own (2 px wide and 3 px in, as the kit's RING and
   RING_INSET). While a card is open no other row lights up. */
scrolledwindow.sidebar:not(.card-open) list > row.setting-row:hover:not(:disabled),
list > row.setting-row.card-open {
    border-radius: 13px;
    outline: 2px solid alpha(@accent_bg_color, 0.6);
    outline-offset: -5px;
}
list > row.setting-row.card-open { background-color: @popover_bg_color; }
list > row.setting-row.card-open:dir(ltr) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
}
list > row.setting-row.card-open:dir(rtl) {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
}
.card-tab.setting-card { color: @popover_bg_color; }
.card-tab > .card-tab-edge { color: transparent; }
.card-tab.setting-card > .card-tab-edge { color: alpha(@accent_bg_color, 0.6); }
.card-description { padding: 16px 16px 0; opacity: 0.75; }

/* A card's choices are tiles with a faint outline; the picked one is tinted,
   outlined in the accent and named in bold. The radio stays for screen
   readers and the keyboard, out of sight. */
checkbutton.option {
    min-height: 24px;
    padding: 10px 12px;
    border-radius: 10px;
    box-shadow: inset 0 0 0 1px alpha(currentColor, 0.18);
}
checkbutton.option > radio {
    opacity: 0;
    min-width: 0;
    min-height: 0;
    margin: 0;
    padding: 0;
    border: none;
    -gtk-icon-size: 0;
}
checkbutton.option:hover { background-color: alpha(currentColor, 0.06); }
checkbutton.option:checked {
    background-color: alpha(@accent_bg_color, 0.12);
    box-shadow: inset 0 0 0 2px alpha(@accent_bg_color, 0.7);
}
checkbutton.option:checked .option-name { font-weight: 600; }
/* Solid, so the focus stands out 3:1 from the card, and outside the tile,
   apart from the outline that marks it picked. */
checkbutton.option:focus-visible {
    outline: 2px solid @accent_color;
    outline-offset: 2px;
}
.option-detail { font-size: 0.9em; opacity: 0.75; }

/* A card opens over the content: its accent ring and the shadow it casts
   with its bridge (drawn by the kit in the card-shade color) lift it. */
.hover-card {
    background-color: @popover_bg_color;
    color: @popover_fg_color;
    border-radius: 12px;
}
.hover-card.setting-card { box-shadow: inset 0 0 0 2px alpha(@accent_bg_color, 0.6); }
.card-shade.setting-card { color: #10131e; }
.hover-card.side-right.joined-start { border-top-left-radius: 0; }
.hover-card.side-right.joined-end { border-bottom-left-radius: 0; }
.hover-card.side-left.joined-start { border-top-right-radius: 0; }
.hover-card.side-left.joined-end { border-bottom-right-radius: 0; }
/* The bridge runs over the sidebar's edge, where its overlay scrollbar would
   show beside it. */
scrolledwindow.sidebar.card-open > scrollbar { opacity: 0; }
/* A grayed row dims its name and icon, not the reason it gives in place of
   its value: that has to stay readable. */
row.setting-row:disabled .setting-title,
row.setting-row:disabled .setting-badge { opacity: 0.45; }
row.setting-row:disabled .setting-reason {
    color: @card_fg_color;
    opacity: 0.7;
    filter: none;
}
.setting-badge {
    min-width: 32px;
    min-height: 32px;
    border-radius: 9999px;
    background-color: alpha(currentColor, 0.08);
}
/* Filled while the setting applies: the state shows in a shape too. */
.setting-row.applied .setting-badge {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
}
.setting-row.applied .setting-value {
    color: @accent_color;
    font-weight: bold;
}
/* While its card is open the card shows the value, so the row's own hides,
   keeping its place so nothing on the row moves. */
.setting-row.card-open .setting-value { opacity: 0; }
"""


# A view can be recreated; installing one provider per view accumulates CSS
# providers on the display for the whole lifetime of the process.
_providers: dict[Gdk.Display, Gtk.CssProvider] = {}


def install(display: Gdk.Display | None = None) -> None:
    display = display or Gdk.Display.get_default()
    if display is None or display in _providers:
        return
    provider = Gtk.CssProvider()
    provider.load_from_string(CSS)
    Gtk.StyleContext.add_provider_for_display(
        display, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    )
    _providers[display] = provider
