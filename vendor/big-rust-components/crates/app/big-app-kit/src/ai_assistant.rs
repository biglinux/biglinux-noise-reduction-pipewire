// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared AI assistant contracts.
//!
//! Keep provider config, secret lookup, and chat UI reusable. Apps own storage,
//! keyring, HTTP policy, and domain context.

use adw::prelude::*;

use crate::gtk_accessibility_policy as accessibility;
use crate::widgets::{BigClosableSettingsWindow, BigClosableSettingsWindowSpec};

#[path = "ai_provider_catalog.rs"]
mod ai_provider_catalog;
#[path = "ai_provider_settings.rs"]
mod ai_provider_settings;

pub use ai_provider_catalog::{
    DEFAULT_AI_PROVIDER_ID, DEFAULT_LOCAL_AI_BASE_URL, builtin_ai_provider_default_model,
    builtin_ai_provider_index, builtin_ai_provider_model_browser_base_url,
    builtin_ai_provider_spec, builtin_ai_provider_specs,
};
pub use ai_provider_settings::{
    BigAiProviderModelBrowseRequest, BigAiProviderModelBrowser, BigAiProviderModelSelected,
    BigAiProviderSettingsContent, BigAiProviderSettingsLabels, BigAiProviderSettingsLauncher,
};

/// Static description of an AI provider the assistant panel can target.
///
/// Apps build one [`BigAiProviderSpec`] per supported backend (Groq, OpenAI,
/// a local Ollama, etc.) and feed the list into provider pickers and
/// [`missing_ai_configuration`]. The spec is display-free: no GTK widgets,
/// no network state — just identity, presentation hints, and a flag that
/// drives secret-store policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderSpec {
    /// Stable identifier persisted in settings and matched against
    /// [`BigAiAssistantConfig::provider_id`].
    pub id: String,
    /// Localized name presented in provider pickers.
    pub label: String,
    /// Model name selected on first use when the user has not yet picked one.
    pub default_model: String,
    /// Web page where the user can obtain an API key. `None` for providers
    /// that do not require one (typically local runtimes).
    pub api_key_url: Option<String>,
    /// `true` for runtimes hosted on the user's machine (Ollama, llama.cpp).
    /// Used by [`provider_requires_secret`] to skip the keyring lookup and by
    /// [`missing_ai_configuration`] to require a `base_url` instead.
    pub local: bool,
}

impl BigAiProviderSpec {
    /// Build a remote-hosted provider spec (`local = false`) with an
    /// API-key signup URL the panel can surface when the user has no key.
    #[must_use]
    pub fn remote(
        id: impl Into<String>,
        label: impl Into<String>,
        default_model: impl Into<String>,
        api_key_url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            default_model: default_model.into(),
            api_key_url: Some(api_key_url.into()),
            local: false,
        }
    }

    /// Build a local-runtime provider spec (`local = true`, `api_key_url =
    /// None`). The caller is still expected to configure a `base_url` in
    /// [`BigAiAssistantConfig`] before the assistant becomes usable.
    #[must_use]
    pub fn local(
        id: impl Into<String>,
        label: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            default_model: default_model.into(),
            api_key_url: None,
            local: true,
        }
    }
}

/// User-visible AI assistant settings as resolved at runtime.
///
/// The host app owns persistence (typically `gio::Settings` or a TOML file)
/// and rebuilds this struct whenever the user changes provider, model, or
/// base URL. The panel re-runs [`missing_ai_configuration`] each time it
/// receives a new value to decide whether to show the configuration banner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiAssistantConfig {
    /// `false` disables the panel entry point entirely without losing the
    /// rest of the configuration.
    pub enabled: bool,
    /// Matches [`BigAiProviderSpec::id`] of the selected provider.
    pub provider_id: String,
    /// Model identifier (`"llama3.2"`, `"gpt-4o-mini"`, ...) sent on each
    /// request.
    pub model: String,
    /// Required for local providers (`http://localhost:11434/v1`, ...) and
    /// optional override for remote providers behind a proxy. Treated as
    /// missing when the trimmed value is empty.
    pub base_url: Option<String>,
    /// `true` once the host has confirmed via [`BigAiSecretStore`] that a
    /// secret is present for `provider_id`. Stored separately so the actual
    /// secret never crosses this struct.
    pub has_api_key: bool,
}

/// Editable AI provider settings shown by the shared provider settings UI.
///
/// Apps own where this snapshot comes from and how it is persisted. The shared
/// GTK component only edits the values and emits a full replacement snapshot on
/// every change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderSettingsSnapshot {
    /// Whether the assistant entry point is enabled.
    pub enabled: bool,
    /// Selected provider id.
    pub provider_id: String,
    /// API key value currently shown by the UI. Apps may persist it in a keyring.
    pub api_key: String,
    /// Selected model id.
    pub model: String,
    /// Base URL used by local/OpenAI-compatible runtimes.
    pub base_url: String,
    /// Optional OpenRouter site URL metadata.
    pub openrouter_site_url: String,
    /// Optional OpenRouter site name metadata.
    pub openrouter_site_name: String,
}

impl Default for BigAiProviderSettingsSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            provider_id: DEFAULT_AI_PROVIDER_ID.to_owned(),
            api_key: String::new(),
            model: builtin_ai_provider_default_model(DEFAULT_AI_PROVIDER_ID).to_owned(),
            base_url: DEFAULT_LOCAL_AI_BASE_URL.to_owned(),
            openrouter_site_url: String::new(),
            openrouter_site_name: String::new(),
        }
    }
}

/// Message the panel emits when it needs the host to talk to a keyring or
/// secret service on its behalf.
///
/// The panel never touches the secret store directly; it forwards one of
/// these requests so the host can apply its own policy (rate limiting,
/// audit log, OS-specific backend selection).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BigAiSecretRequest {
    /// Ask the host to read the API key for `provider_id` and report the
    /// result back as a fresh [`BigAiAssistantConfig`] with `has_api_key`
    /// updated.
    LookupProviderApiKey {
        /// Identifier matching [`BigAiProviderSpec::id`].
        provider_id: String,
    },
    /// Ask the host to persist a new API key. When `redacted` is `true`,
    /// the value the panel showed was a placeholder mask, so the host
    /// should ignore the write rather than overwrite the real secret.
    StoreProviderApiKey {
        /// Identifier matching [`BigAiProviderSpec::id`].
        provider_id: String,
        /// `true` when the displayed value was a mask, not the real key.
        redacted: bool,
    },
    /// Ask the host to delete the API key entirely for `provider_id`.
    ClearProviderApiKey {
        /// Identifier matching [`BigAiProviderSpec::id`].
        provider_id: String,
    },
}

/// Input messages a Relm4-style AI panel component consumes.
///
/// Hosts forward UI events (button clicks, settings changes) and the
/// component routes them to the matching state transition. Variant names
/// mirror the actions the user can perform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BigAiPanelInput {
    /// User asked to reveal the panel (sidebar toggle, keyboard shortcut).
    OpenRequested,
    /// User asked to hide the panel.
    CloseRequested,
    /// Prompt text-view contents changed; carries the full updated text.
    PromptChanged(String),
    /// User pressed Send or hit Enter in the prompt view.
    SendRequested,
    /// Host detected new settings (provider, model, key state) and is
    /// pushing them in so the panel can re-validate.
    ProviderConfigChanged(BigAiAssistantConfig),
}

/// Output messages the panel emits for the host to react to.
///
/// All side effects (network I/O, secret store access, navigation) live in
/// the host. The panel only signals intent through this enum.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BigAiPanelOutput {
    /// Configuration is incomplete; host should surface a banner pointing
    /// the user at AI settings instead of letting the request go out.
    NeedConfiguration,
    /// Configuration is valid; host should issue the chat request.
    SendPrompt {
        /// Exact prompt text the user typed, untrimmed.
        prompt: String,
    },
    /// User clicked the gear icon; host should navigate to AI settings.
    OpenSettings,
}

/// Backend that persists per-provider API keys.
///
/// Implementations typically wrap [`crate::keyring`] or an OS-specific secret
/// service. The trait is sealed by `Error` so callers can propagate backend
/// failures without leaking secret material into log surfaces.
pub trait BigAiSecretStore {
    /// Backend-specific error type.
    type Error;

    /// Fetch the stored API key for `provider_id`, if any.
    fn lookup_provider_api_key(&self, provider_id: &str) -> Result<Option<String>, Self::Error>;

    /// Store `api_key` for `provider_id`, overwriting any existing value.
    fn store_provider_api_key(&self, provider_id: &str, api_key: &str) -> Result<(), Self::Error>;

    /// Remove any stored API key for `provider_id`.
    fn clear_provider_api_key(&self, provider_id: &str) -> Result<(), Self::Error>;
}

/// Translated strings the AI assistant panel needs at construction time.
///
/// Apps typically build this once from their gettext catalog and pass it into
/// [`BigAiPanelShell`]. Use [`BigAiPanelLabels::default`] for English fallbacks.
///
/// # Examples
///
/// ```
/// use big_app_kit::ai_assistant::BigAiPanelLabels;
///
/// let labels = BigAiPanelLabels {
///     title: "Assistant".into(),
///     ..Default::default()
/// };
/// assert_eq!(labels.title, "Assistant");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiPanelLabels {
    /// Human-readable title shown in UI.
    pub title: String,
    /// Tooltip on the "new chat" header button.
    pub new_chat_tooltip: String,
    /// Tooltip on the "history" header button.
    pub history_tooltip: String,
    /// Tooltip on the gear button that opens AI settings.
    pub settings_tooltip: String,
    /// Tooltip on the close button that hides the panel.
    pub close_tooltip: String,
    /// AT-SPI accessible name attached to the prompt text view so screen
    /// readers announce it correctly.
    pub prompt_accessible_name: String,
    /// AT-SPI accessible description with usage hints (e.g. "Enter to
    /// send, Shift+Enter for newline").
    pub prompt_accessible_description: String,
    /// Label on the primary "send" button.
    pub send_label: String,
    /// Label on the destructive "stop" button shown during a response.
    pub stop_label: String,
    /// Tooltip on the stop button.
    pub stop_tooltip: String,
}

impl Default for BigAiPanelLabels {
    fn default() -> Self {
        Self {
            title: "AI Assistant".into(),
            new_chat_tooltip: "Start a new conversation".into(),
            history_tooltip: "View conversation history".into(),
            settings_tooltip: "Open AI Assistant settings".into(),
            close_tooltip: "Close AI panel".into(),
            prompt_accessible_name: "Prompt".into(),
            prompt_accessible_description:
                "Ask the AI assistant a question. Enter to send, Shift+Enter for newline".into(),
            send_label: "Send".into(),
            stop_label: "Stop".into(),
            stop_tooltip: "Cancel the current response".into(),
        }
    }
}

/// Pre-built GTK widget tree of the AI assistant side panel.
///
/// [`BigAiPanelShell::new`] assembles header, transcript scroller, prompt
/// box, status row, and action buttons in the order Big apps expect, plus
/// the relevant CSS classes (`ai-chat-panel`, `card`, `suggested-action`).
/// Each field stays public so a Relm4 component or app glue can attach
/// signals, swap widget contents, or insert extra children without
/// rebuilding the layout.
#[derive(Debug, Clone)]
pub struct BigAiPanelShell {
    /// Vertical container; this is what callers add to a window or
    /// sidebar.
    pub root: gtk::Box,
    /// Horizontal title bar holding the action buttons.
    pub header: gtk::Box,
    /// Vertical container into which message rows are appended.
    pub transcript_box: gtk::Box,
    /// Padded wrapper that hosts `transcript_box` inside `scroller`.
    pub scroll_content: gtk::Box,
    /// Vertically scrolling viewport showing the transcript.
    pub scroller: gtk::ScrolledWindow,
    /// Multi-line entry where the user types prompts.
    pub prompt_view: gtk::TextView,
    /// Scroller that grows the prompt view between 40 and 160 pixels.
    pub prompt_scroller: gtk::ScrolledWindow,
    /// Horizontal row at the bottom that holds spinner and status label.
    pub status_row: gtk::Box,
    /// Primary "send" button, styled `suggested-action`.
    pub send: gtk::Button,
    /// "Stop" button shown only while a response is streaming.
    pub stop: gtk::Button,
    /// Header action that starts a fresh conversation.
    pub new_chat: gtk::Button,
    /// Header action that opens the conversation history.
    pub history: gtk::Button,
    /// Header action that opens AI settings.
    pub settings: gtk::Button,
    /// Header action that hides the panel.
    pub close: gtk::Button,
    /// Status text shown below the transcript (errors, hints).
    pub status: gtk::Label,
    /// Spinner shown next to `status` while a request is in flight.
    pub status_spinner: gtk::Spinner,
}

impl BigAiPanelShell {
    /// Build the panel widget tree using `labels` for every user-visible
    /// string. The returned shell is unconnected — callers wire signals
    /// from `send`, `stop`, header buttons and the prompt view themselves.
    #[must_use]
    pub fn new(labels: &BigAiPanelLabels) -> Self {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("ai-chat-header");
        header.set_margin_start(10);
        header.set_margin_end(6);
        header.set_margin_top(6);
        header.set_margin_bottom(6);

        let title = gtk::Label::builder()
            .label(&labels.title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["heading", "title-4"])
            .build();
        let [new_chat_spec, history_spec, settings_spec, close_spec] = header_button_specs(labels);
        let new_chat = accessibility::icon_button(
            new_chat_spec.icon_name,
            new_chat_spec.tooltip,
            &HEADER_BUTTON_CSS_CLASSES,
        );
        let history = accessibility::icon_button(
            history_spec.icon_name,
            history_spec.tooltip,
            &HEADER_BUTTON_CSS_CLASSES,
        );
        let settings = accessibility::icon_button(
            settings_spec.icon_name,
            settings_spec.tooltip,
            &HEADER_BUTTON_CSS_CLASSES,
        );
        let close = accessibility::icon_button(
            close_spec.icon_name,
            close_spec.tooltip,
            &HEADER_BUTTON_CSS_CLASSES,
        );
        header.append(&title);
        header.append(&new_chat);
        header.append(&history);
        header.append(&settings);
        header.append(&close);

        let transcript_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        let scroll_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .margin_start(8)
            .margin_end(8)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        scroll_content.append(&transcript_box);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&scroll_content)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let prompt_view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .build();
        prompt_view.set_accessible_role(gtk::AccessibleRole::TextBox);
        prompt_view.update_property(&[
            gtk::accessible::Property::Label(&labels.prompt_accessible_name),
            gtk::accessible::Property::Description(&labels.prompt_accessible_description),
        ]);
        prompt_view.add_css_class("ai-chat-prompt");

        let prompt_scroller = gtk::ScrolledWindow::builder()
            .child(&prompt_view)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(40)
            .max_content_height(160)
            .propagate_natural_height(true)
            .has_frame(true)
            .build();
        prompt_scroller.add_css_class("card");

        let send = gtk::Button::builder()
            .label(&labels.send_label)
            .css_classes(["suggested-action"])
            .build();
        let stop = gtk::Button::builder()
            .label(&labels.stop_label)
            .css_classes(["destructive-action"])
            .visible(false)
            .build();
        accessibility::set_accessible_label(&stop, &labels.stop_tooltip);

        let input_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        input_row.add_css_class("ai-chat-input-row");
        input_row.set_margin_start(10);
        input_row.set_margin_end(10);
        input_row.set_margin_top(6);
        input_row.set_margin_bottom(10);
        prompt_scroller.set_hexpand(true);
        prompt_scroller.set_valign(gtk::Align::Center);
        input_row.append(&prompt_scroller);
        let buttons = gtk::Box::new(gtk::Orientation::Vertical, 4);
        buttons.set_valign(gtk::Align::Center);
        buttons.append(&send);
        buttons.append(&stop);
        input_row.append(&buttons);

        let status = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(40)
            .css_classes(["dim-label", "caption"])
            .build();
        let status_spinner = gtk::Spinner::new();
        status_spinner.set_size_request(12, 12);
        status_spinner.set_visible(false);
        let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        status_row.set_margin_start(10);
        status_row.set_margin_end(10);
        status_row.set_valign(gtk::Align::Center);
        status_row.append(&status_spinner);
        status_row.append(&status);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for css_class in PANEL_ROOT_CSS_CLASSES {
            root.add_css_class(css_class);
        }
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&header);
        root.append(&scroller);
        root.append(&status_row);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&input_row);

        Self {
            root,
            header,
            transcript_box,
            scroll_content,
            scroller,
            prompt_view,
            prompt_scroller,
            status_row,
            send,
            stop,
            new_chat,
            history,
            settings,
            close,
            status,
            status_spinner,
        }
    }
}

/// Localizable labels used by [`BigAiProviderCatalogLauncher`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderCatalogLabels {
    /// Window title and visible heading.
    pub window_title: String,
    /// Short description shown above the provider list.
    pub subtitle: String,
    /// Button tooltip and accessible label.
    pub button_label: String,
    /// Label for local OpenAI-compatible runtimes.
    pub local_runtime: String,
    /// Label for hosted provider APIs.
    pub remote_provider: String,
    /// Generic provider noun used in accessible row labels.
    pub provider: String,
    /// Default-model label used in row subtitles and accessible row labels.
    pub default_model: String,
}

impl Default for BigAiProviderCatalogLabels {
    fn default() -> Self {
        Self {
            window_title: "AI Providers".to_owned(),
            subtitle: "Shared provider catalog from BigLinux AI components".to_owned(),
            button_label: "AI providers".to_owned(),
            local_runtime: "Local runtime".to_owned(),
            remote_provider: "Remote provider".to_owned(),
            provider: "provider".to_owned(),
            default_model: "default model".to_owned(),
        }
    }
}

/// Shared launcher for the built-in BigLinux AI provider catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderCatalogLauncher {
    labels: BigAiProviderCatalogLabels,
}

impl BigAiProviderCatalogLauncher {
    /// Create a launcher with app-provided localized labels.
    #[must_use]
    pub fn new(labels: BigAiProviderCatalogLabels) -> Self {
        Self { labels }
    }

    /// Create a launcher with English fallback labels.
    #[must_use]
    pub fn with_default_labels() -> Self {
        Self::new(BigAiProviderCatalogLabels::default())
    }

    /// Build an icon button that opens the shared provider catalog.
    #[must_use]
    pub fn build_button(&self, parent: &gtk::Window) -> gtk::Button {
        let button = accessibility::icon_button(
            "emoji-objects-symbolic",
            &self.labels.button_label,
            &["flat"],
        );
        let parent = parent.clone();
        let launcher = self.clone();
        button.connect_clicked(move |_| {
            launcher.present(&parent);
        });
        button
    }

    /// Present the shared provider catalog in a transient window.
    pub fn present(&self, parent: &impl IsA<gtk::Window>) {
        let catalog_window = BigClosableSettingsWindow::new(
            &BigClosableSettingsWindowSpec::new(&self.labels.window_title).size(520, 460),
            parent,
        );
        catalog_window
            .window()
            .add_css_class("big-ai-provider-catalog-window");
        let content = self.build_catalog_content();
        catalog_window.set_content(&content);
        catalog_window.present();
    }

    fn build_catalog_content(&self) -> gtk::Box {
        let provider_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        for provider in builtin_ai_provider_specs() {
            provider_list.append(&self.provider_row(&provider));
        }

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        let title = gtk::Label::builder()
            .label(&self.labels.window_title)
            .halign(gtk::Align::Start)
            .css_classes(["title-2"])
            .build();
        accessibility::set_accessible_label(&title, &self.labels.window_title);
        let subtitle = gtk::Label::builder()
            .label(&self.labels.subtitle)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        accessibility::set_accessible_label(&subtitle, &self.labels.subtitle);
        content.append(&title);
        content.append(&subtitle);
        content.append(&provider_list);
        content
    }

    fn provider_row(&self, provider: &BigAiProviderSpec) -> adw::ActionRow {
        let provider_kind = if provider.local {
            &self.labels.local_runtime
        } else {
            &self.labels.remote_provider
        };
        let subtitle = format!(
            "{} - {} {}",
            provider_kind, self.labels.default_model, provider.default_model
        );
        let accessible_label = format!(
            "{} {}, {} {}",
            provider.label, self.labels.provider, self.labels.default_model, provider.default_model
        );
        let row = adw::ActionRow::builder()
            .title(&provider.label)
            .subtitle(&subtitle)
            .build();
        accessibility::set_accessible_label(&row, &accessible_label);
        row
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HeaderButtonSpec<'a> {
    icon_name: &'static str,
    tooltip: &'a str,
}

const PANEL_ROOT_CSS_CLASSES: [&str; 2] = ["ai-chat-panel", "ai-side-panel"];
const HEADER_BUTTON_CSS_CLASSES: [&str; 1] = ["flat"];

fn header_button_specs(labels: &BigAiPanelLabels) -> [HeaderButtonSpec<'_>; 4] {
    [
        HeaderButtonSpec {
            icon_name: "document-new-symbolic",
            tooltip: labels.new_chat_tooltip.as_str(),
        },
        HeaderButtonSpec {
            icon_name: "document-open-recent-symbolic",
            tooltip: labels.history_tooltip.as_str(),
        },
        HeaderButtonSpec {
            icon_name: "emblem-system-symbolic",
            tooltip: labels.settings_tooltip.as_str(),
        },
        HeaderButtonSpec {
            icon_name: "window-close-symbolic",
            tooltip: labels.close_tooltip.as_str(),
        },
    ]
}

/// Report whether the `provider requires secret` condition currently holds.
#[must_use]
pub fn provider_requires_secret(provider: &BigAiProviderSpec) -> bool {
    !provider.local
}

/// Collect the `missing ai configuration` entries derived from the supplied inputs.
#[must_use]
pub fn missing_ai_configuration(
    providers: &[BigAiProviderSpec],
    config: &BigAiAssistantConfig,
) -> Vec<&'static str> {
    let Some(provider) = providers
        .iter()
        .find(|provider| provider.id == config.provider_id)
    else {
        return vec!["provider"];
    };
    if provider_requires_secret(provider) && !config.has_api_key {
        return vec!["api_key"];
    }
    if provider.local && config.base_url.as_deref().unwrap_or("").trim().is_empty() {
        return vec!["base_url"];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn providers() -> Vec<BigAiProviderSpec> {
        vec![
            BigAiProviderSpec::remote("groq", "Groq", "llama", "https://console.groq.com/keys"),
            BigAiProviderSpec::local("local", "Local", "llama3.2"),
        ]
    }

    #[test]
    fn builtin_provider_catalog_matches_biglinux_defaults() {
        let providers = builtin_ai_provider_specs();
        let ids: Vec<&str> = providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect();

        assert_eq!(
            ids,
            vec![
                "groq",
                "gemini",
                "openrouter",
                "cerebras",
                "github",
                "mistral",
                "local"
            ]
        );
        assert_eq!(DEFAULT_AI_PROVIDER_ID, "groq");
        assert_eq!(
            builtin_ai_provider_default_model("gemini"),
            "gemini-2.5-flash"
        );
        assert_eq!(builtin_ai_provider_index("missing"), 0);
    }

    #[test]
    fn builtin_provider_browser_base_url_uses_local_override() {
        assert_eq!(
            builtin_ai_provider_model_browser_base_url("openrouter", DEFAULT_LOCAL_AI_BASE_URL),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            builtin_ai_provider_model_browser_base_url("local", " http://127.0.0.1:1234/v1 "),
            "http://127.0.0.1:1234/v1"
        );
        assert_eq!(
            builtin_ai_provider_model_browser_base_url("missing", DEFAULT_LOCAL_AI_BASE_URL),
            ""
        );
    }

    #[test]
    fn provider_catalog_launcher_keeps_localizable_labels() {
        let labels = BigAiProviderCatalogLabels {
            window_title: "Providers".to_owned(),
            subtitle: "One shared catalog".to_owned(),
            button_label: "Open providers".to_owned(),
            local_runtime: "Local".to_owned(),
            remote_provider: "Remote".to_owned(),
            provider: "provider".to_owned(),
            default_model: "model".to_owned(),
        };
        let launcher = BigAiProviderCatalogLauncher::new(labels.clone());

        assert_eq!(launcher.labels, labels);
        assert_eq!(
            BigAiProviderCatalogLauncher::with_default_labels()
                .labels
                .button_label,
            "AI providers"
        );
    }

    #[test]
    fn remote_provider_requires_key_owned_by_app_adapter() {
        let provider = &providers()[0];

        assert!(provider_requires_secret(provider));
        assert_eq!(
            provider.api_key_url.as_deref(),
            Some("https://console.groq.com/keys")
        );
    }

    #[test]
    fn missing_configuration_reports_provider_key_or_base_url() {
        let providers = providers();
        let remote = BigAiAssistantConfig {
            enabled: true,
            provider_id: "groq".into(),
            model: "llama".into(),
            base_url: None,
            has_api_key: false,
        };
        let local = BigAiAssistantConfig {
            enabled: true,
            provider_id: "local".into(),
            model: "llama3.2".into(),
            base_url: Some(" ".into()),
            has_api_key: false,
        };

        assert_eq!(
            missing_ai_configuration(&providers, &remote),
            vec!["api_key"]
        );
        assert_eq!(
            missing_ai_configuration(&providers, &local),
            vec!["base_url"]
        );
    }

    #[test]
    fn valid_local_config_needs_no_secret() {
        let providers = providers();
        let config = BigAiAssistantConfig {
            enabled: true,
            provider_id: "local".into(),
            model: "llama3.2".into(),
            base_url: Some("http://localhost:11434/v1".into()),
            has_api_key: false,
        };

        assert!(missing_ai_configuration(&providers, &config).is_empty());
    }

    #[test]
    fn default_panel_labels_are_generic() {
        let labels = BigAiPanelLabels::default();

        assert_eq!(labels.title, "AI Assistant");
        assert!(labels.prompt_accessible_description.contains("Enter"));
        assert_eq!(labels.send_label, "Send");
    }

    #[test]
    fn panel_shell_exposes_display_free_extension_contracts() {
        let labels = BigAiPanelLabels::default();
        let button_specs = header_button_specs(&labels);

        assert_eq!(
            button_specs.map(|spec| spec.icon_name),
            [
                "document-new-symbolic",
                "document-open-recent-symbolic",
                "emblem-system-symbolic",
                "window-close-symbolic"
            ]
        );
        assert_eq!(button_specs[0].tooltip, labels.new_chat_tooltip);
        assert_eq!(button_specs[1].tooltip, labels.history_tooltip);
        assert_eq!(button_specs[2].tooltip, labels.settings_tooltip);
        assert_eq!(button_specs[3].tooltip, labels.close_tooltip);
        assert_eq!(PANEL_ROOT_CSS_CLASSES, ["ai-chat-panel", "ai-side-panel"]);
        assert_eq!(HEADER_BUTTON_CSS_CLASSES, ["flat"]);
    }
}
