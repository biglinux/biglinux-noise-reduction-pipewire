// SPDX-License-Identifier: MIT

//! `ControllerPane` — bridge a widget-rooted Relm4 component into a [`PaneContent`].
//!
//! The media apps (audio/video/camera) are Relm4 components, not the plain-widget
//! structs the big-terminal panes are. Once a media shell's `type Root` is a mountable
//! widget (an `adw::ToolbarView`/`gtk::Box`, NOT an `adw::ApplicationWindow` — see
//! RESTRUCTURE ADR **D14**), this bridge mounts it as a pane:
//!
//! 1. launch the controller (`Component::builder().launch(init)`),
//! 2. wrap its root: `ControllerPane::new(controller.widget(), title, kbd, controller)`
//!    — the controller is moved in as the keepalive, so it runs for the pane's life
//!    and shuts down (tearing its widgets down) when the pane drops,
//! 3. forward the component's title output into [`title_sink`](ControllerPane::title_sink),
//! 4. optionally map `PaneActivity` to a component input via
//!    [`with_activity`](ControllerPane::with_activity) (e.g. a video pane pausing its
//!    GL redraw on `Occluded` while the playback job keeps running).
//!
//! It is deliberately NOT generic over the component type: it holds the already-
//! extracted root widget plus an opaque keepalive, so one concrete type serves
//! every component without monomorphizing per shell.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::{KeyboardNeed, PaneActivity, PaneContent};

type TitleSubscriber = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

/// Adapts a launched widget-rooted Relm4 controller (or any widget + keepalive)
/// into a [`PaneContent`]. See the module docs for the wiring contract.
pub struct ControllerPane {
    root: gtk::Widget,
    title: Rc<RefCell<String>>,
    subscriber: TitleSubscriber,
    keyboard: KeyboardNeed,
    on_activity: Option<Box<dyn Fn(PaneActivity)>>,
    /// Holds the `Controller<C>` (any type) so the component runs for the pane's
    /// lifetime and shuts down — tearing its widget subtree down — when dropped.
    _keepalive: Box<dyn Any>,
}

impl ControllerPane {
    /// Wrap `root` (the controller's root widget) as a pane. `keepalive` is held
    /// for the pane's lifetime — pass the `Controller<C>` so the component keeps
    /// running; dropping the pane drops it, shutting the component down.
    pub fn new(
        root: &impl IsA<gtk::Widget>,
        title: impl Into<String>,
        keyboard: KeyboardNeed,
        keepalive: impl Any,
    ) -> Self {
        Self {
            root: root.clone().upcast(),
            title: Rc::new(RefCell::new(title.into())),
            subscriber: Rc::new(RefCell::new(None)),
            keyboard,
            on_activity: None,
            _keepalive: Box::new(keepalive),
        }
    }

    /// Map [`PaneContent::set_activity`] to a component action (e.g. send a
    /// pause-redraw input on `Occluded`). Jobs MUST keep running — only pause
    /// animation/redraw (see [`PaneActivity`]).
    #[must_use]
    pub fn with_activity(mut self, on_activity: impl Fn(PaneActivity) + 'static) -> Self {
        self.on_activity = Some(Box::new(on_activity));
        self
    }

    /// Box this adapter as object-safe [`PaneContent`] for tab/split/module
    /// containers. This is the common hand-off point for Relm4 app shells.
    #[must_use]
    pub fn into_boxed(self) -> Box<dyn PaneContent> {
        Box::new(self)
    }

    /// A cloneable title sink — forward the component's title output into it.
    /// Holds only WEAK refs (no widget⇄closure cycle): it updates the cached
    /// title and notifies the subscriber, and no-ops once the pane is dropped.
    pub fn title_sink(&self) -> impl Fn(&str) + Clone + use<> {
        let title = Rc::downgrade(&self.title);
        let subscriber = Rc::downgrade(&self.subscriber);
        move |new_title: &str| {
            if let Some(title) = title.upgrade() {
                *title.borrow_mut() = new_title.to_owned();
            }
            if let Some(subscriber) = subscriber.upgrade()
                && let Some(callback) = subscriber.borrow().as_ref()
            {
                callback(new_title);
            }
        }
    }
}

impl PaneContent for ControllerPane {
    fn root(&self) -> gtk::Widget {
        self.root.clone()
    }

    fn title(&self) -> String {
        self.title.borrow().clone()
    }

    fn connect_title_changed(&self, on_change: Box<dyn Fn(&str)>) {
        *self.subscriber.borrow_mut() = Some(on_change);
    }

    fn keyboard_need(&self) -> KeyboardNeed {
        self.keyboard
    }

    fn set_activity(&self, activity: PaneActivity) {
        if let Some(hook) = self.on_activity.as_ref() {
            hook(activity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControllerPane, PaneContent};

    #[test]
    fn controller_pane_has_object_safe_handoff_api() {
        fn assert_pane_content<T: PaneContent>() {}
        fn assert_handoff(_: fn(ControllerPane) -> Box<dyn PaneContent>) {}

        assert_pane_content::<ControllerPane>();
        assert_handoff(ControllerPane::into_boxed);
    }
}
