// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Standard BigLinux didactic cards for settings and onboarding UI.

use std::cell::RefCell;
use std::collections::HashMap;

use adw::prelude::*;
use relm4::gtk;

thread_local! {
    // Decoded illustrations are immutable and shareable. Cache one GdkPaintable
    // per resource path so every card (and every dialog re-open, across the
    // process) reuses it instead of re-decoding the SVG on each build — that
    // per-build decode was never freed and leaked MBs per dialog open. Bounded:
    // one entry per distinct illustration in the binary's gresource.
    static ILLUSTRATION_PAINTABLES: RefCell<HashMap<String, gtk::gdk::Paintable>> =
        RefCell::new(HashMap::new());
}

/// DEFAULT DIDACTIC CARD SPACING constant.
pub const DEFAULT_DIDACTIC_CARD_SPACING: i32 = 18;
/// DEFAULT DIDACTIC TEXT SPACING constant.
pub const DEFAULT_DIDACTIC_TEXT_SPACING: i32 = 4;
/// DEFAULT DIDACTIC DESCRIPTION MAX CHARS constant.
pub const DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS: i32 = 76;
/// DEFAULT DIDACTIC PAGE SPACING constant.
pub const DEFAULT_DIDACTIC_PAGE_SPACING: i32 = 20;
/// DEFAULT DIDACTIC PAGE MARGIN TOP constant.
pub const DEFAULT_DIDACTIC_PAGE_MARGIN_TOP: i32 = 16;
/// DEFAULT DIDACTIC PAGE MARGIN BOTTOM constant.
pub const DEFAULT_DIDACTIC_PAGE_MARGIN_BOTTOM: i32 = 24;
/// DEFAULT DIDACTIC PAGE MARGIN SIDES constant.
pub const DEFAULT_DIDACTIC_PAGE_MARGIN_SIDES: i32 = 24;

/// Visual layout flavour selected when realising a
/// [`BigDidacticCardSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigDidacticCardLayout {
    /// Illustration on the left, text + optional control on the right.
    Horizontal,
    /// Illustration spans the full width above the text column.
    FullWidth,
    /// Like `Horizontal` but with no embedded control widget.
    Summary,
    /// Plain card with no illustration at all.
    Plain,
}

/// Display-free specification describing big didactic card behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDidacticCardSpec {
    /// Resource prefix.
    pub resource_prefix: String,
    /// Illustration name.
    pub illustration_name: Option<String>,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Layout.
    pub layout: BigDidacticCardLayout,
    /// Card spacing.
    pub card_spacing: i32,
    /// Text spacing.
    pub text_spacing: i32,
    /// Description max chars.
    pub description_max_chars: i32,
}

impl BigDidacticCardSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        resource_prefix: impl Into<String>,
        illustration_name: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            resource_prefix: resource_prefix.into(),
            illustration_name: Some(illustration_name.into()),
            title: title.into(),
            description: description.into(),
            layout: BigDidacticCardLayout::Horizontal,
            card_spacing: DEFAULT_DIDACTIC_CARD_SPACING,
            text_spacing: DEFAULT_DIDACTIC_TEXT_SPACING,
            description_max_chars: DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS,
        }
    }

    /// Build a plain-layout card with no illustration. Useful for
    /// onboarding flows where art is missing.
    #[must_use]
    pub fn plain(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            resource_prefix: String::new(),
            illustration_name: None,
            title: title.into(),
            description: description.into(),
            layout: BigDidacticCardLayout::Plain,
            card_spacing: DEFAULT_DIDACTIC_CARD_SPACING,
            text_spacing: DEFAULT_DIDACTIC_TEXT_SPACING,
            description_max_chars: DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS,
        }
    }

    /// Switch the layout to `FullWidth` (illustration spans card).
    #[must_use]
    pub fn full_width(mut self) -> Self {
        self.layout = BigDidacticCardLayout::FullWidth;
        self
    }

    /// Switch the layout to `Summary` (no embedded control).
    #[must_use]
    pub fn summary(mut self) -> Self {
        self.layout = BigDidacticCardLayout::Summary;
        self
    }

    /// Override the inter-element spacing values. Negative inputs are
    /// clamped to 0.
    #[must_use]
    pub fn spacing(mut self, card_spacing: i32, text_spacing: i32) -> Self {
        self.card_spacing = card_spacing.max(0);
        self.text_spacing = text_spacing.max(0);
        self
    }

    /// Cap the per-line character count of the description label.
    /// Inputs below 1 are coerced to 1 so the label always wraps.
    #[must_use]
    pub fn description_max_chars(mut self, max_chars: i32) -> Self {
        self.description_max_chars = max_chars.max(1);
        self
    }

    /// Join `resource_prefix` with `illustration_name` into the full
    /// GResource path. Returns `None` when the spec carries no
    /// illustration.
    #[must_use]
    pub fn resource_path(&self) -> Option<String> {
        self.illustration_name
            .as_ref()
            .map(|name| format!("{}/{}", self.resource_prefix.trim_end_matches('/'), name))
    }
}

/// Realised widget wrapper around a [`BigDidacticCardSpec`].
#[derive(Debug, Clone)]
pub struct BigDidacticCard {
    root: gtk::Box,
}

impl BigDidacticCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, control: Option<&impl IsA<gtk::Widget>>) -> Self {
        let root = match spec.layout {
            BigDidacticCardLayout::Horizontal => horizontal_card(&spec, control),
            BigDidacticCardLayout::FullWidth => full_width_card(&spec, control),
            BigDidacticCardLayout::Summary => horizontal_card::<gtk::Widget>(&spec, None),
            BigDidacticCardLayout::Plain => plain_card(&spec, control),
        };
        apply_a11y(&root, &spec.title);
        Self { root }
    }

    /// Return a reference to the `root` exposed by this [`BigDidacticCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

/// Convenience: build a card with an embedded control and return its
/// root box.
#[must_use]
pub fn didactic_card(spec: BigDidacticCardSpec, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    BigDidacticCard::new(spec, Some(control)).into_root()
}

/// Convenience: build a summary-layout card (no embedded control) and
/// return its root box.
#[must_use]
pub fn didactic_summary(spec: BigDidacticCardSpec) -> gtk::Box {
    BigDidacticCard::new(spec.summary(), None::<&gtk::Widget>).into_root()
}

/// Wrap `card` inside a fresh `AdwPreferencesGroup` so it can be added
/// directly to an `AdwPreferencesPage`.
#[must_use]
pub fn didactic_preferences_group(card: &impl IsA<gtk::Widget>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.add(card);
    group
}

/// Stack `cards` under an optional `heading` and `intro` label,
/// applying the didactic-section CSS classes.
#[must_use]
pub fn didactic_section(heading: &str, intro: &str, cards: &[gtk::Widget]) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .build();

    if !heading.is_empty() {
        let heading_label = gtk::Label::builder()
            .label(heading)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["didactic-section-heading"])
            .build();
        section.append(&heading_label);
    }

    if !intro.is_empty() {
        let intro_label = gtk::Label::builder()
            .label(intro)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(80)
            .css_classes(["didactic-section-intro"])
            .build();
        section.append(&intro_label);
    }

    let stack = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .hexpand(true)
        .css_classes(["didactic-stack"])
        .build();
    for card in cards {
        stack.append(card);
    }
    section.append(&stack);

    section
}

/// Wrap `sections` in a scrolled vertical box using the standard
/// didactic-page margins and spacing constants.
#[must_use]
pub fn didactic_page(sections: &[gtk::Widget]) -> gtk::ScrolledWindow {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(DEFAULT_DIDACTIC_PAGE_SPACING)
        .margin_top(DEFAULT_DIDACTIC_PAGE_MARGIN_TOP)
        .margin_bottom(DEFAULT_DIDACTIC_PAGE_MARGIN_BOTTOM)
        .margin_start(DEFAULT_DIDACTIC_PAGE_MARGIN_SIDES)
        .margin_end(DEFAULT_DIDACTIC_PAGE_MARGIN_SIDES)
        .hexpand(true)
        .build();
    for section in sections {
        body.append(section);
    }

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&body)
        .build()
}

fn horizontal_card<T: IsA<gtk::Widget>>(
    spec: &BigDidacticCardSpec,
    control: Option<&T>,
) -> gtk::Box {
    let card = card_root(gtk::Orientation::Horizontal, spec.card_spacing);
    if let Some(picture) = illustration(spec) {
        card.append(&picture);
    }
    card.append(&text_column(spec));
    if let Some(control) = control {
        control.as_ref().set_valign(gtk::Align::Center);
        card.append(control);
    }
    card
}

fn plain_card<T: IsA<gtk::Widget>>(spec: &BigDidacticCardSpec, control: Option<&T>) -> gtk::Box {
    let card = card_root(gtk::Orientation::Horizontal, spec.card_spacing);
    card.append(&text_column(spec));
    if let Some(control) = control {
        control.as_ref().set_valign(gtk::Align::Center);
        card.append(control);
    }
    card
}

fn full_width_card<T: IsA<gtk::Widget>>(
    spec: &BigDidacticCardSpec,
    control: Option<&T>,
) -> gtk::Box {
    let card = card_root(gtk::Orientation::Vertical, spec.text_spacing * 2);
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(spec.card_spacing)
        .build();
    if let Some(picture) = illustration(spec) {
        header.append(&picture);
    }
    header.append(&text_column(spec));
    card.append(&header);

    if let Some(control) = control {
        control.as_ref().set_hexpand(true);
        control.as_ref().set_margin_top(spec.text_spacing * 2);
        card.append(control);
    }

    card
}

fn card_root(orientation: gtk::Orientation, spacing: i32) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(orientation)
        .spacing(spacing)
        .hexpand(true)
        .build();
    card.add_css_class("didactic-card");
    card
}

fn illustration(spec: &BigDidacticCardSpec) -> Option<gtk::Picture> {
    let path = spec.resource_path()?;
    // Reuse a cached, already-decoded paintable when we have one; otherwise
    // decode once via `for_resource` and cache that paintable for next time.
    let cached = ILLUSTRATION_PAINTABLES.with(|c| c.borrow().get(&path).cloned());
    let picture = if let Some(paintable) = cached {
        gtk::Picture::for_paintable(&paintable)
    } else {
        let picture = gtk::Picture::for_resource(&path);
        if let Some(paintable) = picture.paintable() {
            ILLUSTRATION_PAINTABLES.with(|c| {
                c.borrow_mut().insert(path.clone(), paintable);
            });
        }
        picture
    };
    picture.add_css_class("didactic-illustration");
    picture.set_valign(gtk::Align::Center);
    picture.set_accessible_role(gtk::AccessibleRole::Presentation);
    Some(picture)
}

fn text_column(spec: &BigDidacticCardSpec) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(spec.text_spacing)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label(&spec.title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["didactic-title"])
        .build();
    column.append(&title);

    if !spec.description.is_empty() {
        let description = gtk::Label::builder()
            .label(&spec.description)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(spec.description_max_chars)
            .css_classes(["didactic-description"])
            .build();
        column.append(&description);
    }

    column
}

fn apply_a11y(card: &gtk::Box, title: &str) {
    card.set_accessible_role(gtk::AccessibleRole::Group);
    card.update_property(&[gtk::accessible::Property::Label(title)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_path_trims_trailing_slash() {
        let spec = BigDidacticCardSpec::new("/app/illustrations/", "x.svg", "Title", "Body");
        assert_eq!(
            spec.resource_path().as_deref(),
            Some("/app/illustrations/x.svg")
        );
    }

    #[test]
    fn plain_card_has_no_resource_path() {
        let spec = BigDidacticCardSpec::plain("Title", "Body");
        assert_eq!(spec.resource_path(), None);
        assert_eq!(spec.layout, BigDidacticCardLayout::Plain);
    }

    #[test]
    fn spacing_sanitizes_negative_values() {
        let spec = BigDidacticCardSpec::plain("Title", "Body")
            .spacing(-1, -2)
            .description_max_chars(0);
        assert_eq!(spec.card_spacing, 0);
        assert_eq!(spec.text_spacing, 0);
        assert_eq!(spec.description_max_chars, 1);
    }
}
