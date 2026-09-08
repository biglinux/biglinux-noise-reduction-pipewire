// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared AI provider settings UI.

use adw::prelude::*;

use crate::gtk_accessibility_policy as accessibility;
use crate::widgets::{BigClosableSettingsWindow, BigClosableSettingsWindowSpec};

use super::{
    BigAiProviderSettingsSnapshot, DEFAULT_AI_PROVIDER_ID, builtin_ai_provider_default_model,
    builtin_ai_provider_index, builtin_ai_provider_model_browser_base_url,
    builtin_ai_provider_spec, builtin_ai_provider_specs,
};

/// Localizable labels for [`BigAiProviderSettingsContent`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderSettingsLabels {
    /// Main page title.
    pub title: String,
    /// Page description.
    pub description: String,
    /// Toggle row title.
    pub enable_title: String,
    /// Toggle row subtitle.
    pub enable_subtitle: String,
    /// Model group title.
    pub model_group_title: String,
    /// Provider row title.
    pub provider_title: String,
    /// Provider row accessible name.
    pub provider_accessible_name: String,
    /// Model row title.
    pub model_title: String,
    /// Model entry accessible name.
    pub model_accessible_name: String,
    /// Browse-model button label.
    pub browse_models: String,
    /// API key group title.
    pub api_key_group_title: String,
    /// API key row title.
    pub api_key_title: String,
    /// API key entry accessible name.
    pub api_key_accessible_name: String,
    /// Get-key button label.
    pub get_api_key: String,
    /// Advanced group title.
    pub advanced_group_title: String,
    /// Local base URL row title.
    pub base_url_title: String,
    /// OpenRouter site URL row title.
    pub openrouter_site_url_title: String,
    /// OpenRouter site name row title.
    pub openrouter_site_name_title: String,
}

impl Default for BigAiProviderSettingsLabels {
    fn default() -> Self {
        Self {
            title: "AI Assistant".to_owned(),
            description: "Configure the shared BigLinux AI provider, model, and API access."
                .to_owned(),
            enable_title: "Enable AI Assistant".to_owned(),
            enable_subtitle: "Show AI assistant actions in applications that support them."
                .to_owned(),
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
        }
    }
}

/// Request sent to an app-owned model-browser integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigAiProviderModelBrowseRequest {
    /// Provider whose catalog should be queried.
    pub provider_id: String,
    /// Provider base URL to use for the catalog request.
    pub base_url: String,
    /// API key currently entered by the user.
    pub api_key: String,
    /// Current model shown in the settings UI.
    pub current_model: String,
}

/// Callback used by an app-owned model browser to write the selected model.
pub type BigAiProviderModelSelected = std::rc::Rc<dyn Fn(String)>;

/// App-owned hook that can open a provider model browser.
pub type BigAiProviderModelBrowser =
    std::rc::Rc<dyn Fn(BigAiProviderModelBrowseRequest, BigAiProviderModelSelected)>;

/// Shared editable AI provider settings widget.
pub struct BigAiProviderSettingsContent {
    root: gtk::Box,
    owner: std::rc::Rc<dyn std::any::Any>,
}

impl BigAiProviderSettingsContent {
    /// Build shared settings content.
    ///
    /// `on_snapshot_changed` is called with a complete snapshot whenever a field
    /// changes. `model_browser` is optional because some host apps only expose
    /// manual model entry.
    #[must_use]
    pub fn new(
        labels: BigAiProviderSettingsLabels,
        initial: BigAiProviderSettingsSnapshot,
        on_snapshot_changed: std::rc::Rc<dyn Fn(BigAiProviderSettingsSnapshot)>,
        model_browser: Option<BigAiProviderModelBrowser>,
    ) -> Self {
        let widgets = std::rc::Rc::new(BigAiProviderSettingsWidgets::new(&labels, &initial));
        let sections = BigAiProviderSettingsSections::new(&labels, &widgets);
        let root = build_ai_provider_settings_root(&labels, &sections);

        apply_provider_settings_visibility(&widgets, &sections, &initial.provider_id);
        apply_ai_provider_master_visibility(&sections, initial.enabled);
        wire_ai_provider_settings(&widgets, &sections, on_snapshot_changed, model_browser);

        Self {
            root,
            owner: widgets,
        }
    }

    /// Root widget to mount in a preferences page or dialog.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Strong owner for signal-connected widget state. Keep this alive for as
    /// long as `root` is mounted.
    #[must_use]
    pub fn owner(&self) -> std::rc::Rc<dyn std::any::Any> {
        self.owner.clone()
    }
}

/// Shared launcher for the editable AI provider settings UI.
pub struct BigAiProviderSettingsLauncher {
    labels: BigAiProviderSettingsLabels,
    initial: BigAiProviderSettingsSnapshot,
    on_snapshot_changed: std::rc::Rc<dyn Fn(BigAiProviderSettingsSnapshot)>,
    model_browser: Option<BigAiProviderModelBrowser>,
}

impl BigAiProviderSettingsLauncher {
    /// Create a settings launcher.
    #[must_use]
    pub fn new(
        labels: BigAiProviderSettingsLabels,
        initial: BigAiProviderSettingsSnapshot,
        on_snapshot_changed: std::rc::Rc<dyn Fn(BigAiProviderSettingsSnapshot)>,
        model_browser: Option<BigAiProviderModelBrowser>,
    ) -> Self {
        Self {
            labels,
            initial,
            on_snapshot_changed,
            model_browser,
        }
    }

    /// Build an icon button that opens the shared AI provider settings window.
    #[must_use]
    pub fn build_button(&self, parent: &gtk::Window) -> gtk::Button {
        let button =
            accessibility::icon_button("emblem-system-symbolic", &self.labels.title, &["flat"]);
        let parent = parent.clone();
        let launcher = self.clone();
        button.connect_clicked(move |_| {
            launcher.present(&parent);
        });
        button
    }

    /// Present the shared AI provider settings window.
    pub fn present(&self, parent: &impl IsA<gtk::Window>) {
        let content = BigAiProviderSettingsContent::new(
            self.labels.clone(),
            self.initial.clone(),
            self.on_snapshot_changed.clone(),
            self.model_browser.clone(),
        );
        let owner = content.owner();
        let settings_window = BigClosableSettingsWindow::new(
            &BigClosableSettingsWindowSpec::new(&self.labels.title),
            parent,
        );
        settings_window
            .window()
            .add_css_class("big-ai-provider-settings-window");
        settings_window.set_content(content.root());
        settings_window.window().connect_destroy(move |_| {
            let _keep_alive = owner.clone();
        });
        settings_window.present();
    }
}

impl Clone for BigAiProviderSettingsLauncher {
    fn clone(&self) -> Self {
        Self {
            labels: self.labels.clone(),
            initial: self.initial.clone(),
            on_snapshot_changed: self.on_snapshot_changed.clone(),
            model_browser: self.model_browser.clone(),
        }
    }
}

struct BigAiProviderSettingsWidgets {
    enable_switch: gtk::Switch,
    provider_dropdown: gtk::DropDown,
    api_key_entry: gtk::PasswordEntry,
    get_key_button: gtk::LinkButton,
    model_entry: gtk::Entry,
    browse_button: gtk::Button,
    base_url_row: adw::EntryRow,
    openrouter_site_url_row: adw::EntryRow,
    openrouter_site_name_row: adw::EntryRow,
}

impl BigAiProviderSettingsWidgets {
    fn new(labels: &BigAiProviderSettingsLabels, initial: &BigAiProviderSettingsSnapshot) -> Self {
        let enable_switch = gtk::Switch::builder()
            .active(initial.enabled)
            .valign(gtk::Align::Center)
            .build();
        accessibility::set_accessible_label(&enable_switch, &labels.enable_title);

        let provider_dropdown =
            build_ai_provider_dropdown(&labels.provider_accessible_name, &initial.provider_id);

        let api_key_entry = gtk::PasswordEntry::builder()
            .text(&initial.api_key)
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        accessibility::set_accessible_label(&api_key_entry, &labels.api_key_accessible_name);

        let get_key_button = gtk::LinkButton::builder()
            .label(&labels.get_api_key)
            .valign(gtk::Align::Center)
            .build();
        accessibility::set_accessible_label(&get_key_button, &labels.get_api_key);

        let model_entry = gtk::Entry::builder()
            .text(&initial.model)
            .hexpand(true)
            .build();
        accessibility::set_accessible_label(&model_entry, &labels.model_accessible_name);

        let browse_button = gtk::Button::builder()
            .label(&labels.browse_models)
            .valign(gtk::Align::Center)
            .build();
        accessibility::set_accessible_label(&browse_button, &labels.browse_models);

        let base_url_row = adw::EntryRow::builder()
            .title(&labels.base_url_title)
            .text(&initial.base_url)
            .build();
        let openrouter_site_url_row = adw::EntryRow::builder()
            .title(&labels.openrouter_site_url_title)
            .text(&initial.openrouter_site_url)
            .build();
        let openrouter_site_name_row = adw::EntryRow::builder()
            .title(&labels.openrouter_site_name_title)
            .text(&initial.openrouter_site_name)
            .build();

        Self {
            enable_switch,
            provider_dropdown,
            api_key_entry,
            get_key_button,
            model_entry,
            browse_button,
            base_url_row,
            openrouter_site_url_row,
            openrouter_site_name_row,
        }
    }
}

struct BigAiProviderSettingsSections {
    enable: gtk::ListBox,
    model: gtk::Box,
    api_key: gtk::Box,
    advanced: gtk::Box,
}

impl BigAiProviderSettingsSections {
    fn new(labels: &BigAiProviderSettingsLabels, widgets: &BigAiProviderSettingsWidgets) -> Self {
        Self {
            enable: build_ai_provider_enable_group(labels, widgets),
            model: build_ai_provider_model_group(labels, widgets),
            api_key: build_ai_provider_api_key_group(labels, widgets),
            advanced: build_ai_provider_advanced_group(labels, widgets),
        }
    }

    fn clone_for_callbacks(&self) -> Self {
        Self {
            enable: self.enable.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            advanced: self.advanced.clone(),
        }
    }
}

fn build_ai_provider_settings_root(
    labels: &BigAiProviderSettingsLabels,
    sections: &BigAiProviderSettingsSections,
) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let title = gtk::Label::builder()
        .label(&labels.title)
        .halign(gtk::Align::Start)
        .css_classes(["title-2"])
        .build();
    accessibility::set_accessible_label(&title, &labels.title);
    let description = gtk::Label::builder()
        .label(&labels.description)
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    accessibility::set_accessible_label(&description, &labels.description);
    root.append(&title);
    root.append(&description);
    root.append(&sections.enable);
    root.append(&sections.model);
    root.append(&sections.api_key);
    root.append(&sections.advanced);
    root
}

fn build_ai_provider_enable_group(
    labels: &BigAiProviderSettingsLabels,
    widgets: &BigAiProviderSettingsWidgets,
) -> gtk::ListBox {
    let row = adw::ActionRow::builder()
        .title(&labels.enable_title)
        .subtitle(&labels.enable_subtitle)
        .build();
    row.add_suffix(&widgets.enable_switch);
    row.set_activatable_widget(Some(&widgets.enable_switch));

    let list = boxed_settings_list();
    list.append(&row);
    list
}

fn build_ai_provider_model_group(
    labels: &BigAiProviderSettingsLabels,
    widgets: &BigAiProviderSettingsWidgets,
) -> gtk::Box {
    let provider_row = adw::ActionRow::builder()
        .title(&labels.provider_title)
        .build();
    provider_row.add_suffix(&widgets.provider_dropdown);
    provider_row.set_activatable_widget(Some(&widgets.provider_dropdown));

    let model_control = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    model_control.append(&widgets.model_entry);
    model_control.append(&widgets.browse_button);
    let model_row = adw::ActionRow::builder().title(&labels.model_title).build();
    model_row.add_suffix(&model_control);
    model_row.set_activatable_widget(Some(&widgets.model_entry));

    let (section, list) = titled_settings_section(&labels.model_group_title);
    list.append(&provider_row);
    list.append(&model_row);
    section
}

fn build_ai_provider_api_key_group(
    labels: &BigAiProviderSettingsLabels,
    widgets: &BigAiProviderSettingsWidgets,
) -> gtk::Box {
    let control = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    control.append(&widgets.api_key_entry);
    control.append(&widgets.get_key_button);
    let row = adw::ActionRow::builder()
        .title(&labels.api_key_title)
        .build();
    row.add_suffix(&control);
    row.set_activatable_widget(Some(&widgets.api_key_entry));

    let (section, list) = titled_settings_section(&labels.api_key_group_title);
    list.append(&row);
    section
}

fn build_ai_provider_advanced_group(
    labels: &BigAiProviderSettingsLabels,
    widgets: &BigAiProviderSettingsWidgets,
) -> gtk::Box {
    let (section, list) = titled_settings_section(&labels.advanced_group_title);
    list.append(&widgets.base_url_row);
    list.append(&widgets.openrouter_site_url_row);
    list.append(&widgets.openrouter_site_name_row);
    section
}

fn titled_settings_section(title: &str) -> (gtk::Box, gtk::ListBox) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let title_label = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["heading"])
        .build();
    accessibility::set_accessible_label(&title_label, title);
    let list = boxed_settings_list();
    section.append(&title_label);
    section.append(&list);
    (section, list)
}

fn boxed_settings_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

fn build_ai_provider_dropdown(accessible_name: &str, provider_id: &str) -> gtk::DropDown {
    let labels: Vec<String> = builtin_ai_provider_specs()
        .iter()
        .map(|provider| provider.label.clone())
        .collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&label_refs);
    let dropdown = gtk::DropDown::builder()
        .model(&model)
        .selected(u32::try_from(builtin_ai_provider_index(provider_id)).unwrap_or(0))
        .hexpand(true)
        .build();
    accessibility::set_accessible_label(&dropdown, accessible_name);
    dropdown
}

fn wire_ai_provider_settings(
    widgets: &std::rc::Rc<BigAiProviderSettingsWidgets>,
    sections: &BigAiProviderSettingsSections,
    on_snapshot_changed: std::rc::Rc<dyn Fn(BigAiProviderSettingsSnapshot)>,
    model_browser: Option<BigAiProviderModelBrowser>,
) {
    let emit_snapshot = std::rc::Rc::new({
        let widgets_weak = std::rc::Rc::downgrade(widgets);
        move || {
            let Some(widgets) = widgets_weak.upgrade() else {
                return;
            };
            on_snapshot_changed(collect_ai_provider_settings_snapshot(&widgets));
        }
    });

    {
        let widgets_weak = std::rc::Rc::downgrade(widgets);
        let emit_snapshot = emit_snapshot.clone();
        let sections = sections.clone_for_callbacks();
        widgets.enable_switch.connect_active_notify(move |switch| {
            apply_ai_provider_master_visibility(&sections, switch.is_active());
            if let Some(widgets) = widgets_weak.upgrade() {
                let provider_id = selected_builtin_ai_provider_id(&widgets.provider_dropdown);
                apply_provider_settings_visibility(&widgets, &sections, &provider_id);
            }
            emit_snapshot();
        });
    }
    {
        let widgets_weak = std::rc::Rc::downgrade(widgets);
        let emit_snapshot = emit_snapshot.clone();
        let sections = sections.clone_for_callbacks();
        widgets
            .provider_dropdown
            .connect_selected_notify(move |dropdown| {
                let Some(widgets) = widgets_weak.upgrade() else {
                    return;
                };
                let provider_id = selected_builtin_ai_provider_id(dropdown);
                apply_provider_settings_visibility(&widgets, &sections, &provider_id);
                widgets
                    .model_entry
                    .set_text(builtin_ai_provider_default_model(&provider_id));
                emit_snapshot();
            });
    }
    {
        let emit_snapshot = emit_snapshot.clone();
        widgets
            .api_key_entry
            .connect_changed(move |_| emit_snapshot());
    }
    {
        let emit_snapshot = emit_snapshot.clone();
        widgets
            .model_entry
            .connect_changed(move |_| emit_snapshot());
    }
    {
        let emit_snapshot = emit_snapshot.clone();
        widgets
            .base_url_row
            .connect_changed(move |_| emit_snapshot());
    }
    {
        let emit_snapshot = emit_snapshot.clone();
        widgets
            .openrouter_site_url_row
            .connect_changed(move |_| emit_snapshot());
    }
    {
        let emit_snapshot = emit_snapshot.clone();
        widgets
            .openrouter_site_name_row
            .connect_changed(move |_| emit_snapshot());
    }

    if let Some(model_browser) = model_browser {
        let widgets_weak = std::rc::Rc::downgrade(widgets);
        widgets.browse_button.connect_clicked(move |_| {
            let Some(widgets) = widgets_weak.upgrade() else {
                return;
            };
            let request = BigAiProviderModelBrowseRequest {
                provider_id: selected_builtin_ai_provider_id(&widgets.provider_dropdown),
                base_url: browser_base_url_for_widgets(&widgets),
                api_key: widgets.api_key_entry.text().trim().to_owned(),
                current_model: widgets.model_entry.text().trim().to_owned(),
            };
            let model_entry = widgets.model_entry.clone();
            let selected = std::rc::Rc::new(move |model: String| {
                model_entry.set_text(&model);
            });
            model_browser(request, selected);
        });
    } else {
        widgets.browse_button.set_sensitive(false);
    }
}

fn collect_ai_provider_settings_snapshot(
    widgets: &BigAiProviderSettingsWidgets,
) -> BigAiProviderSettingsSnapshot {
    BigAiProviderSettingsSnapshot {
        enabled: widgets.enable_switch.is_active(),
        provider_id: selected_builtin_ai_provider_id(&widgets.provider_dropdown),
        api_key: widgets.api_key_entry.text().to_string(),
        model: widgets.model_entry.text().trim().to_owned(),
        base_url: widgets.base_url_row.text().trim().to_owned(),
        openrouter_site_url: widgets.openrouter_site_url_row.text().trim().to_owned(),
        openrouter_site_name: widgets.openrouter_site_name_row.text().trim().to_owned(),
    }
}

fn apply_ai_provider_master_visibility(sections: &BigAiProviderSettingsSections, enabled: bool) {
    sections.model.set_visible(enabled);
    sections.api_key.set_visible(enabled);
    sections.advanced.set_visible(enabled);
}

fn apply_provider_settings_visibility(
    widgets: &BigAiProviderSettingsWidgets,
    sections: &BigAiProviderSettingsSections,
    provider_id: &str,
) {
    let provider = builtin_ai_provider_spec(provider_id);
    let is_local = provider.as_ref().is_some_and(|provider| provider.local);
    let is_openrouter = provider_id == "openrouter";
    sections
        .api_key
        .set_visible(!is_local && widgets.enable_switch.is_active());
    widgets.base_url_row.set_visible(is_local);
    widgets.openrouter_site_url_row.set_visible(is_openrouter);
    widgets.openrouter_site_name_row.set_visible(is_openrouter);
    if let Some(url) = provider.and_then(|provider| provider.api_key_url) {
        widgets.get_key_button.set_uri(&url);
    }
}

fn selected_builtin_ai_provider_id(provider_dropdown: &gtk::DropDown) -> String {
    let idx = usize::try_from(provider_dropdown.selected()).unwrap_or(0);
    builtin_ai_provider_specs().get(idx).map_or_else(
        || DEFAULT_AI_PROVIDER_ID.to_owned(),
        |provider| provider.id.clone(),
    )
}

fn browser_base_url_for_widgets(widgets: &BigAiProviderSettingsWidgets) -> String {
    let provider_id = selected_builtin_ai_provider_id(&widgets.provider_dropdown);
    builtin_ai_provider_model_browser_base_url(&provider_id, widgets.base_url_row.text().trim())
}
