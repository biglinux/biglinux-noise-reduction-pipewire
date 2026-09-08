// SPDX-License-Identifier: MIT

//! Headless smoke for the assembled `Tabbed` window ([`BigPaneTabs`]).
//!
//! Builds a single-pane tab plus a three-way split tab (terminal | editor /
//! files) from stub [`PaneContent`]s, asserts the tab count, realizes the
//! window, then quits 0. Proves the keystone composes end-to-end — `PaneContent`
//! leaves mount in `adw::TabView` and arrange into nested `gtk::Paned` — without
//! a real display panicking.
//!
//! Run headless: provide a Wayland/X11 display, then
//! `cargo run -p big-relm4-components --example pane_tabs_demo`.

use adw::prelude::*;
use big_relm4_components::layout::split_panes::BigSplitPlacement;
use big_relm4_components::pane::{
    BigPaneTabs, BigSplitOrientation, ControllerPane, KeyboardNeed, PaneContent, PaneNode,
};
use relm4::gtk;
use relm4::gtk::glib;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

/// Minimal content: a centered label as its root, a fixed title.
struct StubPane {
    root: gtk::Label,
    title: String,
}

impl StubPane {
    fn new(title: &str) -> Self {
        let root = gtk::Label::builder()
            .label(title)
            .vexpand(true)
            .hexpand(true)
            .build();
        Self {
            root,
            title: title.to_owned(),
        }
    }
}

impl PaneContent for StubPane {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn connect_title_changed(&self, _on_change: Box<dyn Fn(&str)>) {}
    fn keyboard_need(&self) -> KeyboardNeed {
        KeyboardNeed::OnDemand
    }
}

fn main() {
    let app = RelmApp::new("com.biglinux.PaneTabsDemo");
    app.run::<PaneTabsDemo>(());
}

struct PaneTabsDemo {
    _tabs: BigPaneTabs,
}

impl SimpleComponent for PaneTabsDemo {
    type Input = ();
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(960)
            .default_height(640)
            .title("BigPaneTabs demo")
            .build()
    }

    fn init(
        (): Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let tabs = build_tabs();
        root.set_content(Some(tabs.root()));

        // Auto-quit once realized: long enough to map + apply split ratios (and
        // for an external screenshot), short enough to stay a smoke.
        glib::timeout_add_seconds_local_once(3, || {
            println!("pane_tabs_demo: ok (4 tabs incl ControllerPane, split realized)");
            if let Some(app) = gtk::gio::Application::default() {
                app.quit();
            }
        });

        ComponentParts {
            model: Self { _tabs: tabs },
            widgets: (),
        }
    }
}

fn build_tabs() -> BigPaneTabs {
    let tabs = BigPaneTabs::new();

    // Tab 1: a single pane.
    tabs.add_pane(Box::new(StubPane::new("Terminal")));

    // Tab 2: terminal | (editor / files) — a three-way split.
    let node = PaneNode::split(
        BigSplitOrientation::Horizontal,
        0.5,
        PaneNode::leaf("term"),
        PaneNode::split(
            BigSplitOrientation::Vertical,
            0.6,
            PaneNode::leaf("editor"),
            PaneNode::leaf("files"),
        ),
    );
    let split_page = tabs.add_split(
        &node,
        vec![
            (
                "term".to_owned(),
                Box::new(StubPane::new("Terminal 2")) as Box<dyn PaneContent>,
            ),
            ("editor".to_owned(), Box::new(StubPane::new("Editor"))),
            ("files".to_owned(), Box::new(StubPane::new("Files"))),
        ],
    );

    // Tab 3: a ControllerPane wrapping a widget-rooted "component" (here a
    // stub widget + a `()` keepalive). Its title sink — what an app wires to
    // the component's title output — updates the live tab label.
    let content = gtk::Label::builder()
        .label("controller content")
        .vexpand(true)
        .hexpand(true)
        .build();
    let controller_pane = ControllerPane::new(&content, "Player", KeyboardNeed::OnDemand, ());
    let title_sink = controller_pane.title_sink();
    tabs.add_pane(Box::new(controller_pane));
    title_sink("Player — Now Playing");

    // Tab 4: start as a single pane, then LIVE-split it (exercises the
    // runtime split_pane path: PaneNode::split_leaf + re-render in place).
    let live_page = tabs.add_split(
        &PaneNode::leaf("solo"),
        vec![(
            "solo".to_owned(),
            Box::new(StubPane::new("Live")) as Box<dyn PaneContent>,
        )],
    );
    tabs.split_pane(
        &live_page,
        &"solo".to_owned(),
        Box::new(StubPane::new("Live-Split (bottom)")),
        BigSplitPlacement::Bottom,
        0.5,
    );

    assert_eq!(tabs.tab_count(), 4, "expected exactly four tabs");
    let _ = &split_page;
    // Show the live-split tab so the screenshot surfaces the runtime split.
    tabs.select_page(&live_page);
    tabs
}
