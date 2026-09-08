use big_relm4_components::input::switch::BigSwitchControlSpec;

#[test]
fn switch_control_defaults_tooltip_to_accessible_label() {
    let resolved = BigSwitchControlSpec::new("Enable rule", true).resolved();

    assert_eq!(resolved.accessible_label, "Enable rule");
    assert_eq!(resolved.tooltip_label(), "Enable rule");
    assert!(resolved.active);
}

#[test]
fn switch_control_accepts_dedicated_tooltip() {
    let resolved = BigSwitchControlSpec::new("Enable command git", false)
        .tooltip("Toggle command-specific rules")
        .resolved();

    assert_eq!(resolved.accessible_label, "Enable command git");
    assert_eq!(resolved.tooltip_label(), "Toggle command-specific rules");
    assert!(!resolved.active);
}
