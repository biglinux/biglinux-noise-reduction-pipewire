#[test]
fn user_units_resolve_configuration_in_the_process_environment() {
    for (unit, name) in [
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-mic.service"), "mic"),
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-aec.service"), "aec"),
        (include_str!("../usr/lib/systemd/user/biglinux-microphone-output.service"), "output"),
    ] {
        assert!(!unit.contains("%h/.config"));
        assert!(unit.contains(&format!("--check-config @config/{name}.args")));
    }
}
