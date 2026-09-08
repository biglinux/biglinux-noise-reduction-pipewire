// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Tests for desktop integration helpers. Split out of `desktop.rs` (recipe:
//! file-size budget); as a child module the tests reach the parent's items
//! via `super::*`.

use super::*;
use relm4::gtk::Application;

#[test]
fn action_scope_resolves_detailed_name() {
    assert_eq!(
        BigActionSpec::new("open", "Open", BigActionScope::Window).detailed_name(),
        "win.open"
    );
}

#[test]
fn action_registry_appends_specs_in_declaration_order() {
    let registry = BigActionRegistry::new()
        .action(BigActionSpec::new("open", "Open", BigActionScope::App))
        .action(BigActionSpec::new("close", "Close", BigActionScope::Window));

    assert_eq!(
        registry
            .actions
            .iter()
            .map(BigActionSpec::detailed_name)
            .collect::<Vec<_>>(),
        vec!["app.open", "win.close"]
    );
}

#[test]
fn resource_spec_derives_packaging_names() {
    let spec = BigResourceSpec::new("br.com.biglinux.App", "big-app");
    assert_eq!(spec.desktop_file_id, "br.com.biglinux.App.desktop");
    assert_eq!(spec.appstream_id, "br.com.biglinux.App.metainfo.xml");
}

#[test]
fn system_power_actions_resolve_to_systemctl_args() {
    assert_eq!(BigSystemPowerAction::Suspend.command_args(), ["suspend"]);
    assert_eq!(BigSystemPowerAction::PowerOff.command_args(), ["poweroff"]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn collect_file_paths_keeps_local_paths() {
    let files = [
        gio::File::for_path("/tmp/big-app-kit-a.txt"),
        gio::File::for_uri("https://example.invalid/file.txt"),
        gio::File::for_path("/tmp/big-app-kit-b.txt"),
    ];

    assert_eq!(
        collect_file_paths(&files),
        [
            PathBuf::from("/tmp/big-app-kit-a.txt"),
            PathBuf::from("/tmp/big-app-kit-b.txt")
        ]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn collect_file_path_strings_match_ui_open_contract() {
    let files = [
        gio::File::for_path("/tmp/big-app-kit-a.txt"),
        gio::File::for_uri("https://example.invalid/file.txt"),
    ];

    assert_eq!(
        collect_file_path_strings(&files),
        ["/tmp/big-app-kit-a.txt".to_string()]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn active_window_helpers_return_false_without_window() {
    let app = Application::builder()
        .application_id("br.com.biglinux.big-app-kit.active-window-test")
        .build();
    let files = [gio::File::for_path("/tmp/big-app-kit-a.txt")];

    assert!(!with_active_window::<_, gtk::Window>(&app, |_| {
        panic!("callback must not run without an active window")
    }));
    assert!(!open_files_on_active_window::<_, gtk::Window>(
        &app,
        &files,
        |_, _| panic!("open_files callback must not run without an active window")
    ));
    assert!(!open_file_path_strings_on_active_window::<_, gtk::Window>(
        &app,
        &files,
        |_, _| panic!("open path strings callback must not run without an active window")
    ));
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_action_runs_callback() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = called.clone();
    install_action(&group, "demo", move || called_clone.set(true));

    group.activate_action("demo", None);

    assert!(called.get());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_action_enabled_blocks_callback_when_disabled() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = called.clone();
    install_action_enabled(&group, "demo", false, move || called_clone.set(true));

    group.activate_action("demo", None);

    assert!(!called.get());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_quit_action_registers_application_action() {
    let app = gio::Application::new(
        Some("br.com.biglinux.big-app-kit.quit-test"),
        gio::ApplicationFlags::empty(),
    );

    install_quit_action(&app);

    assert!(app.lookup_action("quit").is_some());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_static_accels_records_action_bindings() {
    let app = Application::builder()
        .application_id("br.com.biglinux.big-app-kit.accel-test")
        .build();

    install_static_accels(&app, &[("app.open", &["<Primary>O", "F10"])]);

    assert_eq!(
        app.accels_for_action("app.open")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["<Control>o", "F10"]
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_i32_action_parses_positive_index() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let captured = Rc::new(Cell::new(None));
    let captured_clone = captured.clone();
    install_i32_action(&group, "pick", move |idx| captured_clone.set(Some(idx)));

    group.activate_action("pick", Some(&7_i32.to_variant()));

    assert_eq!(captured.get(), Some(7));
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_i32_action_ignores_negative_index() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = called.clone();
    install_i32_action(&group, "pick", move |_| called_clone.set(true));

    group.activate_action("pick", Some(&(-1_i32).to_variant()));

    assert!(!called.get());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_toggle_action_flips_state_and_fires_callback() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let captured = Rc::new(Cell::new(None));
    let captured_clone = captured.clone();
    let action = install_toggle_action(&group, "mute", false, move |_, value| {
        captured_clone.set(Some(value));
    });

    group.activate_action("mute", None);
    assert_eq!(captured.get(), Some(true));
    assert_eq!(action.state().and_then(|s| s.get::<bool>()), Some(true));

    group.activate_action("mute", None);
    assert_eq!(captured.get(), Some(false));
    assert_eq!(action.state().and_then(|s| s.get::<bool>()), Some(false));
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_radio_action_updates_state_on_valid_value() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let captured = Rc::new(RefCell::new(None));
    let captured_clone = captured.clone();
    let action = install_radio_action(
        &group,
        "view",
        "grid",
        &["grid", "list"],
        move |_, value| {
            *captured_clone.borrow_mut() = Some(value.to_string());
        },
    );

    group.activate_action("view", Some(&"list".to_variant()));
    assert_eq!(captured.borrow().as_deref(), Some("list"));
    assert_eq!(
        action.state().and_then(|s| s.get::<String>()).as_deref(),
        Some("list"),
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_radio_action_rejects_unknown_value() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let captured = Rc::new(RefCell::new(None));
    let captured_clone = captured.clone();
    let action = install_radio_action(
        &group,
        "view",
        "grid",
        &["grid", "list"],
        move |_, value| {
            *captured_clone.borrow_mut() = Some(value.to_string());
        },
    );

    group.activate_action("view", Some(&"bogus".to_variant()));
    assert!(captured.borrow().is_none());
    assert_eq!(
        action.state().and_then(|s| s.get::<String>()).as_deref(),
        Some("grid"),
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_string_action_parses_text() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let captured = Rc::new(RefCell::new(None));
    let captured_clone = captured.clone();
    install_string_action(&group, "name", move |value| {
        *captured_clone.borrow_mut() = Some(value);
    });

    group.activate_action("name", Some(&"demo".to_variant()));

    assert_eq!(captured.borrow().as_deref(), Some("demo"));
}
