// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Sidebar split and resize policy for media windows.

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_SIDEBAR_MIN_WIDTH_PX: f64 = 260.0;
const DEFAULT_SIDEBAR_MAX_WIDTH_PX: f64 = 480.0;
const DEFAULT_SIDEBAR_RESIZE_LIMIT_PX: f64 = 900.0;
const DEFAULT_SIDEBAR_WIDTH_FRACTION: f64 = 1.0;
const DEFAULT_RESIZE_HANDLE_WIDTH_PX: i32 = 6;
const SIDEBAR_KEYBOARD_RESIZE_STEP_PX: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum BigMediaSidebarResizeCommand {
    ShrinkStep,
    GrowStep,
    Minimum,
    Maximum,
}

impl BigMediaSidebarResizeCommand {
    fn resized_width(self, spec: BigMediaSidebarSpec, current_width: f64) -> f64 {
        match self {
            Self::ShrinkStep => spec.resized_width(current_width, -SIDEBAR_KEYBOARD_RESIZE_STEP_PX),
            Self::GrowStep => spec.resized_width(current_width, SIDEBAR_KEYBOARD_RESIZE_STEP_PX),
            Self::Minimum => spec.min_width(),
            Self::Maximum => spec.resize_limit_width(),
        }
    }

    fn from_key(keyval: gtk::gdk::Key) -> Option<Self> {
        use gtk::gdk::Key;
        match keyval {
            Key::Left => Some(Self::ShrinkStep),
            Key::Right => Some(Self::GrowStep),
            Key::Home => Some(Self::Minimum),
            Key::End => Some(Self::Maximum),
            _ => None,
        }
    }
}

/// Display-free sidebar sizing policy for [`BigMediaSidebarShell`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct BigMediaSidebarSpec {
    min_width: f64,
    max_width: f64,
    resize_limit: f64,
    width_fraction: f64,
    resize_handle_width: i32,
    is_visible_initially: bool,
}

impl BigMediaSidebarSpec {
    /// Build the default media sidebar policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initial sidebar width range used by `AdwOverlaySplitView`.
    #[must_use]
    pub fn width_range(mut self, min_width: f64, max_width: f64) -> Self {
        self.min_width = min_width;
        self.max_width = max_width.max(min_width);
        self.resize_limit = self.resize_limit.max(self.max_width);
        self
    }

    /// Set the maximum width allowed while the user drags the resize handle.
    #[must_use]
    pub fn resize_limit(mut self, resize_limit: f64) -> Self {
        self.resize_limit = resize_limit.max(self.max_width);
        self
    }

    /// Set the split-view width fraction.
    #[must_use]
    pub fn width_fraction(mut self, width_fraction: f64) -> Self {
        self.width_fraction = width_fraction.clamp(0.0, 1.0);
        self
    }

    /// Set the resize handle width in pixels.
    #[must_use]
    pub fn resize_handle_width(mut self, width: i32) -> Self {
        self.resize_handle_width = width.max(1);
        self
    }

    /// Set whether the sidebar starts visible.
    #[must_use]
    pub fn visible_initially(mut self, is_visible: bool) -> Self {
        self.is_visible_initially = is_visible;
        self
    }

    /// Minimum sidebar width in pixels.
    #[must_use]
    pub fn min_width(self) -> f64 {
        self.min_width
    }

    /// Maximum initial sidebar width in pixels.
    #[must_use]
    pub fn max_width(self) -> f64 {
        self.max_width
    }

    /// Maximum user-resized sidebar width in pixels.
    #[must_use]
    pub fn resize_limit_width(self) -> f64 {
        self.resize_limit
    }

    /// Width fraction passed to `AdwOverlaySplitView`.
    #[must_use]
    pub fn sidebar_width_fraction(self) -> f64 {
        self.width_fraction
    }

    /// Resize handle width in pixels.
    #[must_use]
    pub fn handle_width(self) -> i32 {
        self.resize_handle_width
    }

    /// Whether the sidebar starts visible.
    #[must_use]
    pub fn is_visible_initially(self) -> bool {
        self.is_visible_initially
    }

    /// Calculate the next user-resized width from a drag offset.
    #[must_use]
    pub fn resized_width(self, start_width: f64, offset_x: f64) -> f64 {
        (start_width + offset_x).clamp(self.min_width, self.resize_limit)
    }
}

impl Default for BigMediaSidebarSpec {
    fn default() -> Self {
        Self {
            min_width: DEFAULT_SIDEBAR_MIN_WIDTH_PX,
            max_width: DEFAULT_SIDEBAR_MAX_WIDTH_PX,
            resize_limit: DEFAULT_SIDEBAR_RESIZE_LIMIT_PX,
            width_fraction: DEFAULT_SIDEBAR_WIDTH_FRACTION,
            resize_handle_width: DEFAULT_RESIZE_HANDLE_WIDTH_PX,
            is_visible_initially: false,
        }
    }
}

/// Shared overlay split shell for media sidebars.
#[derive(Debug, Clone)]
pub struct BigMediaSidebarShell {
    split_view: adw::OverlaySplitView,
    sidebar_container: gtk::Box,
    resize_handle: gtk::Button,
}

impl BigMediaSidebarShell {
    /// Build a sidebar split around an app-owned media content widget.
    #[must_use]
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        spec: BigMediaSidebarSpec,
    ) -> Self {
        sidebar.set_hexpand(true);

        let resize_handle = gtk::Button::new();
        resize_handle.set_width_request(spec.handle_width());
        resize_handle.set_valign(gtk::Align::Fill);
        resize_handle.add_css_class("flat");
        resize_handle.add_css_class("sidebar-drag-handle");
        resize_handle.set_cursor_from_name(Some("col-resize"));
        configure_sidebar_resize_handle_accessibility(&resize_handle);

        let sidebar_container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sidebar_container.append(sidebar);
        sidebar_container.append(&resize_handle);

        let split_view = adw::OverlaySplitView::builder()
            .sidebar(&sidebar_container)
            .content(content)
            .show_sidebar(spec.is_visible_initially())
            .min_sidebar_width(spec.min_width())
            .max_sidebar_width(spec.max_width())
            .sidebar_width_fraction(spec.sidebar_width_fraction())
            .build();

        wire_sidebar_resize_handle(&resize_handle, &split_view, spec);

        Self {
            split_view,
            sidebar_container,
            resize_handle,
        }
    }

    /// The assembled split view.
    #[must_use]
    pub fn split_view(&self) -> &adw::OverlaySplitView {
        &self.split_view
    }

    /// Container holding the caller's sidebar and the resize handle.
    #[must_use]
    pub fn sidebar_container(&self) -> &gtk::Box {
        &self.sidebar_container
    }

    /// The resize handle widget.
    #[must_use]
    pub fn resize_handle(&self) -> &gtk::Button {
        &self.resize_handle
    }
}

fn wire_sidebar_resize_handle(
    resize_handle: &gtk::Button,
    split_view: &adw::OverlaySplitView,
    spec: BigMediaSidebarSpec,
) {
    let resize = gtk::GestureDrag::new();
    let resize_start_width = std::rc::Rc::new(std::cell::Cell::new(0.0_f64));

    resize.connect_drag_begin({
        let split_view = split_view.downgrade();
        let resize_start_width = resize_start_width.clone();
        move |_, _, _| {
            if let Some(split_view) = split_view.upgrade() {
                resize_start_width.set(split_view.max_sidebar_width());
            }
        }
    });
    resize.connect_drag_update({
        let split_view = split_view.downgrade();
        move |_, offset_x, _| {
            let Some(split_view) = split_view.upgrade() else {
                return;
            };
            split_view
                .set_max_sidebar_width(spec.resized_width(resize_start_width.get(), offset_x));
        }
    });
    resize_handle.add_controller(resize);

    resize_handle.connect_clicked({
        let split_view = split_view.downgrade();
        move |_| {
            if let Some(split_view) = split_view.upgrade() {
                split_view.set_max_sidebar_width(
                    BigMediaSidebarResizeCommand::GrowStep
                        .resized_width(spec, split_view.max_sidebar_width()),
                );
            }
        }
    });

    let keyboard = gtk::EventControllerKey::new();
    keyboard.connect_key_pressed({
        let split_view = split_view.downgrade();
        move |_, keyval, _, _| {
            let Some(command) = BigMediaSidebarResizeCommand::from_key(keyval) else {
                return gtk::glib::Propagation::Proceed;
            };
            let Some(split_view) = split_view.upgrade() else {
                return gtk::glib::Propagation::Proceed;
            };
            split_view
                .set_max_sidebar_width(command.resized_width(spec, split_view.max_sidebar_width()));
            gtk::glib::Propagation::Stop
        }
    });
    resize_handle.add_controller(keyboard);
}

fn configure_sidebar_resize_handle_accessibility(resize_handle: &gtk::Button) {
    resize_handle.set_focusable(true);
    resize_handle.set_can_focus(true);
    crate::feedback::tooltip::set(resize_handle, "Resize sidebar");
    resize_handle.update_property(&[
        gtk::accessible::Property::Label("Resize sidebar"),
        gtk::accessible::Property::Description(
            "Use Left and Right arrow keys to resize the media sidebar. Use Home and End for minimum and maximum width.",
        ),
    ]);
}

#[cfg(test)]
mod tests {
    use super::{BigMediaSidebarResizeCommand, BigMediaSidebarSpec};

    #[test]
    fn sidebar_resize_width_clamps_to_media_policy() {
        let spec = BigMediaSidebarSpec::new()
            .width_range(260.0, 480.0)
            .resize_limit(900.0);

        assert_eq!(spec.resized_width(480.0, 120.0), 600.0);
        assert_eq!(spec.resized_width(480.0, -500.0), 260.0);
        assert_eq!(spec.resized_width(480.0, 1000.0), 900.0);
    }

    #[test]
    fn sidebar_spec_keeps_resize_limit_at_least_max_width() {
        let spec = BigMediaSidebarSpec::new()
            .width_range(300.0, 640.0)
            .resize_limit(400.0);

        assert_eq!(spec.min_width(), 300.0);
        assert_eq!(spec.max_width(), 640.0);
        assert_eq!(spec.resize_limit_width(), 640.0);
    }

    #[test]
    fn sidebar_width_fraction_stays_in_split_view_range() {
        assert_eq!(
            BigMediaSidebarSpec::new()
                .width_fraction(2.0)
                .sidebar_width_fraction(),
            1.0
        );
        assert_eq!(
            BigMediaSidebarSpec::new()
                .width_fraction(-1.0)
                .sidebar_width_fraction(),
            0.0
        );
    }

    #[test]
    fn sidebar_resize_keyboard_commands_map_from_navigation_keys() {
        use gtk::gdk::Key;

        assert_eq!(
            BigMediaSidebarResizeCommand::from_key(Key::Left),
            Some(BigMediaSidebarResizeCommand::ShrinkStep)
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::from_key(Key::Right),
            Some(BigMediaSidebarResizeCommand::GrowStep)
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::from_key(Key::Home),
            Some(BigMediaSidebarResizeCommand::Minimum)
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::from_key(Key::End),
            Some(BigMediaSidebarResizeCommand::Maximum)
        );
        assert_eq!(BigMediaSidebarResizeCommand::from_key(Key::Escape), None);
    }

    #[test]
    fn sidebar_resize_keyboard_commands_apply_policy_limits() {
        let spec = BigMediaSidebarSpec::new()
            .width_range(260.0, 480.0)
            .resize_limit(520.0);

        assert_eq!(
            BigMediaSidebarResizeCommand::ShrinkStep.resized_width(spec, 300.0),
            276.0
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::ShrinkStep.resized_width(spec, 270.0),
            260.0
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::GrowStep.resized_width(spec, 480.0),
            504.0
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::GrowStep.resized_width(spec, 510.0),
            520.0
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::Minimum.resized_width(spec, 400.0),
            260.0
        );
        assert_eq!(
            BigMediaSidebarResizeCommand::Maximum.resized_width(spec, 400.0),
            520.0
        );
    }
}
