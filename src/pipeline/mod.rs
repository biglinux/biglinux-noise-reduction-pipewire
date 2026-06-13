//! Filter-chain configuration pipeline.
//!
//! `apply` is the single entry point used by the service layer to
//! materialise three files on disk:
//!
//! | File | Role |
//! |------|------|
//! | `~/.config/biglinux-microphone/aec.args`    | AEC module args body  |
//! | `~/.config/biglinux-microphone/mic.args`    | Mic filter-chain args body |
//! | `~/.config/biglinux-microphone/output.args` | Output filter-chain args body |
//!
//! Each file is written via the atomic write helper so a partial write
//! cannot leave the loader reading a truncated args body.
//!
//! Process layout: each filter graph runs in its own
//! `biglinux-microphone-pwloader` process — `biglinux-microphone-aec.service`,
//! `biglinux-microphone-mic.service`, `biglinux-microphone-output.service`.
//! The loader connects as a regular client of the main PipeWire daemon
//! and asks the daemon to instantiate the module inside its local
//! context; the audio nodes are exported to the daemon and driven by
//! the daemon's data-loop. All three loaders therefore share one clock,
//! eliminating the cross-process drift that the previous standalone
//! `pipewire -c` worker produced.
//!
//! Disabled chains are removed from disk entirely so the corresponding
//! virtual node disappears from the PipeWire graph.

mod echo_cancel;
mod graph;
mod mic;
mod nodes;
mod output;

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use log::{debug, info};

use crate::config::AppSettings;

pub use echo_cancel::{
    build_echo_cancel_conf as build_echo_cancel_conf_for, echo_cancel_wanted,
    ECHO_CANCEL_CONF_FILE, EC_CAPTURE_NODE_NAME, EC_SOURCE_NAME,
};
pub use mic::{
    ai_node_in_mic_chain, build_mic_conf as build_mic_conf_for, cascade_mic_off, mic_chain_wanted,
    MIC_CAPTURE_NODE_NAME, MIC_CONF_FILE, MIC_DESCRIPTION, MIC_NODE_NAME,
};
pub use output::{
    build_output_conf as build_output_conf_for, output_ai_processing, OUTPUT_CONF_FILE,
    OUTPUT_DESCRIPTION, OUTPUT_NODE_NAME,
};

fn xdg_config_root() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".config")
    })
}

/// XDG base dir that holds the per-loader args bodies consumed by
/// `biglinux-microphone-pwloader`. Each filter graph ships a single
/// `<name>.args` file here; the systemd unit reads that path and passes
/// the contents verbatim to `pw_context_load_module()`.
#[must_use]
pub fn pwloader_args_dir() -> PathBuf {
    xdg_config_root().join("biglinux-microphone")
}

/// Legacy XDG base dir for PipeWire drop-in configs (loaded by the
/// old `filter-chain.service` topology). Kept around so `apply` can
/// scrub stale files from earlier installs.
#[must_use]
pub fn pipewire_drop_in_dir() -> PathBuf {
    xdg_config_root().join("pipewire/filter-chain.conf.d")
}

/// Legacy XDG base dir that previously held the standalone output
/// filter `pipewire -c` config. Same role as [`pipewire_drop_in_dir`]:
/// retained only so legacy cleanup can remove leftovers.
#[must_use]
pub fn pipewire_standalone_dir() -> PathBuf {
    xdg_config_root().join("pipewire")
}

/// XDG base dir that previously held WirePlumber drop-in configs for
/// per-app routing. Kept around so `apply` can scrub legacy files
/// from earlier installs.
#[must_use]
pub fn wireplumber_drop_in_dir() -> PathBuf {
    xdg_config_root().join("wireplumber/wireplumber.conf.d")
}

/// Absolute path of the mic args file consumed by
/// `biglinux-microphone-mic.service`.
#[must_use]
pub fn mic_conf_path() -> PathBuf {
    pwloader_args_dir().join(MIC_CONF_FILE)
}

/// Absolute path of the output args file consumed by
/// `biglinux-microphone-output.service`.
#[must_use]
pub fn output_conf_path() -> PathBuf {
    pwloader_args_dir().join(OUTPUT_CONF_FILE)
}

/// Absolute path of the AEC args file consumed by
/// `biglinux-microphone-aec.service`. Only materialised when the user
/// toggles AEC on, so the module is absent from the graph when the
/// feature is off.
#[must_use]
pub fn echo_cancel_conf_path() -> PathBuf {
    pwloader_args_dir().join(ECHO_CANCEL_CONF_FILE)
}

/// File name of the WirePlumber drop-in shipped by the per-app routing
/// implementation. Removed on every `apply` so a stale rule cannot
/// keep redirecting streams after an upgrade.
pub const LEGACY_ROUTING_CONF_FILE: &str = "50-biglinux-output-routing.conf";

/// Files written by previous Python or Rust versions of this
/// configurator that the current code no longer maintains. Paths are
/// joined under [`xdg_config_root`] (i.e. `~/.config`) at deletion
/// time, never accessed during normal operation.
const LEGACY_FILES: &[&str] = &[
    // Python era — mic filter chains.
    "pipewire/filter-chain.conf.d/source-gtcrn-smart.conf",
    "pipewire/filter-chain.conf.d/source-ulunas-smart.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise-smart.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise-config.conf",
    // Python era — output chains.
    "pipewire/filter-chain.conf.d/big-output-filter.conf",
    "pipewire/big-output-filter.conf",
    // Rust intermediate revisions.
    "pipewire/filter-chain.conf.d/20-biglinux-output.conf",
    "pipewire/filter-chain.conf",
    "pipewire/pipewire.conf.d/50-biglinux-microphone-realtime.conf",
    // Per-app routing era of the Rust port.
    "wireplumber/wireplumber.conf.d/50-biglinux-output-routing.conf",
    "wireplumber/wireplumber.conf.d/50-biglinux-microphone-routing.conf",
    // Standalone AEC era (pre drop-in consolidation): the
    // `biglinux-microphone-echocancel.service` unit consumed this
    // file; the unit is gone but installed copies of the conf would
    // still be picked up by old user-enabled symlinks.
    "pipewire/biglinux-microphone-echocancel.conf",
    // filter-chain.service drop-in era — superseded by
    // dedicated `biglinux-microphone-{aec,mic,output}.service`
    // units that each load a single module via pwloader.
    "pipewire/filter-chain.conf.d/05-biglinux-echocancel.conf",
    "pipewire/filter-chain.conf.d/10-biglinux-microphone.conf",
    "pipewire/biglinux-microphone-output.conf",
];

/// Best-effort migration step: deletes every config file written by a
/// previous Python or Rust version of this configurator. Missing files
/// are not an error; permission errors are logged and otherwise
/// ignored so the app keeps starting even on quirky home directories.
///
/// Safe to call at every app launch — when nothing is stale the
/// function is a no-op.
pub fn purge_legacy_files() {
    let root = xdg_config_root();
    for rel in LEGACY_FILES {
        let path = root.join(rel);
        match fs::remove_file(&path) {
            Ok(()) => info!("pipeline: removed legacy file {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => debug!("pipeline: skip {}: {e}", path.display()),
        }
    }
    // GTCRN external override left on tmpfs by the Python plugin.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = std::path::Path::new(&runtime).join("gtcrn-ladspa-controls");
        let _ = fs::remove_file(&path);
    }

    purge_legacy_services();
}

/// Stop and disable user-level systemd units shipped by older versions
/// whose roles have moved. Specifically: the standalone
/// `biglinux-microphone-echocancel.service` was folded into the
/// previous `filter-chain.service` drop-in topology, and that drop-in
/// topology has since been replaced by per-loader units. Without this
/// step, an upgrading user keeps the old worker running until next
/// logout because the `[Install]` symlink in
/// `~/.config/systemd/user/default.target.wants/` outlives the unit
/// file removed by the package upgrade.
fn purge_legacy_services() {
    const LEGACY_USER_UNITS: &[&str] = &["biglinux-microphone-echocancel.service"];
    for unit in LEGACY_USER_UNITS {
        // `disable --now` is the documented way to stop a running unit
        // and remove its [Install] symlinks in the same call. We let
        // failures fall through silently: the unit may simply not be
        // present, in which case there is nothing to clean up.
        let result = BigSubprocessSpec::builder()
            .program("systemctl")
            .args(["--user", "disable", "--now", unit])
            .stdout(BigSubprocessOutputMode::Null)
            .stderr(BigSubprocessOutputMode::Null)
            .build()
            .run();
        match result {
            Ok(o) if o.status.success() => info!("pipeline: stopped legacy service {unit}"),
            Ok(_) => debug!("pipeline: legacy service {unit} not present"),
            Err(e) => debug!("pipeline: systemctl invocation failed for {unit}: {e}"),
        }
    }
}

/// Materialise every config file under the default XDG paths.
pub fn apply(settings: &AppSettings) -> io::Result<()> {
    apply_to_dirs(
        settings,
        &pwloader_args_dir(),
        &pipewire_drop_in_dir(),
        &wireplumber_drop_in_dir(),
    )
}

/// Write the generated files under explicit directories. This is the
/// variant tests use to avoid touching the user's real config.
///
/// `args_dir` hosts the three pwloader args bodies (mic, output, AEC).
/// `pipewire_dropin_dir` and `wireplumber_dir` are scanned for legacy
/// drop-ins from previous topologies, all deleted on every call.
pub fn apply_to_dirs(
    settings: &AppSettings,
    args_dir: &Path,
    pipewire_dropin_dir: &Path,
    wireplumber_dir: &Path,
) -> io::Result<()> {
    fs::create_dir_all(args_dir)?;

    // Mic chain: only materialise it when the user wants *any* mic
    // processing. Otherwise we'd leave a "BigLinux Microphone" virtual
    // source hanging in the PipeWire graph even with every toggle off.
    let mic_path = args_dir.join(MIC_CONF_FILE);
    if mic::mic_chain_wanted(settings) {
        atomic_write(&mic_path, mic::build_mic_conf(settings).as_bytes())?;
        info!("pipeline: wrote mic args to {}", mic_path.display());
    } else {
        remove_if_exists(&mic_path)?;
        info!("pipeline: mic chain idle, {} cleared", mic_path.display());
    }

    // Output chain runs in its own pwloader process (see
    // `biglinux-microphone-output.service`). The args body is written
    // unconditionally so the unit can come up in **bypass mode** even
    // when `output_filter.enabled` is false. Stopping the unit would
    // tear down the smart-filter sink, and browsers (Chromium
    // especially) pause HTMLMediaElement when their output sink
    // disappears mid-playback. Bypassing inside the graph (GTCRN
    // Enable=0, gate floor, compressor unity, HPF pass-through) keeps
    // the sink present and the streams attached.
    let out_path = args_dir.join(OUTPUT_CONF_FILE);
    atomic_write(&out_path, output::build_output_conf(settings).as_bytes())?;
    info!("pipeline: wrote output args to {}", out_path.display());

    // Echo-cancel: materialised only when the user wants AEC. The
    // dedicated `biglinux-microphone-aec.service` consumes it; the mic
    // unit `After=` orders against it so `echo-cancel-source` exists
    // before the mic chain resolves its `target.object`.
    let ec_path = args_dir.join(ECHO_CANCEL_CONF_FILE);
    if echo_cancel::echo_cancel_wanted(settings) {
        atomic_write(
            &ec_path,
            echo_cancel::build_echo_cancel_conf(settings).as_bytes(),
        )?;
        info!("pipeline: wrote echo-cancel args to {}", ec_path.display());
    } else {
        remove_if_exists(&ec_path)?;
        info!("pipeline: echo-cancel idle, {} cleared", ec_path.display());
    }

    // Legacy cleanup: previous topologies wrote the mic + AEC inside
    // `filter-chain.conf.d/` and the output chain at the root of
    // `~/.config/pipewire/`. All three must go so an upgraded install
    // doesn't load a phantom second GTCRN inside `filter-chain.service`
    // or keep the orphaned standalone output `pipewire -c` config
    // around. The WirePlumber per-app routing rule from the very
    // first Rust port is similarly stripped. Standalone-dir leftovers
    // are handled by [`purge_legacy_files`] at app startup.
    remove_if_exists(&pipewire_dropin_dir.join("20-biglinux-output.conf"))?;
    remove_if_exists(&pipewire_dropin_dir.join("10-biglinux-microphone.conf"))?;
    remove_if_exists(&pipewire_dropin_dir.join("05-biglinux-echocancel.conf"))?;
    remove_if_exists(&wireplumber_dir.join(LEGACY_ROUTING_CONF_FILE))?;

    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Remove every file `apply` would have written. Used on uninstall and
/// from the UI's "reset to defaults" action. Missing files are not an
/// error.
pub fn remove_all() -> io::Result<()> {
    let legacy_routing = wireplumber_drop_in_dir().join(LEGACY_ROUTING_CONF_FILE);
    let legacy_dropin_root = pipewire_drop_in_dir();
    let legacy_standalone_root = pipewire_standalone_dir();
    for path in [
        mic_conf_path(),
        output_conf_path(),
        echo_cancel_conf_path(),
        legacy_routing,
        legacy_dropin_root.join("10-biglinux-microphone.conf"),
        legacy_dropin_root.join("05-biglinux-echocancel.conf"),
        legacy_standalone_root.join("biglinux-microphone-output.conf"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => debug!("pipeline: removed {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(body)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;
    use tempfile::tempdir;

    fn dirs(t: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
        (
            t.path().join("bigmic-args"),
            t.path().join("pw-dropin"),
            t.path().join("wp"),
        )
    }

    #[test]
    fn apply_writes_mic_and_output_when_enabled() {
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        let settings = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };

        apply_to_dirs(&settings, &args, &pw_dropin, &wp).unwrap();

        assert!(args.join(MIC_CONF_FILE).exists());
        assert!(args.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_uses_atomic_temp_files() {
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        apply_to_dirs(&AppSettings::default(), &args, &pw_dropin, &wp).unwrap();

        if args.exists() {
            for entry in fs::read_dir(&args).unwrap() {
                let p = entry.unwrap().path();
                assert_ne!(p.extension().and_then(|s| s.to_str()), Some("tmp"));
            }
        }
    }

    #[test]
    fn apply_is_idempotent_across_runs() {
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        let settings = AppSettings::default();
        apply_to_dirs(&settings, &args, &pw_dropin, &wp).unwrap();
        let first = fs::read_to_string(args.join(MIC_CONF_FILE)).unwrap();

        apply_to_dirs(&settings, &args, &pw_dropin, &wp).unwrap();
        let second = fs::read_to_string(args.join(MIC_CONF_FILE)).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn apply_writes_output_conf_even_when_master_off() {
        // The conf is written unconditionally so the output unit can
        // come up in bypass mode without ever needing a stop/start
        // round trip while audio is playing.
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        let settings = AppSettings::default();
        assert!(!settings.output_filter.enabled);
        apply_to_dirs(&settings, &args, &pw_dropin, &wp).unwrap();

        assert!(args.join(MIC_CONF_FILE).exists());
        assert!(
            args.join(OUTPUT_CONF_FILE).exists(),
            "output args must always be present so the unit can run in bypass"
        );
    }

    #[test]
    fn apply_keeps_output_conf_after_disable() {
        // Going from enabled → disabled inside a session must not
        // remove the args file — see
        // `apply_writes_output_conf_even_when_master_off` for the
        // rationale.
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        let enabled = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        apply_to_dirs(&enabled, &args, &pw_dropin, &wp).unwrap();
        assert!(args.join(OUTPUT_CONF_FILE).exists());

        apply_to_dirs(&AppSettings::default(), &args, &pw_dropin, &wp).unwrap();
        assert!(args.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_removes_legacy_routing_drop_in() {
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);
        fs::create_dir_all(&wp).unwrap();
        let legacy = wp.join(LEGACY_ROUTING_CONF_FILE);
        fs::write(&legacy, b"# stale per-app rule\n").unwrap();

        apply_to_dirs(&AppSettings::default(), &args, &pw_dropin, &wp).unwrap();

        assert!(
            !legacy.exists(),
            "legacy WirePlumber routing rule must be deleted on apply"
        );
    }

    #[test]
    fn apply_removes_legacy_filter_chain_dropins() {
        // Stale `05-biglinux-echocancel.conf` / `10-biglinux-microphone.conf`
        // from the previous filter-chain.service drop-in topology must
        // be scrubbed on every apply so the upstream filter-chain
        // daemon does not double-load our modules alongside the new
        // pwloader instances.
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);
        fs::create_dir_all(&pw_dropin).unwrap();
        let stale_aec = pw_dropin.join("05-biglinux-echocancel.conf");
        let stale_mic = pw_dropin.join("10-biglinux-microphone.conf");
        fs::write(&stale_aec, b"# stale aec drop-in\n").unwrap();
        fs::write(&stale_mic, b"# stale mic drop-in\n").unwrap();

        apply_to_dirs(&AppSettings::default(), &args, &pw_dropin, &wp).unwrap();

        assert!(!stale_aec.exists());
        assert!(!stale_mic.exists());
    }

    #[test]
    fn apply_clears_mic_conf_when_all_filters_off() {
        use crate::config::{
            CompressorConfig, EchoCancelConfig, EqualizerConfig, GateConfig, HpfConfig,
            NoiseReductionConfig, StereoConfig,
        };
        let dir = tempdir().unwrap();
        let (args, pw_dropin, wp) = dirs(&dir);

        apply_to_dirs(&AppSettings::default(), &args, &pw_dropin, &wp).unwrap();
        assert!(args.join(MIC_CONF_FILE).exists());

        let off = AppSettings {
            noise_reduction: NoiseReductionConfig {
                enabled: false,
                ..NoiseReductionConfig::default()
            },
            gate: GateConfig {
                enabled: false,
                ..GateConfig::default()
            },
            hpf: HpfConfig {
                enabled: false,
                ..HpfConfig::default()
            },
            stereo: StereoConfig {
                enabled: false,
                ..StereoConfig::default()
            },
            equalizer: EqualizerConfig {
                enabled: false,
                ..EqualizerConfig::default()
            },
            compressor: CompressorConfig {
                enabled: false,
                ..CompressorConfig::default()
            },
            // EC default is `true`; this test exercises the *every filter
            // off* path so we explicitly opt out here.
            echo_cancel: EchoCancelConfig { enabled: false },
            ..AppSettings::default()
        };
        apply_to_dirs(&off, &args, &pw_dropin, &wp).unwrap();
        assert!(
            !args.join(MIC_CONF_FILE).exists(),
            "mic virtual source must disappear once every filter is off"
        );
    }

    #[test]
    fn remove_all_is_noop_when_files_missing() {
        let _ = remove_all();
    }

    #[test]
    fn legacy_files_constant_lists_known_python_paths() {
        // Smoke test: every entry must be a relative path under the
        // user's `~/.config/`. Absolute paths or shell metacharacters
        // would be a bug — `purge_legacy_files` joins them with
        // `xdg_config_root` unconditionally.
        for rel in LEGACY_FILES {
            assert!(
                !rel.starts_with('/'),
                "legacy entry must be relative: {rel}"
            );
            assert!(
                !rel.contains(".."),
                "legacy entry must not climb dirs: {rel}"
            );
        }
        // Coverage: at least the headline Python file names are listed.
        let joined = LEGACY_FILES.join("\n");
        for needle in [
            "source-gtcrn-smart.conf",
            "source-rnnoise-smart.conf",
            "big-output-filter.conf",
        ] {
            assert!(joined.contains(needle), "missing legacy entry: {needle}");
        }
    }

    #[test]
    fn purge_legacy_files_does_not_panic_when_none_present() {
        // Run the public function — it consults the user's real XDG
        // root which probably has none of these files in CI / a fresh
        // dev box. The contract is "missing → no-op", so this just
        // exercises the path.
        purge_legacy_files();
    }
}
