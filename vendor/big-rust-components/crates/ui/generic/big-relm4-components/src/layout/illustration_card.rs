// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Card layout with illustration, text and app-supplied controls.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use relm4::gtk;

const DEFAULT_PICTURE_WIDTH: i32 = 92;
const DEFAULT_PICTURE_HEIGHT: i32 = 68;
const DEFAULT_OUTER_MARGIN_BOTTOM: i32 = 12;
const DEFAULT_INNER_MARGIN: i32 = 16;
const DEFAULT_LABEL_WIDTH: i32 = 140;

/// Illustration source for a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigIllustrationSource {
    /// GResource path (`/org/example/foo.svg`).
    Resource(String),
    /// Local filesystem path.
    File(PathBuf),
}

/// Data needed to build an illustrated settings card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigIllustrationCardSpec {
    /// Resource prefix.
    pub resource_prefix: String,
    /// Illustration name.
    pub illustration_name: String,
    /// Source.
    pub source: BigIllustrationSource,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Picture width.
    pub picture_width: i32,
    /// Picture height.
    pub picture_height: i32,
    /// Inner margin.
    pub inner_margin: i32,
    /// Outer margin bottom.
    pub outer_margin_bottom: i32,
}

impl BigIllustrationCardSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        resource_prefix: impl Into<String>,
        illustration_name: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let resource_prefix = resource_prefix.into();
        let illustration_name = illustration_name.into();
        let resource_path = format!("{resource_prefix}{illustration_name}");
        Self {
            resource_prefix,
            illustration_name,
            source: BigIllustrationSource::Resource(resource_path),
            title: title.into(),
            description: description.into(),
            picture_width: DEFAULT_PICTURE_WIDTH,
            picture_height: DEFAULT_PICTURE_HEIGHT,
            inner_margin: DEFAULT_INNER_MARGIN,
            outer_margin_bottom: DEFAULT_OUTER_MARGIN_BOTTOM,
        }
    }

    /// Builds a from file value.
    #[must_use]
    pub fn from_file(
        illustration_path: impl Into<PathBuf>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            resource_prefix: String::new(),
            illustration_name: String::new(),
            source: BigIllustrationSource::File(illustration_path.into()),
            title: title.into(),
            description: description.into(),
            picture_width: DEFAULT_PICTURE_WIDTH,
            picture_height: DEFAULT_PICTURE_HEIGHT,
            inner_margin: DEFAULT_INNER_MARGIN,
            outer_margin_bottom: DEFAULT_OUTER_MARGIN_BOTTOM,
        }
    }

    /// Configure the `picture_size` setting and return the updated builder.
    ///
    /// The supplied `width` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustrationCardSpec`].
    #[must_use]
    pub fn picture_size(mut self, width: i32, height: i32) -> Self {
        self.picture_width = width;
        self.picture_height = height;
        self
    }

    /// Configure the `margins` setting and return the updated builder.
    ///
    /// The supplied `inner` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigIllustrationCardSpec`].
    #[must_use]
    pub fn margins(mut self, inner: i32, outer_bottom: i32) -> Self {
        self.inner_margin = inner;
        self.outer_margin_bottom = outer_bottom;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigIllustrationCardResolved {
        BigIllustrationCardResolved {
            source: self.source.clone(),
            resource_path: match &self.source {
                BigIllustrationSource::Resource(resource) => resource.clone(),
                BigIllustrationSource::File(path) => path.to_string_lossy().into_owned(),
            },
            picture_width: self.picture_width.max(1),
            picture_height: self.picture_height.max(1),
            inner_margin: self.inner_margin.max(0),
            outer_margin_bottom: self.outer_margin_bottom.max(0),
        }
    }
}

/// Pure card contract. Safe for no-display tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigIllustrationCardResolved {
    /// Source.
    pub source: BigIllustrationSource,
    /// Resource path.
    pub resource_path: String,
    /// Picture width.
    pub picture_width: i32,
    /// Picture height.
    pub picture_height: i32,
    /// Inner margin.
    pub inner_margin: i32,
    /// Outer margin bottom.
    pub outer_margin_bottom: i32,
}

/// Built card plus slots for app wiring.
#[derive(Debug, Clone)]
pub struct BigIllustrationCard {
    root: gtk::Box,
    body: gtk::Box,
    picture: gtk::Picture,
    title_label: gtk::Label,
    description_label: gtk::Label,
}

impl BigIllustrationCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigIllustrationCardSpec, control: &impl IsA<gtk::Widget>) -> Self {
        Self::new_optional(spec, Some(control.as_ref()))
    }

    /// Creates a new with extra.
    #[must_use]
    pub fn new_with_extra(
        spec: BigIllustrationCardSpec,
        control: &impl IsA<gtk::Widget>,
        extra: Option<&impl IsA<gtk::Widget>>,
    ) -> Self {
        let card = Self::new_optional(spec, Some(control.as_ref()));
        if let Some(extra) = extra {
            card.add_row(&extra_box(extra, DEFAULT_INNER_MARGIN));
        }
        card
    }

    /// Creates a new optional.
    #[must_use]
    pub fn new_optional(spec: BigIllustrationCardSpec, control: Option<&gtk::Widget>) -> Self {
        let resolved = spec.resolved();
        let root = card_root(resolved.outer_margin_bottom);
        let row = card_row(resolved.inner_margin);

        let picture = picture_for_source(&resolved.source);
        apply_picture_size(&picture, resolved.picture_width, resolved.picture_height);
        let picture_slot = picture_slot(resolved.picture_width, resolved.picture_height);
        picture_slot.append(&picture);
        row.append(&picture_slot);

        let (text_box, title_label, description_label) = text_block(&spec.title, &spec.description);
        row.append(&text_box);

        if let Some(control) = control {
            control.set_valign(gtk::Align::Center);
            title_label.set_mnemonic_widget(Some(control));
            row.append(control);
        }
        root.append(&row);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        root.append(&body);

        Self {
            root,
            body,
            picture,
            title_label,
            description_label,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `widget` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `body` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn body(&self) -> &gtk::Box {
        &self.body
    }

    /// Append `row` to the card's body box (used to stack settings
    /// rows below the illustration).
    pub fn add_row(&self, row: &impl IsA<gtk::Widget>) {
        self.body.append(row);
    }

    /// Return a reference to the `picture` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn picture(&self) -> &gtk::Picture {
        &self.picture
    }

    /// Return a reference to the `title label` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn title_label(&self) -> &gtk::Label {
        &self.title_label
    }

    /// Return a reference to the `description label` exposed by this [`BigIllustrationCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn description_label(&self) -> &gtk::Label {
        &self.description_label
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

/// Build a labelled control row: `[label | scale | spin button]`.
#[must_use]
pub fn slider_spin_row(label: &str, scale: &gtk::Scale, spin: &gtk::SpinButton) -> gtk::Box {
    let row = row_root();
    let title = row_label(label);
    row.append(&title);

    scale.set_hexpand(true);
    scale.set_valign(gtk::Align::Center);
    scale.set_draw_value(false);
    title.set_mnemonic_widget(Some(scale));
    row.append(scale);

    spin.set_valign(gtk::Align::Center);
    spin.set_numeric(true);
    spin.set_width_chars(5);
    spin.add_css_class("numeric");
    row.append(spin);
    row
}

/// Build a `[label | flexible space | switch]` row.
#[must_use]
pub fn switch_row(label: &str, switch: &gtk::Switch) -> gtk::Box {
    let row = row_root();
    let title = row_label(label);
    title.set_mnemonic_widget(Some(switch));
    row.append(&title);

    let filler = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .build();
    row.append(&filler);

    switch.set_valign(gtk::Align::Center);
    switch.set_halign(gtk::Align::End);
    row.append(switch);
    row
}

/// Compose a `[label | control]` row for dropdowns, entries, and buttons.
#[must_use]
pub fn labelled_row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = row_root();
    let title = row_label(label);
    title.set_mnemonic_widget(Some(control));
    row.append(&title);

    control.set_hexpand(true);
    row.append(control);
    row
}

/// Horizontal hairline used between logical groups inside a card.
#[must_use]
pub fn group_separator() -> gtk::Separator {
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_start(DEFAULT_INNER_MARGIN);
    sep.set_margin_end(DEFAULT_INNER_MARGIN);
    sep.set_margin_top(8);
    sep.set_margin_bottom(8);
    sep
}

/// Standalone page section header.
#[must_use]
pub fn section_header(title: &str, margin_top: i32) -> gtk::Label {
    gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .margin_top(margin_top)
        .margin_bottom(8)
        .css_classes(vec!["title-3"])
        .build()
}

/// Build a card-sized illustration from a filesystem path.
#[must_use]
pub fn file_illustration(path: &Path) -> gtk::Picture {
    let picture = gtk::Picture::for_filename(path);
    apply_picture_size(&picture, 120, 80);
    picture
}

fn card_root(outer_margin_bottom: i32) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("card");
    root.set_margin_bottom(outer_margin_bottom);
    root
}

fn card_row(inner_margin: i32) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_margin_top(inner_margin);
    row.set_margin_bottom(inner_margin);
    row.set_margin_start(inner_margin);
    row.set_margin_end(inner_margin);
    row
}

fn text_block(title: &str, description: &str) -> (gtk::Box, gtk::Label, gtk::Label) {
    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text_box.set_hexpand(true);
    text_box.set_valign(gtk::Align::Center);

    let title_label = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    title_label.add_css_class("heading");

    let description_label = gtk::Label::builder()
        .label(description)
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .build();
    description_label.add_css_class("dim-label");

    text_box.append(&title_label);
    text_box.append(&description_label);
    (text_box, title_label, description_label)
}

fn row_root() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(DEFAULT_INNER_MARGIN)
        .margin_end(DEFAULT_INNER_MARGIN)
        .margin_bottom(12)
        .build()
}

fn row_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .width_request(DEFAULT_LABEL_WIDTH)
        .use_underline(true)
        .build()
}

fn picture_for_source(source: &BigIllustrationSource) -> gtk::Picture {
    match source {
        BigIllustrationSource::Resource(resource) => gtk::Picture::for_resource(resource),
        BigIllustrationSource::File(path) => gtk::Picture::for_filename(path),
    }
}

fn apply_picture_size(picture: &gtk::Picture, width: i32, height: i32) {
    picture.set_size_request(width, height);
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_halign(gtk::Align::Center);
    picture.set_valign(gtk::Align::Center);
    picture.set_hexpand(false);
    picture.set_vexpand(false);
    picture.set_overflow(gtk::Overflow::Hidden);
}

fn picture_slot(width: i32, height: i32) -> gtk::Box {
    let slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    slot.set_size_request(width, height);
    slot.set_halign(gtk::Align::Center);
    slot.set_valign(gtk::Align::Center);
    slot.set_hexpand(false);
    slot.set_vexpand(false);
    slot.set_overflow(gtk::Overflow::Hidden);
    slot
}

fn extra_box(extra: &impl IsA<gtk::Widget>, inner_margin: i32) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
    box_.set_margin_start(inner_margin);
    box_.set_margin_end(inner_margin);
    box_.set_margin_bottom(inner_margin);
    box_.append(extra.as_ref());
    box_
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_builds_resource_path() {
        let resolved =
            BigIllustrationCardSpec::new("/app/illustrations/", "normalize.svg", "Title", "Body")
                .resolved();

        assert_eq!(resolved.resource_path, "/app/illustrations/normalize.svg");
        assert_eq!(resolved.picture_width, 92);
        assert_eq!(resolved.picture_height, 68);
    }

    #[test]
    fn resolved_sanitizes_sizes_and_margins() {
        let resolved = BigIllustrationCardSpec::new("/", "x.svg", "Title", "Body")
            .picture_size(0, -1)
            .margins(-1, -1)
            .resolved();

        assert_eq!(resolved.picture_width, 1);
        assert_eq!(resolved.picture_height, 1);
        assert_eq!(resolved.inner_margin, 0);
        assert_eq!(resolved.outer_margin_bottom, 0);
    }

    #[test]
    fn file_spec_keeps_source_path() {
        let resolved = BigIllustrationCardSpec::from_file("/tmp/x.svg", "Title", "Body").resolved();

        assert_eq!(
            resolved.source,
            BigIllustrationSource::File(PathBuf::from("/tmp/x.svg"))
        );
        assert_eq!(resolved.resource_path, "/tmp/x.svg");
    }
}
