//! Accessibility helpers for reusable GTK/libadwaita rows and grouped cards.

use adw::{self, prelude::ActionRowExt};
use gtk::glib::object::{Cast, IsA, ObjectExt};
use gtk::prelude::{AccessibleExt, AccessibleExtManual, WidgetExt};

/// Stamp an explicit accessible label on a row-like widget.
///
/// libadwaita rows often install an internal `LABELLED_BY` relation. This
/// helper removes that relation before setting the direct accessible label, and
/// hides duplicate inner switch/check controls from keyboard focus.
pub fn label_row<W: IsA<gtk::Accessible>>(row: &W, title: &str, subtitle: Option<&str>) {
    let label = match subtitle {
        Some(subtitle) if !subtitle.is_empty() => format!("{title}. {subtitle}"),
        _ => title.to_owned(),
    };
    row.reset_relation(gtk::AccessibleRelation::LabelledBy);
    row.update_property(&[gtk::accessible::Property::Label(&label)]);
    if let Some(action_row) = row.dynamic_cast_ref::<adw::ActionRow>()
        && let Some(inner) = action_row.activatable_widget()
    {
        inner.set_focusable(false);
        inner.set_can_focus(false);
    }
    if let Some(widget) = row.dynamic_cast_ref::<gtk::Widget>() {
        defocus_inner_controls(widget);
    }
}

fn defocus_inner_controls(root: &gtk::Widget) {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if widget.is::<gtk::Switch>() || widget.is::<gtk::CheckButton>() {
            widget.set_focusable(false);
            widget.set_can_focus(false);
        } else {
            defocus_inner_controls(&widget);
        }
        child = widget.next_sibling();
    }
}

/// Stamp an accessible label on the row wrapper around a preference group.
///
/// This handles controls whose visible card/group is nested in the row that the
/// accessibility tree exposes.
pub fn label_pref_group_wrapper<W: IsA<gtk::Widget>>(widget: &W, label: &str) {
    let mut current = widget.parent();
    while let Some(current_widget) = current.clone() {
        if current_widget.is::<gtk::ListBox>() {
            return;
        }
        let is_labelled_row = current_widget.clone().downcast::<gtk::ListBoxRow>().is_ok()
            || current_widget.type_().name().ends_with("Row");
        if is_labelled_row {
            current_widget.update_property(&[gtk::accessible::Property::Label(label)]);
        }
        current = current_widget.parent();
    }
}
