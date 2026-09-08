// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Keyboard shortcut editor dialog shell.

use adw::prelude::*;
use big_app_kit::keybindings::{
    parse_custom, remove_custom_override, serialize_custom, set_custom_override,
};
use relm4::gtk;
use relm4::gtk::gdk;
use std::{cell::RefCell, rc::Rc};

use super::shortcut_capture::{BigShortcutCaptureLabels, show_shortcut_capture_dialog};

const DEFAULT_WIDTH: i32 = 520;
const DEFAULT_HEIGHT: i32 = 580;
const DEFAULT_MAXIMUM_SIZE: i32 = 500;
const DEFAULT_MARGIN: i32 = 12;

/// Grouping header for a set of [`BigShortcutEntry`] rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShortcutCategory {
    /// Id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: Option<String>,
}

impl BigShortcutCategory {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
        }
    }

    /// Configure the `description` setting and return the updated builder.
    ///
    /// The supplied `description` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigShortcutCategory`].
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Single entry inside a big shortcut collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShortcutEntry {
    /// Category id.
    pub category_id: String,
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Action id.
    pub action_id: String,
    /// Default accel.
    pub default_accel: String,
    /// Current label.
    pub current_label: String,
}

impl BigShortcutEntry {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        category_id: impl Into<String>,
        title: impl Into<String>,
        action_id: impl Into<String>,
        default_accel: impl Into<String>,
        current_label: impl Into<String>,
    ) -> Self {
        Self {
            category_id: category_id.into(),
            title: title.into(),
            subtitle: None,
            action_id: action_id.into(),
            default_accel: default_accel.into(),
            current_label: current_label.into(),
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigShortcutEntry`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// Display-free specification describing big shortcut dialog behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShortcutDialogSpec {
    /// Title.
    pub title: String,
    /// Default width.
    pub default_width: i32,
    /// Default height.
    pub default_height: i32,
    /// Maximum size.
    pub maximum_size: i32,
    /// Margin.
    pub margin: i32,
    /// Reset all label.
    pub reset_all_label: Option<String>,
    /// Reset all icon name.
    pub reset_all_icon_name: String,
}

impl BigShortcutDialogSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            default_width: DEFAULT_WIDTH,
            default_height: DEFAULT_HEIGHT,
            maximum_size: DEFAULT_MAXIMUM_SIZE,
            margin: DEFAULT_MARGIN,
            reset_all_label: None,
            reset_all_icon_name: "view-refresh-symbolic".into(),
        }
    }

    /// Configure the `reset_all_label` setting and return the updated builder.
    ///
    /// The supplied `label` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigShortcutDialogSpec`].
    #[must_use]
    pub fn reset_all_label(mut self, label: impl Into<String>) -> Self {
        self.reset_all_label = Some(label.into());
        self
    }

    /// Configure the `reset_all_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigShortcutDialogSpec`].
    #[must_use]
    pub fn reset_all_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.reset_all_icon_name = icon_name.into();
        self
    }
}

/// Handle returned for every realised shortcut row so the host can
/// connect signals and update labels.
#[derive(Debug, Clone)]
pub struct BigShortcutRowHandle {
    /// Action id.
    pub action_id: String,
    /// Default accel.
    pub default_accel: String,
    /// Title.
    pub title: String,
    row: adw::ActionRow,
    key_label: gtk::Label,
}

impl BigShortcutRowHandle {
    /// Return a reference to the `row` exposed by this [`BigShortcutRowHandle`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `key label` exposed by this [`BigShortcutRowHandle`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn key_label(&self) -> &gtk::Label {
        &self.key_label
    }
}

/// Modeless preferences-style window listing every shortcut grouped
/// by category, with per-row reset and capture affordances.
#[derive(Debug, Clone)]
pub struct BigShortcutDialog {
    // bigagents: app-local-window-owner — this handle IS the window's owner
    // (builds, presents, exposes it via `window()`); no signal handler captures
    // it, so there is no window ⇄ handler cycle. Consumers capture weakly.
    window: adw::Window,
    rows: Vec<BigShortcutRowHandle>,
    reset_all_row: Option<adw::ActionRow>,
}

impl BigShortcutDialog {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        spec: BigShortcutDialogSpec,
        categories: &[BigShortcutCategory],
        entries: &[BigShortcutEntry],
    ) -> Self {
        let window = adw::Window::builder()
            .title(&spec.title)
            .modal(false)
            .default_width(spec.default_width)
            .default_height(spec.default_height)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&adw::HeaderBar::new());

        let scroll = gtk::ScrolledWindow::builder().vexpand(true).build();
        let clamp = adw::Clamp::builder()
            .maximum_size(spec.maximum_size)
            .margin_start(spec.margin)
            .margin_end(spec.margin)
            .margin_top(spec.margin)
            .margin_bottom(spec.margin)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();

        let mut rows = Vec::new();
        let reset_all_row = reset_all_row(&spec);
        if let Some(row) = &reset_all_row {
            let group = adw::PreferencesGroup::new();
            group.add(row);
            content.append(&group);
        }
        for category in categories {
            let group = preference_group(category);
            for entry in entries_for_category(entries, &category.id) {
                let (row, key_label) = shortcut_row(entry);
                rows.push(BigShortcutRowHandle {
                    action_id: entry.action_id.clone(),
                    default_accel: entry.default_accel.clone(),
                    title: entry.title.clone(),
                    row: row.clone(),
                    key_label,
                });
                group.add(&row);
            }
            content.append(&group);
        }

        clamp.set_child(Some(&content));
        scroll.set_child(Some(&clamp));
        toolbar_view.set_content(Some(&scroll));
        window.set_content(Some(&toolbar_view));

        Self {
            window,
            rows,
            reset_all_row,
        }
    }

    /// Return a reference to the `window` exposed by this [`BigShortcutDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn window(&self) -> &adw::Window {
        &self.window
    }

    /// Return a reference to the `rows` exposed by this [`BigShortcutDialog`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn rows(&self) -> &[BigShortcutRowHandle] {
        &self.rows
    }

    /// Return the current `reset all row` value held by this [`BigShortcutDialog`].
    #[must_use]
    pub fn reset_all_row(&self) -> Option<&adw::ActionRow> {
        self.reset_all_row.as_ref()
    }

    /// Consume `self` and yield the underlying window.
    #[must_use]
    pub fn into_window(self) -> adw::Window {
        self.window
    }
}

/// Wire every shortcut row to the shared shortcut-capture dialog.
///
/// `big-app-kit` owns the pure accelerator serialization contract, while this
/// component owns the rendered row activation and capture dialog lifecycle.
/// After each reset or rebind, `on_serialized_changed` receives the full
/// serialized custom-override string so apps can persist it in their settings.
pub fn connect_shortcut_dialog_rebinds<FK, FF, FS>(
    dialog: &BigShortcutDialog,
    parent: &impl IsA<gtk::Window>,
    custom_value: &str,
    labels: BigShortcutCaptureLabels,
    key_to_binding: FK,
    format_binding: FF,
    on_serialized_changed: FS,
) where
    FK: Fn(gdk::Key, gdk::ModifierType) -> String + Clone + 'static,
    FF: Fn(&str) -> String + Clone + 'static,
    FS: Fn(String) + Clone + 'static,
{
    let overrides = Rc::new(RefCell::new(parse_custom(custom_value)));
    let parent_weak = parent.as_ref().downgrade();

    for shortcut_row in dialog.rows() {
        let display_name = shortcut_row.title.clone();
        let action_id = shortcut_row.action_id.clone();
        let default_accel = shortcut_row.default_accel.clone();
        let overrides_ref = overrides.clone();
        let key_label = shortcut_row.key_label().clone();
        let labels = labels.clone();
        let key_to_binding = key_to_binding.clone();
        let format_binding = format_binding.clone();
        let on_serialized_changed = on_serialized_changed.clone();
        let parent_weak = parent_weak.clone();

        shortcut_row.row().connect_activated(move |_| {
            let Some(parent) = parent_weak.upgrade() else {
                return;
            };

            let action_id_reset = action_id.clone();
            let action_id_apply = action_id.clone();
            let default_accel_reset = default_accel.clone();
            let default_accel_apply = default_accel.clone();
            let overrides_reset = overrides_ref.clone();
            let overrides_apply = overrides_ref.clone();
            let key_label_reset = key_label.clone();
            let key_label_apply = key_label.clone();
            let format_reset = format_binding.clone();
            let format_apply = format_binding.clone();
            let on_reset_changed = on_serialized_changed.clone();
            let on_apply_changed = on_serialized_changed.clone();

            show_shortcut_capture_dialog(
                &parent,
                &display_name,
                labels.clone(),
                key_to_binding.clone(),
                format_binding.clone(),
                move || {
                    let mut overrides = overrides_reset.borrow_mut();
                    remove_custom_override(&mut overrides, &action_id_reset);
                    key_label_reset.set_label(&format_reset(&default_accel_reset));
                    on_reset_changed(serialize_custom(&overrides));
                },
                move |new_accel| {
                    key_label_apply.set_label(&format_apply(&new_accel));
                    let mut overrides = overrides_apply.borrow_mut();
                    set_custom_override(
                        &mut overrides,
                        action_id_apply.clone(),
                        new_accel,
                        &default_accel_apply,
                    );
                    on_apply_changed(serialize_custom(&overrides));
                },
            );
        });
    }
}

/// Collect the `entries for category` entries derived from the supplied inputs.
#[must_use]
pub fn entries_for_category<'a>(
    entries: &'a [BigShortcutEntry],
    category_id: &str,
) -> Vec<&'a BigShortcutEntry> {
    entries
        .iter()
        .filter(|entry| entry.category_id == category_id)
        .collect()
}

fn preference_group(category: &BigShortcutCategory) -> adw::PreferencesGroup {
    let mut builder = adw::PreferencesGroup::builder().title(&category.title);
    if let Some(description) = &category.description {
        builder = builder.description(description);
    }
    builder.build()
}

fn shortcut_row(entry: &BigShortcutEntry) -> (adw::ActionRow, gtk::Label) {
    let mut row_builder = adw::ActionRow::builder()
        .title(&entry.title)
        .activatable(true);
    if let Some(subtitle) = &entry.subtitle {
        row_builder = row_builder.subtitle(subtitle);
    }
    let row = row_builder.build();

    let key_label = gtk::Label::builder().label(&entry.current_label).build();
    key_label.add_css_class("dim-label");
    key_label.add_css_class("monospace");
    row.add_suffix(&key_label);

    let edit_icon = gtk::Image::from_icon_name("document-edit-symbolic");
    edit_icon.add_css_class("dim-label");
    row.add_suffix(&edit_icon);

    (row, key_label)
}

fn reset_all_row(spec: &BigShortcutDialogSpec) -> Option<adw::ActionRow> {
    let label = spec.reset_all_label.as_ref()?;
    let row = adw::ActionRow::builder()
        .title(label)
        .activatable(true)
        .build();
    let icon = gtk::Image::from_icon_name(&spec.reset_all_icon_name);
    icon.add_css_class("dim-label");
    row.add_suffix(&icon);
    Some(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_for_category_preserves_order() {
        let entries = vec![
            BigShortcutEntry::new("Playback", "Play", "play", "Space", "Space"),
            BigShortcutEntry::new("Audio", "Mute", "mute", "M", "M"),
            BigShortcutEntry::new("Playback", "Next", "next", "N", "N"),
        ];

        let titles = entries_for_category(&entries, "Playback")
            .into_iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>();

        assert_eq!(titles, vec!["Play", "Next"]);
    }

    #[test]
    fn category_description_builder_sets_optional_text() {
        let category = BigShortcutCategory::new("Video", "Video").description("Fullscreen");

        assert_eq!(category.description.as_deref(), Some("Fullscreen"));
    }

    #[test]
    fn entry_subtitle_builder_sets_optional_text() {
        let entry = BigShortcutEntry::new("App", "Quit", "quit", "<Ctrl>q", "Ctrl+Q")
            .subtitle("Close the app");

        assert_eq!(entry.subtitle.as_deref(), Some("Close the app"));
    }

    #[test]
    fn reset_all_label_builder_sets_optional_text() {
        let spec = BigShortcutDialogSpec::new("Shortcuts").reset_all_label("Reset all");

        assert_eq!(spec.reset_all_label.as_deref(), Some("Reset all"));
    }
}
