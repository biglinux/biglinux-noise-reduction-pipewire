// SPDX-License-Identifier: MIT

//! `BigZoneLayoutEditor` — the visual "drag controls between zones" editor
//! (RESTRUCTURE phase **D3** / decision **D8**).
//!
//! A miniature of the target chrome (one `CenterBox` row per
//! [`BarSpec`](super::zone_layout::BarSpec), with
//! start/center/end drop zones) where the user **drags** controls between zones
//! and **clicks** to toggle a control's visibility. It is persistence-agnostic:
//! it edits an in-memory [`ZoneLayout`] and emits it through
//! [`connect_changed`](BigZoneLayoutEditor::connect_changed) on every change —
//! the app maps that to its own store (video-player → `ui-zone-*`/`ui-show-*`
//! gsettings, big-shell → its panel config).
//!
//! Generalizes the bespoke editor that previously lived only in
//! `big-video-player` (`appearance/layout/{buttons_page,miniature}.rs`).
//!
//! Leak contract (H9): the editor's widgets are owned solely by its `root`
//! frame; the drag/drop/click closures capture an `Rc<EditorState>` that holds
//! only WEAK widget refs (no widget⇄closure strong cycle), so dropping the
//! editor finalizes the whole subtree.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::feedback::tooltip;

use super::zone_layout::{ZoneLayout, ZoneLayoutSpec, ZonePlacement, slot_key};

/// Change sink: invoked with the new [`ZoneLayout`] on every edit.
type ChangeCallback = Box<dyn Fn(&ZoneLayout)>;

/// Self-contained CSS for the miniature (one provider per display, replaced on
/// re-injection). Dark, theme-independent — the miniature is a realistic preview
/// of chrome that is itself usually dark, regardless of the app's GTK theme.
const EDITOR_CSS: &str = "
.zle-frame { border-radius: 10px; border: 1px solid alpha(@borders, 0.5); }
.zle-layout { background: #1a1a1e; min-height: 120px; }
.zle-bar { background: rgba(42,42,48,0.9); padding: 4px 8px; min-height: 34px; }
.zle-seek { margin: 2px 8px 0; opacity: 0.4; }
.zle-zone {
    min-height: 28px; min-width: 36px; padding: 2px 4px; border-radius: 6px;
    border: 1.5px dashed alpha(white, 0.18); transition: all 200ms ease;
}
.zle-zone-active { border-color: @accent_color; background: alpha(@accent_color, 0.18); border-style: solid; }
.zle-button {
    min-width: 28px; min-height: 28px; padding: 2px; border-radius: 6px;
    background: rgba(255,255,255,0.08); color: white; transition: opacity 150ms ease;
}
.zle-button:hover { background: rgba(255,255,255,0.20); }
.zle-title { color: alpha(white, 0.45); font-size: 11px; font-style: italic; }
";

/// Install [`EDITOR_CSS`] once per display (replacing a previous provider).
fn inject_css() {
    thread_local! {
        static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(EDITOR_CSS);
    PROVIDER.with(|cell| {
        if let Some(old) = cell.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
    });
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
    PROVIDER.with(|cell| *cell.borrow_mut() = Some(provider));
}

/// Shared editor state captured by the gesture closures. Holds only WEAK widget
/// refs + plain data, so capturing `Rc<EditorState>` in a closure-on-a-widget
/// never closes a strong widget cycle.
struct EditorState {
    /// Slot keys in spec render order (drives the rebuild walk).
    slot_keys: Vec<String>,
    /// `slot_key` → the zone box (weak; owned by the widget tree).
    slot_boxes: HashMap<String, glib::WeakRef<gtk::Box>>,
    /// `control id` → its draggable button (weak).
    buttons: HashMap<String, glib::WeakRef<gtk::Button>>,
    /// The live edited value (placements rebuilt from the tree; `hidden` from clicks).
    layout: RefCell<ZoneLayout>,
    /// The change sink set via [`BigZoneLayoutEditor::connect_changed`].
    on_changed: RefCell<Option<ChangeCallback>>,
}

impl EditorState {
    /// Rebuild `layout.slots` from the live widget tree (each zone box's children
    /// in order), keep `hidden` as-is, and notify the change sink.
    fn rebuild_and_notify(&self) {
        let mut slots: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for key in &self.slot_keys {
            let mut ids = Vec::new();
            if let Some(zone) = self.slot_boxes.get(key).and_then(glib::WeakRef::upgrade) {
                let mut child = zone.first_child();
                while let Some(widget) = child {
                    ids.push(widget.widget_name().to_string());
                    child = widget.next_sibling();
                }
            }
            slots.insert(key.clone(), ids);
        }
        let snapshot = {
            let mut layout = self.layout.borrow_mut();
            layout.slots = slots;
            layout.clone()
        };
        if let Some(cb) = self.on_changed.borrow().as_ref() {
            cb(&snapshot);
        }
    }

    /// Toggle a control's visibility (opacity + `hidden`) and notify.
    fn toggle_visibility(&self, id: &str, button: &gtk::Button) {
        let now_visible = {
            let mut layout = self.layout.borrow_mut();
            let visible = !layout.is_visible(id);
            layout.set_visible(id, visible);
            visible
        };
        button.set_opacity(if now_visible { 1.0 } else { 0.3 });
        self.rebuild_and_notify();
    }
}

/// Visual zone-layout editor. Build with [`new`](Self::new), embed
/// [`root`](Self::root), read the current value with [`layout`](Self::layout),
/// and subscribe with [`connect_changed`](Self::connect_changed).
pub struct BigZoneLayoutEditor {
    root: gtk::Frame,
    state: Rc<EditorState>,
}

impl BigZoneLayoutEditor {
    /// Build the editor for `spec`, populated from `initial` (which is
    /// [`normalized`](ZoneLayout::normalized) against the spec first).
    #[must_use]
    pub fn new(spec: &ZoneLayoutSpec, initial: &ZoneLayout) -> Self {
        inject_css();
        let layout = initial.normalized(spec);

        // Zone boxes, keyed by slot, owned by the bars built below.
        let mut slot_boxes_strong: HashMap<String, gtk::Box> = HashMap::new();
        for key in spec.slot_keys() {
            let zone = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            zone.add_css_class("zle-zone");
            zone.set_widget_name(&key);
            zone.set_hexpand(true);
            slot_boxes_strong.insert(key, zone);
        }

        // Buttons, keyed by control id.
        let mut buttons_strong: HashMap<String, gtk::Button> = HashMap::new();
        for control in &spec.controls {
            let button = gtk::Button::from_icon_name(&control.icon);
            button.set_widget_name(&control.id);
            button.add_css_class("zle-button");
            tooltip::set(&button, &control.label);
            button.update_property(&[gtk::accessible::Property::Label(&control.label)]);
            button.set_cursor_from_name(Some("grab"));
            button.set_opacity(if layout.is_visible(&control.id) {
                1.0
            } else {
                0.3
            });
            buttons_strong.insert(control.id.clone(), button);
        }

        // Place each control into its zone, in the layout's order.
        for key in spec.slot_keys() {
            if let Some(zone) = slot_boxes_strong.get(&key)
                && let Some(ids) = layout.slots.get(&key)
            {
                for id in ids {
                    if let Some(button) = buttons_strong.get(id) {
                        zone.append(button);
                    }
                }
            }
        }

        // Shared state (weak widget refs only).
        let state = Rc::new(EditorState {
            slot_keys: spec.slot_keys(),
            slot_boxes: slot_boxes_strong
                .iter()
                .map(|(k, b)| (k.clone(), b.downgrade()))
                .collect(),
            buttons: buttons_strong
                .iter()
                .map(|(k, b)| (k.clone(), b.downgrade()))
                .collect(),
            layout: RefCell::new(layout),
            on_changed: RefCell::new(None),
        });

        // Per-control: drag source + click-to-toggle.
        for (id, button) in &buttons_strong {
            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            let drag_id = id.clone();
            drag.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&drag_id.to_value()))
            });
            button.add_controller(drag);

            let state = state.clone();
            let click_id = id.clone();
            button.connect_clicked(move |button| state.toggle_visibility(&click_id, button));
        }

        // Per-zone: drop target (reorder by pointer-x + reparent).
        for (key, zone) in &slot_boxes_strong {
            // bigagents: app-local-dnd — moves internal control ids between zones.
            let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            {
                let zone = zone.downgrade();
                drop.connect_enter(move |_, _, _| {
                    if let Some(zone) = zone.upgrade() {
                        zone.add_css_class("zle-zone-active");
                    }
                    gdk::DragAction::MOVE
                });
            }
            {
                let zone = zone.downgrade();
                drop.connect_leave(move |_| {
                    if let Some(zone) = zone.upgrade() {
                        zone.remove_css_class("zle-zone-active");
                    }
                });
            }
            {
                let state = state.clone();
                let target_key = key.clone();
                drop.connect_drop(move |_, value, x, _| {
                    let Ok(id) = value.get::<String>() else {
                        return false;
                    };
                    let Some(button) = state.buttons.get(&id).and_then(glib::WeakRef::upgrade)
                    else {
                        return false;
                    };
                    let Some(target) = state
                        .slot_boxes
                        .get(&target_key)
                        .and_then(glib::WeakRef::upgrade)
                    else {
                        return false;
                    };
                    if let Some(parent) = button.parent().and_downcast::<gtk::Box>() {
                        parent.remove(&button);
                    }
                    // Insert after the last child whose center is left of the drop x.
                    let mut after: Option<gtk::Widget> = None;
                    let mut child = target.first_child();
                    while let Some(widget) = child {
                        if let Some(bounds) = widget.compute_bounds(&target) {
                            let mid = f64::from(bounds.x()) + f64::from(bounds.width()) / 2.0;
                            if x > mid {
                                after = Some(widget.clone());
                            }
                        }
                        child = widget.next_sibling();
                    }
                    target.insert_child_after(&button, after.as_ref());
                    target.remove_css_class("zle-zone-active");
                    state.rebuild_and_notify();
                    true
                });
            }
            zone.add_controller(drop);
        }

        // Assemble the miniature: one CenterBox per bar, stacked vertically.
        let layout_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layout_box.add_css_class("zle-layout");
        for bar in &spec.bars {
            if bar.decoration.seek_slider {
                let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
                seek.set_value(35.0);
                seek.set_sensitive(false);
                seek.add_css_class("zle-seek");
                layout_box.append(&seek);
            }
            let center_box = gtk::CenterBox::new();
            center_box.add_css_class("zle-bar");
            for &placement in &bar.zones {
                let Some(zone) = slot_boxes_strong.get(&slot_key(&bar.id, placement)) else {
                    continue;
                };
                match placement {
                    ZonePlacement::Start => center_box.set_start_widget(Some(zone)),
                    ZonePlacement::Center => center_box.set_center_widget(Some(zone)),
                    ZonePlacement::End => center_box.set_end_widget(Some(zone)),
                }
            }
            // A title placeholder fills the center when the bar has no center zone.
            if let Some(title) = &bar.decoration.title
                && !bar.zones.contains(&ZonePlacement::Center)
            {
                let label = gtk::Label::new(Some(title));
                label.add_css_class("zle-title");
                label.set_hexpand(true);
                center_box.set_center_widget(Some(&label));
            }
            layout_box.append(&center_box);
        }

        let root = gtk::Frame::new(None);
        root.add_css_class("zle-frame");
        root.set_child(Some(&layout_box));

        Self { root, state }
    }

    /// The editor widget — embed this in a preferences page.
    #[must_use]
    pub fn root(&self) -> &gtk::Frame {
        &self.root
    }

    /// A clone of the current edited value.
    #[must_use]
    pub fn layout(&self) -> ZoneLayout {
        self.state.layout.borrow().clone()
    }

    /// Subscribe to layout changes. Called on every drag/drop and visibility
    /// toggle with the new [`ZoneLayout`]. Replaces any previous subscriber.
    pub fn connect_changed(&self, on_change: impl Fn(&ZoneLayout) + 'static) {
        *self.state.on_changed.borrow_mut() = Some(Box::new(on_change));
    }
}
