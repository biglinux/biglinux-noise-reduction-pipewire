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
//! the daemon's data-loop. All three loaders therefore share one clock
//! and avoid the drift caused by independently clocked `pipewire -c`
//! workers.
//!
//! Disabled mic and AEC chains are removed from disk so their virtual
//! nodes disappear. Output args remain ready for the next service start.

mod echo_cancel;
mod graph;
mod mic;
mod nodes;
mod output;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::atomic_write_private as atomic_write;
#[cfg(test)]
use std::fs::read_to_string;

use log::{debug, info};

use crate::config::AppSettings;

pub use echo_cancel::{EC_CAPTURE_NODE_NAME, EC_SOURCE_NAME, ECHO_CANCEL_CONF_FILE};
pub use mic::{
    MIC_CAPTURE_NODE_NAME, MIC_CONF_FILE, MIC_DESCRIPTION, MIC_NODE_NAME, ai_node_in_mic_chain,
    build_mic_conf as build_mic_conf_for, cascade_mic_off, mic_chain_wanted,
};
pub use output::{
    OUTPUT_CONF_FILE, OUTPUT_DESCRIPTION, OUTPUT_NODE_NAME,
    build_output_conf as build_output_conf_for, output_ai_processing,
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
            .program("/usr/bin/systemctl")
            .args(["--user", "disable", "--now", unit])
            .allow_list(["/usr/bin/systemctl"])
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
    let root = xdg_config_root();
    apply_to_dirs(
        settings,
        &pwloader_args_dir(),
        &root.join("pipewire/filter-chain.conf.d"),
        &root.join("wireplumber/wireplumber.conf.d"),
    )
}

/// Publish the current graphs and remove the old app-owned routing drop-ins.
pub fn apply_to_dirs(
    settings: &AppSettings,
    args_dir: &Path,
    pipewire_dir: &Path,
    wireplumber_dir: &Path,
) -> io::Result<()> {
    apply_to_dir(settings, args_dir)?;
    for name in [
        "20-biglinux-output.conf",
        "10-biglinux-microphone.conf",
        "05-biglinux-echocancel.conf",
    ] {
        remove_file_if_exists(&pipewire_dir.join(name))?;
    }
    remove_file_if_exists(&wireplumber_dir.join(LEGACY_ROUTING_CONF_FILE))
}

/// Write the generated files under an explicit directory. This is the
/// variant tests use to avoid touching the user's real config.
///
/// `args_dir` hosts the three pwloader args bodies (mic, output, AEC).
pub fn apply_to_dir(settings: &AppSettings, args_dir: &Path) -> io::Result<()> {
    // Mic chain: only materialise it when the user wants *any* mic
    // processing. Otherwise we'd leave a "Filter noise" virtual
    // source hanging in the PipeWire graph even with every toggle off.
    let mic_path = args_dir.join(MIC_CONF_FILE);
    if mic::mic_chain_wanted(settings) {
        atomic_write(&mic_path, mic::build_mic_conf(settings).as_bytes())?;
        info!("pipeline: wrote mic args to {}", mic_path.display());
    } else {
        remove_file_if_exists(&mic_path)?;
        info!("pipeline: mic chain idle, {} cleared", mic_path.display());
    }

    // Output chain runs in its own pwloader process (see
    // `biglinux-microphone-output.service`). Keep the args body current even
    // while the service is disabled so it can start immediately with the
    // latest settings when the user enables the output filter.
    let out_path = args_dir.join(OUTPUT_CONF_FILE);
    atomic_write(&out_path, output::build_output_conf(settings).as_bytes())?;
    info!("pipeline: wrote output args to {}", out_path.display());

    // Echo-cancel: materialised only when the user wants AEC. The
    // dedicated `biglinux-microphone-aec.service` consumes it; the mic
    // unit `After=` orders against it so `echo-cancel-source` exists
    // before the mic chain resolves its `target.object`.
    let ec_path = args_dir.join(ECHO_CANCEL_CONF_FILE);
    if settings.echo_cancel.enabled {
        atomic_write(&ec_path, echo_cancel::build_echo_cancel_conf().as_bytes())?;
        info!("pipeline: wrote echo-cancel args to {}", ec_path.display());
    } else {
        remove_file_if_exists(&ec_path)?;
        info!("pipeline: echo-cancel idle, {} cleared", ec_path.display());
    }

    Ok(())
}

/// Remove every file `apply` would have written. Used on uninstall and
/// from the UI's "reset to defaults" action. Missing files are not an
/// error.
pub fn remove_all() -> io::Result<()> {
    for path in [mic_conf_path(), output_conf_path(), echo_cancel_conf_path()] {
        let existed = path.exists();
        match remove_file_if_exists(&path) {
            Ok(()) if existed => debug!("pipeline: removed {}", path.display()),
            Ok(()) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub(crate) fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;
    use tempfile::tempdir;

    fn args_dir(t: &tempfile::TempDir) -> PathBuf {
        t.path().join("bigmic-args")
    }

    #[test]
    fn apply_writes_mic_and_output_when_enabled() {
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        let settings = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };

        apply_to_dir(&settings, &args).unwrap();

        assert!(args.join(MIC_CONF_FILE).exists());
        assert!(args.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_uses_atomic_temp_files() {
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        apply_to_dir(&AppSettings::default(), &args).unwrap();

        assert!(
            std::fs::read_dir(&args)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "tmp"))
                .collect::<Vec<_>>()
                .is_empty()
        );
    }

    #[test]
    fn apply_is_idempotent_across_runs() {
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        let settings = AppSettings::default();
        apply_to_dir(&settings, &args).unwrap();
        let first = read_to_string(args.join(MIC_CONF_FILE)).unwrap();

        apply_to_dir(&settings, &args).unwrap();
        let second = read_to_string(args.join(MIC_CONF_FILE)).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn apply_writes_output_conf_even_when_master_off() {
        // The conf is written unconditionally so the output unit can
        // come up in bypass mode without ever needing a stop/start
        // round trip while audio is playing.
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        let settings = AppSettings::default();
        assert!(!settings.output_filter.enabled);
        apply_to_dir(&settings, &args).unwrap();

        assert!(args.join(MIC_CONF_FILE).exists());
        assert!(
            args.join(OUTPUT_CONF_FILE).exists(),
            "output args must remain ready for the next service start"
        );
    }

    #[test]
    fn apply_keeps_output_conf_after_disable() {
        // Going from enabled → disabled must not remove the args file;
        // the next service start must use the latest settings.
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        let enabled = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        apply_to_dir(&enabled, &args).unwrap();
        assert!(args.join(OUTPUT_CONF_FILE).exists());

        apply_to_dir(&AppSettings::default(), &args).unwrap();
        assert!(args.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_clears_mic_conf_when_all_filters_off() {
        use crate::config::{
            CompressorConfig, EchoCancelConfig, EqualizerConfig, GateConfig, HpfConfig,
            NoiseReductionConfig, StereoConfig,
        };
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);

        apply_to_dir(&AppSettings::default(), &args).unwrap();
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
            echo_cancel: EchoCancelConfig {
                enabled: false,
                ..Default::default()
            },
            ..AppSettings::default()
        };
        apply_to_dir(&off, &args).unwrap();
        assert!(
            !args.join(MIC_CONF_FILE).exists(),
            "mic virtual source must disappear once every filter is off"
        );
    }
}
