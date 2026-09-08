// SPDX-License-Identifier: MIT

//! `BigTabButton` — one tab in a custom tab strip, as a Relm4
//! [`FactoryComponent`].
//!
//! The strip is a [`relm4::factory::FactoryVecDeque<BigTabButton>`] owning a
//! horizontal `GtkBox`; the host syncs it from its own page registry and
//! handles [`BigTabButtonOutput`] events (select / close / context-menu /
//! move). The model owns the tab's display state (title, icon, progress,
//! selection, hover, attention flash); the root widget is the tab button box.
//!
//! App-specific theming, the close-button tooltip, the accessible
//! description, and the attention-flash cadence are injected through a shared
//! [`BigTabButtonChrome`] (cloned per item via `Rc`), so the widget renders
//! identically to an app's hand-rolled strip while the factory lives here. The
//! tab title is composed by the host (e.g. appending a pane count) and pushed
//! in already-composed via [`BigTabButtonInput::SetTitle`].

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use gtk::pango;
use gtk::{
    Align, Box as GtkBox, Button, GestureClick, Image, Label, Orientation, PropagationPhase,
};
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk;

use crate::feedback::tooltip;
use crate::layout::tab_strip::{
    BigTabCloseButtonState, BigTabCloseMode, DEFAULT_ACTIVE_ONLY_CLOSE_BALANCE_WIDTH,
    DEFAULT_TAB_LABEL_WIDTH_CHARS, DEFAULT_TAB_STRIP_BOX_SPACING,
};

const CLOSE_ICON: &str = "window-close-symbolic";
/// Uniform row height of tabs in a vertical (side) strip, logical pixels.
const VERTICAL_TAB_HEIGHT: i32 = 34;
/// Row height of tabs docked in a horizontal (top/bottom) strip — a touch
/// taller than the packed header look.
const HORIZONTAL_DOCK_TAB_HEIGHT: i32 = 36;

/// CSS class names the [`BigTabButton`] render applies to its own widgets.
///
/// The [`Default`] uses canonical `big-tab-*` names (style them from the
/// shared stylesheet). Apps migrating a hand-rolled strip can override every
/// field with their existing class names so the rendered result — and their
/// theme CSS — stays byte-identical.
#[derive(Debug, Clone)]
pub struct BigTabButtonClasses {
    /// Base classes for the tab button root box.
    pub button: Vec<String>,
    /// Class toggled on the active (selected) tab.
    pub active: String,
    /// Class toggled while the attention flash is active.
    pub attention: String,
    /// Classes for the inline progress label.
    pub progress_label: Vec<String>,
    /// Classes for the close button.
    pub close_button: Vec<String>,
    /// Class set when the close button follows hover-reveal policy.
    pub close_hover_mode: String,
    /// Class set when the close button follows active-only policy.
    pub close_active_only: String,
    /// Class set when the active-only close button is on the selected tab.
    pub close_active_only_selected: String,
}

impl Default for BigTabButtonClasses {
    fn default() -> Self {
        Self {
            button: vec!["big-tab-button".to_owned(), "raised".to_owned()],
            active: "active".to_owned(),
            attention: "big-tab-attention".to_owned(),
            progress_label: vec!["caption".to_owned(), "big-tab-progress".to_owned()],
            close_button: vec![
                "circular".to_owned(),
                "flat".to_owned(),
                "big-tab-close-button".to_owned(),
            ],
            close_hover_mode: "big-tab-close-hover-mode".to_owned(),
            close_active_only: "big-tab-close-active-only".to_owned(),
            close_active_only_selected: "big-tab-close-active-only-selected".to_owned(),
        }
    }
}

/// Per-strip chrome shared by every [`BigTabButton`] (cloned via `Rc` into each
/// tab's [`BigTabButtonInit`]). Carries the app's theming, the close-button
/// tooltip, the tab's accessible description, and the attention-flash duration.
#[derive(Debug, Clone)]
pub struct BigTabButtonChrome {
    /// CSS class names applied during render.
    pub classes: BigTabButtonClasses,
    /// Translated tooltip + accessible label for the per-tab close button.
    pub close_tooltip: String,
    /// Translated accessible description for the tab button root.
    pub accessible_description: String,
    /// How long the attention flash (`Flash`) stays lit before auto-clearing.
    pub attention_timeout_ms: u32,
}

impl Default for BigTabButtonChrome {
    fn default() -> Self {
        Self {
            classes: BigTabButtonClasses::default(),
            close_tooltip: "Close tab".to_owned(),
            accessible_description: "Tab".to_owned(),
            attention_timeout_ms: 1000,
        }
    }
}

/// Display state needed to render one tab. The host keeps the durable per-tab
/// data; this is the projection the factory renders.
#[derive(Debug, Clone)]
pub struct BigTabButtonInit {
    /// Final tab title, already composed by the host.
    pub title: String,
    /// Optional symbolic icon name shown before the label.
    pub icon: Option<String>,
    /// Optional progress percentage (`0..=100`) shown inline.
    pub progress: Option<u8>,
    /// Whether this tab starts selected.
    pub selected: bool,
    /// Close-button visibility policy.
    pub close_mode: BigTabCloseMode,
    /// Whether the tab expands to fill the strip evenly.
    pub expand_evenly: bool,
    /// Whether the strip is vertical (side placement): tabs fill the strip
    /// width instead of sizing to their title.
    pub vertical: bool,
    /// Whether the strip is docked horizontally (top/bottom): tabs get a
    /// slightly taller row than in the header.
    pub docked_horizontal: bool,
    /// Shared per-strip chrome (theming, tooltip, a11y, flash cadence).
    pub chrome: Rc<BigTabButtonChrome>,
}

/// One tab button in the strip.
// Independent UI display flags (selection/hover/move-suppress/attention/expand);
// grouping them into a sub-struct would add indirection without meaning.
#[allow(clippy::struct_excessive_bools)]
pub struct BigTabButton {
    title: String,
    icon: Option<String>,
    progress: Option<u8>,
    selected: bool,
    hovered: bool,
    close_mode: BigTabCloseMode,
    expand_evenly: bool,
    vertical: bool,
    docked_horizontal: bool,
    /// Hide the close button while a tab/pane move is in flight.
    suppress_close: bool,
    /// Attention flash active.
    flashing: bool,
    chrome: Rc<BigTabButtonChrome>,
    /// The button widget, stashed in `init_widgets` so the owning host can
    /// hand it back via [`BigTabButton::button_widget`] — external CSS coloring
    /// keeps poking the real widget; `render` only toggles its own known
    /// classes, so externally-added classes coexist.
    root: Option<GtkBox>,
}

/// Handles `update_view`/`render` mutate.
pub struct BigTabButtonWidgets {
    root: GtkBox,
    icon: Image,
    progress: Label,
    label: Label,
    close: Button,
    close_balance: GtkBox,
}

/// Input messages driving one tab's display state.
#[derive(Debug, Clone)]
pub enum BigTabButtonInput {
    /// Replace the rendered tab title (host-composed).
    SetTitle(String),
    /// Set or clear the leading icon.
    SetIcon(Option<String>),
    /// Set or clear the inline progress percentage.
    SetProgress(Option<u8>),
    /// Set the selected (active) state.
    SetSelected(bool),
    /// Change the close-button visibility policy.
    SetCloseMode(BigTabCloseMode),
    /// Toggle even-expansion layout.
    SetExpandEvenly(bool),
    /// Toggle vertical-strip layout (tabs fill the strip width).
    SetVertical(bool),
    /// Toggle the taller top/bottom-dock row height.
    SetDockedHorizontal(bool),
    /// Set the hover state (drives hover-reveal close mode).
    SetHovered(bool),
    /// Hide the close button entirely (used while a move is in flight).
    SuppressClose(bool),
    /// Start the attention flash.
    Flash,
    /// Clear the attention flash (auto-sent after the chrome timeout).
    ClearFlash,
}

/// Output events emitted by a tab button, each carrying its [`DynamicIndex`].
#[derive(Debug)]
pub enum BigTabButtonOutput {
    /// Close button clicked.
    CloseRequested(DynamicIndex),
    /// Secondary (right) click at widget-local `(x, y)`: open the context menu.
    ContextMenu(DynamicIndex, f64, f64),
    /// Primary press at local `x` (the host decides select vs. commit-move).
    Press(DynamicIndex, f64),
    /// Pointer motion at local `x` (drives the move drop-target preview).
    Motion(DynamicIndex, f64),
    /// Pointer left the button.
    Leave(DynamicIndex),
}

impl BigTabButton {
    /// The button widget (available once `init_widgets` ran), for the owning
    /// host to expose to its per-tab CSS coloring / geometry code.
    #[must_use]
    pub fn button_widget(&self) -> Option<GtkBox> {
        self.root.clone()
    }
}

impl FactoryComponent for BigTabButton {
    type Init = BigTabButtonInit;
    type Input = BigTabButtonInput;
    type Output = BigTabButtonOutput;
    type CommandOutput = ();
    type ParentWidget = GtkBox;
    type Index = DynamicIndex;
    type Root = GtkBox;
    type Widgets = BigTabButtonWidgets;

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            title: init.title,
            icon: init.icon,
            progress: init.progress.filter(|p| *p <= 100),
            selected: init.selected,
            hovered: false,
            close_mode: init.close_mode,
            expand_evenly: init.expand_evenly,
            vertical: init.vertical,
            docked_horizontal: init.docked_horizontal,
            suppress_close: false,
            flashing: false,
            chrome: init.chrome,
            root: None,
        }
    }

    fn init_root(&self) -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(DEFAULT_TAB_STRIP_BOX_SPACING)
            .css_classes(self.chrome.classes.button.clone())
            .build()
    }

    fn init_widgets(
        &mut self,
        index: &DynamicIndex,
        root: Self::Root,
        _returned_widget: &gtk::Widget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let close_balance = GtkBox::builder()
            .valign(Align::Center)
            .width_request(DEFAULT_ACTIVE_ONLY_CLOSE_BALANCE_WIDTH)
            .visible(false)
            .build();
        root.append(&close_balance);
        let icon = Image::new();
        icon.set_visible(false);
        root.append(&icon);
        let progress = Label::builder()
            .css_classes(self.chrome.classes.progress_label.clone())
            .visible(false)
            .build();
        root.append(&progress);
        let label = Label::builder()
            .ellipsize(pango::EllipsizeMode::Start)
            .xalign(0.5)
            .width_chars(DEFAULT_TAB_LABEL_WIDTH_CHARS)
            .build();
        root.append(&label);
        let close_classes: Vec<&str> = self
            .chrome
            .classes
            .close_button
            .iter()
            .map(String::as_str)
            .collect();
        let close = tooltip::icon_button(CLOSE_ICON, &self.chrome.close_tooltip, &close_classes);
        close.set_valign(Align::Center);
        root.append(&close);
        root.set_accessible_role(gtk::AccessibleRole::Tab);
        root.update_property(&[
            gtk::accessible::Property::Label(""),
            gtk::accessible::Property::Description(self.chrome.accessible_description.as_str()),
        ]);

        {
            let sender = sender.clone();
            let index = index.clone();
            close.connect_clicked(move |_| {
                sender
                    .output(BigTabButtonOutput::CloseRequested(index.clone()))
                    .ok();
            });
        }
        {
            let left = GestureClick::builder()
                .button(gtk::gdk::BUTTON_PRIMARY)
                .propagation_phase(PropagationPhase::Capture)
                .build();
            let sender = sender.clone();
            let index = index.clone();
            left.connect_pressed(move |_, _, x, _| {
                sender
                    .output(BigTabButtonOutput::Press(index.clone(), x))
                    .ok();
            });
            root.add_controller(left);
        }
        {
            let right = GestureClick::builder()
                .button(gtk::gdk::BUTTON_SECONDARY)
                .build();
            let sender = sender.clone();
            let index = index.clone();
            right.connect_pressed(move |_, _, x, y| {
                sender
                    .output(BigTabButtonOutput::ContextMenu(index.clone(), x, y))
                    .ok();
            });
            root.add_controller(right);
        }
        {
            let motion = gtk::EventControllerMotion::new();
            let enter_sender = sender.clone();
            motion.connect_enter(move |_, _, _| {
                enter_sender.input(BigTabButtonInput::SetHovered(true));
            });
            let motion_sender = sender.clone();
            let motion_index = index.clone();
            motion.connect_motion(move |_, x, _| {
                motion_sender.input(BigTabButtonInput::SetHovered(true));
                motion_sender
                    .output(BigTabButtonOutput::Motion(motion_index.clone(), x))
                    .ok();
            });
            let leave_sender = sender.clone();
            let leave_index = index.clone();
            motion.connect_leave(move |_| {
                leave_sender.input(BigTabButtonInput::SetHovered(false));
                leave_sender
                    .output(BigTabButtonOutput::Leave(leave_index.clone()))
                    .ok();
            });
            root.add_controller(motion);
        }

        self.root = Some(root.clone());
        let mut widgets = BigTabButtonWidgets {
            root,
            icon,
            progress,
            label,
            close,
            close_balance,
        };
        self.render(&mut widgets);
        widgets
    }

    fn update(&mut self, message: Self::Input, sender: FactorySender<Self>) {
        match message {
            BigTabButtonInput::SetTitle(title) => self.title = title,
            BigTabButtonInput::SetIcon(icon) => self.icon = icon,
            BigTabButtonInput::SetProgress(p) => self.progress = p.filter(|v| *v <= 100),
            BigTabButtonInput::SetSelected(s) => self.selected = s,
            BigTabButtonInput::SetCloseMode(m) => self.close_mode = m,
            BigTabButtonInput::SetExpandEvenly(e) => self.expand_evenly = e,
            BigTabButtonInput::SetVertical(v) => self.vertical = v,
            BigTabButtonInput::SetDockedHorizontal(d) => self.docked_horizontal = d,
            BigTabButtonInput::SetHovered(h) => self.hovered = h,
            BigTabButtonInput::SuppressClose(s) => self.suppress_close = s,
            BigTabButtonInput::Flash => {
                self.flashing = true;
                let sender = sender.clone();
                glib::timeout_add_local_once(
                    Duration::from_millis(u64::from(self.chrome.attention_timeout_ms)),
                    move || sender.input(BigTabButtonInput::ClearFlash),
                );
            }
            BigTabButtonInput::ClearFlash => self.flashing = false,
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: FactorySender<Self>) {
        self.render(widgets);
    }
}

impl BigTabButton {
    /// Apply the model to the widgets. Idempotent; called from `init_widgets`
    /// and `update_view`.
    fn render(&self, widgets: &mut BigTabButtonWidgets) {
        widgets.label.set_text(&self.title);
        tooltip::set(&widgets.label, &self.title);
        widgets
            .root
            .update_property(&[gtk::accessible::Property::Label(self.title.as_str())]);
        widgets
            .root
            .update_state(&[gtk::accessible::State::Selected(Some(self.selected))]);

        if let Some(name) = &self.icon {
            widgets.icon.set_icon_name(Some(name));
            widgets.icon.set_visible(true);
        } else {
            widgets.icon.set_icon_name(None);
            widgets.icon.set_visible(false);
        }

        if let Some(p) = self.progress {
            widgets.progress.set_label(&format!("[{p}%]"));
            widgets.progress.set_visible(true);
        } else {
            widgets.progress.set_label("");
            widgets.progress.set_visible(false);
        }

        widgets
            .root
            .set_hexpand(self.expand_evenly || self.vertical);
        // Vertical (side) strips read as a LIST: every tab spans the strip
        // width at a uniform height, the label grows so the close button
        // pins to the trailing edge, and text left-aligns.
        widgets.label.set_hexpand(self.vertical);
        widgets
            .label
            .set_xalign(if self.vertical { 0.0 } else { 0.5 });
        widgets.root.set_height_request(if self.vertical {
            VERTICAL_TAB_HEIGHT
        } else if self.docked_horizontal {
            HORIZONTAL_DOCK_TAB_HEIGHT
        } else {
            -1
        });
        let classes = &self.chrome.classes;
        set_class(&widgets.root, &classes.active, self.selected);
        set_class(&widgets.root, &classes.attention, self.flashing);

        // Close-button visibility + css follow the shared close-mode policy,
        // suppressed entirely while a move is in flight.
        let css = self.close_mode.css_state(self.selected);
        set_class(&widgets.root, &classes.close_hover_mode, css.hover_mode);
        set_class(&widgets.root, &classes.close_active_only, css.active_only);
        set_class(
            &widgets.root,
            &classes.close_active_only_selected,
            css.active_only_selected,
        );
        let state = if self.suppress_close {
            BigTabCloseButtonState::hidden()
        } else {
            self.close_mode
                .button_state(self.selected, self.hovered, self.expand_evenly)
        };
        widgets.close_balance.set_visible(state.balance_visible);
        widgets.close.set_visible(state.occupies_space);
        widgets
            .close
            .set_opacity(if state.visible { 1.0 } else { 0.0 });
        widgets.close.set_sensitive(state.sensitive);
    }
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}
