// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Simple/Advanced disclosure row.
//!
//! A settings idiom: keep the common controls visible and tuck the rarely
//! touched ones behind one consistent "Advanced" expander. Returns a bare
//! [`adw::ExpanderRow`] so the caller adds its advanced rows with
//! [`adw::prelude::ExpanderRowExt::add_row`] and persists the open/closed
//! state via `connect_expanded_notify`.

use adw::prelude::*;

/// CSS class set on every disclosure row, for app-side theming.
pub const CSS_CLASS: &str = "big-advanced-disclosure";

/// Build an "Advanced" disclosure expander. `title`/`subtitle` are
/// caller-provided (and thus translatable in the app's domain); an empty
/// `subtitle` is omitted. `expanded` seeds the open state.
#[must_use]
pub fn advanced_disclosure(title: &str, subtitle: &str, expanded: bool) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(title)
        .expanded(expanded)
        .build();
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    row.add_css_class(CSS_CLASS);
    row
}
