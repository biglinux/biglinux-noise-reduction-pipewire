//! Shared layout widgets.

pub mod advanced_disclosure;
pub mod choice_card;
pub mod choice_list_card;
pub mod chrome_band_host;
pub mod chrome_band_prefs;
pub mod column_chrome;
pub mod column_persist;
pub mod control_center;
pub mod custom_toolbar;
pub mod dialog_footer;
pub mod dialog_page;
pub mod dialog_teardown;
pub mod didactic_card;
pub mod hamburger_menu;
pub mod illustration_card;
pub mod labeled_frame;
pub mod media_window;
pub mod pane_header;
pub mod preferences;
pub mod preferences_dialog;
pub mod preferences_page_dialog;
pub mod saved_layouts_dialog;
pub mod settings_center;
pub mod sidebar_dialog;
pub mod sidebar_header;
pub mod sidebar_shell;
pub mod sidebar_tree;
pub mod split_chooser;
pub mod split_panes;
pub mod tab_button;
pub mod tab_group_chips;
pub mod tab_move_chrome;
pub mod tab_prefs;
pub mod tab_strip;
pub mod tab_strip_host;
pub mod tab_strip_placement;
pub mod tab_workspace;
pub mod toolbar_pane;
pub mod widget_container;
pub mod window_shell;
pub mod workspace_dock;
pub mod workspace_sidebar_split;
/// Runtime applier that reparents live controls into their zone containers per
/// a [`zone_layout`](crate::input::zone_layout) value (leak-safe, generic).
pub mod zone_applier;
