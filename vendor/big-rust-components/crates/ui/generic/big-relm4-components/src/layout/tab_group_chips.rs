// SPDX-License-Identifier: MIT

//! Opt-in tab-group chip overlay for [`BigTabStrip`](super::tab_strip_host::BigTabStrip).
//!
//! The strip remains group-agnostic. Apps that own group runtime state can
//! build chips from [`BigTabGroupChipSpec`], insert them into the strip's
//! existing tab bar before each group's first tab, and keep persistence,
//! menus, collapse decisions, and tab membership in app code.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use gtk::pango::EllipsizeMode;
use gtk::{
    Align, Box as GtkBox, Button, GestureClick, Image, Label, Orientation, PropagationPhase, Widget,
};
use relm4::gtk;

use crate::feedback::tooltip;
use crate::i18n::{gettext_noop, t};
use crate::layout::tab_strip_host::BigTabPage;

/// CSS class applied to group chip buttons.
pub const CSS_CLASS_TAB_GROUP_CHIP: &str = "tab-group-chip";
/// CSS class applied to collapsed group chip buttons.
pub const CSS_CLASS_COLLAPSED: &str = "collapsed";
/// CSS class applied to chips whose group has no visible name.
pub const CSS_CLASS_UNNAMED: &str = "unnamed";
/// CSS class applied to the text label inside a chip.
pub const CSS_CLASS_GROUP_NAME_LABEL: &str = "group-name-label";
/// CSS class applied to the expand/collapse icon inside a chip.
pub const CSS_CLASS_GROUP_TOGGLE_ICON: &str = "group-toggle-icon";
/// CSS class applied to tab buttons that belong to a group.
pub const CSS_CLASS_IN_GROUP: &str = "in-group";
/// CSS class applied by callers when the active tab is inside this group.
pub const CSS_CLASS_ACTIVE: &str = "active";

const ICON_EXPANDED: &str = "pan-down-symbolic";
const ICON_COLLAPSED: &str = "pan-end-symbolic";
const GROUP_LABEL_WIDTH_CHARS: i32 = 18;
const ON_LIGHT: &str = "#000000";
const ON_DARK: &str = "#FFFFFF";
const FALLBACK_TEXT: &str = "#000000";

const MSG_CLICK_TO_EXPAND: &str = gettext_noop("Click to expand {name}");
const MSG_CLICK_TO_COLLAPSE: &str = gettext_noop("Click to collapse {name}");
const MSG_GROUP_COLLAPSED: &str = gettext_noop("Tab group {name}, collapsed");
const MSG_GROUP_EXPANDED: &str = gettext_noop("Tab group {name}, expanded");

type ToggleRequestedCallback = Rc<dyn Fn()>;
type ContextMenuRequestedCallback = Rc<dyn Fn(&Button, f64, f64)>;

/// Display-free group chip state projected from an app's group runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTabGroupChipSpec {
    /// User-visible group name. Empty names render as just the tab count.
    pub name: String,
    /// Group color as a CSS color string, normally `#rrggbb`.
    pub color: String,
    /// Whether the app currently treats this group as collapsed.
    pub collapsed: bool,
    /// Number of tabs currently in the group.
    pub tab_count: usize,
}

impl BigTabGroupChipSpec {
    /// Build a chip spec from explicit group projection fields.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        color: impl Into<String>,
        collapsed: bool,
        tab_count: usize,
    ) -> Self {
        Self {
            name: name.into(),
            color: color.into(),
            collapsed,
            tab_count,
        }
    }

    /// Build a chip spec from any app-local snapshot adapter.
    #[must_use]
    pub fn from_snapshot(snapshot: &impl BigTabGroupChipSnapshot) -> Self {
        Self::new(
            snapshot.group_name(),
            snapshot.group_color(),
            snapshot.is_group_collapsed(),
            snapshot.group_tab_count(),
        )
    }

    /// Visible chip label, matching the terminal shape: `A (2)` or `(2)`.
    #[must_use]
    pub fn display_label(&self) -> String {
        chip_display_label(&self.name, self.tab_count)
    }

    /// Accessible chip label naming the group and expanded/collapsed state.
    #[must_use]
    pub fn accessible_label(&self) -> String {
        chip_accessible_label(&self.display_label(), self.collapsed)
    }

    /// Tooltip text describing the immediate click action.
    #[must_use]
    pub fn tooltip_label(&self) -> String {
        chip_tooltip_label(&self.display_label(), self.collapsed)
    }

    /// Sanitized CSS class derived from [`Self::color`].
    #[must_use]
    pub fn color_class(&self) -> String {
        tab_group_color_class(&self.color)
    }
}

/// Adapter trait for app-local group snapshots.
pub trait BigTabGroupChipSnapshot {
    /// Group name.
    fn group_name(&self) -> &str;
    /// Group color as a CSS color string.
    fn group_color(&self) -> &str;
    /// Whether the group is collapsed.
    fn is_group_collapsed(&self) -> bool;
    /// Number of tabs in the group.
    fn group_tab_count(&self) -> usize;
}

/// Optional event hooks emitted by a group chip.
#[derive(Clone, Default)]
pub struct BigTabGroupChipCallbacks {
    toggle_requested: Option<ToggleRequestedCallback>,
    context_menu_requested: Option<ContextMenuRequestedCallback>,
}

impl BigTabGroupChipCallbacks {
    /// Create callbacks with no installed hooks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `callback` when the chip is primary-clicked.
    #[must_use]
    pub fn on_toggle_requested(mut self, callback: impl Fn() + 'static) -> Self {
        self.toggle_requested = Some(Rc::new(callback));
        self
    }

    /// Run `callback` when the chip receives a secondary-click context-menu request.
    #[must_use]
    pub fn on_context_menu_requested(
        mut self,
        callback: impl Fn(&Button, f64, f64) + 'static,
    ) -> Self {
        self.context_menu_requested = Some(Rc::new(callback));
        self
    }
}

/// A chip widget paired with its app-local group key for insertion.
#[derive(Clone)]
pub struct BigTabGroupChipMount {
    /// App-local group key.
    pub group_key: String,
    /// Chip widget built by [`build_tab_group_chip`] or app code.
    pub chip: Button,
}

impl BigTabGroupChipMount {
    /// Build a mount entry.
    #[must_use]
    pub fn new(group_key: impl Into<String>, chip: &Button) -> Self {
        Self {
            group_key: group_key.into(),
            chip: chip.clone(),
        }
    }
}

/// One tab button in strip order, plus its optional group key.
#[derive(Clone)]
pub struct BigTabGroupTabMount {
    /// App-local group key for this tab, if grouped.
    pub group_key: Option<String>,
    /// The rendered tab button widget.
    pub tab_button: Widget,
}

impl BigTabGroupTabMount {
    /// Build a mount entry from a strip page.
    #[must_use]
    pub fn from_page(page: &Rc<BigTabPage>, group_key: Option<&str>) -> Self {
        Self {
            group_key: group_key.map(str::to_owned),
            tab_button: page.tab_button().upcast(),
        }
    }

    /// Build a mount entry from a raw tab button widget.
    #[must_use]
    pub fn from_tab_button<W: IsA<Widget>>(tab_button: &W, group_key: Option<&str>) -> Self {
        Self {
            group_key: group_key.map(str::to_owned),
            tab_button: tab_button.clone().upcast(),
        }
    }
}

/// Build a GTK chip button from display-free group state and caller callbacks.
#[must_use]
pub fn build_tab_group_chip(
    spec: &BigTabGroupChipSpec,
    callbacks: BigTabGroupChipCallbacks,
) -> Button {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .valign(Align::Center)
        .build();
    let icon = Image::from_icon_name(if spec.collapsed {
        ICON_COLLAPSED
    } else {
        ICON_EXPANDED
    });
    icon.add_css_class(CSS_CLASS_GROUP_TOGGLE_ICON);
    row.append(&icon);

    let display_label = spec.display_label();
    let label = Label::new(Some(&display_label));
    label.set_ellipsize(EllipsizeMode::End);
    label.set_max_width_chars(GROUP_LABEL_WIDTH_CHARS);
    label.add_css_class(CSS_CLASS_GROUP_NAME_LABEL);
    row.append(&label);

    let chip = Button::builder()
        .child(&row)
        .focus_on_click(false)
        .valign(Align::Center)
        .build();
    chip.add_css_class("flat");
    chip.add_css_class(CSS_CLASS_TAB_GROUP_CHIP);
    chip.add_css_class(&spec.color_class());
    set_widget_class(&chip, CSS_CLASS_COLLAPSED, spec.collapsed);
    set_widget_class(&chip, CSS_CLASS_UNNAMED, spec.name.is_empty());
    tooltip::set(&chip, spec.tooltip_label());
    chip.update_property(&[gtk::accessible::Property::Label(&spec.accessible_label())]);

    if let Some(on_toggle) = callbacks.toggle_requested {
        chip.connect_clicked(move |_| on_toggle());
    }

    if let Some(on_context_menu) = callbacks.context_menu_requested {
        let right = GestureClick::new();
        right.set_button(gtk::gdk::BUTTON_SECONDARY);
        right.set_propagation_phase(PropagationPhase::Capture);
        right.connect_pressed(move |gesture, _n, x, y| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            let Some(chip) = widget.downcast_ref::<Button>() else {
                return;
            };
            on_context_menu(chip, x, y);
        });
        chip.add_controller(right);
    }

    chip
}

/// Rebuild the strip's tab bar by inserting each chip before the first tab
/// carrying its group key.
///
/// Only the passed tab buttons and chips are detached/reinserted. Other
/// children owned by [`BigTabStrip`](super::tab_strip_host::BigTabStrip), such
/// as its context-menu popover, are left alone.
pub fn rebuild_tab_bar_with_group_chips(
    tab_bar: &GtkBox,
    tabs: &[BigTabGroupTabMount],
    chips: &[BigTabGroupChipMount],
) {
    let chip_by_group: HashMap<&str, &Button> = chips
        .iter()
        .map(|mount| (mount.group_key.as_str(), &mount.chip))
        .collect();

    for tab in tabs {
        if tab.tab_button.parent().is_some() {
            tab_bar.remove(&tab.tab_button);
        }
    }
    for mount in chips {
        if mount.chip.parent().is_some() {
            tab_bar.remove(&mount.chip);
        }
    }

    let mut placed = HashSet::new();
    for tab in tabs {
        if let Some(group_key) = &tab.group_key
            && placed.insert(group_key.clone())
            && let Some(chip) = chip_by_group.get(group_key.as_str())
        {
            tab_bar.append(*chip);
        }
        tab_bar.append(&tab.tab_button);
    }
}

/// Visible chip label: `Name (count)`, or `(count)` for unnamed groups.
#[must_use]
pub fn chip_display_label(name: &str, tab_count: usize) -> String {
    if name.is_empty() {
        format!("({tab_count})")
    } else {
        format!("{name} ({tab_count})")
    }
}

/// Accessible label for a chip display label and collapsed state.
#[must_use]
pub fn chip_accessible_label(display_label: &str, collapsed: bool) -> String {
    let template = if collapsed {
        t(MSG_GROUP_COLLAPSED)
    } else {
        t(MSG_GROUP_EXPANDED)
    };
    template.replace("{name}", display_label)
}

/// Tooltip for the chip's primary click action.
#[must_use]
pub fn chip_tooltip_label(display_label: &str, collapsed: bool) -> String {
    let template = if collapsed {
        t(MSG_CLICK_TO_EXPAND)
    } else {
        t(MSG_CLICK_TO_COLLAPSE)
    };
    template.replace("{name}", display_label)
}

/// CSS class name derived from a group color string.
#[must_use]
pub fn tab_group_color_class(color: &str) -> String {
    let mut class = String::from("tab-group-color-");
    for ch in color.chars() {
        if ch.is_ascii_alphanumeric() {
            class.push(ch);
        } else {
            class.push('_');
        }
    }
    class
}

/// CSS template for the shared grouped-tab border/underline hook.
#[must_use]
pub fn format_tab_group_border_css(color: &str) -> String {
    format!(".big-tab-button.in-group {{ border-bottom-color: {color}; }}")
}

/// CSS for a color-scoped chip and grouped tab button.
#[must_use]
pub fn format_tab_group_color_css(color: &str) -> String {
    let class = tab_group_color_class(color);
    let text = contrasting_text_for_css_color(color);
    format!(
        ".{class}.{CSS_CLASS_TAB_GROUP_CHIP} {{ background-color: {color}; color: {text}; }}\n\
         .{class}.big-tab-button.{CSS_CLASS_IN_GROUP} {{ border-bottom-color: {color}; }}"
    )
}

/// Build a CSS provider containing [`format_tab_group_color_css`].
#[must_use]
pub fn tab_group_color_css_provider(color: &str) -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format_tab_group_color_css(color));
    provider
}

fn contrasting_text_for_css_color(color: &str) -> &'static str {
    let Some((red, green, blue)) = parse_hex_rgb(color) else {
        return FALLBACK_TEXT;
    };
    let luminance =
        (0.2126 * linear_srgb(red)) + (0.7152 * linear_srgb(green)) + (0.0722 * linear_srgb(blue));
    if luminance > 0.45 { ON_LIGHT } else { ON_DARK }
}

fn parse_hex_rgb(color: &str) -> Option<(u8, u8, u8)> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((red, green, blue))
}

fn linear_srgb(byte: u8) -> f64 {
    let value = f64::from(byte) / 255.0;
    if value <= 0.03928 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn set_widget_class(widget: &impl IsA<Widget>, class: &str, active: bool) {
    if active {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSnapshot {
        name: String,
        color: String,
        collapsed: bool,
        tab_count: usize,
    }

    impl BigTabGroupChipSnapshot for FakeSnapshot {
        fn group_name(&self) -> &str {
            &self.name
        }

        fn group_color(&self) -> &str {
            &self.color
        }

        fn is_group_collapsed(&self) -> bool {
            self.collapsed
        }

        fn group_tab_count(&self) -> usize {
            self.tab_count
        }
    }

    #[test]
    fn spec_builds_accessible_label_from_display_label() {
        let spec = BigTabGroupChipSpec::new("A", "#4d7cff", true, 2);

        assert_eq!(spec.display_label(), "A (2)");
        assert_eq!(spec.accessible_label(), "Tab group A (2), collapsed");
        assert_eq!(spec.tooltip_label(), "Click to expand A (2)");
    }

    #[test]
    fn unnamed_group_display_uses_count_only() {
        let spec = BigTabGroupChipSpec::new("", "#4d7cff", false, 3);

        assert_eq!(spec.display_label(), "(3)");
        assert_eq!(spec.accessible_label(), "Tab group (3), expanded");
    }

    #[test]
    fn css_template_outputs_group_border_hook() {
        assert_eq!(
            format_tab_group_border_css("#4d7cff"),
            ".big-tab-button.in-group { border-bottom-color: #4d7cff; }"
        );
        assert_eq!(
            tab_group_color_class("#4d-7c ff"),
            "tab-group-color-_4d_7c_ff"
        );
        assert_eq!(
            format_tab_group_color_css("#ffffff"),
            ".tab-group-color-_ffffff.tab-group-chip { background-color: #ffffff; color: #000000; }\n\
         .tab-group-color-_ffffff.big-tab-button.in-group { border-bottom-color: #ffffff; }"
        );
    }

    #[test]
    fn spec_constructs_from_snapshot_adapter() {
        let snapshot = FakeSnapshot {
            name: "Work".to_owned(),
            color: "#aabbcc".to_owned(),
            collapsed: false,
            tab_count: 4,
        };

        assert_eq!(
            BigTabGroupChipSpec::from_snapshot(&snapshot),
            BigTabGroupChipSpec::new("Work", "#aabbcc", false, 4)
        );
    }
}
