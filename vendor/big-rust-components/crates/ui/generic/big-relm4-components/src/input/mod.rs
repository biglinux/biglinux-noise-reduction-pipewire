//! Input widgets shared by BigLinux Relm4/libadwaita apps.

pub mod accelerator_label;
pub mod adjust_panel;
pub mod appearance_page;
pub mod button_content;
pub mod color_picker;
pub mod combo_row;
pub mod debounce;
pub mod dropdown;
pub mod dropdown_row;
pub mod entry_action_control;
pub mod find_bar;
pub mod form_dialog;
pub mod illustrated_slider_row;
pub mod inline_spin_set;
pub mod labeled_scale_row;
pub mod personalization_rows;
pub mod preference_cards;
pub mod preference_rows;
pub mod read_only_inline_control;
pub mod revealed_entry_row;
pub mod scale_popover;
pub mod scale_trough;
pub mod search;
pub mod search_filter_bar;
pub mod shortcut_capture;
pub mod shortcut_dialog;
pub mod slider_row;
pub mod spin_row;
pub mod switch;
pub mod switch_row;
pub mod toggle_group;
/// Display-free zone-layout model, re-exported from `big-app-kit-core` so the
/// spec layer owns it GTK-free while every `big_relm4_components::input::zone_layout::*`
/// consumer path keeps working. The GTK editor ([`zone_layout_editor`]) and the
/// runtime applier ([`crate::layout::zone_applier`]) build on it.
#[doc(inline)]
pub use big_app_kit_core::zone_layout;
pub mod zone_layout_editor;
