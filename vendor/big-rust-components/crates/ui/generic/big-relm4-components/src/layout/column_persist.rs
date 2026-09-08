//! Column-view width persistence helpers.
//!
//! Apps own where the encoded string is stored; this module owns the shared
//! `title=width` encode/decode used by BigLinux `gtk::ColumnView` tables, plus
//! the debounced resize→persist wiring. Promoted from the duplicated audio-player
//! chrome (RESTRUCTURE P5).

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

/// Encode visible column titles and fixed widths as `title=width,title=width`.
///
/// Empty titles are skipped because they are not stable persistence keys.
#[must_use]
pub fn encode_column_width_entries<'a>(
    columns: impl IntoIterator<Item = (&'a str, i32)>,
) -> Option<String> {
    let encoded: Vec<String> = columns
        .into_iter()
        .filter(|(title, _)| !title.is_empty())
        .map(|(title, width)| format!("{title}={width}"))
        .collect();

    if encoded.is_empty() {
        None
    } else {
        Some(encoded.join(","))
    }
}

/// Serialize the current fixed widths of a [`gtk::ColumnView`].
///
/// Returns `None` when the view has no titled columns.
#[must_use]
pub fn column_view_widths_string(column_view: &gtk::ColumnView) -> Option<String> {
    let columns = column_view.columns();
    let mut widths = Vec::new();
    for index in 0..columns.n_items() {
        if let Some(column) = columns.item(index).and_downcast::<gtk::ColumnViewColumn>() {
            let title = column
                .title()
                .map(|text| text.to_string())
                .unwrap_or_default();
            widths.push((title, column.fixed_width()));
        }
    }
    encode_column_width_entries(widths.iter().map(|(title, width)| (title.as_str(), *width)))
}

/// Apply a `title=width,…` spec, setting each named column's fixed width.
/// Unknown columns, malformed entries, and non-positive widths are skipped.
/// The inverse of [`column_view_widths_string`].
pub fn apply_column_widths(column_view: &gtk::ColumnView, spec: &str) {
    let columns = column_view.columns();
    for entry in spec.split(',') {
        let Some((name, width_str)) = entry.split_once('=') else {
            continue;
        };
        let Ok(width) = width_str.parse::<i32>() else {
            continue;
        };
        if width <= 0 {
            continue;
        }
        for index in 0..columns.n_items() {
            if let Some(column) = columns.item(index).and_downcast::<gtk::ColumnViewColumn>()
                && column.title().map(|title| title.to_string()).as_deref() == Some(name)
            {
                column.set_fixed_width(width);
                break;
            }
        }
    }
}

/// Persist column widths as the user resizes: every resizable column's
/// `fixed-width` notify is debounced to one idle tick, then `on_persist` receives
/// the freshly [encoded](column_view_widths_string) spec. `applying` is set by the
/// caller while it restores widths, to suppress the echo. No-op for views with no
/// titled/resizable columns.
pub fn wire_column_width_persist(
    column_view: &gtk::ColumnView,
    applying: &Rc<Cell<bool>>,
    on_persist: impl Fn(String) + Clone + 'static,
) {
    let pending = Rc::new(Cell::new(false));
    let columns = column_view.columns();
    for index in 0..columns.n_items() {
        let Some(column) = columns.item(index).and_downcast::<gtk::ColumnViewColumn>() else {
            continue;
        };
        if !column.is_resizable() {
            continue;
        }
        let view = column_view.downgrade();
        let applying = applying.clone();
        let pending = pending.clone();
        let on_persist = on_persist.clone();
        column.connect_fixed_width_notify(move |_| {
            if applying.get() || pending.get() {
                return;
            }
            pending.set(true);
            let view = view.clone();
            let pending = pending.clone();
            let on_persist = on_persist.clone();
            glib::idle_add_local_once(move || {
                pending.set(false);
                if let Some(view) = view.upgrade()
                    && let Some(spec) = column_view_widths_string(&view)
                {
                    on_persist(spec);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_titled_columns_in_order() {
        let encoded =
            encode_column_width_entries([("Name", 240), ("Artist", 180), ("Duration", 72)]);

        assert_eq!(encoded.as_deref(), Some("Name=240,Artist=180,Duration=72"));
    }

    #[test]
    fn skips_empty_titles() {
        let encoded = encode_column_width_entries([("", 80), ("Size", 96)]);

        assert_eq!(encoded.as_deref(), Some("Size=96"));
    }

    #[test]
    fn returns_none_without_stable_titles() {
        let encoded = encode_column_width_entries([("", 80), ("", 96)]);

        assert_eq!(encoded, None);
    }
}
