// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK/libadwaita widget builders backed by `big-app-kit` specs.
//!
//! These helpers keep common BigLinux desktop UI assembly small. They do not
//! own app domain state; consumers wire callbacks and Relm4 messages.

use adw::prelude::*;
use gtk::{gdk, gio};
use relm4::gtk;

use crate::collections::{BigCollectionMode, BigCollectionSpec};
use crate::dialogs::BigDialogSpec;
use crate::files::{BigDropTargetSpec, BigFileEntry, BigFilePickerSpec, BigPickerMode};
use crate::forms::{BigFieldKind, BigFieldSpec, BigSettingsGroupSpec, BigSettingsPageSpec};
use crate::gtk_accessibility_policy as accessibility;
use crate::previews::{BigPreviewKind, BigPreviewSpec};
use crate::shell::{BigHeaderPolicy, BigShellSpec};
use crate::tasks::{
    BigDiagnosticsSpec, BigJobQueueSpec, BigTaskProgress, BigTaskSpec, BigTaskState,
};

mod drop;

/// File-drop parsing and recursive expansion helpers.
pub use drop::{
    add_file_drop_target, drop_paths_from_text, drop_paths_from_value, expand_drop_paths,
    parse_uri_list, path_drop_target,
};

#[cfg(test)]
mod tests;

/// Display-free contract for a transient settings window with an explicit
/// header close button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigClosableSettingsWindowSpec {
    /// Window title.
    pub title: String,
    /// AT-SPI label and tooltip for the header close button.
    pub close_label: String,
    /// Initial window width.
    pub default_width: i32,
    /// Initial window height.
    pub default_height: i32,
}

impl BigClosableSettingsWindowSpec {
    /// Build the default settings-window contract for `title`.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            close_label: format!("Close {title}"),
            title,
            default_width: 560,
            default_height: 620,
        }
    }

    /// Override the initial window size.
    #[must_use]
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.default_width = width;
        self.default_height = height;
        self
    }
}

/// Shared non-modal settings window chrome used by app-kit owned panels.
#[derive(Debug, Clone)]
pub struct BigClosableSettingsWindow {
    // bigagents: app-local-window-owner - this wrapper is the window owner;
    // signal handlers capture only WeakRef values and do not form a cycle.
    window: adw::Window,
    scroller: gtk::ScrolledWindow,
}

impl BigClosableSettingsWindow {
    /// Realise a settings window from a display-free spec.
    #[must_use]
    pub fn new(spec: &BigClosableSettingsWindowSpec, parent: &impl IsA<gtk::Window>) -> Self {
        let window = adw::Window::builder()
            .title(&spec.title)
            .modal(false)
            .transient_for(parent)
            .default_width(spec.default_width)
            .default_height(spec.default_height)
            .build();

        let title = adw::WindowTitle::new(&spec.title, "");
        let header = adw::HeaderBar::builder()
            .title_widget(&title)
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        let close_button =
            accessibility::icon_button("window-close-symbolic", &spec.close_label, &["flat"]);
        let window_weak = window.downgrade();
        close_button.connect_clicked(move |_| {
            if let Some(window) = window_weak.upgrade() {
                window.close();
            }
        });
        header.pack_end(&close_button);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let root = adw::ToolbarView::new();
        root.add_top_bar(&header);
        root.set_content(Some(&scroller));
        window.set_content(Some(&root));

        Self { window, scroller }
    }

    /// Borrow the window so callers can apply CSS classes or lifecycle hooks.
    #[must_use]
    pub fn window(&self) -> &adw::Window {
        &self.window
    }

    /// Mount the settings content inside the standard scrolled body.
    pub fn set_content(&self, content: &impl IsA<gtk::Widget>) {
        self.scroller.set_child(Some(content));
    }

    /// Present the settings window.
    pub fn present(&self) {
        self.window.present();
    }
}

/// Big drop target widget widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigDropTargetWidget {
    root: gtk::Box,
    target: gtk::DropTarget,
    spec: BigDropTargetSpec,
}

impl BigDropTargetWidget {
    /// Realise a [`BigDropTargetWidget`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: BigDropTargetSpec, title: &str, description: &str) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["big-drop-target"])
            .build();

        let title_label = gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Center)
            .css_classes(["title-3"])
            .build();
        let description_label = gtk::Label::builder()
            .label(description)
            .halign(gtk::Align::Center)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        root.append(&title_label);
        root.append(&description_label);
        root.update_property(&[gtk::accessible::Property::Label(title)]);

        let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::COPY);
        root.add_controller(target.clone());

        Self { root, target, spec }
    }

    /// Wire `callback` to fire whenever a `text/uri-list` drop lands on
    /// the target. Drops outside the spec's allowed kinds or above
    /// `max_items` are silently rejected.
    pub fn connect_uri_list<F>(&self, callback: F)
    where
        F: Fn(Vec<String>) + 'static,
    {
        let spec = self.spec.clone();
        self.target.connect_drop(move |_, value, _, _| {
            let Ok(text) = value.get::<String>() else {
                return false;
            };
            let uris = parse_uri_list(&text, spec.max_items);
            if uris.is_empty() {
                return false;
            }
            callback(uris);
            true
        });
    }

    /// Borrow the root widget owned by this [`BigDropTargetWidget`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }
}

/// Big file picker button widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigFilePickerButton {
    button: gtk::Button,
    spec: BigFilePickerSpec,
}

impl BigFilePickerButton {
    /// Realise a [`BigFilePickerButton`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: BigFilePickerSpec) -> Self {
        let (icon, label) = picker_icon_and_label(&spec);
        let button = gtk::Button::builder()
            .icon_name(icon)
            .label(label)
            .css_classes(["suggested-action"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        accessibility::set_accessible_label(&button, label);
        Self { button, spec }
    }

    /// Return a reference to the `button` exposed by this [`BigFilePickerButton`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn button(&self) -> &gtk::Button {
        &self.button
    }

    /// Return a reference to the `spec` exposed by this [`BigFilePickerButton`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn spec(&self) -> &BigFilePickerSpec {
        &self.spec
    }
}

/// Big path row widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigPathRow {
    row: adw::ActionRow,
    button: gtk::Button,
}

impl BigPathRow {
    /// Realise a [`BigPathRow`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: BigFilePickerSpec, current_path: Option<&str>) -> Self {
        let row = adw::ActionRow::builder().title(&spec.title).build();
        if let Some(path) = current_path {
            row.set_subtitle(path);
        }
        let (icon, label) = picker_icon_and_label(&spec);
        let button = accessibility::icon_button(icon, label, &["flat", "circular"]);
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        Self { row, button }
    }

    /// Return a reference to the `row` exposed by this [`BigPathRow`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `button` exposed by this [`BigPathRow`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn button(&self) -> &gtk::Button {
        &self.button
    }
}

/// Big file collection widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigFileCollection {
    root: gtk::ScrolledWindow,
    list: gtk::ListBox,
}

impl BigFileCollection {
    /// Realise a [`BigFileCollection`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: BigCollectionSpec, items: &[BigFileEntry]) -> Self {
        let list = gtk::ListBox::new();
        list.add_css_class(match spec.mode {
            BigCollectionMode::Grid | BigCollectionMode::PreviewGrid => "boxed-list",
            BigCollectionMode::Queue => "boxed-list",
            BigCollectionMode::List | BigCollectionMode::ListDetail => "navigation-sidebar",
        });
        for file_entry in items {
            list.append(&file_entry_row(file_entry, spec.reorderable));
        }
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        Self { root, list }
    }

    /// Borrow the root widget owned by this [`BigFileCollection`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Return a reference to the `list` exposed by this [`BigFileCollection`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn list(&self) -> &gtk::ListBox {
        &self.list
    }
}

/// Big settings page widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigSettingsPage {
    page: adw::PreferencesPage,
}

impl BigSettingsPage {
    /// Realise a [`BigSettingsPage`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigSettingsPageSpec) -> Self {
        let page = adw::PreferencesPage::builder().title(&spec.title).build();
        for group in &spec.groups {
            page.add(&settings_group(group));
        }
        Self { page }
    }

    /// Return a reference to the `page` exposed by this [`BigSettingsPage`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn page(&self) -> &adw::PreferencesPage {
        &self.page
    }
}

/// Big task progress row widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigTaskProgressRow {
    row: adw::ActionRow,
    progress: gtk::ProgressBar,
    cancel_button: gtk::Button,
    retry_button: gtk::Button,
}

impl BigTaskProgressRow {
    /// Realise a [`BigTaskProgressRow`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigTaskSpec, state: &BigTaskProgress) -> Self {
        let row = adw::ActionRow::builder()
            .title(&spec.title)
            .subtitle(&state.message)
            .build();
        let progress = gtk::ProgressBar::new();
        apply_progress(&progress, state);
        let cancel_button =
            accessibility::icon_button("process-stop-symbolic", "Cancel", &["flat", "circular"]);
        let retry_button =
            accessibility::icon_button("view-refresh-symbolic", "Retry", &["flat", "circular"]);
        cancel_button.set_sensitive(spec.cancellable && !state.is_terminal());
        retry_button.set_sensitive(spec.retryable && state.state == BigTaskState::Failed);

        let suffix = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();
        suffix.append(&progress);
        suffix.append(&cancel_button);
        suffix.append(&retry_button);
        row.add_suffix(&suffix);

        Self {
            row,
            progress,
            cancel_button,
            retry_button,
        }
    }

    /// Push a new progress sample into the row: re-renders the
    /// subtitle, fraction, and pulse.
    pub fn update(&self, state: &BigTaskProgress) {
        self.row.set_subtitle(&state.message);
        apply_progress(&self.progress, state);
    }

    /// Return a reference to the `row` exposed by this [`BigTaskProgressRow`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `cancel button` exposed by this [`BigTaskProgressRow`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn cancel_button(&self) -> &gtk::Button {
        &self.cancel_button
    }

    /// Return a reference to the `retry button` exposed by this [`BigTaskProgressRow`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn retry_button(&self) -> &gtk::Button {
        &self.retry_button
    }
}

/// Big job queue widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigJobQueue {
    root: gtk::Box,
    rows: Vec<BigTaskProgressRow>,
}

impl BigJobQueue {
    /// Realise a [`BigJobQueue`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigJobQueueSpec, tasks: &[(BigTaskSpec, BigTaskProgress)]) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        root.append(
            &gtk::Label::builder()
                .label(&spec.title)
                .halign(gtk::Align::Start)
                .css_classes(["heading"])
                .build(),
        );
        let rows = tasks
            .iter()
            .map(|(task, progress)| {
                let row = BigTaskProgressRow::new(task, progress);
                root.append(row.row());
                row
            })
            .collect();
        Self { root, rows }
    }

    /// Borrow the root widget owned by this [`BigJobQueue`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `rows` exposed by this [`BigJobQueue`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn rows(&self) -> &[BigTaskProgressRow] {
        &self.rows
    }
}

/// Big diagnostics panel widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigDiagnosticsPanel {
    root: gtk::Box,
    text: gtk::TextView,
}

impl BigDiagnosticsPanel {
    /// Realise a [`BigDiagnosticsPanel`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigDiagnosticsSpec, details: &str) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        root.append(
            &gtk::Label::builder()
                .label(&spec.title)
                .halign(gtk::Align::Start)
                .css_classes(["heading"])
                .build(),
        );
        let text = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .vexpand(true)
            .build();
        text.buffer().set_text(details);
        root.append(&text);
        Self { root, text }
    }

    /// Borrow the root widget owned by this [`BigDiagnosticsPanel`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `text` exposed by this [`BigDiagnosticsPanel`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn text(&self) -> &gtk::TextView {
        &self.text
    }
}

/// Big preview panel widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigPreviewPanel {
    root: adw::StatusPage,
}

impl BigPreviewPanel {
    /// Realise a [`BigPreviewPanel`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigPreviewSpec, title: &str) -> Self {
        let root = adw::StatusPage::builder()
            .title(title)
            .icon_name(preview_icon_name(spec.kind))
            .description(preview_description(spec))
            .build();
        Self { root }
    }

    /// Borrow the root widget owned by this [`BigPreviewPanel`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &adw::StatusPage {
        &self.root
    }
}

/// Big app shell widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigAppShell {
    root: adw::ToolbarView,
    header: adw::HeaderBar,
    content: gtk::Box,
}

impl BigAppShell {
    /// Realise a [`BigAppShell`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigShellSpec) -> Self {
        let root = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let title = adw::WindowTitle::new(&spec.title, "");
        header.set_title_widget(Some(&title));
        root.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(match spec.header_policy {
                BigHeaderPolicy::Unified => 0,
                BigHeaderPolicy::SidebarAndContent | BigHeaderPolicy::TraditionalToolbar => 1,
            })
            .hexpand(true)
            .vexpand(true)
            .build();
        root.set_content(Some(&content));
        Self {
            root,
            header,
            content,
        }
    }

    /// Borrow the root widget owned by this [`BigAppShell`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ToolbarView {
        &self.root
    }

    /// Return a reference to the `header` exposed by this [`BigAppShell`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// Return a reference to the `content` exposed by this [`BigAppShell`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn content(&self) -> &gtk::Box {
        &self.content
    }
}

/// Big dialog panel widget wrapper used by BigLinux apps.
#[derive(Debug, Clone)]
pub struct BigDialogPanel {
    root: adw::StatusPage,
}

impl BigDialogPanel {
    /// Realise a [`BigDialogPanel`] from the supplied spec.
    ///
    /// Builds the GTK widget tree eagerly; the returned struct exposes
    /// the underlying widgets for signal wiring.
    #[must_use]
    pub fn new(spec: &BigDialogSpec) -> Self {
        let resolved = spec.resolved();
        let root = adw::StatusPage::builder()
            .title(&spec.title)
            .description(spec.body.as_deref().unwrap_or(resolved.severity))
            .icon_name(dialog_icon_name(resolved.severity))
            .build();
        Self { root }
    }

    /// Borrow the root widget owned by this [`BigDialogPanel`].
    ///
    /// Use the borrow to wire signals or embed the widget into a
    /// larger surface; the parent retains ownership.
    #[must_use]
    pub fn root(&self) -> &adw::StatusPage {
        &self.root
    }
}

/// Build a pre-revealed [`adw::Banner`] surfacing a form-level error.
///
/// Place above the form's [`adw::PreferencesGroup`]s (or directly under
/// the header bar in dialog flows) to surface validation failures without
/// blocking input. The banner stays visible until callers hide it.
#[must_use]
pub fn validation_banner(message: &str) -> adw::Banner {
    adw::Banner::builder().title(message).revealed(true).build()
}

fn picker_icon_and_label(spec: &BigFilePickerSpec) -> (&'static str, &'static str) {
    match spec.mode {
        BigPickerMode::OpenFile => ("document-open-symbolic", "Select file"),
        BigPickerMode::OpenFolder => ("folder-open-symbolic", "Select folder"),
        BigPickerMode::SaveFile => ("document-save-symbolic", "Save file"),
        BigPickerMode::DestinationFolder => ("folder-symbolic", "Select destination"),
    }
}

fn file_entry_row(file_entry: &BigFileEntry, reorderable: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&file_entry.display_name)
        .subtitle(&file_entry.uri)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("text-x-generic-symbolic"));
    if reorderable {
        row.add_suffix(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));
    }
    row
}

fn settings_group(spec: &BigSettingsGroupSpec) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(&spec.title).build();
    for field in &spec.fields {
        group.add(&field_row(field));
    }
    group
}

fn field_row(field: &BigFieldSpec) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(&field.label).build();
    if let Some(text) = field.tooltip.as_deref() {
        row.set_subtitle(text);
    }
    match field.kind {
        BigFieldKind::Switch => row.add_suffix(&gtk::Switch::new()),
        BigFieldKind::Spin => row.add_suffix(&gtk::SpinButton::with_range(0.0, 100.0, 1.0)),
        BigFieldKind::Password => row.add_suffix(&gtk::PasswordEntry::new()),
        BigFieldKind::Text | BigFieldKind::Path => row.add_suffix(&gtk::Entry::new()),
        BigFieldKind::Combo => row.add_suffix(&gtk::DropDown::from_strings(&[])),
        BigFieldKind::Slider => {
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
            scale.set_width_request(160);
            row.add_suffix(&scale);
        }
        BigFieldKind::Color | BigFieldKind::Font => {
            row.add_suffix(&gtk::Button::builder().label("Select").build());
        }
    }
    row
}

fn apply_progress(progress: &gtk::ProgressBar, state: &BigTaskProgress) {
    if let Some(fraction) = state.fraction {
        progress.set_fraction(fraction);
    } else {
        progress.pulse();
    }
    progress.set_text(Some(&state.message));
    progress.set_show_text(true);
}

fn preview_icon_name(kind: BigPreviewKind) -> &'static str {
    match kind {
        BigPreviewKind::Image
        | BigPreviewKind::MiniImageEditor
        | BigPreviewKind::CropResizeRotate => "image-x-generic-symbolic",
        BigPreviewKind::Video => "video-x-generic-symbolic",
        BigPreviewKind::Audio => "audio-x-generic-symbolic",
        BigPreviewKind::Metadata => "dialog-information-symbolic",
        BigPreviewKind::MiniTextEditor => "text-x-generic-symbolic",
    }
}

fn preview_description(spec: &BigPreviewSpec) -> &'static str {
    if spec.read_only { "Preview" } else { "Editor" }
}

fn dialog_icon_name(severity: &str) -> &'static str {
    match severity {
        "error" => "dialog-error-symbolic",
        "warning" => "dialog-warning-symbolic",
        "confirm" => "dialog-question-symbolic",
        _ => "dialog-information-symbolic",
    }
}

/// Build a [`gtk::StringList`] populated with `string_values` in order, ready
/// for use with `gtk::DropDown` or any model-backed list widget.
#[must_use]
pub fn string_list_model(string_values: &[String]) -> gtk::StringList {
    let model = gtk::StringList::new(&[]);
    for string_value in string_values {
        model.append(string_value);
    }
    model
}

/// Build a [`gio::Menu`] from a slice of [`crate::actions::BigActionSpec`]
/// values. Each spec contributes one menu entry labelled with its
/// `label` and bound to its scoped `detailed_name()`.
#[must_use]
pub fn gio_menu_from_actions(actions: &[crate::actions::BigActionSpec]) -> gio::Menu {
    let menu = gio::Menu::new();
    for action in actions {
        menu.append(Some(&action.label), Some(&action.detailed_name()));
    }
    menu
}
