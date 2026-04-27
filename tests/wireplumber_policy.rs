use std::fs;
use std::path::Path;

#[test]
fn wireplumber_drop_in_loads_biglinux_aec_policy_hook() {
    let conf = fs::read_to_string(
        "usr/share/wireplumber/wireplumber.conf.d/60-biglinux-echo-cancel-routing.conf",
    )
    .unwrap();

    assert!(conf.contains("biglinux/echo-cancel-routing.lua"));
    assert!(conf.contains("hooks.biglinux.echo-cancel-routing = required"));
    assert!(conf.contains("type = script/lua"));
    assert!(conf.contains("provides = hooks.biglinux.echo-cancel-routing"));
    assert!(conf.contains("requires = [ metadata.default, metadata.filters ]"));
}

#[test]
fn echo_cancel_policy_targets_only_physical_sources() {
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("local EC_CAPTURE_NODE_NAME = \"echo-cancel-capture\""));
    assert!(script.contains("[\"mic-biglinux\"] = true"));
    assert!(script.contains("[\"echo-cancel-source\"] = true"));
    assert!(script.contains("metadata:set (id, \"filter.smart\", \"Spa:String:JSON\", \"false\")"));
    assert!(script.contains("default.configured.audio.source"));
    assert!(script.contains("default.audio.source"));
    assert!(script.contains("si_flags.has_defined_target = true"));
    assert!(script.contains("event:set_data (\"target\", target)"));
    assert!(script.contains("event:stop_processing ()"));
}

#[test]
fn packaged_wireplumber_files_are_installed_by_main_pkgbuild() {
    let pkgbuild = fs::read_to_string("packaging/arch/PKGBUILD").unwrap();

    for path in [
        "usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua",
        "usr/share/wireplumber/wireplumber.conf.d/60-biglinux-echo-cancel-routing.conf",
    ] {
        assert!(
            Path::new(path).exists(),
            "{path} must exist in the source tree"
        );
        assert!(
            pkgbuild.contains(path),
            "{path} must be explicitly installed by packaging/arch/PKGBUILD",
        );
    }
}
