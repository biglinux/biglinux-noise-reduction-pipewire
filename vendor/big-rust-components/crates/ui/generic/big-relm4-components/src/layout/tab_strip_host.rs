// SPDX-License-Identifier: MIT

//! `BigTabStrip` — a custom tab-strip HOST built on the [`BigTabButton`]
//! factory: a horizontal strip of tab buttons + an `adw::ViewStack` content
//! host, with selection, reorder, click-to-move/drop, tab groups (via external
//! CSS coloring of each button), a shared context-menu popover, and a
//! push-only listener API for a parent controller.
//!
//! Content is opaque: each tab holds a [`BigTabPage`] wrapping any
//! `gtk::Widget`, so the strip is reusable by any app (terminal, editor, file
//! manager). App specifics inject through [`BigTabButtonChrome`] +
//! [`BigTabStripChrome`] + a [`TitleComposer`]; the strip itself is free of any
//! app crate. Lifted from big-terminal's `tabs/custom` (K2-T6b.2).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::gtk;
use relm4::gtk::gdk;
use relm4::gtk::gio;
use relm4::gtk::glib;
use relm4::gtk::{Box as GtkBox, Orientation, PopoverMenu, PropagationPhase, Widget};

use super::tab_button::{
    BigTabButton, BigTabButtonChrome, BigTabButtonInit, BigTabButtonInput, BigTabButtonOutput,
};
use super::tab_strip::{BigTabCloseMode, BigTabStripChrome, DEFAULT_TAB_BAR_SPACING};
use super::tab_workspace::{selection_index_after_move, selection_index_after_remove};

#[path = "tab_strip_host/geometry.rs"]
mod geometry;
#[path = "tab_strip_host/page.rs"]
mod page;

pub use geometry::set_tab_context_menu_window_state;
use geometry::{clamp_to_i32, drop_placement_for_pointer};
pub use page::BigTabPage;

/// Composes a tab's rendered title from its base label and a secondary count
/// (e.g. big-terminal appends a `[N]` terminal-pane count). The default ignores the
/// count and renders the base label verbatim.
pub type TitleComposer = Rc<dyn Fn(&str, usize) -> String>;

/// Default [`TitleComposer`]: render the base title unchanged.
#[must_use]
pub fn identity_title_composer() -> TitleComposer {
    Rc::new(|base: &str, _count: usize| base.to_owned())
}

type ViewListener = Rc<dyn Fn(&BigTabStrip)>;
type PageListener = Rc<dyn Fn(&BigTabStrip, &Rc<BigTabPage>, i32)>;
type SetupMenuListener = Rc<dyn Fn(&BigTabStrip, Option<&Rc<BigTabPage>>)>;
type MoveCommittedListener = Rc<dyn Fn(&BigTabStrip, &Rc<BigTabPage>, &Rc<BigTabPage>)>;
type MoveInsideRequestedListener = Rc<dyn Fn(&BigTabStrip, &Rc<BigTabPage>, &Rc<BigTabPage>)>;
type CloseRequestCallback = Rc<dyn Fn(&BigTabStrip, &Rc<BigTabPage>) -> bool>;
/// App hook that appends additional sections to the standard tab context menu.
///
/// Intended for terminal-style rich menus that add duplicate, color, and group
/// actions after the shared move/close sections.
pub type BigTabContextMenuExtender = Rc<dyn Fn(&BigTabStrip, &Rc<BigTabPage>, &gio::Menu)>;

/// Where a click-to-move drop lands relative to the target tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPlacement {
    /// Drop before the target (reorder to its left).
    Left,
    /// Drop onto the target (the parent decides: e.g. split-into).
    Inside,
    /// Drop after the target (reorder to its right).
    Right,
}

/// The strip's push-only event observer lists (one cohesive concern).
#[derive(Default)]
struct BigTabStripListeners {
    selected: RefCell<Vec<ViewListener>>,
    n_pages: RefCell<Vec<ViewListener>>,
    attached: RefCell<Vec<PageListener>>,
    detached: RefCell<Vec<PageListener>>,
    setup_menu: RefCell<Vec<SetupMenuListener>>,
    move_committed: RefCell<Vec<MoveCommittedListener>>,
    move_inside_requested: RefCell<Vec<MoveInsideRequestedListener>>,
}

struct MoveState {
    moving: Weak<BigTabPage>,
    drop_target: RefCell<Option<Weak<BigTabPage>>>,
    drop_placement: Cell<DropPlacement>,
}

struct BigTabStripInner {
    view_stack: adw::ViewStack,
    tab_bar: GtkBox,
    factory: RefCell<FactoryVecDeque<BigTabButton>>,
    chrome: Rc<BigTabButtonChrome>,
    strip_chrome: BigTabStripChrome,
    title_composer: TitleComposer,
    pages: RefCell<Vec<Rc<BigTabPage>>>,
    selected: Cell<Option<usize>>,
    listeners: BigTabStripListeners,
    menu_model: RefCell<Option<gio::MenuModel>>,
    popover: PopoverMenu,
    move_state: RefCell<Option<MoveState>>,
    close_request_callback: RefCell<Option<CloseRequestCallback>>,
    close_mode: Cell<BigTabCloseMode>,
    expand_evenly: Cell<bool>,
    vertical: Cell<bool>,
    docked_horizontal: Cell<bool>,
}

/// A custom tab strip. Cheap to clone (shared `Rc` inner).
pub struct BigTabStrip {
    inner: Rc<BigTabStripInner>,
}

impl Clone for BigTabStrip {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl PartialEq for BigTabStrip {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Style a dock new-tab stand-in for its mount. Vertical (side) strips get a
/// Firefox-like full-width ROW — icon + label, left-aligned, tab-height —
/// below the last tab; horizontal docks keep a plain icon button.
pub fn style_dock_new_tab_button(button: &gtk::Button, label: &str, vertical: bool) {
    if vertical {
        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        content.append(&gtk::Image::from_icon_name("tab-new-symbolic"));
        let text = gtk::Label::builder().label(label).xalign(0.0).build();
        content.append(&text);
        button.set_child(Some(&content));
        button.set_halign(gtk::Align::Fill);
        button.set_hexpand(true);
        button.set_height_request(34);
        button.add_css_class("big-tab-new-row");
    } else {
        button.set_icon_name("tab-new-symbolic");
        button.set_halign(gtk::Align::Start);
        button.set_hexpand(false);
        button.set_height_request(-1);
        button.set_valign(gtk::Align::Center);
        button.remove_css_class("big-tab-new-row");
    }
}

/// Pre-translated labels for [`BigTabStrip::install_standard_context_menu`].
pub struct BigTabStandardMenuLabels {
    /// "Move Tab" entry (starts click-to-place move mode).
    pub move_tab: String,
    /// "Move Left" entry (horizontal strips).
    pub move_left: String,
    /// "Move Right" entry (horizontal strips).
    pub move_right: String,
    /// "Move Up" entry (vertical strips).
    pub move_up: String,
    /// "Move Down" entry (vertical strips).
    pub move_down: String,
    /// "Detach Tab" entry (shown only when a detach hook is installed).
    pub detach_tab: String,
    /// "Close Tab" entry.
    pub close_tab: String,
    /// "Close Other Tabs" entry.
    pub close_other_tabs: String,
    /// "Close Tabs to the Right" entry.
    pub close_tabs_right: String,
}

/// A weak handle to a [`BigTabStrip`] (for closures stored on its widgets).
#[derive(Clone)]
pub struct WeakBigTabStrip {
    inner: Weak<BigTabStripInner>,
}

impl WeakBigTabStrip {
    /// Upgrade to a live [`BigTabStrip`], if the strip is still alive.
    #[must_use]
    pub fn upgrade(&self) -> Option<BigTabStrip> {
        Some(BigTabStrip {
            inner: self.inner.upgrade()?,
        })
    }
}

impl BigTabStrip {
    /// A weak handle to this strip.
    #[must_use]
    pub fn downgrade(&self) -> WeakBigTabStrip {
        WeakBigTabStrip {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// Build a strip with the given per-button + per-strip chrome and a title
    /// composer (use [`identity_title_composer`] for verbatim titles).
    #[must_use]
    pub fn new(
        chrome: Rc<BigTabButtonChrome>,
        strip_chrome: BigTabStripChrome,
        title_composer: TitleComposer,
    ) -> Self {
        let view_stack = adw::ViewStack::builder().vexpand(true).build();
        view_stack.add_css_class("big-tab-content-stack");
        let tab_bar = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(DEFAULT_TAB_BAR_SPACING)
            .build();
        let popover = PopoverMenu::from_model(None::<&gio::MenuModel>);
        popover.set_has_arrow(false);
        popover.set_parent(&tab_bar);
        crate::feedback::popover_lifecycle::unparent_popover_on_parent_teardown(
            tab_bar.upcast_ref(),
            &popover,
        );
        popover.update_property(&[gtk::accessible::Property::Label(
            &strip_chrome.popover_accessible_label,
        )]);
        let context_menu_class = strip_chrome.context_menu_open_window.clone();
        let tab_bar_for_popover = tab_bar.downgrade();
        popover.connect_closed(move |_| {
            if let Some(tab_bar) = tab_bar_for_popover.upgrade() {
                set_tab_context_menu_window_state(&tab_bar, false, &context_menu_class);
            }
        });
        let (out_tx, out_rx) = relm4::channel::<BigTabButtonOutput>();
        let factory = FactoryVecDeque::builder()
            .launch(tab_bar.clone())
            .forward(&out_tx, |out| out);
        let inner = Rc::new(BigTabStripInner {
            view_stack,
            tab_bar,
            factory: RefCell::new(factory),
            chrome,
            strip_chrome,
            title_composer,
            pages: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            listeners: BigTabStripListeners::default(),
            menu_model: RefCell::new(None),
            popover,
            move_state: RefCell::new(None),
            close_request_callback: RefCell::new(None),
            close_mode: Cell::new(BigTabCloseMode::Always),
            expand_evenly: Cell::new(false),
            vertical: Cell::new(false),
            docked_horizontal: Cell::new(false),
        });
        let weak = Rc::downgrade(&inner);
        relm4::spawn_local(async move {
            while let Some(out) = out_rx.recv().await {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                BigTabStrip { inner }.handle_button_output(&out);
            }
        });
        let view = Self { inner };
        view.attach_escape_key_controller();
        view
    }

    fn handle_button_output(&self, out: &BigTabButtonOutput) {
        let index = match out {
            BigTabButtonOutput::CloseRequested(i)
            | BigTabButtonOutput::ContextMenu(i, _, _)
            | BigTabButtonOutput::Press(i, _)
            | BigTabButtonOutput::Motion(i, _)
            | BigTabButtonOutput::Leave(i) => i.current_index(),
        };
        let Some(page) = self.inner.pages.borrow().get(index).cloned() else {
            return;
        };
        match out {
            BigTabButtonOutput::Press(_, x) => {
                if self.is_move_active() {
                    self.handle_move_click(&page, *x);
                } else {
                    self.set_selected_page(&page);
                }
            }
            BigTabButtonOutput::CloseRequested(_) => self.request_close_page(&page),
            BigTabButtonOutput::ContextMenu(_, x, y) => self.show_context_menu_for(&page, *x, *y),
            BigTabButtonOutput::Motion(_, x) => self.handle_move_motion(&page, *x),
            BigTabButtonOutput::Leave(_) => self.handle_move_leave(&page),
        }
    }

    fn send_to_button(&self, page: &Rc<BigTabPage>, input: BigTabButtonInput) {
        if let Some(idx) = self.position_of(page) {
            self.inner.factory.borrow().send(idx, input);
        }
    }

    fn suppress_close_buttons(&self, suppress: bool) {
        self.inner
            .factory
            .borrow()
            .broadcast(BigTabButtonInput::SuppressClose(suppress));
    }

    fn attach_escape_key_controller(&self) {
        self.inner.tab_bar.set_can_focus(true);
        self.inner.tab_bar.set_focusable(true);
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(PropagationPhase::Capture);
        let weak = self.downgrade();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            let Some(view) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if view.is_move_active() {
                view.cancel_move();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.inner.tab_bar.add_controller(key);
    }

    /// The tab-button strip widget (mount it in a header bar / toolbar).
    #[must_use]
    pub fn tab_bar(&self) -> &GtkBox {
        &self.inner.tab_bar
    }

    /// The content host showing the selected tab's child.
    #[must_use]
    pub fn content_widget(&self) -> &adw::ViewStack {
        &self.inner.view_stack
    }

    /// Number of open tabs.
    #[must_use]
    pub fn n_pages(&self) -> i32 {
        i32::try_from(self.inner.pages.borrow().len()).unwrap_or(i32::MAX)
    }

    /// Append `child` as a new tab and return its [`BigTabPage`] handle.
    pub fn append<W: IsA<Widget>>(&self, child: &W) -> Rc<BigTabPage> {
        let widget: Widget = child.clone().upcast();
        self.inner.view_stack.add(&widget);
        let init = self.default_button_init();
        let position = {
            let mut factory = self.inner.factory.borrow_mut();
            let index = factory.guard().push_back(init);
            index.current_index()
        };
        let button = self
            .button_widget_at(position)
            .expect("factory builds the tab button synchronously on push_back");
        let page = BigTabPage::new(widget, button, Rc::downgrade(&self.inner));
        self.inner.pages.borrow_mut().insert(position, page.clone());
        self.fire_attached(&page, i32::try_from(position).unwrap_or(i32::MAX));
        self.fire_n_pages();
        if self.inner.selected.get().is_none() {
            self.set_selected_page(&page);
        }
        page
    }

    fn default_button_init(&self) -> BigTabButtonInit {
        BigTabButtonInit {
            title: (self.inner.title_composer)("", 1),
            icon: None,
            progress: None,
            selected: false,
            close_mode: self.inner.close_mode.get(),
            expand_evenly: self.inner.expand_evenly.get(),
            vertical: self.inner.vertical.get(),
            docked_horizontal: self.inner.docked_horizontal.get(),
            chrome: self.inner.chrome.clone(),
        }
    }

    fn button_widget_at(&self, index: usize) -> Option<GtkBox> {
        self.inner
            .factory
            .borrow()
            .get(index)
            .and_then(BigTabButton::button_widget)
    }

    /// The currently selected page, if any.
    #[must_use]
    pub fn selected_page(&self) -> Option<Rc<BigTabPage>> {
        let idx = self.inner.selected.get()?;
        self.inner.pages.borrow().get(idx).cloned()
    }

    /// Select the page at the zero-based `index`, or do nothing if absent.
    pub fn select_page_at_index(&self, index: usize) {
        let page = self.inner.pages.borrow().get(index).cloned();
        if let Some(page) = page {
            self.set_selected_page(&page);
        }
    }

    /// Make `page` the selected tab.
    pub fn set_selected_page(&self, page: &Rc<BigTabPage>) {
        let Some(idx) = self.position_of(page) else {
            return;
        };
        if self.inner.selected.get() == Some(idx) {
            return;
        }
        self.apply_selection(idx);
        self.fire_selected();
    }

    fn apply_selection(&self, idx: usize) {
        let factory = self.inner.factory.borrow();
        if let Some(prev) = self.inner.selected.get() {
            factory.send(prev, BigTabButtonInput::SetSelected(false));
        }
        self.inner.selected.set(Some(idx));
        let Some(page) = self.inner.pages.borrow().get(idx).cloned() else {
            return;
        };
        factory.send(idx, BigTabButtonInput::SetSelected(true));
        self.inner.view_stack.set_visible_child(&page.child());
    }

    fn position_of(&self, page: &Rc<BigTabPage>) -> Option<usize> {
        self.inner
            .pages
            .borrow()
            .iter()
            .position(|p| Rc::ptr_eq(p, page))
    }

    /// Close `page` (no close-request confirmation).
    pub fn close_page(&self, page: &Rc<BigTabPage>) {
        let Some(idx) = self.position_of(page) else {
            return;
        };
        self.remove_at(idx);
    }

    /// Close every page except `keep`.
    pub fn close_other_pages(&self, keep: &Rc<BigTabPage>) {
        let pages: Vec<Rc<BigTabPage>> = self
            .inner
            .pages
            .borrow()
            .iter()
            .filter(|p| !Rc::ptr_eq(p, keep))
            .cloned()
            .collect();
        for page in pages {
            self.close_page(&page);
        }
    }

    /// Close every page after `anchor`.
    pub fn close_pages_after(&self, anchor: &Rc<BigTabPage>) {
        let Some(anchor_idx) = self.position_of(anchor) else {
            return;
        };
        let pages: Vec<Rc<BigTabPage>> = self
            .inner
            .pages
            .borrow()
            .iter()
            .skip(anchor_idx + 1)
            .cloned()
            .collect();
        for page in pages {
            self.close_page(&page);
        }
    }

    fn remove_at(&self, idx: usize) {
        let page = self.inner.pages.borrow_mut().remove(idx);
        self.inner.factory.borrow_mut().guard().remove(idx);
        self.inner.view_stack.remove(&page.child());
        let removed_position = i32::try_from(idx).unwrap_or(i32::MAX);
        let prev_selected = self.inner.selected.get();
        let new_selected =
            selection_index_after_remove(idx, prev_selected, self.inner.pages.borrow().len());
        self.inner.selected.set(None);
        if let Some(new_idx) = new_selected
            && let Some(new_page) = self.inner.pages.borrow().get(new_idx).cloned()
        {
            self.set_selected_page(&new_page);
        } else if prev_selected.is_some() {
            self.fire_selected();
        }
        self.fire_detached(&page, removed_position);
        self.fire_n_pages();
    }

    /// Select the next page (right). Returns `false` at the last tab.
    pub fn select_next_page(&self) -> bool {
        let len = self.inner.pages.borrow().len();
        let Some(cur) = self.inner.selected.get() else {
            return false;
        };
        if cur + 1 >= len {
            return false;
        }
        let Some(page) = self.inner.pages.borrow().get(cur + 1).cloned() else {
            return false;
        };
        self.set_selected_page(&page);
        true
    }

    /// Select the previous page (left). Returns `false` at the first tab.
    pub fn select_previous_page(&self) -> bool {
        let cur = self.inner.selected.get().filter(|i| *i > 0);
        let Some(prev) = cur else {
            return false;
        };
        let Some(page) = self.inner.pages.borrow().get(prev - 1).cloned() else {
            return false;
        };
        self.set_selected_page(&page);
        true
    }

    /// Install the standard suite tab context menu: move (click-to-place mode
    /// plus one-step reorder), optional detach, and the three close entries.
    /// Actions target the right-clicked tab (selected before the menu opens);
    /// closes honor the strip's close-request callback. Entry sensitivity
    /// tracks tab count/position, and the reorder labels follow the strip
    /// orientation (Left/Right vs Up/Down). Labels arrive pre-translated by
    /// the app. Apps with richer menus keep composing their own model.
    pub fn install_standard_context_menu(
        &self,
        labels: &BigTabStandardMenuLabels,
        detach: Option<Rc<dyn Fn(Rc<BigTabPage>)>>,
    ) {
        self.install_standard_context_menu_with_extender(labels, detach, None);
    }

    /// Install the standard suite tab context menu and let the app append
    /// additional sections after the shared move/close sections. The intended
    /// consumer is a terminal-style rich tab menu with duplicate/color/group
    /// actions; apps should append sections to `menu` rather than replacing the
    /// standard entries.
    pub fn install_standard_context_menu_with_extender(
        &self,
        labels: &BigTabStandardMenuLabels,
        detach: Option<Rc<dyn Fn(Rc<BigTabPage>)>>,
        extender: Option<BigTabContextMenuExtender>,
    ) {
        let actions = gio::SimpleActionGroup::new();
        let strip = self.downgrade();
        let with_selected = |name: &str, act: fn(&BigTabStrip, &Rc<BigTabPage>)| {
            let action = gio::SimpleAction::new(name, None);
            let strip = strip.clone();
            action.connect_activate(move |_, _| {
                let Some(strip) = strip.upgrade() else { return };
                if let Some(page) = strip.selected_page() {
                    act(&strip, &page);
                }
            });
            action
        };

        let move_tab = with_selected("move-tab", |strip, page| strip.start_move(page));
        let move_back = with_selected("move-back", |strip, page| {
            strip.reorder_backward(page);
        });
        let move_forward = with_selected("move-forward", |strip, page| {
            strip.reorder_forward(page);
        });
        let close_tab = with_selected("close-tab", BigTabStrip::request_close_page);
        let close_others = with_selected("close-other-tabs", |strip, keep| {
            for page in strip.pages_snapshot() {
                if !Rc::ptr_eq(&page, keep) {
                    strip.request_close_page(&page);
                }
            }
        });
        let close_right = with_selected("close-tabs-right", |strip, anchor| {
            let Some(anchor_idx) = strip.position_of(anchor) else {
                return;
            };
            for page in strip.pages_snapshot().into_iter().skip(anchor_idx + 1) {
                strip.request_close_page(&page);
            }
        });
        let detach_action = detach.map(|detach| {
            let action = gio::SimpleAction::new("detach-tab", None);
            let strip = strip.clone();
            action.connect_activate(move |_, _| {
                let Some(strip) = strip.upgrade() else { return };
                if let Some(page) = strip.selected_page() {
                    detach(page);
                }
            });
            action
        });

        for action in [
            &move_tab,
            &move_back,
            &move_forward,
            &close_tab,
            &close_others,
            &close_right,
        ] {
            actions.add_action(action);
        }
        if let Some(detach_action) = &detach_action {
            actions.add_action(detach_action);
        }
        self.inner
            .tab_bar
            .insert_action_group("bigtabstrip", Some(&actions));

        let move_tab_label = labels.move_tab.clone();
        let move_left_label = labels.move_left.clone();
        let move_right_label = labels.move_right.clone();
        let move_up_label = labels.move_up.clone();
        let move_down_label = labels.move_down.clone();
        let detach_tab_label = labels.detach_tab.clone();
        let close_tab_label = labels.close_tab.clone();
        let close_other_tabs_label = labels.close_other_tabs.clone();
        let close_tabs_right_label = labels.close_tabs_right.clone();
        let has_detach_action = detach_action.is_some();
        let build_menu = move |back_label: &str, forward_label: &str| {
            let move_section = gio::Menu::new();
            move_section.append(Some(&move_tab_label), Some("bigtabstrip.move-tab"));
            move_section.append(Some(back_label), Some("bigtabstrip.move-back"));
            move_section.append(Some(forward_label), Some("bigtabstrip.move-forward"));
            if has_detach_action {
                move_section.append(Some(&detach_tab_label), Some("bigtabstrip.detach-tab"));
            }
            let close_section = gio::Menu::new();
            close_section.append(Some(&close_tab_label), Some("bigtabstrip.close-tab"));
            close_section.append(
                Some(&close_other_tabs_label),
                Some("bigtabstrip.close-other-tabs"),
            );
            close_section.append(
                Some(&close_tabs_right_label),
                Some("bigtabstrip.close-tabs-right"),
            );
            let menu = gio::Menu::new();
            menu.append_section(None, &move_section);
            menu.append_section(None, &close_section);
            menu
        };

        // Per-popup refresh: pick the orientation-matching labels and update
        // entry sensitivity for the right-clicked tab.
        self.connect_setup_menu(move |strip, page| {
            let vertical = strip.inner.tab_bar.orientation() == Orientation::Vertical;
            let menu = if vertical {
                build_menu(&move_up_label, &move_down_label)
            } else {
                build_menu(&move_left_label, &move_right_label)
            };
            if let (Some(extender), Some(page)) = (&extender, page) {
                extender(strip, page, &menu);
            }
            strip.set_menu_model(Some(&menu));
            let count = strip.inner.pages.borrow().len();
            let index = page.and_then(|page| strip.position_of(page)).unwrap_or(0);
            let multiple = count > 1;
            let has_following = index + 1 < count;
            move_tab.set_enabled(multiple);
            move_back.set_enabled(index > 0);
            move_forward.set_enabled(has_following);
            close_others.set_enabled(multiple);
            close_right.set_enabled(has_following);
            if let Some(detach_action) = &detach_action {
                detach_action.set_enabled(multiple);
            }
        });
    }

    /// Snapshot of the current pages, in strip order.
    fn pages_snapshot(&self) -> Vec<Rc<BigTabPage>> {
        self.inner.pages.borrow().clone()
    }

    /// Set the context-menu model shown on right-click.
    pub fn set_menu_model<M: IsA<gio::MenuModel>>(&self, model: Option<&M>) {
        let upcast = model.map(|m| m.clone().upcast::<gio::MenuModel>());
        self.inner.menu_model.borrow_mut().clone_from(&upcast);
        self.inner.popover.set_menu_model(upcast.as_ref());
    }

    /// Register a selected-page-changed listener.
    pub fn connect_selected_page_notify<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip) + 'static,
    {
        self.inner.listeners.selected.borrow_mut().push(Rc::new(cb));
    }

    /// Register a page-count-changed listener.
    pub fn connect_n_pages_notify<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip) + 'static,
    {
        self.inner.listeners.n_pages.borrow_mut().push(Rc::new(cb));
    }

    /// Register a page-attached listener.
    pub fn connect_page_attached<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip, &Rc<BigTabPage>, i32) + 'static,
    {
        self.inner.listeners.attached.borrow_mut().push(Rc::new(cb));
    }

    /// Register a page-detached listener.
    pub fn connect_page_detached<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip, &Rc<BigTabPage>, i32) + 'static,
    {
        self.inner.listeners.detached.borrow_mut().push(Rc::new(cb));
    }

    /// Register a context-menu-setup listener (fired before the menu opens).
    pub fn connect_setup_menu<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip, Option<&Rc<BigTabPage>>) + 'static,
    {
        self.inner
            .listeners
            .setup_menu
            .borrow_mut()
            .push(Rc::new(cb));
    }

    /// Register a move-committed listener (a tab was reordered onto a target).
    pub fn connect_move_committed<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip, &Rc<BigTabPage>, &Rc<BigTabPage>) + 'static,
    {
        self.inner
            .listeners
            .move_committed
            .borrow_mut()
            .push(Rc::new(cb));
    }

    /// Register a move-inside-requested listener (a tab was dropped *onto*
    /// another — the parent decides, e.g. split-into).
    pub fn connect_move_inside_requested<F>(&self, cb: F)
    where
        F: Fn(&BigTabStrip, &Rc<BigTabPage>, &Rc<BigTabPage>) + 'static,
    {
        self.inner
            .listeners
            .move_inside_requested
            .borrow_mut()
            .push(Rc::new(cb));
    }

    fn fire_selected(&self) {
        let cbs = self.inner.listeners.selected.borrow().clone();
        for cb in cbs {
            cb(self);
        }
    }

    fn fire_n_pages(&self) {
        let cbs = self.inner.listeners.n_pages.borrow().clone();
        for cb in cbs {
            cb(self);
        }
    }

    fn fire_attached(&self, page: &Rc<BigTabPage>, position: i32) {
        let cbs = self.inner.listeners.attached.borrow().clone();
        for cb in cbs {
            cb(self, page, position);
        }
    }

    fn fire_detached(&self, page: &Rc<BigTabPage>, position: i32) {
        let cbs = self.inner.listeners.detached.borrow().clone();
        for cb in cbs {
            cb(self, page, position);
        }
    }

    fn fire_setup_menu(&self, page: Option<&Rc<BigTabPage>>) {
        let cbs = self.inner.listeners.setup_menu.borrow().clone();
        for cb in cbs {
            cb(self, page);
        }
    }

    fn fire_move_committed(&self, moving: &Rc<BigTabPage>, target: &Rc<BigTabPage>) {
        let cbs = self.inner.listeners.move_committed.borrow().clone();
        for cb in cbs {
            cb(self, moving, target);
        }
    }

    fn fire_move_inside_requested(&self, moving: &Rc<BigTabPage>, target: &Rc<BigTabPage>) {
        let cbs = self.inner.listeners.move_inside_requested.borrow().clone();
        for cb in cbs {
            cb(self, moving, target);
        }
    }

    /// Request closing `page`, honoring any close-request callback (which may
    /// veto or run async confirmation).
    pub fn request_close_page(&self, page: &Rc<BigTabPage>) {
        let close_request_callback = self.inner.close_request_callback.borrow().clone();
        if let Some(close_request_callback) = close_request_callback {
            if close_request_callback(self, page) {
                self.close_page(page);
            }
            return;
        }
        self.close_page(page);
    }

    /// Set a close-request callback: return `true` to allow the immediate close,
    /// `false` to veto (e.g. to run async confirmation and close later).
    pub fn set_close_request_callback<F>(&self, close_request_callback: Option<F>)
    where
        F: Fn(&BigTabStrip, &Rc<BigTabPage>) -> bool + 'static,
    {
        *self.inner.close_request_callback.borrow_mut() =
            close_request_callback.map(|callback| Rc::new(callback) as CloseRequestCallback);
    }

    /// Switch the tab bar between horizontal (header/top/bottom) and vertical
    /// (side placements) orientation. Vertical strips stack tabs one above
    /// the other; tab buttons fill the strip width.
    pub fn set_vertical(&self, vertical: bool) {
        self.inner.vertical.set(vertical);
        self.inner.tab_bar.set_orientation(if vertical {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        let factory = self.inner.factory.borrow();
        factory.broadcast(BigTabButtonInput::SetVertical(vertical));
    }

    /// Mark the strip as docked in a horizontal (top/bottom) dock: tabs get a
    /// slightly taller row than in the header.
    pub fn set_docked_horizontal(&self, docked: bool) {
        self.inner.docked_horizontal.set(docked);
        let factory = self.inner.factory.borrow();
        factory.broadcast(BigTabButtonInput::SetDockedHorizontal(docked));
    }

    /// Apply strip-wide chrome: even tab expansion + close-button mode.
    pub fn apply_tab_chrome(&self, expand_evenly: bool, close_mode: &str) {
        self.inner.expand_evenly.set(expand_evenly);
        self.inner.tab_bar.set_homogeneous(expand_evenly);
        let mode = BigTabCloseMode::from_setting(close_mode);
        self.inner.close_mode.set(mode);
        let factory = self.inner.factory.borrow();
        factory.broadcast(BigTabButtonInput::SetExpandEvenly(expand_evenly));
        factory.broadcast(BigTabButtonInput::SetCloseMode(mode));
    }

    // ---- move / drop / reorder / transfer ----

    /// Whether a click-to-move is currently in flight.
    #[must_use]
    pub fn is_move_active(&self) -> bool {
        self.inner.move_state.borrow().is_some()
    }

    /// Begin a click-to-move for `page` (needs ≥2 tabs).
    pub fn start_move(&self, page: &Rc<BigTabPage>) {
        if self.inner.pages.borrow().len() < 2 {
            return;
        }
        self.cancel_move();
        page.tab_button()
            .add_css_class(&self.inner.strip_chrome.moving);
        self.inner
            .tab_bar
            .add_css_class(&self.inner.strip_chrome.bar_move_mode);
        self.suppress_close_buttons(true);
        *self.inner.move_state.borrow_mut() = Some(MoveState {
            moving: Rc::downgrade(page),
            drop_target: RefCell::new(None),
            drop_placement: Cell::new(DropPlacement::Left),
        });
        self.inner.tab_bar.grab_focus();
    }

    /// Cancel an in-flight move.
    pub fn cancel_move(&self) {
        let Some(state) = self.inner.move_state.borrow_mut().take() else {
            return;
        };
        if let Some(moving) = state.moving.upgrade() {
            moving
                .tab_button()
                .remove_css_class(&self.inner.strip_chrome.moving);
        }
        self.inner
            .tab_bar
            .remove_css_class(&self.inner.strip_chrome.bar_move_mode);
        self.clear_drop_highlights();
        self.suppress_close_buttons(false);
    }

    fn handle_move_motion(&self, page: &Rc<BigTabPage>, x: f64) {
        let state_borrow = self.inner.move_state.borrow();
        let Some(state) = state_borrow.as_ref() else {
            return;
        };
        if state.moving.upgrade().is_some_and(|m| Rc::ptr_eq(&m, page)) {
            return;
        }
        let placement = drop_placement_for_pointer(x, f64::from(page.tab_button().width()));
        let drop_changed = state
            .drop_target
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .is_none_or(|prev| !Rc::ptr_eq(&prev, page))
            || state.drop_placement.get() != placement;
        if !drop_changed {
            return;
        }
        drop(state_borrow);
        self.update_drop_highlight(page, placement);
    }

    fn handle_move_leave(&self, page: &Rc<BigTabPage>) {
        let state_borrow = self.inner.move_state.borrow();
        let Some(state) = state_borrow.as_ref() else {
            return;
        };
        let leaving_target = state
            .drop_target
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|t| Rc::ptr_eq(&t, page));
        drop(state_borrow);
        if leaving_target {
            self.clear_drop_highlights();
        }
    }

    fn handle_move_click(&self, page: &Rc<BigTabPage>, x: f64) {
        let moving = {
            let state_borrow = self.inner.move_state.borrow();
            let Some(state) = state_borrow.as_ref() else {
                return;
            };
            state.moving.upgrade()
        };
        let Some(moving_page) = moving else {
            self.cancel_move();
            return;
        };
        if Rc::ptr_eq(&moving_page, page) {
            self.cancel_move();
            return;
        }
        let placement = drop_placement_for_pointer(x, f64::from(page.tab_button().width()));
        if placement == DropPlacement::Inside {
            self.fire_move_inside_requested(&moving_page, page);
            self.cancel_move();
            return;
        }
        self.commit_move(&moving_page, page, placement);
        self.cancel_move();
    }

    fn commit_move(
        &self,
        moving: &Rc<BigTabPage>,
        target: &Rc<BigTabPage>,
        placement: DropPlacement,
    ) {
        let Some(moving_idx) = self.position_of(moving) else {
            return;
        };
        let Some(target_idx) = self.position_of(target) else {
            return;
        };
        let mut new_idx = match placement {
            DropPlacement::Left => target_idx,
            DropPlacement::Right => target_idx + 1,
            DropPlacement::Inside => return,
        };
        if moving_idx < new_idx {
            new_idx -= 1;
        }
        if moving_idx == new_idx {
            return;
        }
        let Ok(new_pos) = i32::try_from(new_idx) else {
            return;
        };
        self.reorder_page(moving, new_pos);
        self.fire_move_committed(moving, target);
    }

    fn update_drop_highlight(&self, page: &Rc<BigTabPage>, placement: DropPlacement) {
        let chrome = &self.inner.strip_chrome;
        self.clear_drop_highlights();
        match placement {
            DropPlacement::Left => page.tab_button().add_css_class(&chrome.drop_left),
            DropPlacement::Inside => page.tab_button().add_css_class(&chrome.drop_inside),
            DropPlacement::Right => page.tab_button().add_css_class(&chrome.drop_right),
        }
        if let Some(state) = self.inner.move_state.borrow().as_ref() {
            *state.drop_target.borrow_mut() = Some(Rc::downgrade(page));
            state.drop_placement.set(placement);
        }
    }

    fn clear_drop_highlights(&self) {
        let chrome = &self.inner.strip_chrome;
        for page in self.inner.pages.borrow().iter() {
            let button = page.tab_button();
            button.remove_css_class(&chrome.drop_target);
            button.remove_css_class(&chrome.drop_left);
            button.remove_css_class(&chrome.drop_inside);
            button.remove_css_class(&chrome.drop_right);
        }
        if let Some(state) = self.inner.move_state.borrow().as_ref() {
            *state.drop_target.borrow_mut() = None;
        }
    }

    /// Reorder `page` one slot toward the start. Returns `false` if at index 0.
    pub fn reorder_backward(&self, page: &Rc<BigTabPage>) -> bool {
        let Some(idx) = self.position_of(page) else {
            return false;
        };
        if idx == 0 {
            return false;
        }
        self.move_page_to(idx, idx - 1)
    }

    /// Reorder `page` one slot toward the end. Returns `false` if at the last tab.
    pub fn reorder_forward(&self, page: &Rc<BigTabPage>) -> bool {
        let Some(idx) = self.position_of(page) else {
            return false;
        };
        let len = self.inner.pages.borrow().len();
        if idx + 1 >= len {
            return false;
        }
        self.move_page_to(idx, idx + 1)
    }

    /// Move `page` to `position` (clamped to the last slot).
    pub fn reorder_page(&self, page: &Rc<BigTabPage>, position: i32) -> bool {
        let Some(idx) = self.position_of(page) else {
            return false;
        };
        let Ok(target_usize) = usize::try_from(position) else {
            return false;
        };
        let len = self.inner.pages.borrow().len();
        let target = target_usize.min(len.saturating_sub(1));
        self.move_page_to(idx, target)
    }

    fn move_page_to(&self, from: usize, to: usize) -> bool {
        if from == to {
            return false;
        }
        let was_selected = self.inner.selected.get() == Some(from);
        self.inner.factory.borrow_mut().guard().move_to(from, to);
        {
            let mut pages = self.inner.pages.borrow_mut();
            let page = pages.remove(from);
            pages.insert(to, page);
        }
        if was_selected {
            self.inner.selected.set(Some(to));
            self.fire_selected();
        } else if let Some(sel) = self.inner.selected.get() {
            self.inner
                .selected
                .set(Some(selection_index_after_move(sel, from, to)));
        }
        true
    }

    /// Move `page` to `target` strip at `position` (or reorder if same strip).
    pub fn transfer_page(
        &self,
        page: &Rc<BigTabPage>,
        target: &BigTabStrip,
        position: i32,
    ) -> bool {
        if self == target {
            return self.reorder_page(page, position);
        }
        let Some(idx) = self.position_of(page) else {
            return false;
        };
        let removed_position = i32::try_from(idx).unwrap_or(i32::MAX);
        let page_handle = self.inner.pages.borrow_mut().remove(idx);
        self.inner.factory.borrow_mut().guard().remove(idx);
        self.inner.view_stack.remove(&page_handle.child());
        let prev_selected = self.inner.selected.get();
        let next =
            selection_index_after_remove(idx, prev_selected, self.inner.pages.borrow().len());
        self.inner.selected.set(None);
        if let Some(new_idx) = next
            && let Some(new_page) = self.inner.pages.borrow().get(new_idx).cloned()
        {
            self.set_selected_page(&new_page);
        }
        self.fire_detached(&page_handle, removed_position);
        self.fire_n_pages();
        target.adopt_page(&page_handle, position);
        true
    }

    fn adopt_page(&self, page: &Rc<BigTabPage>, position: i32) {
        page.rebind(Rc::downgrade(&self.inner));
        let len = self.inner.pages.borrow().len();
        let target = usize::try_from(position).unwrap_or(0).min(len);
        self.inner.view_stack.add(&page.child());
        let init = page.button_init(
            false,
            self.inner.close_mode.get(),
            self.inner.expand_evenly.get(),
            self.inner.vertical.get(),
            self.inner.docked_horizontal.get(),
            self.inner.chrome.clone(),
        );
        {
            let mut factory = self.inner.factory.borrow_mut();
            let mut guard = factory.guard();
            if target == len {
                guard.push_back(init);
            } else {
                guard.insert(target, init);
            }
        }
        if let Some(button) = self.button_widget_at(target) {
            page.set_button(button);
        }
        self.inner.pages.borrow_mut().insert(target, page.clone());
        let attached_position = i32::try_from(target).unwrap_or(i32::MAX);
        self.fire_attached(page, attached_position);
        self.fire_n_pages();
        self.set_selected_page(page);
    }

    /// Remove `page` from this strip WITHOUT destroying its child (the caller
    /// re-homes the widget, e.g. embedding it in a split).
    pub fn remove_page_for_embed(&self, page: &Rc<BigTabPage>) -> bool {
        let Some(idx) = self.position_of(page) else {
            return false;
        };
        self.inner.pages.borrow_mut().remove(idx);
        self.inner.factory.borrow_mut().guard().remove(idx);
        self.inner.view_stack.remove(&page.child());
        if let Some(selected) = self.inner.selected.get() {
            if selected == idx {
                self.inner.selected.set(None);
            } else if selected > idx {
                self.inner.selected.set(Some(selected - 1));
            }
        }
        self.fire_n_pages();
        true
    }

    /// Remove `page`'s current rendered child from the content stack — the first
    /// half of a live re-render. The caller then unparents the surviving content
    /// widgets from any leftover `gtk::Paned` subtree, rebuilds the tree, and
    /// calls [`attach_page_child`](Self::attach_page_child). (Two-step so the
    /// content widgets are parentless before `render_pane_node` re-homes them —
    /// re-homing a still-parented widget trips a `gtk_paned_set_*_child`
    /// assertion.)
    pub fn detach_page_child(&self, page: &Rc<BigTabPage>) {
        let old = page.child();
        if old.parent().is_some() {
            self.inner.view_stack.remove(&old);
        }
    }

    /// Mount `new_child` as `page`'s content after a live re-render, showing it
    /// if `page` is the selected tab. Pairs with
    /// [`detach_page_child`](Self::detach_page_child).
    pub fn attach_page_child(&self, page: &Rc<BigTabPage>, new_child: &impl IsA<Widget>) {
        let new_widget: Widget = new_child.clone().upcast();
        self.inner.view_stack.add(&new_widget);
        let is_selected = self.position_of(page) == self.inner.selected.get();
        page.set_child(new_widget.clone());
        if is_selected {
            self.inner.view_stack.set_visible_child(&new_widget);
        }
    }

    fn show_context_menu_for(&self, page: &Rc<BigTabPage>, x: f64, y: f64) {
        self.set_selected_page(page);
        self.fire_setup_menu(Some(page));
        // No model even after the setup listeners ran → nothing to show.
        // Popping an empty popover reads as a broken menu.
        if self.inner.menu_model.borrow().is_none() {
            return;
        }
        // Re-parenting the tab bar (tab_strip_position change) unrealizes it,
        // which releases the popover via the teardown hook. Re-attach before
        // popup — a parentless popup aborts the process.
        if self.inner.popover.parent().is_none() {
            self.inner.popover.set_parent(&self.inner.tab_bar);
        }
        let bounds = page.tab_button().compute_bounds(&self.inner.tab_bar);
        let (origin_x, origin_y) =
            bounds.map_or((0.0_f64, 0.0_f64), |r| (f64::from(r.x()), f64::from(r.y())));
        let rect =
            gdk::Rectangle::new(clamp_to_i32(origin_x + x), clamp_to_i32(origin_y + y), 1, 1);
        self.inner.popover.set_pointing_to(Some(&rect));
        set_tab_context_menu_window_state(
            &self.inner.tab_bar,
            true,
            &self.inner.strip_chrome.context_menu_open_window,
        );
        self.inner.popover.popup();
    }
}

impl Default for BigTabStrip {
    fn default() -> Self {
        Self::new(
            Rc::new(BigTabButtonChrome::default()),
            BigTabStripChrome::default(),
            identity_title_composer(),
        )
    }
}
