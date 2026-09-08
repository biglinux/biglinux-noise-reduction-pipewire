// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared immersive chrome and toast-host policy for media apps (audio player,
//! video player, webcam, image viewer): a real `AdwHeaderBar` (top) with an
//! accessible primary menu, a bottom control bar, and an optional resizable
//! sidebar shell.
//!
//! Design: the app owns its media `gtk::Overlay`, mpv/camera/image surface, and
//! any clipped offload surface. The framework owns the repeatable shell policy:
//! toast host creation, content mounting, header, primary menu, bottom revealer,
//! and sidebar split/resize policy. This keeps each app's media plumbing
//! untouched (e.g. the video player's `GtkGraphicsOffload` subsurface, which an
//! `AdwToolbarView` would throttle to ~1 fps) while shared shell mechanics are
//! not re-derived.
//!
//! The primary menu is a real `GtkButton` on a real `AdwHeaderBar`, opening
//! explicit `MenuItem` rows so assistive technologies can name and activate
//! the trigger plus every action.
//! App actions go in the header slots and the bottom bar; auto-hide is driven by
//! the app via [`set_reveal`](BigMediaChrome::set_reveal).

use crate::layout::hamburger_menu::{BigMenuActionItem, build_flat_action_popover};
use adw::prelude::*;
use relm4::gtk;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

#[path = "media_window/sidebar.rs"]
mod sidebar;

pub use sidebar::{BigMediaSidebarShell, BigMediaSidebarSpec};

const DEFAULT_MENU_POPOVER_MAX_HEIGHT_PX: i32 = 560;
const MIN_MENU_POPOVER_MAX_HEIGHT_PX: i32 = 240;
const MENU_POPOVER_VIEWPORT_MARGIN_PX: i32 = 120;
const MENU_POPOVER_VERTICAL_PADDING_PX: i32 = 12;
const MENU_POPOVER_ROW_HEIGHT_PX: i32 = 41;
const DEFAULT_IMMERSION_IDLE_TIMEOUT_MS: u64 = 2_000;
const MIN_IMMERSION_IDLE_TIMEOUT_MS: u64 = 250;

/// Overlay policy for one media scene layer.
///
/// Media apps repeatedly need the same GTK overlay bookkeeping: the video,
/// camera, or image surface must be clipped to the scene and must not affect
/// layout measurement, while floating UI such as spinners, welcome panels, and
/// OSD chrome should be stacked in a predictable order. This display-free spec
/// keeps that policy testable before a rendered GTK smoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigMediaOverlayLayerSpec {
    should_clip_layer: bool,
    should_measure_layer: bool,
}

impl BigMediaOverlayLayerSpec {
    /// Layer policy for the primary media surface.
    ///
    /// Use for mpv/GLArea/camera/image widgets that should be clipped to the
    /// media scene and ignored by the overlay's size request.
    #[must_use]
    pub const fn clipped_media() -> Self {
        Self {
            should_clip_layer: true,
            should_measure_layer: false,
        }
    }

    /// Layer policy for floating UI that should not influence layout size.
    ///
    /// Use for OSD controls, buffering spinners, pointer hit regions, or other
    /// overlays whose position is controlled by their own alignment.
    #[must_use]
    pub const fn floating_overlay() -> Self {
        Self {
            should_clip_layer: false,
            should_measure_layer: false,
        }
    }

    /// Layer policy for an overlay that intentionally participates in layout
    /// measurement.
    ///
    /// This is rare for full-bleed media scenes, but keeps the explicit GTK
    /// policy available for bounded panels that must reserve space.
    #[must_use]
    pub const fn measured_overlay() -> Self {
        Self {
            should_clip_layer: false,
            should_measure_layer: true,
        }
    }

    /// Whether GTK should clip this layer to the overlay allocation.
    #[must_use]
    pub const fn should_clip_layer(self) -> bool {
        self.should_clip_layer
    }

    /// Whether GTK should include this layer in the overlay's size request.
    #[must_use]
    pub const fn should_measure_layer(self) -> bool {
        self.should_measure_layer
    }
}

/// Semantic role for one media overlay child.
///
/// The role names the user's visible layer purpose and carries the shared
/// clip/measurement policy. Consumers should add role layers in stacking order:
/// media surfaces first, then empty/welcome panels, then transient status or
/// viewer adornment overlays, then chrome via [`BigMediaChromePlacement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigMediaOverlayLayerRole {
    /// Primary mpv, GLArea, camera preview, or image viewport surface.
    PrimaryMediaSurface,
    /// Fallback renderer used when the primary surface cannot be realized.
    FallbackMediaSurface,
    /// Empty-state or first-run surface shown before media is active.
    WelcomeOverlay,
    /// Transient status UI such as buffering, loading, or progress overlays.
    StatusOverlay,
    /// Viewer-specific adornment such as OSD labels, histograms, or strips.
    ViewerAdornmentOverlay,
}

impl BigMediaOverlayLayerRole {
    /// Clip and measurement policy for this layer role.
    #[must_use]
    pub const fn layer_spec(self) -> BigMediaOverlayLayerSpec {
        match self {
            Self::PrimaryMediaSurface | Self::FallbackMediaSurface => {
                BigMediaOverlayLayerSpec::clipped_media()
            }
            Self::WelcomeOverlay | Self::StatusOverlay | Self::ViewerAdornmentOverlay => {
                BigMediaOverlayLayerSpec::floating_overlay()
            }
        }
    }

    /// Relative order used by consumers when assembling a media scene.
    #[must_use]
    pub const fn stacking_order(self) -> u8 {
        match self {
            Self::PrimaryMediaSurface | Self::FallbackMediaSurface => 10,
            Self::WelcomeOverlay => 30,
            Self::StatusOverlay | Self::ViewerAdornmentOverlay => 40,
        }
    }
}

/// Which part of [`BigMediaChrome`] to add at the current overlay position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigMediaChromePlacement {
    /// Add the header revealer only.
    Header,
    /// Add the bottom revealer only.
    Bottom,
    /// Add the header revealer and then the bottom revealer.
    HeaderThenBottom,
}

/// Shared `GtkOverlay` stack for full-bleed media scenes.
///
/// The app still owns its renderer widgets and domain state. This wrapper owns
/// only the repeatable GTK policy around the scene: one background child plus
/// ordered overlay layers with explicit clip/measurement behavior.
#[derive(Debug, Clone)]
pub struct BigMediaOverlayStack {
    overlay: gtk::Overlay,
    last_semantic_layer_order: Rc<Cell<u8>>,
}

impl BigMediaOverlayStack {
    /// Create a media overlay stack around the background widget.
    #[must_use]
    pub fn new(background: &impl IsA<gtk::Widget>) -> Self {
        let overlay = gtk::Overlay::builder()
            .hexpand(true)
            .vexpand(true)
            .child(background)
            .build();
        Self {
            overlay,
            last_semantic_layer_order: Rc::new(Cell::new(0)),
        }
    }

    /// The assembled `GtkOverlay`.
    #[must_use]
    pub fn overlay(&self) -> &gtk::Overlay {
        &self.overlay
    }

    /// Set the overflow behavior on the underlying overlay.
    pub fn set_overflow(&self, overflow: gtk::Overflow) {
        self.overlay.set_overflow(overflow);
    }

    /// Add a layer using an explicit clip/measurement policy.
    pub fn add_layer(&self, layer: &impl IsA<gtk::Widget>, layer_spec: BigMediaOverlayLayerSpec) {
        self.overlay.add_overlay(layer);
        self.overlay
            .set_clip_overlay(layer, layer_spec.should_clip_layer());
        self.overlay
            .set_measure_overlay(layer, layer_spec.should_measure_layer());
    }

    /// Add a layer using the shared semantic role policy.
    pub fn add_layer_with_role(
        &self,
        layer: &impl IsA<gtk::Widget>,
        layer_role: BigMediaOverlayLayerRole,
    ) {
        let previous_order = self.last_semantic_layer_order.get();
        let next_order = layer_role.stacking_order();
        debug_assert!(
            next_order >= previous_order,
            "media overlay layer {layer_role:?} was added after a later layer order {previous_order}"
        );
        self.last_semantic_layer_order
            .set(previous_order.max(next_order));
        self.add_layer(layer, layer_role.layer_spec());
    }

    /// Add a clipped, non-measured primary media layer.
    pub fn add_clipped_media_layer(&self, layer: &impl IsA<gtk::Widget>) {
        self.add_layer(layer, BigMediaOverlayLayerSpec::clipped_media());
    }

    /// Add a floating, non-measured overlay layer.
    pub fn add_floating_overlay(&self, layer: &impl IsA<gtk::Widget>) {
        self.add_layer(layer, BigMediaOverlayLayerSpec::floating_overlay());
    }

    /// Add the primary media surface for the scene.
    pub fn add_primary_media_surface(&self, surface: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(surface, BigMediaOverlayLayerRole::PrimaryMediaSurface);
    }

    /// Add a fallback media surface for GL-less or software-rendered sessions.
    pub fn add_fallback_media_surface(&self, surface: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(surface, BigMediaOverlayLayerRole::FallbackMediaSurface);
    }

    /// Add an empty-state or welcome overlay.
    pub fn add_welcome_overlay(&self, overlay: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(overlay, BigMediaOverlayLayerRole::WelcomeOverlay);
    }

    /// Add transient status UI such as buffering or loading state.
    pub fn add_status_overlay(&self, overlay: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(overlay, BigMediaOverlayLayerRole::StatusOverlay);
    }

    /// Add viewer-specific overlays such as an OSD label or histogram.
    pub fn add_viewer_adornment_overlay(&self, overlay: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(overlay, BigMediaOverlayLayerRole::ViewerAdornmentOverlay);
    }

    /// Install media chrome at the current overlay position.
    pub fn install_chrome(&self, chrome: &BigMediaChrome, placement: BigMediaChromePlacement) {
        match placement {
            BigMediaChromePlacement::Header => chrome.install_header_into(&self.overlay),
            BigMediaChromePlacement::Bottom => chrome.install_bottom_into(&self.overlay),
            BigMediaChromePlacement::HeaderThenBottom => chrome.install_into(&self.overlay),
        }
    }
}

/// Create the shared media toast host used as a mountable Relm4 root.
///
/// Media apps keep their renderer widgets and domain state app-local, but the
/// framework owns the repeatable `AdwToastOverlay` construction policy so every
/// video/camera/image shell starts with the same fill behavior and toast host.
#[must_use]
pub fn create_media_toast_overlay_root() -> adw::ToastOverlay {
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_hexpand(true);
    toast_overlay.set_vexpand(true);
    toast_overlay.set_halign(gtk::Align::Fill);
    toast_overlay.set_valign(gtk::Align::Fill);
    toast_overlay
}

/// Mount the media shell content inside the shared toast host.
pub fn mount_media_toast_content(
    toast_overlay: &adw::ToastOverlay,
    content: &impl IsA<gtk::Widget>,
) {
    toast_overlay.set_child(Some(content));
}

/// Create a shared media toast host and mount `content` into it.
#[must_use]
pub fn create_media_toast_overlay_with_content(
    content: &impl IsA<gtk::Widget>,
) -> adw::ToastOverlay {
    let toast_overlay = create_media_toast_overlay_root();
    mount_media_toast_content(&toast_overlay, content);
    toast_overlay
}

/// Spec for the primary (hamburger) menu.
#[derive(Debug, Clone)]
pub struct BigMediaMenuSpec {
    /// Symbolic icon name, e.g. `"open-menu-symbolic"`.
    pub icon_name: String,
    /// Accessible label + tooltip, e.g. `"Main menu"`.
    pub label: String,
    /// Flat ordered menu items (`win.*`/`app.*` actions).
    pub items: Vec<BigMenuActionItem>,
}

/// Shared floating chrome (top `AdwHeaderBar` + bottom bar, each in a `Revealer`)
/// for media-viewer apps. Build it, fill the header slots + bottom child, then
/// [`install_into`](Self::install_into) the app's `gtk::Overlay` (after the app
/// has added its media surfaces, so the bars stack on top).
#[derive(Debug, Clone)]
pub struct BigMediaChrome {
    header: adw::HeaderBar,
    header_revealer: gtk::Revealer,
    bottom_revealer: gtk::Revealer,
    menu_button: gtk::Button,
    menu_popover: gtk::Popover,
}

impl BigMediaChrome {
    /// Build the chrome. `reveal_initially` controls whether the bars start
    /// shown (e.g. `false` for a welcome/background state with no media yet).
    #[must_use]
    pub fn new(menu: BigMediaMenuSpec, reveal_initially: bool) -> Self {
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        let menu_button = crate::feedback::tooltip::icon_button(&menu.icon_name, &menu.label, &[]);
        let menu_popover = build_flat_action_popover(&menu_button, &menu.items);
        menu_popover.set_position(gtk::PositionType::Bottom);
        menu_popover.set_parent(&menu_button);
        menu_button.connect_unrealize({
            let menu_popover = menu_popover.downgrade();
            move |_| {
                if let Some(menu_popover) = menu_popover.upgrade()
                    && menu_popover.parent().is_some()
                {
                    menu_popover.unparent();
                }
            }
        });
        let menu_scroll = scroll_wrap_menu_popover(&menu_popover);
        menu_button.connect_clicked({
            let menu_popover = menu_popover.downgrade();
            let menu_scroll = menu_scroll.downgrade();
            move |button| {
                let Some(menu_popover) = menu_popover.upgrade() else {
                    return;
                };
                if menu_popover.is_visible() {
                    menu_popover.popdown();
                    return;
                }
                if let Some(menu_scroll) = menu_scroll.upgrade() {
                    apply_menu_popover_height(&menu_scroll, Some(button));
                }
                menu_popover.popup();
            }
        });
        header.pack_end(&menu_button);

        // `.osd-header` (app-themed) wrapper — the built-in `.osd` class forces
        // light-on-dark and breaks header icons in the light theme.
        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header_box.add_css_class("osd-header");
        header_box.append(&header);
        let header_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(reveal_initially)
            .child(&header_box)
            .build();

        let bottom_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::End)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(reveal_initially)
            .build();

        Self {
            header,
            header_revealer,
            bottom_revealer,
            menu_button,
            menu_popover,
        }
    }

    /// Add both bars to the app's overlay as top overlays. Call after the app
    /// has added media surfaces so the chrome stays on top.
    pub fn install_into(&self, overlay: &gtk::Overlay) {
        self.install_header_into(overlay);
        self.install_bottom_into(overlay);
    }

    /// Add only the header bar to the app's overlay. Use this when the app needs
    /// explicit overlay stacking order for OSD, welcome, buffering, or sidebars.
    pub fn install_header_into(&self, overlay: &gtk::Overlay) {
        overlay.add_overlay(&self.header_revealer);
        overlay.set_measure_overlay(&self.header_revealer, false);
    }

    /// Add only the bottom bar to the app's overlay. Use this when the app needs
    /// explicit overlay stacking order for OSD, welcome, buffering, or sidebars.
    pub fn install_bottom_into(&self, overlay: &gtk::Overlay) {
        overlay.add_overlay(&self.bottom_revealer);
        overlay.set_measure_overlay(&self.bottom_revealer, false);
    }

    /// Set the bottom bar's content (the app's transport / capture controls).
    pub fn set_bottom_child(&self, child: &impl IsA<gtk::Widget>) {
        self.bottom_revealer.set_child(Some(child));
    }

    /// Add an app action to the header START (left of the title).
    pub fn add_header_start(&self, widget: &impl IsA<gtk::Widget>) {
        self.header.pack_start(widget);
    }

    /// Add an app action to the header END (right, before the primary menu).
    pub fn add_header_end(&self, widget: &impl IsA<gtk::Widget>) {
        self.header.pack_end(widget);
    }

    /// The real `AdwHeaderBar` (for direct tweaks; prefer the slot helpers).
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// The primary menu button.
    #[must_use]
    pub fn menu_button(&self) -> &gtk::Button {
        &self.menu_button
    }

    /// The primary menu popover.
    #[must_use]
    pub fn menu_popover(&self) -> &gtk::Popover {
        &self.menu_popover
    }

    /// The header reveal widget (apps wire auto-hide / OSD onto it).
    #[must_use]
    pub fn header_revealer(&self) -> &gtk::Revealer {
        &self.header_revealer
    }

    /// The bottom reveal widget (apps wire auto-hide / OSD onto it).
    #[must_use]
    pub fn bottom_revealer(&self) -> &gtk::Revealer {
        &self.bottom_revealer
    }

    /// Reveal / hide the floating top + bottom bars together (auto-hide).
    pub fn set_reveal(&self, reveal: bool) {
        self.header_revealer.set_reveal_child(reveal);
        self.bottom_revealer.set_reveal_child(reveal);
    }
}

/// Shared idle-reveal controller for media chrome.
///
/// The controller observes pointer and keyboard activity on a media root, keeps
/// registered revealers or opacity targets visible while the user is active,
/// and hides them after a short idle timeout unless the app temporarily
/// inhibits hiding while a popover or dialog is open.
pub struct BigMediaImmersionController {
    opacity_targets: RefCell<Vec<gtk::glib::WeakRef<gtk::Widget>>>,
    reveal_targets: RefCell<Vec<gtk::glib::WeakRef<gtk::Revealer>>>,
    hide_guard_predicates: RefCell<Vec<Box<dyn Fn() -> bool>>>,
    cursor_host: gtk::glib::WeakRef<gtk::Widget>,
    idle_timeout_ms: Cell<u64>,
    inhibit_count: Cell<u32>,
    should_hide_cursor_when_idle: Cell<bool>,
    idle_timer: RefCell<Option<gtk::glib::SourceId>>,
    self_weak: RefCell<Weak<Self>>,
}

impl BigMediaImmersionController {
    /// Create a controller whose cursor is restored on `cursor_host` activity.
    #[must_use]
    pub fn new(cursor_host: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let controller = Rc::new(Self {
            opacity_targets: RefCell::new(Vec::new()),
            reveal_targets: RefCell::new(Vec::new()),
            hide_guard_predicates: RefCell::new(Vec::new()),
            cursor_host: cursor_host.upcast_ref::<gtk::Widget>().downgrade(),
            idle_timeout_ms: Cell::new(DEFAULT_IMMERSION_IDLE_TIMEOUT_MS),
            inhibit_count: Cell::new(0),
            should_hide_cursor_when_idle: Cell::new(false),
            idle_timer: RefCell::new(None),
            self_weak: RefCell::new(Weak::new()),
        });
        *controller.self_weak.borrow_mut() = Rc::downgrade(&controller);
        controller
    }

    /// Override the default idle timeout used before hiding registered chrome.
    pub fn set_idle_timeout(&self, idle_timeout: std::time::Duration) {
        self.idle_timeout_ms
            .set(bounded_immersion_idle_timeout_ms(idle_timeout));
    }

    /// Hide the cursor on the controller host when media chrome becomes idle.
    pub fn set_hide_cursor_when_idle(&self, should_hide: bool) {
        self.should_hide_cursor_when_idle.set(should_hide);
    }

    /// Reset the idle timer on pointer or keyboard activity over `root`.
    pub fn attach_activity_detection(self: &Rc<Self>, root: &impl IsA<gtk::Widget>) {
        let motion = gtk::EventControllerMotion::new();
        let controller = self.clone();
        motion.connect_motion(move |_, _, _| controller.show_and_schedule_hide());
        root.add_controller(motion);

        let key = gtk::EventControllerKey::new();
        let controller = self.clone();
        key.connect_key_pressed(move |_, _, _, _| {
            controller.show_and_schedule_hide();
            gtk::glib::Propagation::Proceed
        });
        root.add_controller(key);
        self.show_and_schedule_hide();
    }

    /// Fade `widget` out when the media surface becomes idle.
    pub fn register_opacity_target(&self, widget: &impl IsA<gtk::Widget>) {
        self.opacity_targets
            .borrow_mut()
            .push(widget.upcast_ref::<gtk::Widget>().downgrade());
    }

    /// Hide `revealer` when the media surface becomes idle.
    pub fn register_revealer(&self, revealer: &gtk::Revealer) {
        self.reveal_targets.borrow_mut().push(revealer.downgrade());
        self.register_focus_hide_guard(revealer);
    }

    /// Keep chrome visible while `should_keep_visible` returns `true`.
    pub fn register_hide_guard(&self, should_keep_visible: impl Fn() -> bool + 'static) {
        self.hide_guard_predicates
            .borrow_mut()
            .push(Box::new(should_keep_visible));
    }

    /// Keep chrome visible while `widget` or one of its descendants has focus.
    pub fn register_focus_hide_guard(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.upcast_ref::<gtk::Widget>().downgrade();
        self.register_hide_guard(move || {
            widget
                .upgrade()
                .is_some_and(|widget| media_chrome_subtree_has_focus(&widget))
        });
    }

    /// Keep chrome visible while `popover` is open.
    pub fn register_popover_hide_guard(&self, popover: &gtk::Popover) {
        let popover = popover.downgrade();
        self.register_hide_guard(move || {
            popover
                .upgrade()
                .is_some_and(|popover| popover.is_visible())
        });
    }

    /// Keep registered chrome visible until a matching [`Self::uninhibit`].
    pub fn inhibit(&self) {
        self.inhibit_count.set(self.inhibit_count.get() + 1);
        self.show_all();
    }

    /// Release one hide inhibition and restart the idle timer if none remain.
    pub fn uninhibit(&self) {
        let new_count = self.inhibit_count.get().saturating_sub(1);
        self.inhibit_count.set(new_count);
        if new_count == 0 {
            self.show_and_schedule_hide();
        }
    }

    /// Reveal registered chrome without arming an idle hide.
    pub fn show_now(&self) {
        self.cancel_pending();
        self.show_all();
    }

    /// Hide registered chrome now unless a hide guard says it must remain shown.
    pub fn hide_now(&self) {
        self.cancel_pending();
        self.hide_all();
    }

    /// Reveal registered chrome and arm the idle hide timer when guards allow it.
    pub fn show_and_schedule_hide(&self) {
        self.cancel_pending();
        self.show_all();
        if self.should_keep_chrome_visible() {
            return;
        }
        let controller = self.self_weak.borrow().clone();
        let idle_timeout = std::time::Duration::from_millis(self.idle_timeout_ms.get());
        let id = gtk::glib::timeout_add_local(idle_timeout, move || {
            if let Some(controller) = controller.upgrade() {
                controller.hide_all();
                controller.idle_timer.borrow_mut().take();
            }
            gtk::glib::ControlFlow::Break
        });
        *self.idle_timer.borrow_mut() = Some(id);
    }

    fn cancel_pending(&self) {
        if let Some(source_id) = self.idle_timer.borrow_mut().take() {
            source_id.remove();
        }
    }

    fn guard_snapshot(&self) -> BigMediaImmersionGuardSnapshot {
        BigMediaImmersionGuardSnapshot {
            active_inhibitor_count: self.inhibit_count.get(),
            active_hide_guard_count: self
                .hide_guard_predicates
                .borrow()
                .iter()
                .filter(|should_keep_visible| should_keep_visible())
                .count(),
        }
    }

    fn should_keep_chrome_visible(&self) -> bool {
        self.guard_snapshot().should_keep_chrome_visible()
    }

    fn show_all(&self) {
        for widget in self
            .opacity_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            widget.set_opacity(1.0);
        }
        for revealer in self
            .reveal_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            revealer.set_reveal_child(true);
        }
        if self.should_hide_cursor_when_idle.get()
            && let Some(host) = self.cursor_host.upgrade()
        {
            host.set_cursor_from_name(Some("default"));
        }
    }

    fn hide_all(&self) {
        if self.should_keep_chrome_visible() {
            return;
        }
        for widget in self
            .opacity_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            widget.set_opacity(0.0);
        }
        for revealer in self
            .reveal_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            revealer.set_reveal_child(false);
        }
        if self.should_hide_cursor_when_idle.get()
            && let Some(host) = self.cursor_host.upgrade()
        {
            host.set_cursor_from_name(Some("none"));
        }
    }
}

impl Drop for BigMediaImmersionController {
    fn drop(&mut self) {
        if let Some(source_id) = self.idle_timer.borrow_mut().take() {
            source_id.remove();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BigMediaImmersionGuardSnapshot {
    active_inhibitor_count: u32,
    active_hide_guard_count: usize,
}

impl BigMediaImmersionGuardSnapshot {
    const fn should_keep_chrome_visible(self) -> bool {
        self.active_inhibitor_count > 0 || self.active_hide_guard_count > 0
    }
}

fn bounded_immersion_idle_timeout_ms(idle_timeout: std::time::Duration) -> u64 {
    let timeout_ms = idle_timeout.as_millis().try_into().unwrap_or(u64::MAX);
    timeout_ms.max(MIN_IMMERSION_IDLE_TIMEOUT_MS)
}

/// Return whether `chrome_widget` or one of its descendants has keyboard focus.
#[must_use]
pub fn media_chrome_subtree_has_focus(chrome_widget: &impl IsA<gtk::Widget>) -> bool {
    let chrome_widget = chrome_widget.upcast_ref::<gtk::Widget>();
    if chrome_widget.has_focus() {
        return true;
    }

    let Some(window) = chrome_widget.root().and_downcast::<gtk::Window>() else {
        return false;
    };
    let Some(focused_widget) = gtk::prelude::GtkWindowExt::focus(&window) else {
        return false;
    };

    widget_contains_descendant(chrome_widget, &focused_widget)
}

fn widget_contains_descendant(widget: &gtk::Widget, descendant: &gtk::Widget) -> bool {
    if descendant == widget {
        return true;
    }

    let mut parent = descendant.parent();
    while let Some(current_parent) = parent {
        if &current_parent == widget {
            return true;
        }
        parent = current_parent.parent();
    }

    false
}

fn scroll_wrap_menu_popover(popover: &gtk::Popover) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .max_content_height(menu_popover_max_height(0))
        .build();
    if let Some(menu_child) = popover.child() {
        popover.set_child(None::<&gtk::Widget>);
        scroller.set_child(Some(&menu_child));
        popover.set_child(Some(&scroller));
    }
    scroller
}

fn apply_menu_popover_height(scroller: &gtk::ScrolledWindow, button: Option<&gtk::Button>) {
    let viewport_height = button
        .and_then(|button| button.root())
        .and_then(|root| root.downcast::<gtk::Window>().ok())
        .map_or(0, |window| window.height());
    scroller.set_max_content_height(menu_popover_max_height(viewport_height));
}

fn menu_popover_max_height(viewport_height: i32) -> i32 {
    let candidate_height = if viewport_height <= 0 {
        DEFAULT_MENU_POPOVER_MAX_HEIGHT_PX
    } else {
        viewport_height - MENU_POPOVER_VIEWPORT_MARGIN_PX
    };
    let bounded_height = candidate_height.clamp(
        MIN_MENU_POPOVER_MAX_HEIGHT_PX,
        DEFAULT_MENU_POPOVER_MAX_HEIGHT_PX,
    );
    snap_menu_popover_height_to_rows(bounded_height)
}

fn snap_menu_popover_height_to_rows(max_height: i32) -> i32 {
    if max_height <= MIN_MENU_POPOVER_MAX_HEIGHT_PX {
        return MIN_MENU_POPOVER_MAX_HEIGHT_PX;
    }

    let row_area_height = max_height - MENU_POPOVER_VERTICAL_PADDING_PX;
    let full_row_count = (row_area_height / MENU_POPOVER_ROW_HEIGHT_PX).max(1);
    let snapped_height =
        MENU_POPOVER_VERTICAL_PADDING_PX + (full_row_count * MENU_POPOVER_ROW_HEIGHT_PX);

    snapped_height.clamp(MIN_MENU_POPOVER_MAX_HEIGHT_PX, max_height)
}

#[cfg(test)]
mod tests {
    use super::BigMediaImmersionGuardSnapshot;
    use super::{
        BigMediaChromePlacement, BigMediaOverlayLayerRole, BigMediaOverlayLayerSpec,
        DEFAULT_MENU_POPOVER_MAX_HEIGHT_PX, MENU_POPOVER_ROW_HEIGHT_PX,
        MENU_POPOVER_VERTICAL_PADDING_PX, MIN_IMMERSION_IDLE_TIMEOUT_MS,
        MIN_MENU_POPOVER_MAX_HEIGHT_PX, bounded_immersion_idle_timeout_ms, menu_popover_max_height,
        snap_menu_popover_height_to_rows,
    };

    #[test]
    fn menu_popover_height_fits_720p_viewports() {
        assert_eq!(menu_popover_max_height(720), 545);
    }

    #[test]
    fn menu_popover_height_leaves_margin_on_short_viewports() {
        assert_eq!(menu_popover_max_height(600), 463);
    }

    #[test]
    fn menu_popover_height_keeps_minimum_for_tiny_viewports() {
        assert_eq!(menu_popover_max_height(300), MIN_MENU_POPOVER_MAX_HEIGHT_PX);
    }

    #[test]
    fn menu_popover_height_uses_default_before_realize() {
        assert_eq!(menu_popover_max_height(0), 545);
    }

    #[test]
    fn menu_popover_height_snaps_to_complete_rows() {
        let snapped_height = snap_menu_popover_height_to_rows(DEFAULT_MENU_POPOVER_MAX_HEIGHT_PX);

        assert_eq!(
            (snapped_height - MENU_POPOVER_VERTICAL_PADDING_PX) % MENU_POPOVER_ROW_HEIGHT_PX,
            0,
            "menu viewport should not clip the final visible row"
        );
    }

    #[test]
    fn media_overlay_layer_specs_encode_clip_and_measure_policy() {
        let media_layer = BigMediaOverlayLayerSpec::clipped_media();
        assert!(media_layer.should_clip_layer());
        assert!(!media_layer.should_measure_layer());

        let floating_layer = BigMediaOverlayLayerSpec::floating_overlay();
        assert!(!floating_layer.should_clip_layer());
        assert!(!floating_layer.should_measure_layer());

        let measured_layer = BigMediaOverlayLayerSpec::measured_overlay();
        assert!(!measured_layer.should_clip_layer());
        assert!(measured_layer.should_measure_layer());
    }

    #[test]
    fn media_overlay_layer_roles_encode_media_scene_policy() {
        for role in [
            BigMediaOverlayLayerRole::PrimaryMediaSurface,
            BigMediaOverlayLayerRole::FallbackMediaSurface,
        ] {
            assert!(
                role.layer_spec().should_clip_layer(),
                "{role:?} should clip to the media scene"
            );
            assert!(
                !role.layer_spec().should_measure_layer(),
                "{role:?} should not influence overlay measurement"
            );
            assert_eq!(role.stacking_order(), 10);
        }

        for role in [
            BigMediaOverlayLayerRole::WelcomeOverlay,
            BigMediaOverlayLayerRole::StatusOverlay,
            BigMediaOverlayLayerRole::ViewerAdornmentOverlay,
        ] {
            assert!(
                !role.layer_spec().should_clip_layer(),
                "{role:?} should float over the media scene"
            );
            assert!(
                !role.layer_spec().should_measure_layer(),
                "{role:?} should not influence overlay measurement"
            );
        }

        assert!(
            BigMediaOverlayLayerRole::PrimaryMediaSurface.stacking_order()
                < BigMediaOverlayLayerRole::WelcomeOverlay.stacking_order()
        );
        assert!(
            BigMediaOverlayLayerRole::WelcomeOverlay.stacking_order()
                < BigMediaOverlayLayerRole::StatusOverlay.stacking_order()
        );
    }

    #[test]
    fn media_chrome_placements_name_the_overlay_mount_points() {
        let header_placement = BigMediaChromePlacement::Header;
        assert!(matches!(header_placement, BigMediaChromePlacement::Header));
        assert_ne!(
            BigMediaChromePlacement::Header,
            BigMediaChromePlacement::Bottom
        );
        assert_eq!(
            format!("{:?}", BigMediaChromePlacement::HeaderThenBottom),
            "HeaderThenBottom"
        );
    }

    #[test]
    fn immersion_guard_snapshot_pins_chrome_for_inhibitors_or_hide_guards() {
        assert!(
            BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 1,
                active_hide_guard_count: 0,
            }
            .should_keep_chrome_visible()
        );
        assert!(
            BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 0,
                active_hide_guard_count: 1,
            }
            .should_keep_chrome_visible()
        );
        assert!(
            !BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 0,
                active_hide_guard_count: 0,
            }
            .should_keep_chrome_visible()
        );
    }

    #[test]
    fn immersion_idle_timeout_never_flickers_immediately() {
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::ZERO),
            MIN_IMMERSION_IDLE_TIMEOUT_MS
        );
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::from_millis(1)),
            MIN_IMMERSION_IDLE_TIMEOUT_MS
        );
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::from_millis(1_500)),
            1_500
        );
    }
}
