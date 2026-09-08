// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! CLI contracts for the shared component story runner.

pub(crate) const DEFAULT_VIEWPORT_WIDTH: i32 = 1280;
pub(crate) const DEFAULT_VIEWPORT_HEIGHT: i32 = 720;
pub(crate) const DEFAULT_HOLD_MS: u64 = 4_000;

#[derive(Debug, Clone)]
pub(crate) enum StoryCommand {
    Help,
    List,
    Run(StoryArgs),
}

impl StoryCommand {
    pub(crate) fn from_env(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let program = args.next().unwrap_or_else(|| "component_story".to_owned());
        let mut story_args = StoryArgs {
            gtk_args: vec![program],
            story: StoryKind::HamburgerMenu,
            viewport: Viewport {
                width: DEFAULT_VIEWPORT_WIDTH,
                height: DEFAULT_VIEWPORT_HEIGHT,
            },
            hold_ms: DEFAULT_HOLD_MS,
            opens_transient: false,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(Self::Help),
                "--list" => return Ok(Self::List),
                "--story" => {
                    let value = next_value(&mut args, "--story")?;
                    story_args.story = StoryKind::parse(&value)?;
                }
                "--viewport" => {
                    let value = next_value(&mut args, "--viewport")?;
                    story_args.viewport = Viewport::parse(&value)?;
                }
                "--hold-ms" => {
                    let value = next_value(&mut args, "--hold-ms")?;
                    story_args.hold_ms = value.parse().map_err(|_| {
                        format!("invalid --hold-ms value `{value}`; expected milliseconds")
                    })?;
                }
                "--closed" => story_args.opens_transient = false,
                "--open-transient" => story_args.opens_transient = true,
                other => return Err(format!("unknown argument `{other}`")),
            }
        }

        Ok(Self::Run(story_args))
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}"))
}

pub(crate) fn print_usage() {
    println!(
        "\
Usage: component_story [--list] [--story NAME] [--viewport WIDTHxHEIGHT] [--hold-ms MS] [--closed]

Stories:
  adwaita-controls
  ai-provider-catalog
  ai-provider-settings
  hamburger-menu
  media-window
  searchable-action-list
  searchable-selection-list
  toolbar-pane
  workspace-dock-pane
  workspace-sidebar-split
  workspace-document-shell
  workspace-window-shell

Examples:
  cargo run -p big-relm4-components --example component_story -- --story hamburger-menu
  cargo run -p big-relm4-components --example component_story -- --viewport 1280x720 --hold-ms 8000
  cargo run -p big-relm4-components --example component_story -- --story hamburger-menu --open-transient
"
    );
}

#[derive(Debug, Clone)]
pub(crate) struct StoryArgs {
    pub(crate) gtk_args: Vec<String>,
    pub(crate) story: StoryKind,
    pub(crate) viewport: Viewport,
    pub(crate) hold_ms: u64,
    pub(crate) opens_transient: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StoryKind {
    AdwaitaControls,
    AiProviderCatalog,
    AiProviderSettings,
    HamburgerMenu,
    MediaWindow,
    SearchableActionList,
    SearchableSelectionList,
    ToolbarPane,
    WorkspaceDockPane,
    WorkspaceSidebarSplit,
    WorkspaceDocumentShell,
    WorkspaceWindowShell,
}

impl StoryKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "adwaita-controls" => Ok(Self::AdwaitaControls),
            "ai-provider-catalog" => Ok(Self::AiProviderCatalog),
            "ai-provider-settings" => Ok(Self::AiProviderSettings),
            "hamburger-menu" => Ok(Self::HamburgerMenu),
            "media-window" => Ok(Self::MediaWindow),
            "searchable-action-list" => Ok(Self::SearchableActionList),
            "searchable-selection-list" => Ok(Self::SearchableSelectionList),
            "toolbar-pane" => Ok(Self::ToolbarPane),
            "workspace-dock-pane" => Ok(Self::WorkspaceDockPane),
            "workspace-sidebar-split" => Ok(Self::WorkspaceSidebarSplit),
            "workspace-document-shell" => Ok(Self::WorkspaceDocumentShell),
            "workspace-window-shell" => Ok(Self::WorkspaceWindowShell),
            _ => Err(format!("unknown story `{value}`")),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AdwaitaControls => "adwaita-controls",
            Self::AiProviderCatalog => "ai-provider-catalog",
            Self::AiProviderSettings => "ai-provider-settings",
            Self::HamburgerMenu => "hamburger-menu",
            Self::MediaWindow => "media-window",
            Self::SearchableActionList => "searchable-action-list",
            Self::SearchableSelectionList => "searchable-selection-list",
            Self::ToolbarPane => "toolbar-pane",
            Self::WorkspaceDockPane => "workspace-dock-pane",
            Self::WorkspaceSidebarSplit => "workspace-sidebar-split",
            Self::WorkspaceDocumentShell => "workspace-document-shell",
            Self::WorkspaceWindowShell => "workspace-window-shell",
        }
    }

    pub(crate) fn print_list() {
        println!("adwaita-controls");
        println!("ai-provider-catalog");
        println!("ai-provider-settings");
        println!("hamburger-menu");
        println!("media-window");
        println!("searchable-action-list");
        println!("searchable-selection-list");
        println!("toolbar-pane");
        println!("workspace-dock-pane");
        println!("workspace-sidebar-split");
        println!("workspace-document-shell");
        println!("workspace-window-shell");
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Viewport {
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl Viewport {
    fn parse(value: &str) -> Result<Self, String> {
        let Some((width, height)) = value.split_once('x') else {
            return Err(format!(
                "invalid --viewport value `{value}`; expected WIDTHxHEIGHT"
            ));
        };
        let width = positive_dimension(width, "width")?;
        let height = positive_dimension(height, "height")?;
        Ok(Self { width, height })
    }
}

fn positive_dimension(value: &str, role: &str) -> Result<i32, String> {
    let dimension: i32 = value
        .parse()
        .map_err(|_| format!("invalid viewport {role} `{value}`"))?;
    if dimension <= 0 {
        return Err(format!("viewport {role} must be positive, got {dimension}"));
    }
    Ok(dimension)
}
