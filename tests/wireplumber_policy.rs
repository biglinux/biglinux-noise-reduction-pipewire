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
fn aec_reference_is_pinned_to_physical_alsa_sink() {
    // The sink-target hook must use the physical ALSA sink, not the
    // JamesDSP virtual sink. Anchoring to ALSA means the AEC reference
    // matches whatever the kernel hands to the DAC, regardless of how
    // many post-processors (JamesDSP, future convolvers) sit upstream.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("biglinux/echo-cancel-sink-target"));
    assert!(script.contains("lookup_physical_sink"));
    assert!(script.contains("is_physical_alsa_sink"));
    assert!(script.contains("alsa_output."));
    // Must claim the target at the node level so the smart-filter pass
    // does not re-wrap us with output-biglinux:monitor.
    assert!(script.contains("si_flags.has_node_defined_target = true"));
    // Must consult `default.configured.audio.sink` first — that's what
    // `wpctl set-default` writes, and the user's choice should win
    // over priority heuristics.
    assert!(script.contains("default.configured.audio.sink"));
    assert!(script.contains("default.audio.sink"));
}

#[test]
fn output_smart_filter_retargets_when_jamesdsp_present() {
    // JamesDSP can become the de-facto sink (either by user choice or
    // because its daemon relocates app streams). The output-biglinux
    // smart filter must follow so apps don't bypass the BigLinux
    // EQ/HPF/gate chain.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("biglinux/output-smart-filter-retarget"));
    assert!(script.contains("OUTPUT_FILTER_NODE_NAME = \"output-biglinux\""));
    assert!(script.contains("JAMESDSP_SINK_NAME = \"jamesdsp_sink\""));
    assert!(script.contains("filter.smart.target"));
    // Override is published via the `filters` metadata so WirePlumber
    // re-evaluates routing without restarting any service.
    assert!(script.contains("metadata_object (source, \"filters\")"));
}

#[test]
fn jamesdsp_sink_appearance_triggers_rescan() {
    // When jamesdsp_sink materialises after WP has already linked
    // echo-cancel-sink, we need a rescan to re-run the sink-target
    // hook; otherwise the AEC reference can stay on the wrong node
    // until an unrelated event happens.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("biglinux/jamesdsp-sink-rescan"));
    assert!(script.contains("schedule-rescan"));
}

#[test]
fn default_metadata_change_triggers_rescan_and_retarget() {
    // wpctl set-default (or any client writing the `default` metadata)
    // does not emit session-item-added. Without a metadata-changed
    // hook, switching the default sink leaves the AEC reference and
    // the output-biglinux smart filter pinned to the previous target.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("biglinux/default-changed-rescan"));
    assert!(script.contains("\"event.type\", \"=\", \"metadata-changed\""));
    assert!(script.contains("\"metadata.name\", \"=\", \"default\""));
    assert!(script.contains("default.configured.audio.sink"));
    assert!(script.contains("default.configured.audio.source"));
    assert!(script.contains("default.audio.sink"));
    assert!(script.contains("default.audio.source"));
    // Must both retarget the smart filter and force a rescan; either
    // one alone leaves the graph in a stale state.
    assert!(script.contains("maybe_retarget_output_smart_filter"));
    assert!(script.contains("schedule-rescan"));
}

#[test]
fn jamesdsp_sink_removal_clears_smart_filter_override() {
    // When jamesdsp_sink disappears (JamesDSP stopped or restarting),
    // the `filters` metadata override on output-biglinux must be
    // reconciled. session-item-added never fires for removals, so a
    // dedicated session-item-removed hook is required.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    assert!(script.contains("biglinux/session-item-removed-retarget"));
    assert!(script.contains("\"event.type\", \"=\", \"session-item-removed\""));
    assert!(script.contains("\"event.session-item.interface\", \"=\", \"linkable\""));
    // The clear path inside maybe_retarget_output_smart_filter writes
    // a nil value — confirm it is still present so removals actually
    // tear the override down.
    assert!(script.contains("metadata:set (filter_id, \"filter.smart.target\", nil, nil)"));
}

#[test]
fn legacy_aec_smart_filter_hook_short_circuits_on_node_name() {
    // The session-item-added hook fires for every linkable. It must
    // bail before allocating a node proxy when the subject is not the
    // AEC source, otherwise every stream pays an unnecessary proxy
    // lookup on a hot path.
    let script =
        fs::read_to_string("usr/share/wireplumber/scripts/biglinux/echo-cancel-routing.lua")
            .unwrap();

    let body = script
        .split("local function disable_legacy_aec_smart_filter")
        .nth(1)
        .expect("disable_legacy_aec_smart_filter helper missing");
    let head = body.split("get_associated_proxy").next().unwrap_or("");
    assert!(
        head.contains("si.properties [\"node.name\"] ~= EC_SOURCE_NAME"),
        "expected node.name short-circuit before node proxy lookup"
    );
}

#[test]
fn aec_module_runs_mono_with_stereo_to_mono_downmix() {
    // The mic is mono and libspa-aec-webrtc expects ref and capture
    // channel counts to match. Keeping the AEC mono lets PipeWire's
    // audioconvert downmix the stereo physical sink monitor (FL+FR)
    // before it reaches the canceller, so both speaker channels are
    // averaged into the reference instead of one side being dropped.
    let conf = fs::read_to_string("src/pipeline/echo_cancel.rs").unwrap();
    assert!(conf.contains("audio.channels = 1"));
    assert!(conf.contains("audio.position = [ MONO ]"));
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
