// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared sleep timer dialog pages.

use adw::prelude::*;
use relm4::gtk;
use std::{cell::Cell, rc::Rc};

const DEFAULT_MINUTES: i32 = 30;
const MIN_MINUTES: f64 = 1.0;
const MAX_MINUTES: f64 = 480.0;
const STEP_MINUTES: f64 = 5.0;
const PAGE_STEP_MINUTES: f64 = 15.0;

/// BIG SLEEP ACTION PAUSE constant.
pub const BIG_SLEEP_ACTION_PAUSE: &str = "pause";
/// BIG SLEEP ACTION QUIT constant.
pub const BIG_SLEEP_ACTION_QUIT: &str = "quit";
/// BIG SLEEP ACTION SUSPEND constant.
pub const BIG_SLEEP_ACTION_SUSPEND: &str = "suspend";
/// BIG SLEEP ACTION POWEROFF constant.
pub const BIG_SLEEP_ACTION_POWEROFF: &str = "poweroff";

/// Action shown in the sleep timer action combo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSleepTimerAction {
    /// Id.
    pub id: String,
    /// Label.
    pub label: String,
}

impl BigSleepTimerAction {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Collect the `standard sleep timer actions` entries derived from the supplied inputs.
#[must_use]
pub fn standard_sleep_timer_actions(
    pause_label: impl Into<String>,
    quit_label: impl Into<String>,
    suspend_label: impl Into<String>,
    poweroff_label: impl Into<String>,
) -> Vec<BigSleepTimerAction> {
    vec![
        BigSleepTimerAction::new(BIG_SLEEP_ACTION_PAUSE, pause_label),
        BigSleepTimerAction::new(BIG_SLEEP_ACTION_QUIT, quit_label),
        BigSleepTimerAction::new(BIG_SLEEP_ACTION_SUSPEND, suspend_label),
        BigSleepTimerAction::new(BIG_SLEEP_ACTION_POWEROFF, poweroff_label),
    ]
}

/// Text used by the sleep timer setup page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSleepTimerSetupLabels {
    /// Description.
    pub description: String,
    /// Duration group.
    pub duration_group: String,
    /// Minutes title.
    pub minutes_title: String,
    /// Minutes subtitle.
    pub minutes_subtitle: String,
    /// Action group.
    pub action_group: String,
    /// Action title.
    pub action_title: String,
    /// Action subtitle.
    pub action_subtitle: String,
    /// Start button.
    pub start_button: String,
}

/// Text used by the active timer page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSleepTimerActiveLabels {
    /// Title.
    pub title: String,
    /// Cancel button.
    pub cancel_button: String,
}

/// Data needed to build the setup page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSleepTimerSetupSpec {
    /// Labels.
    pub labels: BigSleepTimerSetupLabels,
    /// Actions.
    pub actions: Vec<BigSleepTimerAction>,
    /// Saved minutes.
    pub saved_minutes: i32,
    /// Saved action id.
    pub saved_action_id: String,
    /// Icon size.
    pub icon_size: i32,
}

impl BigSleepTimerSetupSpec {
    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSleepTimerSetupResolved {
        let minutes = if self.saved_minutes > 0 {
            self.saved_minutes
        } else {
            DEFAULT_MINUTES
        };
        let selected_action = self
            .actions
            .iter()
            .position(|action| action.id == self.saved_action_id)
            .unwrap_or(0) as u32;

        BigSleepTimerSetupResolved {
            minutes,
            selected_action,
            icon_size: self.icon_size.max(1),
        }
    }
}

/// Pure setup contract. Safe for no-display tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigSleepTimerSetupResolved {
    /// Minutes.
    pub minutes: i32,
    /// Selected action.
    pub selected_action: u32,
    /// Icon size.
    pub icon_size: i32,
}

/// Built setup page plus app wiring handles.
#[derive(Debug, Clone)]
pub struct BigSleepTimerSetupPage {
    page: adw::PreferencesPage,
    spin: adw::SpinRow,
    action_combo: adw::ComboRow,
    start_button: gtk::Button,
    actions: Vec<BigSleepTimerAction>,
}

impl BigSleepTimerSetupPage {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        spec: BigSleepTimerSetupSpec,
        header_icon: Option<gtk::Widget>,
        spin_prefix: Option<gtk::Widget>,
        action_prefix: Option<gtk::Widget>,
    ) -> Self {
        let resolved = spec.resolved();
        let page = adw::PreferencesPage::new();

        page.add(&header_group(
            header_icon,
            resolved.icon_size,
            None,
            Some(&spec.labels.description),
        ));

        let timer_group = adw::PreferencesGroup::builder()
            .title(&spec.labels.duration_group)
            .build();
        let spin = adw::SpinRow::builder()
            .title(&spec.labels.minutes_title)
            .subtitle(&spec.labels.minutes_subtitle)
            .adjustment(&gtk::Adjustment::new(
                f64::from(resolved.minutes),
                MIN_MINUTES,
                MAX_MINUTES,
                STEP_MINUTES,
                PAGE_STEP_MINUTES,
                0.0,
            ))
            .build();
        if let Some(prefix) = spin_prefix {
            spin.add_prefix(&prefix);
        }
        timer_group.add(&spin);
        page.add(&timer_group);

        let action_group = adw::PreferencesGroup::builder()
            .title(&spec.labels.action_group)
            .build();
        let action_labels = spec
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>();
        let action_model = gtk::StringList::new(&action_labels);
        let action_combo = adw::ComboRow::builder()
            .title(&spec.labels.action_title)
            .subtitle(&spec.labels.action_subtitle)
            .model(&action_model)
            .build();
        if let Some(prefix) = action_prefix {
            action_combo.add_prefix(&prefix);
        }
        action_combo.set_selected(resolved.selected_action);
        action_group.add(&action_combo);
        page.add(&action_group);

        let button_group = adw::PreferencesGroup::new();
        let start_button = gtk::Button::builder()
            .label(&spec.labels.start_button)
            .css_classes(vec!["suggested-action".to_string(), "pill".to_string()])
            .halign(gtk::Align::Center)
            .margin_top(12)
            .build();
        button_group.add(&start_button);
        page.add(&button_group);

        Self {
            page,
            spin,
            action_combo,
            start_button,
            actions: spec.actions,
        }
    }

    /// Return a reference to the `page` exposed by this [`BigSleepTimerSetupPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn page(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Return a reference to the `spin` exposed by this [`BigSleepTimerSetupPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spin(&self) -> &adw::SpinRow {
        &self.spin
    }

    /// Return a reference to the `action combo` exposed by this [`BigSleepTimerSetupPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn action_combo(&self) -> &adw::ComboRow {
        &self.action_combo
    }

    /// Return a reference to the `start button` exposed by this [`BigSleepTimerSetupPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn start_button(&self) -> &gtk::Button {
        &self.start_button
    }

    /// Return a reference to the `selected action id` exposed by this [`BigSleepTimerSetupPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn selected_action_id(&self) -> String {
        let index = self.action_combo.selected() as usize;
        self.actions
            .get(index)
            .or_else(|| self.actions.first())
            .map_or_else(String::new, |action| action.id.clone())
    }

    /// Consume `self` and yield the underlying page.
    #[must_use]
    pub fn into_page(self) -> adw::PreferencesPage {
        self.page
    }
}

/// Data needed to build the active timer page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSleepTimerActiveSpec {
    /// Labels.
    pub labels: BigSleepTimerActiveLabels,
    /// Remaining seconds.
    pub remaining_seconds: u32,
    /// Icon size.
    pub icon_size: i32,
}

/// Built active page plus app wiring handles.
#[derive(Debug, Clone)]
pub struct BigSleepTimerActivePage {
    page: adw::PreferencesPage,
    countdown: gtk::Label,
    cancel_button: gtk::Button,
}

impl BigSleepTimerActivePage {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSleepTimerActiveSpec, header_icon: Option<gtk::Widget>) -> Self {
        let page = adw::PreferencesPage::new();
        let countdown = gtk::Label::builder()
            .label(format_duration(spec.remaining_seconds))
            .css_classes(vec!["title-1".to_string()])
            .build();

        page.add(&header_group(
            header_icon,
            spec.icon_size.max(1),
            Some(&spec.labels.title),
            None,
        ));

        let header_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .build();
        header_box.append(&countdown);
        let countdown_group = adw::PreferencesGroup::new();
        countdown_group.add(&header_box);
        page.add(&countdown_group);

        let cancel_group = adw::PreferencesGroup::new();
        let cancel_button = gtk::Button::builder()
            .label(&spec.labels.cancel_button)
            .css_classes(vec!["destructive-action".to_string(), "pill".to_string()])
            .halign(gtk::Align::Center)
            .margin_top(16)
            .build();
        cancel_group.add(&cancel_button);
        page.add(&cancel_group);

        Self {
            page,
            countdown,
            cancel_button,
        }
    }

    /// Animate the countdown label down to 0 from `remaining_seconds`,
    /// ticking once per second. The timer auto-stops when the label is
    /// dropped or the count reaches 0.
    pub fn start_preview_countdown(&self, remaining_seconds: u32) {
        let remaining = Rc::new(Cell::new(remaining_seconds));
        let label_weak = self.countdown.downgrade();
        let remaining_ref = Rc::clone(&remaining);
        gtk::glib::timeout_add_seconds_local(1, move || {
            let Some(label) = label_weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            let current = remaining_ref.get();
            if current == 0 {
                return gtk::glib::ControlFlow::Break;
            }
            let next = current.saturating_sub(1);
            remaining_ref.set(next);
            label.set_label(&format_duration(next));
            gtk::glib::ControlFlow::Continue
        });
    }

    /// Return a reference to the `page` exposed by this [`BigSleepTimerActivePage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn page(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Return a reference to the `cancel button` exposed by this [`BigSleepTimerActivePage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn cancel_button(&self) -> &gtk::Button {
        &self.cancel_button
    }

    /// Consume `self` and yield the underlying page.
    #[must_use]
    pub fn into_page(self) -> adw::PreferencesPage {
        self.page
    }
}

/// Wire the setup page's "Start" button so clicking it reads the spin
/// value, calls `start_timer(window, minutes, action_id)`, and closes
/// `dialog`.
pub fn connect_sleep_timer_start<W, D>(
    setup_page: &BigSleepTimerSetupPage,
    dialog: &D,
    window: &W,
    start_timer: impl Fn(&W, u32, String) + 'static,
) where
    W: gtk::glib::object::ObjectType,
    D: IsA<gtk::Window> + gtk::glib::object::ObjectType,
{
    let dialog_ref = dialog.downgrade();
    connect_sleep_timer_start_with_close(setup_page, window, start_timer, move || {
        if let Some(dialog) = dialog_ref.upgrade() {
            dialog.close();
        }
    });
}

/// Wire the setup page's "Start" button with an explicit close action.
///
/// Use this variant for dialog surfaces that are not `gtk::Window`, such as
/// `adw::Dialog`.
pub fn connect_sleep_timer_start_with_close<W>(
    setup_page: &BigSleepTimerSetupPage,
    window: &W,
    start_timer: impl Fn(&W, u32, String) + 'static,
    close_dialog: impl Fn() + 'static,
) where
    W: gtk::glib::object::ObjectType,
{
    let spin = setup_page.spin().clone();
    let setup_ref = setup_page.clone();
    let window_ref = window.downgrade();
    setup_page.start_button().connect_clicked(move |_| {
        if let Some(window) = window_ref.upgrade() {
            let minutes = spin.value() as u32;
            let action = setup_ref.selected_action_id();
            start_timer(&window, minutes, action);
        }
        close_dialog();
    });
}

/// Format `seconds` as `MM:SS` (zero-padded), used by the countdown
/// label.
pub use crate::time::countdown_mm_ss as format_duration;

fn header_group(
    icon: Option<gtk::Widget>,
    icon_size: i32,
    title: Option<&str>,
    description: Option<&str>,
) -> adw::PreferencesGroup {
    let header_group = adw::PreferencesGroup::new();
    let header_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .halign(gtk::Align::Center)
        .margin_top(8)
        .margin_bottom(4)
        .build();

    if let Some(icon) = icon {
        if let Ok(image) = icon.clone().downcast::<gtk::Image>() {
            image.set_pixel_size(icon_size);
            header_box.append(&image);
        } else {
            header_box.append(&icon);
        }
    }

    if let Some(title) = title {
        let title_label = gtk::Label::builder()
            .label(title)
            .css_classes(vec!["title-2".to_string()])
            .build();
        header_box.append(&title_label);
    }

    if let Some(description) = description {
        let desc = gtk::Label::builder()
            .label(description)
            .css_classes(vec!["caption".to_string()])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .max_width_chars(50)
            .build();
        header_box.append(&desc);
    }

    header_group.add(&header_box);
    header_group
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> BigSleepTimerSetupLabels {
        BigSleepTimerSetupLabels {
            description: "Set a timer".into(),
            duration_group: "Duration".into(),
            minutes_title: "Minutes".into(),
            minutes_subtitle: "How long".into(),
            action_group: "Action".into(),
            action_title: "When timer ends".into(),
            action_subtitle: "Choose action".into(),
            start_button: "Start".into(),
        }
    }

    #[test]
    fn format_duration_uses_minute_second_pairs() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(65), "01:05");
    }

    #[test]
    fn setup_resolved_defaults_invalid_minutes() {
        let spec = BigSleepTimerSetupSpec {
            labels: labels(),
            actions: vec![BigSleepTimerAction::new("pause", "Pause")],
            saved_minutes: 0,
            saved_action_id: "missing".into(),
            icon_size: 0,
        };

        assert_eq!(
            spec.resolved(),
            BigSleepTimerSetupResolved {
                minutes: DEFAULT_MINUTES,
                selected_action: 0,
                icon_size: 1,
            }
        );
    }

    #[test]
    fn setup_resolved_selects_matching_action() {
        let spec = BigSleepTimerSetupSpec {
            labels: labels(),
            actions: vec![
                BigSleepTimerAction::new("pause", "Pause"),
                BigSleepTimerAction::new("quit", "Quit"),
            ],
            saved_minutes: 45,
            saved_action_id: "quit".into(),
            icon_size: 64,
        };

        assert_eq!(spec.resolved().selected_action, 1);
        assert_eq!(spec.resolved().minutes, 45);
    }

    #[test]
    fn standard_actions_keep_stable_ids() {
        let actions = standard_sleep_timer_actions("Pause", "Quit", "Suspend", "Power off");
        let ids = actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                BIG_SLEEP_ACTION_PAUSE,
                BIG_SLEEP_ACTION_QUIT,
                BIG_SLEEP_ACTION_SUSPEND,
                BIG_SLEEP_ACTION_POWEROFF,
            ]
        );
    }
}
