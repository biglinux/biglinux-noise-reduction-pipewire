// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Isolated UI story runner for shared BigLinux Relm4 components.
//!
//! This example starts a tiny libadwaita window, mounts one shared component
//! story, opens the relevant transient UI, prints a readiness marker, and then
//! optionally exits. Capture it with the Linux UI a11y headless harness instead
//! of starting a full application when the component itself is under review.
//!
//! Run:
//! `cargo run -p big-relm4-components --example component_story -- --story hamburger-menu`

use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::actions::install_action;
use big_app_kit::ai_assistant::{
    BigAiProviderCatalogLabels, BigAiProviderCatalogLauncher, BigAiProviderSettingsContent,
    BigAiProviderSettingsLabels, BigAiProviderSettingsSnapshot,
};
use big_app_kit::window_shell::{
    BigApplicationWindowShellSpec, BigWindowActionSpec, BigWindowActionSurface,
    BigWindowOverlayPolicy, BigWindowStatePersistenceSpec, BigWindowToastPolicy,
    BigWorkspaceCapability, BigWorkspaceWindowAccessibilitySpec, BigWorkspaceWindowShellSpec,
};
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::input::toggle_group::{
    BigToggleGroup, BigToggleGroupSpec, BigToggleOptionSpec,
};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::hamburger_menu::{
    BigHamburgerMenuSpec, BigMenuActionItem, build_hamburger_menu_button,
};
use big_relm4_components::layout::tab_strip_host::BigTabStrip;
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};
use big_relm4_components::layout::window_shell::BigWorkspaceWindowShell;
use big_relm4_components::layout::workspace_dock::{BigWorkspaceDockPane, BigWorkspaceDockSide};
use big_relm4_components::layout::workspace_sidebar_split::{
    BigWorkspaceSidebarSplit, BigWorkspaceSidebarSplitSpec,
};
use big_relm4_components::list::searchable_action_list::{
    BigSearchableActionList, BigSearchableActionListEmptySpec, BigSearchableActionListSpec,
    BigSearchableActionRowSpec,
};
use big_relm4_components::list::searchable_selection_list::{
    BigSearchableSelectionList, BigSearchableSelectionListEmptySpec,
    BigSearchableSelectionListSpec, BigSearchableSelectionRowSpec,
};
use big_relm4_components::pane::{BigSplitTree, BigWorkspaceShell, SplitPaneSurface};
use relm4::gtk;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

#[path = "component_story/cli.rs"]
mod component_story_cli;
#[path = "component_story/media.rs"]
mod component_story_media;
#[path = "component_story/runtime.rs"]
mod component_story_runtime;

use component_story_cli::{
    DEFAULT_VIEWPORT_HEIGHT, DEFAULT_VIEWPORT_WIDTH, StoryArgs, StoryCommand, StoryKind,
    print_usage,
};
use component_story_media::build_media_window_story;
use component_story_runtime::{schedule_story_quit, schedule_story_ready};

fn main() {
    let command = match StoryCommand::from_env(std::env::args()) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    match command {
        StoryCommand::Help => print_usage(),
        StoryCommand::List => StoryKind::print_list(),
        StoryCommand::Run(story_args) => {
            let gtk_args = story_args.gtk_args.clone();
            let app = RelmApp::new("br.com.biglinux.big_relm4_components.component_story")
                .with_args(gtk_args);
            app.run::<UiStoryApp>(story_args);
        }
    }
}

struct UiStoryApp {
    _story_surface: StorySurface,
}

impl SimpleComponent for UiStoryApp {
    type Input = ();
    type Output = ();
    type Init = StoryArgs;
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(DEFAULT_VIEWPORT_WIDTH)
            .default_height(DEFAULT_VIEWPORT_HEIGHT)
            .title("BigLinux UI Story")
            .build()
    }

    fn init(
        story_args: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.set_default_size(story_args.viewport.width, story_args.viewport.height);
        root.set_title(Some(&format!(
            "BigLinux UI Story: {}",
            story_args.story.as_str()
        )));

        let story_surface = match story_args.story {
            StoryKind::AdwaitaControls => build_adwaita_controls_story(&root),
            StoryKind::AiProviderCatalog => build_ai_provider_catalog_story(&root),
            StoryKind::AiProviderSettings => build_ai_provider_settings_story(&root),
            StoryKind::HamburgerMenu => build_hamburger_menu_story(&root),
            StoryKind::MediaWindow => build_media_window_story(&root),
            StoryKind::SearchableActionList => build_searchable_action_list_story(&root),
            StoryKind::SearchableSelectionList => build_searchable_selection_list_story(&root),
            StoryKind::ToolbarPane => build_toolbar_pane_story(&root),
            StoryKind::WorkspaceDockPane => build_workspace_dock_pane_story(&root),
            StoryKind::WorkspaceSidebarSplit => build_workspace_sidebar_split_story(&root),
            StoryKind::WorkspaceDocumentShell => build_workspace_document_shell_story(&root),
            StoryKind::WorkspaceWindowShell => build_workspace_window_shell_story(&root),
        };

        schedule_story_ready(
            &story_surface.ready_widget,
            story_surface.transient_menu_button.as_ref(),
            &story_args,
        );
        schedule_story_quit(story_args.hold_ms);

        ComponentParts {
            model: Self {
                _story_surface: story_surface,
            },
            widgets: (),
        }
    }
}

struct StorySurface {
    ready_widget: gtk::Widget,
    transient_menu_button: Option<gtk::MenuButton>,
    _keepalive_objects: Vec<Box<dyn std::any::Any>>,
}

fn build_ai_provider_catalog_story(root: &adw::ApplicationWindow) -> StorySurface {
    let parent_window: gtk::Window = root.clone().upcast();
    let launcher = BigAiProviderCatalogLauncher::new(BigAiProviderCatalogLabels {
        window_title: "AI Providers".to_owned(),
        subtitle: "Shared provider catalog from BigLinux AI components".to_owned(),
        button_label: "AI providers".to_owned(),
        local_runtime: "Local runtime".to_owned(),
        remote_provider: "Remote provider".to_owned(),
        provider: "provider".to_owned(),
        default_model: "default model".to_owned(),
    });
    let open_catalog_button = launcher.build_button(&parent_window);

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(12)
        .build();
    page.append(
        &gtk::Label::builder()
            .label("AI provider catalog story")
            .css_classes(["title-2"])
            .build(),
    );
    page.append(
        &gtk::Label::builder()
            .label("Shared BigLinux AI provider registry")
            .css_classes(["dim-label"])
            .build(),
    );
    page.append(&open_catalog_button);

    let dialog_page = BigDialogPage::plain();
    dialog_page.content().append(&page);
    root.set_content(Some(dialog_page.root()));

    StorySurface {
        ready_widget: open_catalog_button.upcast(),
        transient_menu_button: None,
        _keepalive_objects: Vec::new(),
    }
}

fn build_ai_provider_settings_story(root: &adw::ApplicationWindow) -> StorySurface {
    let settings_content = BigAiProviderSettingsContent::new(
        BigAiProviderSettingsLabels {
            title: "AI Assistant".to_owned(),
            description: "Shared editable provider settings from BigLinux AI components".to_owned(),
            enable_title: "Enable AI Assistant".to_owned(),
            enable_subtitle: "Show AI actions in applications that support them.".to_owned(),
            model_group_title: "Model".to_owned(),
            provider_title: "AI provider".to_owned(),
            provider_accessible_name: "AI provider".to_owned(),
            model_title: "AI model".to_owned(),
            model_accessible_name: "AI model".to_owned(),
            browse_models: "Browse".to_owned(),
            api_key_group_title: "API Key".to_owned(),
            api_key_title: "API Key".to_owned(),
            api_key_accessible_name: "API Key".to_owned(),
            get_api_key: "Get API Key".to_owned(),
            advanced_group_title: "Advanced".to_owned(),
            base_url_title: "Base URL (Local)".to_owned(),
            openrouter_site_url_title: "OpenRouter Site URL".to_owned(),
            openrouter_site_name_title: "OpenRouter Site Name".to_owned(),
        },
        BigAiProviderSettingsSnapshot {
            enabled: true,
            api_key: "story-key".to_owned(),
            ..BigAiProviderSettingsSnapshot::default()
        },
        Rc::new(|_| {}),
        None,
    );
    let ready_widget: gtk::Widget = settings_content.root().clone().upcast();
    let owner = settings_content.owner();

    let dialog_page = BigDialogPage::plain();
    dialog_page.content().append(settings_content.root());
    root.set_content(Some(dialog_page.root()));

    StorySurface {
        ready_widget,
        transient_menu_button: None,
        _keepalive_objects: vec![Box::new(owner)],
    }
}

fn build_adwaita_controls_story(root: &adw::ApplicationWindow) -> StorySurface {
    let export_button = build_button(
        &BigButtonContentSpec::new("document-save-symbolic", "Export")
            .accessible_label("Export marked cuts")
            .tooltip("Export the marked cuts")
            .css_classes(["suggested-action", "pill"])
            .can_shrink(),
    );

    let mode_group = BigToggleGroup::new(
        BigToggleGroupSpec::new([
            BigToggleOptionSpec::new("fast", "Keep format").description("Fast stream copy"),
            BigToggleOptionSpec::new("precise", "Re-encode").description("Apply export settings"),
        ])
        .active_name("fast")
        .accessible_label("Export mode")
        .tooltip("Choose how marked cuts are exported"),
    );

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(12)
        .build();
    page.append(
        &gtk::Label::builder()
            .label("Adwaita controls story")
            .css_classes(["title-2"])
            .build(),
    );
    page.append(
        &gtk::Label::builder()
            .label("Button content and toggle group")
            .css_classes(["dim-label"])
            .build(),
    );
    page.append(mode_group.root());
    page.append(&export_button);

    let dialog_page = BigDialogPage::plain();
    dialog_page.content().append(&page);
    root.set_content(Some(dialog_page.root()));

    StorySurface {
        ready_widget: export_button.upcast(),
        transient_menu_button: None,
        _keepalive_objects: Vec::new(),
    }
}

fn build_hamburger_menu_story(root: &adw::ApplicationWindow) -> StorySurface {
    let spec = BigHamburgerMenuSpec::new("Main Menu")
        .css_classes(["flat"])
        .items([
            BigMenuActionItem::new("Open File...", "win.open-file"),
            BigMenuActionItem::new("Recent Files", "win.open-recent"),
            BigMenuActionItem::new("Preferences", "win.preferences"),
            BigMenuActionItem::new("Keyboard Shortcuts", "win.keyboard-shortcuts"),
            BigMenuActionItem::new("About", "win.about"),
        ]);
    install_window_action_stubs(root, &spec.items);

    let menu_button = build_hamburger_menu_button(&spec);
    menu_button.set_widget_name("story-hamburger-menu-button");
    let menu_preview = build_menu_contract_preview(&spec);

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(12)
        .build();
    page.append(
        &gtk::Label::builder()
            .label("Hamburger menu story")
            .css_classes(["title-2"])
            .build(),
    );
    page.append(
        &gtk::Label::builder()
            .label("Flat menu preview")
            .css_classes(["dim-label"])
            .build(),
    );
    page.append(&menu_button);
    page.append(&menu_preview);

    let dialog_page = BigDialogPage::plain();
    dialog_page.content().append(&page);
    root.set_content(Some(dialog_page.root()));

    StorySurface {
        ready_widget: menu_button.clone().upcast(),
        transient_menu_button: Some(menu_button),
        _keepalive_objects: Vec::new(),
    }
}

fn build_toolbar_pane_story(root: &adw::ApplicationWindow) -> StorySurface {
    let pane = BigToolbarPane::new(BigToolbarPaneSpec::new().css_class("story-toolbar-pane"));
    let header = BigToolbarHeader::new(
        BigToolbarHeaderSpec::titled("Conversion Progress")
            .show_start_title_buttons(false)
            .css_class("flat")
            .title_css_class("heading"),
    );

    let cancel_button = build_button(
        &BigButtonContentSpec::new("process-stop-symbolic", "Cancel")
            .accessible_label("Cancel conversion")
            .tooltip("Cancel conversion")
            .css_classes(["flat"]),
    );
    header.pack_end(&cancel_button);
    pane.add_top_bar(header.root());

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(12)
        .build();
    content.append(
        &gtk::Label::builder()
            .label("Toolbar pane story")
            .css_classes(["title-2"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label("Shared AdwToolbarView and AdwHeaderBar chrome")
            .css_classes(["dim-label"])
            .build(),
    );
    let progress = gtk::ProgressBar::builder()
        .fraction(0.42)
        .width_request(360)
        .build();
    progress.update_property(&[gtk::accessible::Property::Label("Conversion progress")]);
    content.append(&progress);

    pane.set_content(&content);
    root.set_content(Some(pane.root()));

    StorySurface {
        ready_widget: cancel_button.upcast(),
        transient_menu_button: None,
        _keepalive_objects: Vec::new(),
    }
}

fn build_searchable_action_list_story(root: &adw::ApplicationWindow) -> StorySurface {
    let list = BigSearchableActionList::new(
        BigSearchableActionListSpec::new(
            "Filter sessions...",
            "Search saved sessions",
            [
                BigSearchableActionRowSpec::new(
                    "dev-shell",
                    "Dev Shell",
                    "Open",
                    "dev shell local workspace",
                )
                .subtitle("Local workspace"),
                BigSearchableActionRowSpec::new(
                    "prod-ssh",
                    "Production SSH",
                    "Connect",
                    "production ssh ops@example.com",
                )
                .subtitle("ops@example.com"),
                BigSearchableActionRowSpec::new(
                    "staging-db",
                    "Staging Database",
                    "Connect",
                    "staging database dba@staging",
                )
                .subtitle("dba@staging"),
            ],
        )
        .empty(BigSearchableActionListEmptySpec::new(
            "No matches",
            "Adjust the search terms.",
            "edit-find-symbolic",
        ))
        .margins(0, 0, 0, 0),
        Rc::new(|_| {}),
    );
    let ready_widget: gtk::Widget = list.search().clone().upcast();

    let page = BigDialogPage::plain();
    page.content()
        .append(&story_section_label("Searchable action list story"));
    page.content().append(list.root());
    root.set_content(Some(page.root()));

    StorySurface {
        ready_widget,
        transient_menu_button: None,
        _keepalive_objects: vec![Box::new(list)],
    }
}

fn build_searchable_selection_list_story(root: &adw::ApplicationWindow) -> StorySurface {
    let list = BigSearchableSelectionList::new(
        BigSearchableSelectionListSpec::new(
            "Search font name",
            "Search font name",
            [
                BigSearchableSelectionRowSpec::new(
                    "adwaita-mono",
                    "Adwaita Mono",
                    "adwaita mono regular",
                )
                .subtitle("Regular"),
                BigSearchableSelectionRowSpec::new("fira-code", "Fira Code", "fira code regular")
                    .subtitle("Regular"),
                BigSearchableSelectionRowSpec::new(
                    "jetbrains-mono",
                    "JetBrains Mono",
                    "jetbrains mono medium",
                )
                .subtitle("Medium"),
            ],
        )
        .selected_id("fira-code")
        .list_accessible_label("Fonts")
        .scroller_size(560, 320, 420)
        .empty(BigSearchableSelectionListEmptySpec::new(
            "No matching fonts",
            "Try another font name.",
            "edit-find-symbolic",
        )),
    );
    let ready_widget: gtk::Widget = list.search().clone().upcast();

    let page = BigDialogPage::plain();
    page.content()
        .append(&story_section_label("Searchable selection list story"));
    page.content().append(list.root());
    root.set_content(Some(page.root()));

    StorySurface {
        ready_widget,
        transient_menu_button: None,
        _keepalive_objects: vec![Box::new(list)],
    }
}

fn build_workspace_dock_pane_story(root: &adw::ApplicationWindow) -> StorySurface {
    install_workspace_dock_pane_story_css();

    let workspace_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .hexpand(true)
        .vexpand(true)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .css_classes(["workspace-dock-story-content"])
        .build();
    workspace_content.update_property(&[gtk::accessible::Property::Label(
        "Workspace dock pane story",
    )]);
    workspace_content.append(&story_section_label("Workspace dock pane story"));
    workspace_content.append(&story_text_label("Primary workspace content"));

    let dock = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .width_request(320)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(16)
        .css_classes(["workspace-dock-story-dock"])
        .build();
    dock.update_property(&[gtk::accessible::Property::Label("Workspace action dock")]);
    dock.append(&story_section_label("Workspace action dock"));
    let pin_button = story_button("view-pin-symbolic", "Pin dock");
    dock.append(&pin_button);
    dock.append(&story_button("document-save-symbolic", "Save layout"));
    dock.append(&story_button("view-refresh-symbolic", "Refresh preview"));

    let dock_pane =
        BigWorkspaceDockPane::new(&workspace_content, &dock, BigWorkspaceDockSide::Right);
    root.set_content(Some(dock_pane.root_widget()));

    StorySurface {
        ready_widget: pin_button.upcast(),
        transient_menu_button: None,
        _keepalive_objects: vec![Box::new(dock_pane)],
    }
}

fn build_workspace_sidebar_split_story(root: &adw::ApplicationWindow) -> StorySurface {
    install_workspace_dock_pane_story_css();

    let sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .width_request(280)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(12)
        .css_classes(["workspace-dock-story-dock"])
        .build();
    sidebar.update_property(&[gtk::accessible::Property::Label("Workspace sidebar")]);
    sidebar.append(&story_section_label("Workspace sidebar"));
    let open_project_button = story_button("folder-open-symbolic", "Open project");
    sidebar.append(&open_project_button);
    sidebar.append(&story_button("list-add-symbolic", "New workspace"));
    sidebar.append(&story_button(
        "preferences-system-symbolic",
        "Workspace settings",
    ));

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .hexpand(true)
        .vexpand(true)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .css_classes(["workspace-dock-story-content"])
        .build();
    content.update_property(&[gtk::accessible::Property::Label(
        "Primary workspace content",
    )]);
    content.append(&story_section_label("Workspace sidebar split story"));
    content.append(&story_text_label("Primary workspace content"));
    content.append(&story_button("document-new-symbolic", "Create document"));

    let split = BigWorkspaceSidebarSplit::new(
        &sidebar,
        &content,
        BigWorkspaceSidebarSplitSpec::new()
            .shows_sidebar_initially(true)
            .pins_sidebar_initially(true)
            .is_collapsed_initially(false)
            .accessible_name("Workspace sidebar split story")
            .css_class("workspace-sidebar-split-story"),
    );
    root.set_content(Some(split.split_view()));

    StorySurface {
        ready_widget: open_project_button.upcast(),
        transient_menu_button: None,
        _keepalive_objects: vec![Box::new(split)],
    }
}

fn build_workspace_window_shell_story(root: &adw::ApplicationWindow) -> StorySurface {
    install_workspace_window_shell_story_css();

    let workspace_shell = BigWorkspaceShell::<StorySplitPane, String>::new(BigTabStrip::default());
    let terminal_pane = StorySplitPane::new("Terminal pane", "PTY session");
    let editor_pane = StorySplitPane::new("Editor pane", "Unsaved document");
    let split_tree = BigSplitTree::new(&terminal_pane);
    split_tree.split(gtk::Orientation::Horizontal, &editor_pane);
    let workspace_page = workspace_shell.append_tab(split_tree, "workspace-tab-1".to_owned());
    workspace_page.set_title("Primary workspace");

    let application_shell =
        BigApplicationWindowShellSpec::new("br.com.biglinux.WorkspaceStory", "Workspace Shell")
            .default_size(1280, 720)
            .overlay_policy(BigWindowOverlayPolicy::SharedOverlayRoot)
            .toast_policy(BigWindowToastPolicy::Enabled)
            .persistence_spec(BigWindowStatePersistenceSpec::new("workspace-story-window"))
            .header_action(BigWindowActionSpec::new(
                "win.new-tab",
                "New tab",
                "New workspace tab",
                BigWindowActionSurface::Header,
            ))
            .footer_status_action(BigWindowActionSpec::new(
                "win.toggle-status",
                "Status",
                "Toggle workspace status",
                BigWindowActionSurface::FooterStatus,
            ));
    let shell_spec = BigWorkspaceWindowShellSpec::new(application_shell)
        .capability(BigWorkspaceCapability::Sidebar)
        .capability(BigWorkspaceCapability::Dock)
        .capability(BigWorkspaceCapability::WorkspaceOverlay)
        .workspace_accessibility_spec(
            BigWorkspaceWindowAccessibilitySpec::new(
                "Workspace shell story",
                "Workspace tabs",
                "Workspace split panes",
            )
            .sidebar_accessible_name("Workspace sidebar")
            .dock_accessible_name("Workspace dock"),
        );
    let window_shell = BigWorkspaceWindowShell::new(shell_spec, workspace_shell)
        .expect("workspace shell story spec is valid");

    window_shell
        .append_sidebar_child(&story_section_label("Workspace sidebar"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Open Sessions"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Remote Hosts"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Files"))
        .expect("story enables sidebar capability");

    window_shell
        .append_dock_child(&story_button("document-new-symbolic", "New pane"))
        .expect("story enables dock capability");
    window_shell
        .append_dock_child(&story_button(
            "view-split-left-right-symbolic",
            "Split pane",
        ))
        .expect("story enables dock capability");
    window_shell
        .append_dock_child(&story_button(
            "preferences-system-symbolic",
            "Workspace settings",
        ))
        .expect("story enables dock capability");

    let overlay_badge = gtk::Label::builder()
        .label("Overlay hooks")
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_end(16)
        .css_classes(["workspace-story-overlay-badge"])
        .build();
    overlay_badge.update_property(&[gtk::accessible::Property::Label("Workspace overlay")]);
    window_shell
        .add_workspace_overlay_child(&overlay_badge, false)
        .expect("story enables workspace overlay capability");

    root.set_content(Some(window_shell.root()));

    StorySurface {
        ready_widget: window_shell.workspace_tab_bar_host().clone().upcast(),
        transient_menu_button: None,
        _keepalive_objects: vec![
            Box::new(window_shell),
            Box::new(terminal_pane),
            Box::new(editor_pane),
        ],
    }
}

fn build_workspace_document_shell_story(root: &adw::ApplicationWindow) -> StorySurface {
    install_workspace_window_shell_story_css();

    let workspace_shell = BigWorkspaceShell::<StorySplitPane, String>::new(BigTabStrip::default());
    let canvas_pane = StorySplitPane::new("Canvas pane", "Document preview surface");
    let properties_pane = StorySplitPane::new("Properties pane", "Selected layer settings");
    let split_tree = BigSplitTree::new(&canvas_pane);
    split_tree.split(gtk::Orientation::Horizontal, &properties_pane);
    let workspace_page = workspace_shell.append_tab(split_tree, "document-workspace-1".to_owned());
    workspace_page.set_title("Design review");

    let application_shell = BigApplicationWindowShellSpec::new(
        "br.com.biglinux.DocumentWorkspaceStory",
        "Document Workspace Shell",
    )
    .default_size(1280, 720)
    .overlay_policy(BigWindowOverlayPolicy::SharedOverlayRoot)
    .toast_policy(BigWindowToastPolicy::Enabled)
    .persistence_spec(BigWindowStatePersistenceSpec::new(
        "document-workspace-story-window",
    ))
    .header_action(BigWindowActionSpec::new(
        "win.new-document",
        "New document",
        "Create document workspace",
        BigWindowActionSurface::Header,
    ))
    .footer_status_action(BigWindowActionSpec::new(
        "win.toggle-review-status",
        "Review status",
        "Toggle review status",
        BigWindowActionSurface::FooterStatus,
    ));
    let shell_spec = BigWorkspaceWindowShellSpec::new(application_shell)
        .capability(BigWorkspaceCapability::Sidebar)
        .capability(BigWorkspaceCapability::Dock)
        .capability(BigWorkspaceCapability::WorkspaceOverlay)
        .workspace_accessibility_spec(
            BigWorkspaceWindowAccessibilitySpec::new(
                "Document workspace shell story",
                "Document workspace tabs",
                "Document split panes",
            )
            .sidebar_accessible_name("Document project sidebar")
            .dock_accessible_name("Document action dock"),
        );
    let window_shell = BigWorkspaceWindowShell::new(shell_spec, workspace_shell)
        .expect("document workspace shell story spec is valid");

    window_shell
        .append_sidebar_child(&story_section_label("Document project sidebar"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Pages"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Layers"))
        .expect("story enables sidebar capability");
    window_shell
        .append_sidebar_child(&story_text_label("Assets"))
        .expect("story enables sidebar capability");

    window_shell
        .append_dock_child(&story_button("list-add-symbolic", "Add layer"))
        .expect("story enables dock capability");
    window_shell
        .append_dock_child(&story_button("object-select-symbolic", "Select object"))
        .expect("story enables dock capability");
    window_shell
        .append_dock_child(&story_button(
            "preferences-system-symbolic",
            "Document settings",
        ))
        .expect("story enables dock capability");

    let overlay_badge = gtk::Label::builder()
        .label("Review overlay")
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(16)
        .margin_end(16)
        .css_classes(["workspace-story-overlay-badge"])
        .build();
    overlay_badge.update_property(&[gtk::accessible::Property::Label("Document overlay")]);
    window_shell
        .add_workspace_overlay_child(&overlay_badge, false)
        .expect("story enables workspace overlay capability");

    root.set_content(Some(window_shell.root()));

    StorySurface {
        ready_widget: window_shell.workspace_tab_bar_host().clone().upcast(),
        transient_menu_button: None,
        _keepalive_objects: vec![
            Box::new(window_shell),
            Box::new(canvas_pane),
            Box::new(properties_pane),
        ],
    }
}

struct StorySplitPane {
    root: gtk::Box,
}

impl StorySplitPane {
    fn new(title: &str, subtitle: &str) -> Rc<Self> {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["workspace-story-pane"])
            .build();
        root.set_focusable(true);
        root.update_property(&[gtk::accessible::Property::Label(title)]);
        root.append(&story_section_label(title));
        root.append(&story_text_label(subtitle));
        Rc::new(Self { root })
    }
}

impl SplitPaneSurface for StorySplitPane {
    fn pane_root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn set_header_visible(&self, visible: bool) {
        if visible {
            self.root
                .add_css_class("workspace-story-pane-header-visible");
        } else {
            self.root
                .remove_css_class("workspace-story-pane-header-visible");
        }
    }

    fn grab_focus(&self) {
        self.root.grab_focus();
    }
}

fn story_section_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .css_classes(["heading"])
        .halign(gtk::Align::Start)
        .build()
}

fn story_text_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .wrap(true)
        .build()
}

fn story_button(icon_name: &str, label: &str) -> gtk::Button {
    build_button(
        &BigButtonContentSpec::new(icon_name, label)
            .accessible_label(label)
            .tooltip(label)
            .css_classes(["flat"]),
    )
}

fn install_workspace_dock_pane_story_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "
        .workspace-dock-story-content {
            padding: 20px;
            background: @view_bg_color;
        }
        .workspace-dock-story-dock {
            padding: 16px;
            border-radius: 8px;
            background: @card_bg_color;
        }
        ",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn install_workspace_window_shell_story_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "
        .workspace-story-pane {
            padding: 18px;
            background: @card_bg_color;
        }
        .workspace-story-pane-header-visible {
            border: 1px solid alpha(@accent_bg_color, 0.40);
        }
        .workspace-story-overlay-badge {
            padding: 6px 10px;
            border-radius: 8px;
            background: alpha(@accent_bg_color, 0.92);
            color: @accent_fg_color;
        }
        ",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_menu_contract_preview(spec: &BigHamburgerMenuSpec) -> gtk::Box {
    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .css_classes(["card"])
        .build();
    menu.update_property(&[gtk::accessible::Property::Label("Menu items")]);

    for item in &spec.items {
        let button = gtk::Button::builder()
            .label(&item.label)
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .css_classes(["flat"])
            .build();
        button.set_detailed_action_name(&item.action);
        button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
        menu.append(&button);
    }

    menu
}

fn install_window_action_stubs(root: &adw::ApplicationWindow, items: &[BigMenuActionItem]) {
    let actions = gtk::gio::SimpleActionGroup::new();
    let mut installed_actions: Vec<String> = Vec::new();
    for item in items {
        let Some(action_name) = item.action.strip_prefix("win.") else {
            continue;
        };
        let action_name = action_name.split(['(', ':']).next().unwrap_or(action_name);
        if installed_actions
            .iter()
            .any(|installed| installed == action_name)
        {
            continue;
        }
        install_action(&actions, action_name, || {});
        installed_actions.push(action_name.to_owned());
    }
    root.insert_action_group("win", Some(&actions));
}
