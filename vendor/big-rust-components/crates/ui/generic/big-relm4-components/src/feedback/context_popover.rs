// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Plain context popover with flat action rows.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::gdk;
use relm4::gtk::prelude::*;

use crate::feedback::popover_lifecycle::unparent_popover_on_parent_teardown;

use crate::text::escape_markup_text;

/// Data used to build a context-popover action button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigContextActionButtonSpec {
    label: String,
    icon_name: Option<String>,
    icon_pixel_size: i32,
    css_classes: Vec<String>,
}

impl BigContextActionButtonSpec {
    /// Create a context action button spec with the supplied accessible label.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon_name: None,
            icon_pixel_size: 18,
            css_classes: Vec::new(),
        }
    }

    /// Add a themed icon before the label.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Set the icon pixel size. Values below 1 are clamped when the widget is built.
    #[must_use]
    pub fn icon_pixel_size(mut self, icon_pixel_size: i32) -> Self {
        self.icon_pixel_size = icon_pixel_size;
        self
    }

    /// Add an extra CSS class to the button.
    #[must_use]
    pub fn css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_classes.push(css_class.into());
        self
    }

    /// Label exposed visually and through accessibility.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Optional themed icon name.
    #[must_use]
    pub fn icon_name_ref(&self) -> Option<&str> {
        self.icon_name.as_deref()
    }
}

/// Build a standard flat context-popover action button with optional icon.
#[must_use]
pub fn build_context_action_button(spec: &BigContextActionButtonSpec) -> gtk::Button {
    let label_markup = escape_markup_text(&spec.label);
    let label = gtk::Label::builder()
        .label(format!("<span weight=\"normal\">{label_markup}</span>"))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .single_line_mode(true)
        .use_markup(true)
        .xalign(0.0)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .margin_start(8)
        .margin_end(8)
        .margin_top(7)
        .margin_bottom(7)
        .build();

    if let Some(icon_name) = spec.icon_name_ref() {
        let image = gtk::Image::from_icon_name(icon_name);
        image.set_pixel_size(spec.icon_pixel_size.max(1));
        content.append(&image);
    }
    content.append(&label);

    let button = gtk::Button::builder()
        .child(&content)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&spec.label)]);
    button.add_css_class("flat");
    for css_class in &spec.css_classes {
        button.add_css_class(css_class);
    }
    button
}

/// Option displayed by a searchable picker popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchablePickerOption<T> {
    label: String,
    value: T,
    selected: bool,
}

impl<T> BigSearchablePickerOption<T> {
    /// Create an unselected picker option.
    #[must_use]
    pub fn new(label: impl Into<String>, value: T) -> Self {
        Self {
            label: label.into(),
            value,
            selected: false,
        }
    }

    /// Mark whether this option is the current value.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Visible and accessible label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Borrow the option value.
    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Whether this option is currently selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.selected
    }
}

/// Data contract for a button that opens a searchable single-choice popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchablePickerSpec<T> {
    title: String,
    current_label: String,
    filter_placeholder: String,
    options: Vec<BigSearchablePickerOption<T>>,
    popover_css_classes: Vec<String>,
    width_request: i32,
    min_content_height: i32,
    max_content_height: i32,
}

impl<T> BigSearchablePickerSpec<T> {
    /// Build a searchable picker with the default compact popover size.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        current_label: impl Into<String>,
        filter_placeholder: impl Into<String>,
        options: Vec<BigSearchablePickerOption<T>>,
    ) -> Self {
        Self {
            title: title.into(),
            current_label: current_label.into(),
            filter_placeholder: filter_placeholder.into(),
            options,
            popover_css_classes: Vec::new(),
            width_request: 340,
            min_content_height: 120,
            max_content_height: 320,
        }
    }

    /// Add an extra CSS class to the picker popover.
    #[must_use]
    pub fn popover_css_class(mut self, css_class: impl Into<String>) -> Self {
        self.popover_css_classes.push(css_class.into());
        self
    }

    /// Override the popover width request in pixels.
    #[must_use]
    pub fn width_request(mut self, width_request: i32) -> Self {
        self.width_request = width_request.max(1);
        self
    }

    /// Override the scroller height bounds in pixels.
    #[must_use]
    pub fn content_height(mut self, min_content_height: i32, max_content_height: i32) -> Self {
        self.min_content_height = min_content_height.max(1);
        self.max_content_height = max_content_height.max(self.min_content_height);
        self
    }

    /// Picker title and button accessible label.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Button label for the currently selected value.
    #[must_use]
    pub fn current_label(&self) -> &str {
        &self.current_label
    }

    /// Search field placeholder text.
    #[must_use]
    pub fn filter_placeholder(&self) -> &str {
        &self.filter_placeholder
    }

    /// Option list in display order.
    #[must_use]
    pub fn options(&self) -> &[BigSearchablePickerOption<T>] {
        &self.options
    }
}

/// Build a flat button that opens a searchable single-choice popover.
///
/// The caller owns the option values and selection side effect. The shared
/// helper owns the button/popover structure, filtering, current-selection
/// indicator, accessible names, and popover teardown.
#[must_use]
pub fn build_searchable_picker_button<T, F>(
    spec: BigSearchablePickerSpec<T>,
    on_select: F,
) -> gtk::Button
where
    T: Clone + 'static,
    F: Fn(T) + 'static,
{
    let button = gtk::Button::with_label(spec.current_label());
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.update_property(&[gtk::accessible::Property::Label(spec.title())]);

    let picker = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    picker.add_css_class("menu");
    for css_class in &spec.popover_css_classes {
        picker.add_css_class(css_class);
    }
    picker.set_parent(&button);
    unparent_popover_on_parent_teardown(button.upcast_ref(), &picker);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_width_request(spec.width_request);

    root.append(&build_popover_section_label(spec.title()));

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(spec.filter_placeholder()));
    search.set_hexpand(true);
    search.update_property(&[gtk::accessible::Property::Label(spec.filter_placeholder())]);
    root.append(&search);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let rows: Rc<RefCell<Vec<(String, gtk::Button)>>> = Rc::new(RefCell::new(Vec::new()));
    let on_select: Rc<dyn Fn(T)> = Rc::new(on_select);
    let picker_weak = picker.downgrade();
    for option in spec.options {
        let row = picker_option_row(option.label(), option.is_selected());
        let filter_label = option.label().to_lowercase();
        let value = option.value().clone();
        let on_select = on_select.clone();
        let picker_weak = picker_weak.clone();
        row.connect_clicked(move |_| {
            on_select(value.clone());
            if let Some(picker) = picker_weak.upgrade() {
                picker.popdown();
            }
        });
        rows.borrow_mut().push((filter_label, row.clone()));
        list.append(&row);
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .max_content_height(spec.max_content_height)
        .min_content_height(spec.min_content_height)
        .child(&list)
        .build();
    root.append(&scroller);
    picker.set_child(Some(&root));

    let rows_for_search = rows.clone();
    search.connect_search_changed(move |entry| {
        let query = entry.text().to_lowercase();
        for (label, row) in rows_for_search.borrow().iter() {
            row.set_visible(query.is_empty() || label.contains(&query));
        }
    });

    let search_weak = search.downgrade();
    picker.connect_show(move |_| {
        if let Some(search) = search_weak.upgrade() {
            search.set_text("");
            search.grab_focus();
        }
    });

    let picker_weak = picker.downgrade();
    button.connect_clicked(move |_| {
        if let Some(picker) = picker_weak.upgrade() {
            picker.popup();
        }
    });

    button
}

/// Build a standard section label for compact popover content.
#[must_use]
pub fn build_popover_section_label(label: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(label));
    label.add_css_class("caption-heading");
    label.add_css_class("dim-label");
    label.set_xalign(0.0);
    label.set_margin_start(8);
    label.set_margin_end(8);
    label
}

fn picker_option_row(label: &str, selected: bool) -> gtk::Button {
    let row = gtk::Button::new();
    row.add_css_class("flat");
    row.set_halign(gtk::Align::Fill);
    row.set_hexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_halign(gtk::Align::Fill);
    content.set_hexpand(true);

    let image = gtk::Image::from_icon_name("object-select-symbolic");
    image.set_pixel_size(16);
    image.set_opacity(if selected { 1.0 } else { 0.0 });
    let label_widget = gtk::Label::new(Some(label));
    label_widget.set_xalign(0.0);
    label_widget.set_hexpand(true);

    content.append(&image);
    content.append(&label_widget);
    row.set_child(Some(&content));
    row.update_property(&[gtk::accessible::Property::Label(label)]);
    row
}

/// Right-click / long-press popover that hosts a vertical stack of
/// flat action buttons. Cheap to build, self-unparenting on close.
#[derive(Debug, Clone)]
pub struct BigContextPopover {
    popover: gtk::Popover,
    content: gtk::Box,
}

impl BigContextPopover {
    /// Creates a new instance.
    #[must_use]
    pub fn new(has_arrow: bool) -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(has_arrow)
            .autohide(true)
            .build();

        Self { popover, content }
    }

    /// Borrow the underlying [`gtk::Popover`].
    /// The wrapper keeps ownership; use the borrow to attach signals.
    #[must_use]
    pub fn popover(&self) -> &gtk::Popover {
        &self.popover
    }

    /// Borrow the content box where action buttons are stacked.
    /// The wrapper keeps ownership; use the borrow to attach signals.
    #[must_use]
    pub fn content(&self) -> &gtk::Box {
        &self.content
    }

    /// Append a horizontal separator between two groups of buttons.
    pub fn append_separator(&self) {
        self.content
            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }

    /// Append an action button. The popover popdowns before
    /// `on_clicked` fires so handlers can launch dialogs without the
    /// popover stealing back focus.
    pub fn append_button<F>(&self, label: impl AsRef<str>, css_classes: &[&str], on_clicked: F)
    where
        F: Fn() + 'static,
    {
        let label = label.as_ref();
        let row_label_markup = escape_markup_text(label);
        let row_label = gtk::Label::builder()
            .label(format!("<span weight=\"normal\">{row_label_markup}</span>"))
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .single_line_mode(true)
            .use_markup(true)
            .xalign(0.0)
            .build();
        let button = gtk::Button::builder()
            .child(&row_label)
            .hexpand(true)
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.add_css_class("flat");
        for css_class in css_classes {
            button.add_css_class(css_class);
        }

        let popover = self.popover.downgrade();
        button.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            on_clicked();
        });
        self.content.append(&button);
    }

    /// Append an icon + label action button.
    pub fn append_icon_button<F>(
        &self,
        icon_name: impl AsRef<str>,
        label: impl AsRef<str>,
        css_classes: &[&str],
        on_clicked: F,
    ) where
        F: Fn() + 'static,
    {
        let mut spec =
            BigContextActionButtonSpec::new(label.as_ref()).icon_name(icon_name.as_ref());
        for css_class in css_classes {
            spec = spec.css_class(*css_class);
        }
        let button = build_context_action_button(&spec);
        let popover = self.popover.downgrade();
        button.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            on_clicked();
        });
        self.content.append(&button);
    }

    /// Attach the popover to `parent` and pop it up pointing at the
    /// `(x, y)` coordinate inside the parent.
    pub fn popup_at(self, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        self.popover.set_parent(parent.as_ref());
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

        self.popup_with_auto_unparent();
    }

    /// Attach the popover to `parent` and pop it up using
    /// `position` (Top/Bottom/Left/Right) relative to the widget.
    pub fn popup_for_widget(self, parent: &impl IsA<gtk::Widget>, position: gtk::PositionType) {
        self.popover.set_parent(parent.as_ref());
        self.popover.set_position(position);
        self.popup_with_auto_unparent();
    }

    fn popup_with_auto_unparent(self) {
        auto_unparent_on_close(&self.popover);
        self.popover.popup();
    }
}

/// Make a custom-parented popover clean itself up when closed.
///
/// GTK4 does NOT unparent a popover attached via `set_parent` when its parent
/// widget is disposed: the parent finalizes "with children left" and the live
/// popover pins its whole child subtree (measured leak class — a per-click
/// context menu accumulates one popover per use, forever). Call this right
/// after `set_parent` on any transient popover; popovers owned by a struct
/// should instead unparent in that owner's `Drop`.
pub fn auto_unparent_on_close(popover: &impl IsA<gtk::Popover>) {
    // Use the signal's own `&Popover` arg — capturing a strong clone of the
    // popover in its OWN `closed` handler is a self-cycle (popover ⇄ handler)
    // that prevents it from ever finalizing. Unparent SYNCHRONOUSLY: while
    // `closed` emits, the parent is guaranteed alive; deferring to idle opens
    // a race where the parent finalizes first and the late unparent reads a
    // dangling parent pointer (use-after-free → segfault).
    popover.as_ref().connect_closed(move |popover| {
        if popover.parent().is_some() {
            popover.unparent();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{BigContextActionButtonSpec, BigSearchablePickerOption, BigSearchablePickerSpec};

    #[test]
    fn context_action_button_spec_records_icon_and_label() {
        let spec = BigContextActionButtonSpec::new("Open file")
            .icon_name("document-open-symbolic")
            .icon_pixel_size(20)
            .css_class("destructive-action");

        assert_eq!(spec.label(), "Open file");
        assert_eq!(spec.icon_name_ref(), Some("document-open-symbolic"));
        assert_eq!(spec.icon_pixel_size, 20);
        assert_eq!(spec.css_classes, ["destructive-action"]);
    }

    #[test]
    fn searchable_picker_spec_records_options_and_filter_contract() {
        let spec = BigSearchablePickerSpec::new(
            "Encoding",
            "UTF-8",
            "Filter Encoding",
            vec![
                BigSearchablePickerOption::new("UTF-8", 0).selected(true),
                BigSearchablePickerOption::new("ISO-8859-1", 1),
            ],
        )
        .popover_css_class("product-popover")
        .width_request(420)
        .content_height(80, 240);

        assert_eq!(spec.title(), "Encoding");
        assert_eq!(spec.current_label(), "UTF-8");
        assert_eq!(spec.filter_placeholder(), "Filter Encoding");
        assert_eq!(spec.popover_css_classes, ["product-popover"]);
        assert_eq!(spec.options().len(), 2);
        assert!(spec.options()[0].is_selected());
        assert_eq!(spec.options()[1].label(), "ISO-8859-1");
        assert_eq!(*spec.options()[1].value(), 1);
        assert_eq!(spec.width_request, 420);
        assert_eq!(spec.min_content_height, 80);
        assert_eq!(spec.max_content_height, 240);
    }

    #[test]
    fn searchable_picker_spec_clamps_invalid_size_contract() {
        let spec = BigSearchablePickerSpec::new(
            "Syntax",
            "Auto",
            "Filter Syntax",
            Vec::<BigSearchablePickerOption<&str>>::new(),
        )
        .width_request(0)
        .content_height(0, 0);

        assert_eq!(spec.width_request, 1);
        assert_eq!(spec.min_content_height, 1);
        assert_eq!(spec.max_content_height, 1);
    }
}
