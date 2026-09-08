// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Reusable tab-strip placement for apps that own their window chrome.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;

use crate::layout::tab_prefs::{self, BigTabStripPosition};
use crate::layout::tab_strip_host::{BigTabStrip, style_dock_new_tab_button};

/// Fixed width of a vertical (side) tab strip, logical pixels.
const SIDE_TAB_STRIP_WIDTH: i32 = 176;

/// Shared slot for the app-registered side-width persister.
type SideWidthPersister = Rc<RefCell<Option<Rc<dyn Fn(i32)>>>>;
type PositionAppliedCallback = Rc<dyn Fn(bool)>;

/// One non-header tab strip mount host.
#[derive(Clone)]
struct TabStripDock {
    root: gtk::Widget,
    content: gtk::Box,
}

/// Build a drag grip that resizes a side tab-strip dock.
pub fn build_side_dock_resize_grip(
    dock_content: &gtk::Box,
    grip_before_content: bool,
    on_commit: impl Fn(i32) + 'static,
) -> gtk::Box {
    let grip = gtk::Box::builder()
        .width_request(6)
        .css_classes(["big-tab-dock-grip"])
        .build();
    grip.set_cursor_from_name(Some("col-resize"));
    let drag = gtk::GestureDrag::new();
    let content = dock_content.clone();
    let base = Rc::new(Cell::new(0));
    let origin_root_x = Rc::new(Cell::new(0.0_f32));

    fn pointer_root_x(grip: &gtk::Box, x: f64, y: f64) -> Option<f32> {
        let root: gtk::Widget = grip.root()?.upcast();
        #[allow(clippy::cast_possible_truncation)]
        let point = gtk::graphene::Point::new(x as f32, y as f32);
        grip.compute_point(&root, &point).map(|p| p.x())
    }

    {
        let content = content.clone();
        let base = base.clone();
        let grip = grip.clone();
        let origin_root_x = origin_root_x.clone();
        drag.connect_drag_begin(move |_, x, y| {
            let current = content.width_request();
            base.set(if current > 0 {
                current
            } else {
                content.width()
            });
            origin_root_x.set(pointer_root_x(&grip, x, y).unwrap_or_default());
        });
    }
    let width_at = {
        let grip = grip.clone();
        let base = base.clone();
        move |drag: &gtk::GestureDrag, dx: f64, dy: f64| -> i32 {
            let (start_x, start_y) = drag.start_point().unwrap_or_default();
            let root_x = pointer_root_x(&grip, start_x + dx, start_y + dy)
                .unwrap_or_else(|| origin_root_x.get());
            #[allow(clippy::cast_possible_truncation)]
            let delta = (root_x - origin_root_x.get()).round() as i32;
            let delta = if grip_before_content { -delta } else { delta };
            tab_prefs::clamp_tab_strip_side_width(base.get() + delta)
        }
    };
    {
        let content = content.clone();
        let width_at = width_at.clone();
        drag.connect_drag_update(move |drag, dx, dy| {
            content.set_width_request(width_at(drag, dx, dy));
        });
    }
    drag.connect_drag_end(move |drag, dx, dy| {
        on_commit(width_at(drag, dx, dy));
    });
    grip.add_controller(drag);
    grip
}

/// The new-tab affordance that follows the strip out of the header.
#[derive(Clone)]
struct TabStripNewTab {
    dock_root: gtk::Widget,
    dock_button: gtk::Button,
    header_widget: gtk::Widget,
    accessible_label: String,
}

/// The four non-header tab-strip mount hosts.
#[derive(Clone)]
struct TabStripDocks {
    top: TabStripDock,
    bottom: TabStripDock,
    start: TabStripDock,
    end: TabStripDock,
    new_tab: Rc<RefCell<Option<TabStripNewTab>>>,
    side_width_persister: SideWidthPersister,
}

impl TabStripDocks {
    fn create(tab_list_accessible_name: &str) -> Self {
        let side_width_persister: SideWidthPersister = Rc::new(RefCell::new(None));
        let dock = |css: &str, vertical: bool, grip_before_content: Option<bool>| {
            let content = gtk::Box::builder()
                .orientation(if vertical {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                })
                .css_classes([css, "big-workspace-tab-dock"])
                .build();
            if vertical {
                content.set_width_request(SIDE_TAB_STRIP_WIDTH);
                content.set_vexpand(true);
                content.set_spacing(6);
                content.set_margin_top(6);
                content.set_margin_bottom(6);
                content.set_margin_start(6);
                content.set_margin_end(6);
            } else {
                content.set_hexpand(true);
            }
            content.set_accessible_role(gtk::AccessibleRole::TabList);
            content.update_property(&[gtk::accessible::Property::Label(tab_list_accessible_name)]);
            let handle = gtk::WindowHandle::builder()
                .child(&content)
                .css_classes(["big-workspace-tab-dock"])
                .build();
            let root: gtk::Widget = if let Some(grip_first) = grip_before_content {
                let persister = side_width_persister.clone();
                let grip = build_side_dock_resize_grip(&content, grip_first, move |width| {
                    if let Some(persist) = persister.borrow().as_ref() {
                        persist(width);
                    }
                });
                let row = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .visible(false)
                    .css_classes(["big-workspace-tab-dock"])
                    .build();
                if grip_first {
                    row.append(&grip);
                    row.append(&handle);
                } else {
                    row.append(&handle);
                    row.append(&grip);
                }
                row.upcast()
            } else {
                handle.set_visible(false);
                handle.clone().upcast()
            };
            TabStripDock { root, content }
        };
        Self {
            top: dock("big-workspace-tab-dock-top", false, None),
            bottom: dock("big-workspace-tab-dock-bottom", false, None),
            start: dock("big-workspace-tab-dock-start", true, Some(false)),
            end: dock("big-workspace-tab-dock-end", true, Some(true)),
            new_tab: Rc::new(RefCell::new(None)),
            side_width_persister,
        }
    }

    fn host_for(&self, position: BigTabStripPosition) -> Option<&TabStripDock> {
        match position {
            BigTabStripPosition::Header => None,
            BigTabStripPosition::Top => Some(&self.top),
            BigTabStripPosition::Bottom => Some(&self.bottom),
            BigTabStripPosition::Start => Some(&self.start),
            BigTabStripPosition::End => Some(&self.end),
        }
    }
}

/// Owns the reusable tab-strip placement widgets and re-parenting policy.
#[derive(Clone)]
pub struct BigTabStripPlacement {
    header_host: gtk::Box,
    scroll_host: gtk::ScrolledWindow,
    docks: TabStripDocks,
    position: Rc<Cell<BigTabStripPosition>>,
    on_position_applied: Rc<RefCell<Option<PositionAppliedCallback>>>,
}

/// Map wheel/touchpad deltas onto the strip's scroll axis: horizontal
/// mounts scroll the tabs with vertical wheel motion (promoted from
/// big-terminal); vertical side mounts keep the natural vertical axis.
fn attach_wheel_to_strip_scroll(
    scroll_host: &gtk::ScrolledWindow,
    position: &Rc<Cell<BigTabStripPosition>>,
) {
    const SCROLL_STEP: f64 = 30.0;
    let controller = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let weak = scroll_host.downgrade();
    let position = position.clone();
    controller.connect_scroll(move |_, dx, dy| {
        let Some(scrolled) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let adj = if position.get().is_vertical() {
            scrolled.vadjustment()
        } else {
            scrolled.hadjustment()
        };
        let upper = adj.upper() - adj.page_size();
        let new_value = (adj.value() + (dx + dy) * SCROLL_STEP).clamp(adj.lower(), upper.max(0.0));
        adj.set_value(new_value);
        glib::Propagation::Stop
    });
    scroll_host.add_controller(controller);
}

impl BigTabStripPlacement {
    /// Create placement hosts, wrap `strip` in the persistent scroll host, and
    /// mount it into `header_host`.
    #[must_use]
    pub fn new(
        strip: &BigTabStrip,
        header_host: &gtk::Box,
        tab_list_accessible_name: &str,
    ) -> Self {
        let scroll_host = gtk::ScrolledWindow::builder()
            .css_classes(["scrolled-tab-bar"])
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .propagate_natural_width(false)
            .valign(gtk::Align::Fill)
            .child(strip.tab_bar())
            .build();
        header_host.append(&scroll_host);

        let position = Rc::new(Cell::new(BigTabStripPosition::Header));
        attach_wheel_to_strip_scroll(&scroll_host, &position);

        Self {
            header_host: header_host.clone(),
            scroll_host,
            docks: TabStripDocks::create(tab_list_accessible_name),
            position,
            on_position_applied: Rc::new(RefCell::new(None)),
        }
    }

    /// Current tab-strip position.
    #[must_use]
    pub fn position(&self) -> BigTabStripPosition {
        self.position.get()
    }

    /// Set a callback invoked after a successful mount. The argument is `true`
    /// when the strip is mounted in the header.
    pub fn set_on_position_applied(&self, callback: PositionAppliedCallback) {
        *self.on_position_applied.borrow_mut() = Some(callback);
    }

    /// Return the dock root for a non-header position.
    #[must_use]
    pub fn dock_root(&self, position: BigTabStripPosition) -> Option<gtk::Widget> {
        self.docks.host_for(position).map(|dock| dock.root.clone())
    }

    /// Register how the side-strip width persists after a resize drag.
    pub fn set_side_width_persister(&self, persist: impl Fn(i32) + 'static) {
        *self.docks.side_width_persister.borrow_mut() = Some(Rc::new(persist));
    }

    /// Set both side dock content widths.
    pub fn set_side_width(&self, width: i32) {
        self.docks.start.content.set_width_request(width);
        self.docks.end.content.set_width_request(width);
    }

    /// Register the strip's new-tab affordance by action name.
    pub fn set_new_tab_action(
        &self,
        action_name: &str,
        accessible_label: &str,
        header_new_tab: &impl IsA<gtk::Widget>,
    ) {
        let dock_button = gtk::Button::from_icon_name("tab-new-symbolic");
        dock_button.add_css_class("flat");
        dock_button.set_action_name(Some(action_name));
        crate::feedback::tooltip::set(&dock_button, accessible_label);
        dock_button.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
        *self.docks.new_tab.borrow_mut() = Some(TabStripNewTab {
            dock_root: dock_button.clone().upcast(),
            dock_button,
            header_widget: header_new_tab.clone().upcast(),
            accessible_label: accessible_label.to_owned(),
        });
    }

    /// Register the strip's new-tab affordance with app-supplied dock widgets.
    pub fn set_new_tab_widgets(
        &self,
        dock_root: &impl IsA<gtk::Widget>,
        dock_button: &gtk::Button,
        accessible_label: &str,
        header_new_tab: &impl IsA<gtk::Widget>,
    ) {
        *self.docks.new_tab.borrow_mut() = Some(TabStripNewTab {
            dock_root: dock_root.clone().upcast(),
            dock_button: dock_button.clone(),
            header_widget: header_new_tab.clone().upcast(),
            accessible_label: accessible_label.to_owned(),
        });
    }

    /// Re-parent the tab strip between header and dock placements.
    pub fn set_position(&self, strip: &BigTabStrip, position: BigTabStripPosition) {
        let current = self.position.get();
        if current == position {
            return;
        }
        let tab_bar = strip.tab_bar().clone();
        let new_tab = self.docks.new_tab.borrow().clone();
        match self.docks.host_for(current) {
            Some(dock) => {
                dock.content.remove(&self.scroll_host);
                if let Some(new_tab) = &new_tab {
                    dock.content.remove(&new_tab.dock_root);
                }
                dock.root.set_visible(false);
            }
            None => self.header_host.remove(&self.scroll_host),
        }
        strip.set_vertical(position.is_vertical());
        strip.set_docked_horizontal(matches!(
            position,
            BigTabStripPosition::Top | BigTabStripPosition::Bottom
        ));
        if position.is_vertical() {
            self.scroll_host
                .set_hscrollbar_policy(gtk::PolicyType::Never);
            self.scroll_host
                .set_vscrollbar_policy(gtk::PolicyType::Automatic);
            self.scroll_host.set_hexpand(true);
            self.scroll_host.set_vexpand(false);
            self.scroll_host.set_valign(gtk::Align::Start);
            tab_bar.set_halign(gtk::Align::Fill);
            tab_bar.set_valign(gtk::Align::Start);
            tab_bar.set_hexpand(true);
        } else if position == BigTabStripPosition::Header {
            self.scroll_host
                .set_hscrollbar_policy(gtk::PolicyType::Automatic);
            self.scroll_host
                .set_vscrollbar_policy(gtk::PolicyType::Never);
            self.scroll_host.set_hexpand(true);
            self.scroll_host.set_vexpand(false);
            self.scroll_host.set_propagate_natural_width(false);
            self.scroll_host.set_valign(gtk::Align::Fill);
            tab_bar.set_hexpand(false);
            tab_bar.set_valign(gtk::Align::Fill);
        } else {
            self.scroll_host
                .set_hscrollbar_policy(gtk::PolicyType::Automatic);
            self.scroll_host
                .set_vscrollbar_policy(gtk::PolicyType::Never);
            self.scroll_host.set_hexpand(false);
            self.scroll_host.set_vexpand(false);
            self.scroll_host.set_propagate_natural_width(true);
            self.scroll_host.set_valign(gtk::Align::Center);
            tab_bar.set_hexpand(false);
            tab_bar.set_valign(gtk::Align::Center);
        }
        match self.docks.host_for(position) {
            Some(dock) => {
                dock.content.prepend(&self.scroll_host);
                if let Some(new_tab) = &new_tab {
                    style_dock_new_tab_button(
                        &new_tab.dock_button,
                        &new_tab.accessible_label,
                        position.is_vertical(),
                    );
                    dock.content
                        .insert_child_after(&new_tab.dock_root, Some(&self.scroll_host));
                    new_tab.header_widget.set_visible(false);
                }
                dock.root.set_visible(true);
            }
            None => {
                self.header_host.append(&self.scroll_host);
                if let Some(new_tab) = &new_tab {
                    new_tab.header_widget.set_visible(true);
                }
            }
        }
        self.position.set(position);
        if let Some(callback) = self.on_position_applied.borrow().as_ref() {
            callback(position == BigTabStripPosition::Header);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gtk_ready() -> bool {
        gtk::init().is_ok()
    }

    #[test]
    fn placement_starts_in_header_and_exposes_dock_roots() {
        if !gtk_ready() {
            return;
        }
        let strip = BigTabStrip::default();
        let header_host = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let placement = BigTabStripPlacement::new(&strip, &header_host, "Tabs");

        assert_eq!(placement.position(), BigTabStripPosition::Header);
        assert!(placement.dock_root(BigTabStripPosition::Header).is_none());
        assert!(placement.dock_root(BigTabStripPosition::Top).is_some());
        assert_eq!(
            header_host.first_child(),
            Some(placement.scroll_host.clone().upcast())
        );
    }

    #[test]
    fn set_position_moves_scroll_host_and_calls_hook() {
        if !gtk_ready() {
            return;
        }
        let strip = BigTabStrip::default();
        let header_host = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let placement = BigTabStripPlacement::new(&strip, &header_host, "Tabs");
        let applied = Rc::new(Cell::new(true));
        let applied_for_hook = applied.clone();
        placement.set_on_position_applied(Rc::new(move |in_header| {
            applied_for_hook.set(in_header);
        }));

        placement.set_position(&strip, BigTabStripPosition::Start);

        assert_eq!(placement.position(), BigTabStripPosition::Start);
        assert!(!applied.get());
        assert!(header_host.first_child().is_none());
        assert_eq!(
            placement.docks.start.content.first_child(),
            Some(placement.scroll_host.clone().upcast())
        );
    }

    #[test]
    fn side_width_applies_to_both_side_docks() {
        if !gtk_ready() {
            return;
        }
        let strip = BigTabStrip::default();
        let header_host = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let placement = BigTabStripPlacement::new(&strip, &header_host, "Tabs");

        placement.set_side_width(232);

        assert_eq!(placement.docks.start.content.width_request(), 232);
        assert_eq!(placement.docks.end.content.width_request(), 232);
    }
}
