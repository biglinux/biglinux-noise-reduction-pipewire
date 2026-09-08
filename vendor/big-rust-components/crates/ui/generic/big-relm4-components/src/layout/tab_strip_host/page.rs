// SPDX-License-Identifier: MIT

//! Durable per-tab handle for [`BigTabStrip`](super::BigTabStrip).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use relm4::gtk;
use relm4::gtk::Box as GtkBox;

use crate::layout::tab_button::{BigTabButtonChrome, BigTabButtonInit, BigTabButtonInput};
use crate::layout::tab_strip::BigTabCloseMode;

use super::{BigTabStrip, BigTabStripInner};

type TitleListener = Rc<dyn Fn(&Rc<BigTabPage>)>;

/// The durable per-tab handle held by a [`BigTabStrip`]: an opaque content
/// widget + its factory button + the display state the host reads.
pub struct BigTabPage {
    child: RefCell<gtk::Widget>,
    button: RefCell<GtkBox>,
    title: RefCell<String>,
    base_title: RefCell<String>,
    count: Cell<usize>,
    progress_value: Cell<Option<u8>>,
    icon: RefCell<Option<String>>,
    title_listeners: RefCell<Vec<TitleListener>>,
    view: RefCell<Weak<BigTabStripInner>>,
}

impl PartialEq for BigTabPage {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl BigTabPage {
    pub(super) fn new(
        child: gtk::Widget,
        button: GtkBox,
        view: Weak<BigTabStripInner>,
    ) -> Rc<Self> {
        Rc::new(Self {
            child: RefCell::new(child),
            button: RefCell::new(button),
            title: RefCell::new(String::new()),
            base_title: RefCell::new(String::new()),
            count: Cell::new(1),
            progress_value: Cell::new(None),
            icon: RefCell::new(None),
            title_listeners: RefCell::new(Vec::new()),
            view: RefCell::new(view),
        })
    }

    pub(super) fn rebind(self: &Rc<Self>, view: Weak<BigTabStripInner>) {
        *self.view.borrow_mut() = view;
    }

    fn upgrade_view(&self) -> Option<BigTabStrip> {
        Some(BigTabStrip {
            inner: self.view.borrow().upgrade()?,
        })
    }

    fn compose(&self, base: &str, count: usize) -> String {
        match self.view.borrow().upgrade() {
            Some(inner) => (inner.title_composer)(base, count),
            None => base.to_owned(),
        }
    }

    fn send(self: &Rc<Self>, input: BigTabButtonInput) {
        if let Some(view) = self.upgrade_view() {
            view.send_to_button(self, input);
        }
    }

    /// The current rendered (composed) title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    /// The tab's content widget.
    #[must_use]
    pub fn child(&self) -> gtk::Widget {
        self.child.borrow().clone()
    }

    /// The tab's strip-button widget (for external CSS coloring / geometry).
    #[must_use]
    pub fn tab_button(&self) -> GtkBox {
        self.button.borrow().clone()
    }

    pub(super) fn set_button(&self, button: GtkBox) {
        *self.button.borrow_mut() = button;
    }

    pub(super) fn set_child(&self, child: gtk::Widget) {
        *self.child.borrow_mut() = child;
    }

    pub(super) fn button_init(
        &self,
        selected: bool,
        close_mode: BigTabCloseMode,
        expand_evenly: bool,
        vertical: bool,
        docked_horizontal: bool,
        chrome: Rc<BigTabButtonChrome>,
    ) -> BigTabButtonInit {
        BigTabButtonInit {
            title: self.compose(&self.base_title.borrow(), self.count.get()),
            icon: self.icon.borrow().clone(),
            progress: self.progress_value.get(),
            selected,
            close_mode,
            expand_evenly,
            vertical,
            docked_horizontal,
            chrome,
        }
    }

    /// Trigger the attention flash on this tab's button.
    pub fn start_bell_flash(self: &Rc<Self>) {
        self.send(BigTabButtonInput::Flash);
    }

    /// Set or clear the tab's leading icon.
    pub fn set_icon(self: &Rc<Self>, icon_name: Option<&str>) {
        let icon = icon_name.map(str::to_owned);
        self.icon.borrow_mut().clone_from(&icon);
        self.send(BigTabButtonInput::SetIcon(icon));
    }

    /// Set the tab's base title (recomposed with the current count).
    pub fn set_title(self: &Rc<Self>, title: &str) {
        title.clone_into(&mut self.base_title.borrow_mut());
        self.recompose_title();
    }

    /// Set the tab's secondary count (e.g. pane count); recomposes the title.
    pub fn set_pane_count(self: &Rc<Self>, count: usize) {
        if self.count.get() == count {
            return;
        }
        self.count.set(count);
        self.recompose_title();
    }

    fn recompose_title(self: &Rc<Self>) {
        let composed = self.compose(&self.base_title.borrow(), self.count.get());
        if *self.title.borrow() == composed {
            return;
        }
        (*self.title.borrow_mut()).clone_from(&composed);
        self.send(BigTabButtonInput::SetTitle(composed));
        let cbs = self.title_listeners.borrow().clone();
        for cb in cbs {
            cb(self);
        }
    }

    /// Set or clear the inline progress percentage (`0..=100`).
    pub fn set_progress(self: &Rc<Self>, percent: Option<u8>) {
        let normalized = percent.filter(|p| *p <= 100);
        if self.progress_value.get() == normalized {
            return;
        }
        self.progress_value.set(normalized);
        self.send(BigTabButtonInput::SetProgress(normalized));
    }

    /// Register a title-changed listener.
    pub fn connect_title_notify<F>(self: &Rc<Self>, cb: F)
    where
        F: Fn(&Rc<BigTabPage>) + 'static,
    {
        self.title_listeners.borrow_mut().push(Rc::new(cb));
    }
}
