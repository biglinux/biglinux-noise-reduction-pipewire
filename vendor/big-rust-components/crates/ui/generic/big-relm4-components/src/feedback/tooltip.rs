// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux custom tooltip helper.
//! GTK main-thread tooltip controller with consistent BigLinux styling.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;
use relm4::gtk::{gdk, glib};

mod buttons;
mod colors;
mod config;
mod global;
mod support;

pub use buttons::{
    custom_menu_button, icon_button, icon_menu_button, icon_toggle_button, labeled_icon_button,
    labeled_menu_button,
};
use colors::{adjust_tooltip_background, is_dark_color, theme_colors};
pub use config::BigTooltipConfig;
pub use global::{
    clear, hide_all, set, set_card, set_card_with_icon, set_enabled, set_follow_default,
    set_markup, set_reparent_safe_native, set_show_when_inactive, set_surface_css, set_with_offset,
    update, update_markup,
};
use support::{live_widgets, root_window_active};

#[cfg(test)]
mod tests;

// qdata keys. The glib `set_data`/`data`/`steal_data` API is `unsafe` because the
// store is untyped: the caller must read each key back as the exact type it was
// written. This module is the ONLY writer/reader of these two keys, and uses a
// fixed type per key — `QDATA_STATE` ⇔ `WidgetTooltipState`, `QDATA_HAS_CONTROLLER`
// ⇔ `bool` — so every `// SAFETY:` below points here for the type-consistency proof.
const QDATA_STATE: &str = "big-tooltip-state";
const QDATA_HAS_CONTROLLER: &str = "big-tooltip-has-controller";

thread_local! {
    static GLOBAL: RefCell<Option<BigTooltipHandle>> = const { RefCell::new(None) };
}

/// How a tooltip label renders its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelContentKind {
    /// Plain text via [`gtk::prelude::LabelExt::set_text`].
    PlainText,
    /// Pango markup via [`gtk::prelude::LabelExt::set_markup`].
    Markup,
}

struct WidgetTooltipState {
    popover: gtk::Popover,
    label: gtk::Label,
    /// The card's icon tile, kept so refreshes can update it (the icon tracks
    /// live state — volume level, network status — like the panel glyph).
    icon: Option<gtk::Image>,
    text: String,
    y_offset: i32,
    content_kind: LabelContentKind,
    /// Anchor the popover at the pointer (Plasma-style hover card) rather than
    /// at the widget bounds.
    follow_pointer: bool,
}

struct BigTooltipInner {
    config: BigTooltipConfig,
    active_widget: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    active_popover: RefCell<Option<glib::WeakRef<gtk::Popover>>>,
    closing_popover: RefCell<Option<glib::WeakRef<gtk::Popover>>>,
    show_timer: RefCell<Option<glib::SourceId>>,
    hide_timer: RefCell<Option<glib::SourceId>>,
    widgets: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
    // Weak: this list exists only to dedup the per-window signal handlers
    // attached in `track_window`; it is never read otherwise. Holding strong
    // `gtk::Window` refs here leaked every window that ever showed a tooltip
    // (the window could never finalize). Keep weak and prune on insert.
    windows: RefCell<Vec<glib::WeakRef<gtk::Window>>>,
    css_provider: RefCell<Option<gtk::CssProvider>>,
    // App-supplied override stylesheet (themeable hover card), installed above
    // the default so the app's surface tokens win.
    override_provider: RefCell<Option<gtk::CssProvider>>,
    colors_initialized: Cell<bool>,
    enabled: Cell<bool>,
    // Last pointer position (widget-relative) on the active widget, updated by
    // the motion controller. Used to anchor cursor-following tooltips at the
    // pointer instead of the widget bounds.
    pointer: Cell<(f64, f64)>,
    // When set, every tooltip follows the pointer without opting in per call.
    follow_default: Cell<bool>,
    // When set, tooltips show even if their root window is not the active one
    // (shell panels / layer-shell surfaces are never "active").
    ignore_root_active: Cell<bool>,
}

/// Shared tooltip controller.
#[derive(Clone)]
pub struct BigTooltipHandle {
    inner: Rc<BigTooltipInner>,
}

impl Default for BigTooltipHandle {
    fn default() -> Self {
        Self::new(BigTooltipConfig::default())
    }
}

impl BigTooltipHandle {
    /// Creates a new instance.
    #[must_use]
    pub fn new(config: BigTooltipConfig) -> Self {
        let handle = Self {
            inner: Rc::new(BigTooltipInner {
                config,
                active_widget: RefCell::new(None),
                active_popover: RefCell::new(None),
                closing_popover: RefCell::new(None),
                show_timer: RefCell::new(None),
                hide_timer: RefCell::new(None),
                widgets: RefCell::new(Vec::new()),
                windows: RefCell::new(Vec::new()),
                css_provider: RefCell::new(None),
                override_provider: RefCell::new(None),
                colors_initialized: Cell::new(false),
                enabled: Cell::new(true),
                pointer: Cell::new((0.0, 0.0)),
                follow_default: Cell::new(false),
                ignore_root_active: Cell::new(false),
            }),
        };

        if adw::is_initialized() {
            let style = adw::StyleManager::default();
            let h = handle.clone();
            style.connect_dark_notify(move |_| h.apply_default_colors());
            let h = handle.clone();
            style.connect_color_scheme_notify(move |_| h.apply_default_colors());
        }

        handle
    }

    /// Sets enabled.
    pub fn set_enabled(&self, enabled: bool) {
        self.inner.enabled.set(enabled);
    }

    /// Returns `true` if enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.get()
    }

    /// Register `widget` as a tooltip-bearer for `text`. Empty text
    /// removes any existing binding. Equivalent to
    /// [`Self::add_with_offset`] with offset 0.
    pub fn add(&self, widget: &impl IsA<gtk::Widget>, text: &str) {
        self.add_with_offset(widget, text, 0);
    }

    /// Like [`Self::add`] but shifts the tooltip popover vertically
    /// by `y_offset` pixels — useful when the widget partially
    /// occludes its own tooltip.
    pub fn add_with_offset(&self, widget: &impl IsA<gtk::Widget>, text: &str, y_offset: i32) {
        self.add_internal(
            widget.as_ref(),
            text,
            y_offset,
            LabelContentKind::PlainText,
            false,
            None,
        );
    }

    /// Register `widget` as a tooltip-bearer rendering Pango `markup`.
    /// Mirrors [`Self::add`] but routes the content through
    /// `gtk::Label::set_markup`. Empty markup is ignored.
    pub fn add_with_markup(&self, widget: &impl IsA<gtk::Widget>, markup: &str) {
        self.add_with_markup_and_offset(widget, markup, 0);
    }

    /// Markup-aware counterpart of [`Self::add_with_offset`].
    pub fn add_with_markup_and_offset(
        &self,
        widget: &impl IsA<gtk::Widget>,
        markup: &str,
        y_offset: i32,
    ) {
        self.add_internal(
            widget.as_ref(),
            markup,
            y_offset,
            LabelContentKind::Markup,
            false,
            None,
        );
    }

    /// Register a Plasma-style hover card: a Pango-`markup` tooltip that
    /// follows the pointer and pops up below it, for richer per-item info
    /// (title + detail lines). Empty markup is ignored.
    pub fn add_card(&self, widget: &impl IsA<gtk::Widget>, markup: &str) {
        self.add_internal(
            widget.as_ref(),
            markup,
            0,
            LabelContentKind::Markup,
            true,
            None,
        );
    }

    /// Like [`Self::add_card`] but with a leading themed icon (Plasma-style:
    /// an icon tile beside the title + detail).
    pub fn add_card_with_icon(&self, widget: &impl IsA<gtk::Widget>, icon: &str, markup: &str) {
        self.add_internal(
            widget.as_ref(),
            markup,
            0,
            LabelContentKind::Markup,
            true,
            Some(icon),
        );
    }

    /// Make every tooltip registered afterwards follow the pointer by default
    /// (Plasma-style), without each call opting in. Set once at app startup.
    pub fn set_follow_default(&self, follow: bool) {
        self.inner.follow_default.set(follow);
    }

    /// Show tooltips even when their root window is not the active one. Required
    /// for shell panels / layer-shell surfaces, which are never "active".
    pub fn set_show_when_inactive(&self, show: bool) {
        self.inner.ignore_root_active.set(show);
    }

    /// Install an app-supplied stylesheet for the hover card, layered above the
    /// built-in look so the app's theme tokens win. The CSS targets
    /// `popover.big-tooltip > contents` (the card) and `popover.big-tooltip
    /// label` (its text). Pass an empty string to drop the override.
    pub fn set_surface_css(&self, css: &str) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        if let Some(old) = self.inner.override_provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
        if css.is_empty() {
            return;
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 200,
        );
        *self.inner.override_provider.borrow_mut() = Some(provider);
    }

    fn add_internal(
        &self,
        widget: &gtk::Widget,
        content: &str,
        y_offset: i32,
        kind: LabelContentKind,
        follow_pointer: bool,
        icon: Option<&str>,
    ) {
        if content.is_empty() {
            return;
        }
        let follow_pointer = follow_pointer || self.inner.follow_default.get();

        if self.refresh_existing_with(widget, content, y_offset, kind, follow_pointer, icon) {
            return;
        }

        let popover = gtk::Popover::new();
        popover.set_has_arrow(false);
        // Always anchor above (GTK flips it below when there is no room, e.g. a
        // top bar). Cursor-following only shifts the anchor horizontally — see
        // `show` — so a bottom-bar card never falls off the screen edge.
        popover.set_position(gtk::PositionType::Top);
        popover.set_can_target(false);
        popover.set_focusable(false);
        popover.set_autohide(false);
        popover.add_css_class("big-tooltip");

        let icon = icon.filter(|name| !name.is_empty());
        // Cards (icon or markup) left-align; plain one-line tooltips center.
        let start = icon.is_some() || matches!(kind, LabelContentKind::Markup);
        let label = gtk::Label::builder()
            // Cards have short, structured lines: don't wrap (a wrapping label
            // squeezes near the screen edge); GTK shifts the popover to keep it
            // on-screen. Plain tooltips wrap as before.
            .wrap(!start)
            .max_width_chars(self.inner.config.max_width_chars)
            .halign(if start {
                gtk::Align::Start
            } else {
                gtk::Align::Center
            })
            .xalign(if start { 0.0 } else { 0.5 })
            .build();
        apply_label_content(&label, content, kind);
        let icon_image = if let Some(icon_name) = icon {
            // Icon tile + text column (the styling lives in `apply_css`).
            let image = gtk::Image::from_icon_name(icon_name);
            image.set_pixel_size(34);
            image.set_valign(gtk::Align::Center);
            image.add_css_class("big-tooltip-icon");
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.append(&image);
            row.append(&label);
            popover.set_child(Some(&row));
            Some(image)
        } else {
            popover.set_child(Some(&label));
            None
        };
        popover.set_parent(widget);

        clear_native_tooltip_tree(widget);

        // Mirror the tooltip text into the AT-SPI accessible label. The custom
        // popover above replaces the native tooltip, so without this an
        // icon-only control would expose an empty accessible name to screen
        // readers. Only plain text is mirrored (markup is not a valid name).
        if matches!(kind, LabelContentKind::PlainText) {
            widget.update_property(&[gtk::accessible::Property::Label(content)]);
        }

        let popover_weak = popover.downgrade();
        widget.connect_parent_notify(move |w| {
            if w.parent().is_none()
                && let Some(popover) = popover_weak.upgrade()
                && popover.parent().is_some()
            {
                popover.unparent();
            }
        });
        widget.connect_realize(clear_native_tooltip_tree);
        widget.connect_map(clear_native_tooltip_tree);
        // Belt for widgets that NEVER realize (e.g. rows inside a popover the
        // user never opens): `track_window`'s window-unrealize unparent_all
        // only arms at realize, so their tooltip popover would still be
        // attached at dispose — "Finalizing …, children left" + a dangling
        // parent. `destroy` fires at the START of dispose, while the widget
        // is still valid.
        let popover_weak = popover.downgrade();
        widget.connect_destroy(move |_| {
            if let Some(popover) = popover_weak.upgrade()
                && popover.parent().is_some()
            {
                popover.unparent();
            }
        });

        let state = WidgetTooltipState {
            popover,
            label,
            icon: icon_image,
            text: content.to_string(),
            y_offset,
            content_kind: kind,
            follow_pointer,
        };
        // SAFETY: `QDATA_STATE` is written and read only as `WidgetTooltipState`
        // (see key definition); type-consistent store.
        unsafe {
            widget.set_data(QDATA_STATE, state);
        }

        self.add_controller_if_missing(widget);
        {
            // Prune dead entries on insert (same policy as `windows` below):
            // this process-global list otherwise grows by one GWeakRef per
            // tooltip'd widget forever — measured as unbounded heap growth on
            // window open/close cycles.
            let mut widgets = self.inner.widgets.borrow_mut();
            widgets.retain(|tracked| tracked.upgrade().is_some());
            widgets.push(widget.downgrade());
        }
        self.track_window(widget);
    }

    /// Push a new text value through the live state.
    pub fn update_text(&self, widget: &impl IsA<gtk::Widget>, text: &str) {
        let widget = widget.as_ref();
        let kind = existing_kind(widget).unwrap_or(LabelContentKind::PlainText);
        self.refresh_existing_with(
            widget,
            text,
            existing_offset(widget).unwrap_or(0),
            kind,
            existing_follow(widget),
            None,
        );
    }

    /// Push a new Pango markup value through the live state, preserving
    /// the original offset. Companion to [`Self::update_text`].
    pub fn update_markup(&self, widget: &impl IsA<gtk::Widget>, markup: &str) {
        let widget = widget.as_ref();
        self.refresh_existing_with(
            widget,
            markup,
            existing_offset(widget).unwrap_or(0),
            LabelContentKind::Markup,
            existing_follow(widget),
            None,
        );
    }

    /// Drop the tooltip binding from `widget` and unparent its
    /// popover. Idempotent.
    pub fn clear(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.as_ref();
        // SAFETY: `QDATA_STATE` is always a `WidgetTooltipState` (see key definition).
        let state: Option<WidgetTooltipState> = unsafe { widget.steal_data(QDATA_STATE) };
        if let Some(state) = state {
            state.popover.popdown();
            if state.popover.parent().is_some() {
                state.popover.unparent();
            }
        }
        clear_native_tooltip_tree(widget);
    }

    /// Hide the all surface.
    pub fn hide_all(&self) {
        self.clear_timer();
        self.hide_active(true);
        *self.inner.active_widget.borrow_mut() = None;

        for widget in live_widgets(&self.inner.widgets) {
            if let Some(state) = tooltip_state(&widget) {
                state.popover.popdown();
                state.popover.remove_css_class("visible");
            }
        }
    }

    /// Hide every active tooltip popover and cancel pending timers.
    /// Called by window destructors.
    pub fn cleanup(&self) {
        self.hide_all();
    }

    fn refresh_existing_with(
        &self,
        widget: &gtk::Widget,
        content: &str,
        y_offset: i32,
        kind: LabelContentKind,
        follow_pointer: bool,
        icon: Option<&str>,
    ) -> bool {
        let Some(state) = tooltip_state(widget) else {
            return false;
        };
        let popover = state.popover.clone();
        let label = state.label.clone();
        let icon_image = state.icon.clone();
        apply_label_content(&label, content, kind);
        // Keep the icon tile in sync with live state (volume level, network
        // status), not frozen at the value it had when first shown.
        if let (Some(image), Some(name)) = (&icon_image, icon) {
            image.set_icon_name(Some(name));
        }
        // SAFETY: `QDATA_STATE` is written and read only as `WidgetTooltipState`
        // (see key definition); type-consistent store.
        unsafe {
            widget.set_data(
                QDATA_STATE,
                WidgetTooltipState {
                    popover,
                    label,
                    icon: icon_image,
                    text: content.to_string(),
                    y_offset,
                    content_kind: kind,
                    follow_pointer,
                },
            );
        }
        true
    }

    /// While a follow-pointer card is visible on `widget`, re-anchor it to the
    /// current cursor X so it tracks the pointer.
    fn reposition_visible(&self, widget: &gtk::Widget) {
        let is_active = self
            .inner
            .active_widget
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
            .is_some_and(|active| active == *widget);
        if !is_active {
            return;
        }
        let Some(state) = tooltip_state(widget) else {
            return;
        };
        if !state.follow_pointer || !state.popover.is_visible() {
            return;
        }
        let (px, _py) = self.inner.pointer.get();
        let x = (px as i32).clamp(0, widget.width().max(1) - 1);
        state.popover.set_pointing_to(Some(&gdk::Rectangle::new(
            x,
            state.y_offset,
            1,
            widget.height(),
        )));
    }

    fn add_controller_if_missing(&self, widget: &gtk::Widget) {
        // SAFETY: `QDATA_HAS_CONTROLLER` is written and read only as `bool`
        // (see key definition); the returned pointer stays valid until widget finalize.
        let already: Option<std::ptr::NonNull<bool>> = unsafe { widget.data(QDATA_HAS_CONTROLLER) };
        if already.is_some() {
            return;
        }

        let motion = gtk::EventControllerMotion::new();
        let handle = self.clone();
        let weak = widget.downgrade();
        motion.connect_enter(move |_, x, y| {
            handle.inner.pointer.set((x, y));
            if let Some(widget) = weak.upgrade() {
                handle.on_enter(&widget);
            }
        });

        // Track the pointer for cursor-following hover cards, and re-anchor a
        // visible card so it actually follows the cursor.
        let handle = self.clone();
        let weak_motion = widget.downgrade();
        motion.connect_motion(move |_, x, y| {
            handle.inner.pointer.set((x, y));
            if let Some(widget) = weak_motion.upgrade() {
                handle.reposition_visible(&widget);
            }
        });

        let handle = self.clone();
        let weak = widget.downgrade();
        motion.connect_leave(move |_| {
            if let Some(widget) = weak.upgrade() {
                handle.on_leave(&widget);
            }
        });
        widget.add_controller(motion);

        let click = gtk::GestureClick::new();
        let handle = self.clone();
        click.connect_pressed(move |_, _, _, _| {
            handle.clear_timer();
            handle.hide_active(true);
            *handle.inner.active_widget.borrow_mut() = None;
        });
        widget.add_controller(click);

        // SAFETY: `QDATA_HAS_CONTROLLER` is written and read only as `bool`
        // (see key definition); type-consistent store.
        unsafe {
            widget.set_data(QDATA_HAS_CONTROLLER, true);
        }
    }

    fn track_window(&self, widget: &gtk::Widget) {
        let attach = {
            let handle = self.clone();
            move |widget: &gtk::Widget| {
                let Some(root) = widget.root() else { return };
                let Ok(window) = root.downcast::<gtk::Window>() else {
                    return;
                };
                let mut windows = handle.inner.windows.borrow_mut();
                windows.retain(|tracked| tracked.upgrade().is_some());
                if windows
                    .iter()
                    .any(|tracked| tracked.upgrade().as_ref() == Some(&window))
                {
                    return;
                }
                windows.push(window.downgrade());
                drop(windows);

                let h = handle.clone();
                window.connect_is_active_notify(move |window| {
                    if !window.is_active() {
                        h.hide_all();
                    }
                });
                let h = handle.clone();
                window.connect_maximized_notify(move |_| h.hide_all());
                let h = handle.clone();
                window.connect_fullscreened_notify(move |_| h.hide_all());
                // `unrealize`, NOT `destroy`: GTK4 only emits `destroy` at
                // dispose (refcount zero), which runs AFTER the child buttons
                // already finalized — too late to unparent their tooltip
                // popovers ("Finalizing GtkButton, but it still has children
                // left"). A toplevel unrealizes exactly at gtk_window_destroy
                // (close), before any disposal.
                let h = handle.clone();
                window.connect_unrealize(move |_| h.unparent_all());
            }
        };

        if widget.is_realized() {
            attach(widget);
        } else {
            widget.connect_realize(move |widget| attach(widget));
        }
    }

    fn on_enter(&self, widget: &gtk::Widget) {
        if !self.inner.enabled.get() {
            return;
        }

        let previous = self
            .inner
            .active_widget
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade);
        if previous.as_ref().is_some_and(|previous| previous != widget) {
            self.hide_active(true);
        }

        self.clear_timer();
        *self.inner.active_widget.borrow_mut() = Some(widget.downgrade());

        let handle = self.clone();
        let weak = widget.downgrade();
        let delay = self.inner.config.show_delay_ms;
        let timer = glib::timeout_add_local_once(
            std::time::Duration::from_millis(u64::from(delay)),
            move || {
                if let Some(widget) = weak.upgrade() {
                    handle.show(&widget);
                }
            },
        );
        *self.inner.show_timer.borrow_mut() = Some(timer);
    }

    fn on_leave(&self, widget: &gtk::Widget) {
        let leaving_active = self
            .inner
            .active_widget
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
            .is_some_and(|active| active == *widget);
        if !leaving_active {
            return;
        }
        self.clear_timer();
        self.hide_active(false);
        *self.inner.active_widget.borrow_mut() = None;
    }

    fn show(&self, widget: &gtk::Widget) {
        self.ensure_colors();
        *self.inner.show_timer.borrow_mut() = None;

        // Suppress the tooltip while the control's OWN popover/menu is open
        // (an open app menu, the clock calendar, a network/volume applet…):
        // a tooltip should only hint a CLOSED control, never overlap the
        // content the user already opened.
        if widget_popover_open(widget) {
            return;
        }

        // A shell panel (layer-shell surface) is never the "active" window, so
        // the active-window guard must be skippable or its tooltips never show.
        let root_ok = self.inner.ignore_root_active.get() || root_window_active(widget);
        if !widget.is_mapped() || !root_ok {
            return;
        }
        if self
            .inner
            .active_widget
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
            .is_none_or(|active| active != *widget)
        {
            return;
        }

        let Some(state) = tooltip_state(widget) else {
            return;
        };
        if state.popover.parent().is_none() {
            state.popover.set_parent(widget);
        }

        apply_label_content(&state.label, &state.text, state.content_kind);
        let rect = if state.follow_pointer {
            // Follow the cursor horizontally, but anchor to the widget's full
            // vertical span so the card pops above (or flips below) on the
            // correct side — never off the screen edge under a bottom bar.
            let (px, _py) = self.inner.pointer.get();
            let x = (px as i32).clamp(0, widget.width().max(1) - 1);
            gdk::Rectangle::new(x, state.y_offset, 1, widget.height())
        } else {
            gdk::Rectangle::new(0, state.y_offset, widget.width(), widget.height())
        };
        state.popover.set_pointing_to(Some(&rect));
        state.popover.popup();
        state.popover.set_visible(true);
        state.popover.add_css_class("visible");
        *self.inner.active_popover.borrow_mut() = Some(state.popover.downgrade());
    }

    fn hide_active(&self, immediate: bool) {
        let Some(weak) = self.inner.active_popover.borrow_mut().take() else {
            return;
        };
        let Some(popover) = weak.upgrade() else {
            return;
        };

        popover.remove_css_class("visible");
        *self.inner.closing_popover.borrow_mut() = Some(popover.downgrade());

        if immediate {
            popover.popdown();
            *self.inner.closing_popover.borrow_mut() = None;
            return;
        }

        let handle = self.clone();
        let weak = popover.downgrade();
        let fade = self.inner.config.fade_out_ms;
        let timer = glib::timeout_add_local_once(
            std::time::Duration::from_millis(u64::from(fade)),
            move || {
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                *handle.inner.hide_timer.borrow_mut() = None;
                *handle.inner.closing_popover.borrow_mut() = None;
            },
        );
        *self.inner.hide_timer.borrow_mut() = Some(timer);
    }

    fn clear_timer(&self) {
        if let Some(id) = self.inner.show_timer.borrow_mut().take() {
            id.remove();
        }
        if let Some(id) = self.inner.hide_timer.borrow_mut().take() {
            id.remove();
        }
        if let Some(weak) = self.inner.closing_popover.borrow_mut().take()
            && let Some(popover) = weak.upgrade()
        {
            popover.popdown();
            popover.remove_css_class("visible");
        }
    }

    fn unparent_all(&self) {
        self.clear_timer();
        for widget in live_widgets(&self.inner.widgets) {
            if let Some(state) = tooltip_state(&widget)
                && state.popover.parent().is_some()
            {
                state.popover.unparent();
            }
        }
    }

    fn ensure_colors(&self) {
        if !self.inner.colors_initialized.get() {
            self.apply_default_colors();
            self.inner.colors_initialized.set(true);
        }
    }

    fn apply_default_colors(&self) {
        let (bg, fg) = theme_colors();
        self.apply_css(bg, fg);
    }

    fn apply_css(&self, bg: &str, fg: &str) {
        let tooltip_bg = adjust_tooltip_background(bg);
        let dark = is_dark_color(bg);
        // A hairline rim that reads as a light catch on dark themes and a soft
        // shade on light ones — the "premium" edge definition.
        let rim = if dark {
            "alpha(#ffffff, 0.14)"
        } else {
            "alpha(#000000, 0.12)"
        };
        // A glossy top highlight layered over the solid fill.
        let gloss = if dark {
            "alpha(#ffffff, 0.08)"
        } else {
            "alpha(#ffffff, 0.55)"
        };
        let fade = self.inner.config.css_fade_ms;
        let css = format!(
            r"
popover.big-tooltip {{
    background: transparent;
    box-shadow: none;
    padding: 16px;
    opacity: 0;
    transition: opacity {fade}ms cubic-bezier(0.2, 0.8, 0.2, 1);
}}
popover.big-tooltip.visible {{
    opacity: 1;
}}
popover.big-tooltip > contents {{
    background-image: linear-gradient(145deg, {gloss}, alpha(#ffffff, 0));
    background-color: {tooltip_bg};
    color: {fg};
    padding: 9px 14px;
    border-radius: 14px;
    border: 1px solid {rim};
    box-shadow: 0 1px 2px alpha(#000000, 0.20),
                0 12px 32px alpha(#000000, 0.36);
}}
popover.big-tooltip label {{
    color: {fg};
}}
popover.big-tooltip b {{
    font-weight: 800;
}}
popover.big-tooltip small {{
    color: alpha({fg}, 0.62);
}}
popover.big-tooltip .big-tooltip-icon {{
    background-color: alpha({fg}, 0.08);
    border-radius: 11px;
    padding: 8px;
}}
"
        );

        let Some(display) = gdk::Display::default() else {
            return;
        };
        if let Some(old) = self.inner.css_provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 100,
        );
        *self.inner.css_provider.borrow_mut() = Some(provider);
    }
}

/// Whether `widget` currently has its own popover/menu open — a `MenuButton`
/// asking for its popup, a `ToggleButton` pressed, or a directly-attached
/// popover that is visible. Used to suppress the tooltip for an open control.
fn widget_popover_open(widget: &gtk::Widget) -> bool {
    if let Some(menu_button) = widget.downcast_ref::<gtk::MenuButton>() {
        if menu_button.is_active() {
            return true;
        }
        if let Some(popover) = menu_button.popover()
            && popover.is_visible()
        {
            return true;
        }
    }
    if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>()
        && toggle.is_active()
    {
        return true;
    }
    // A popover attached directly to this widget (set_parent) and shown.
    let mut child = widget.first_child();
    while let Some(node) = child {
        if let Some(popover) = node.downcast_ref::<gtk::Popover>()
            && popover.is_visible()
            && !popover.has_css_class("big-tooltip")
        {
            return true;
        }
        child = node.next_sibling();
    }
    false
}

fn clear_native_tooltip_tree(widget: &gtk::Widget) {
    widget.set_tooltip_text(None);
    widget.set_has_tooltip(false);
    let mut child = widget.first_child();
    while let Some(node) = child {
        clear_native_tooltip_tree(&node);
        child = node.next_sibling();
    }
}

fn tooltip_state(widget: &gtk::Widget) -> Option<&WidgetTooltipState> {
    // SAFETY: `QDATA_STATE` is always a `WidgetTooltipState` (see key definition); the
    // qdata value outlives the borrow — it is only removed by `steal_data`/finalize, and
    // the returned `&` is bounded by `widget`'s lifetime.
    let ptr: Option<std::ptr::NonNull<WidgetTooltipState>> = unsafe { widget.data(QDATA_STATE) };
    ptr.map(|ptr| unsafe { ptr.as_ref() })
}

fn existing_offset(widget: &gtk::Widget) -> Option<i32> {
    tooltip_state(widget).map(|state| state.y_offset)
}

fn existing_kind(widget: &gtk::Widget) -> Option<LabelContentKind> {
    tooltip_state(widget).map(|state| state.content_kind)
}

fn existing_follow(widget: &gtk::Widget) -> bool {
    tooltip_state(widget).is_some_and(|state| state.follow_pointer)
}

fn apply_label_content(label: &gtk::Label, content: &str, kind: LabelContentKind) {
    match kind {
        LabelContentKind::PlainText => label.set_text(content),
        LabelContentKind::Markup => label.set_markup(content),
    }
}
