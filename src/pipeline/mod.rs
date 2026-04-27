//! Filter-chain configuration pipeline.
//!
//! [`apply`] is the single entry point used by the service layer to
//! materialise two files on disk:
//!
//! | File | Role |
//! |------|------|
//! | `~/.config/pipewire/filter-chain.conf.d/10-biglinux-microphone.conf` | Mic filter-chain |
//! | `~/.config/pipewire/biglinux-microphone-output.conf`                  | Output filter-chain (standalone) |
//!
//! Each file is written via the atomic write helper so a partial write
//! cannot leave PipeWire reading a truncated config.
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

use log::{debug, info};

use crate::config::AppSettings;

pub use echo_cancel::{
    build_echo_cancel_conf as build_echo_cancel_conf_for, echo_cancel_wanted,
    ECHO_CANCEL_CONF_FILE, EC_CAPTURE_NODE_NAME, EC_SOURCE_NAME,
};
pub use mic::{
    ai_node_in_mic_chain, build_mic_conf as build_mic_conf_for, mic_chain_wanted,
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

/// XDG base dir for PipeWire drop-in configs (loaded by
/// `filter-chain.service`).
#[must_use]
pub fn pipewire_drop_in_dir() -> PathBuf {
    xdg_config_root().join("pipewire/filter-chain.conf.d")
}

/// XDG base dir that holds **standalone** PipeWire config files — i.e.
/// the ones consumed directly by `pipewire -c <path>` from dedicated
/// systemd units (not drop-ins). Lives at `~/.config/pipewire/`.
#[must_use]
pub fn pipewire_standalone_dir() -> PathBuf {
    xdg_config_root().join("pipewire")
}

/// XDG base dir that previously held WirePlumber drop-in configs for
/// per-app routing. Kept around so [`apply`] can scrub legacy files
/// from earlier installs.
#[must_use]
pub fn wireplumber_drop_in_dir() -> PathBuf {
    xdg_config_root().join("wireplumber/wireplumber.conf.d")
}

/// Absolute path of the mic filter-chain drop-in file.
#[must_use]
pub fn mic_conf_path() -> PathBuf {
    pipewire_drop_in_dir().join(MIC_CONF_FILE)
}

/// Absolute path of the output filter-chain config. The output chain
/// runs in its own `pipewire -c` process (see
/// `biglinux-microphone-output.service`) so the file lives outside
/// `filter-chain.conf.d/`.
#[must_use]
pub fn output_conf_path() -> PathBuf {
    pipewire_standalone_dir().join(OUTPUT_CONF_FILE)
}

/// Absolute path of the echo-cancel standalone config. Hosted by
/// `biglinux-microphone-echocancel.service` and only created when the
/// user toggles AEC on.
#[must_use]
pub fn echo_cancel_conf_path() -> PathBuf {
    pipewire_standalone_dir().join(ECHO_CANCEL_CONF_FILE)
}

/// File name of the WirePlumber drop-in shipped by the per-app routing
/// implementation. Removed on every [`apply`] so a stale rule cannot
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
}

/// Materialise every config file under the default XDG paths.
pub fn apply(settings: &AppSettings) -> io::Result<()> {
    apply_to_dirs(
        settings,
        &pipewire_drop_in_dir(),
        &pipewire_standalone_dir(),
        &wireplumber_drop_in_dir(),
    )
}

/// Write the generated files under explicit directories. This is the
/// variant tests use to avoid touching the user's real config.
///
/// `pipewire_dropin_dir` hosts the mic drop-in; `pipewire_standalone_dir`
/// hosts the standalone output config consumed by the dedicated output
/// systemd unit; `wireplumber_dir` is scanned for the legacy per-app
/// routing rules file (deleted on every call).
pub fn apply_to_dirs(
    settings: &AppSettings,
    pipewire_dropin_dir: &Path,
    pipewire_standalone_dir: &Path,
    wireplumber_dir: &Path,
) -> io::Result<()> {
    fs::create_dir_all(pipewire_dropin_dir)?;
    fs::create_dir_all(pipewire_standalone_dir)?;

    // Mic chain: only materialise it when the user wants *any* mic
    // processing. Otherwise we'd leave a "BigLinux Microphone" virtual
    // source hanging in the PipeWire graph even with every toggle off.
    let mic_path = pipewire_dropin_dir.join(MIC_CONF_FILE);
    if mic::mic_chain_wanted(settings) {
        atomic_write(&mic_path, mic::build_mic_conf(settings).as_bytes())?;
        info!("pipeline: wrote mic config to {}", mic_path.display());
    } else {
        remove_if_exists(&mic_path)?;
        info!("pipeline: mic chain idle, {} cleared", mic_path.display());
    }

    // Output chain lives in its own `pipewire` process (see
    // `biglinux-microphone-output.service`) because GTCRN keeps a
    // per-process singleton — two instances in the same daemon crash.
    //
    // The conf is written unconditionally so the standalone unit can
    // come up in **bypass mode** even when `output_filter.enabled` is
    // false. Stopping the unit would tear down the smart-filter sink,
    // and browsers (Chromium especially) pause HTMLMediaElement when
    // their output sink disappears mid-playback. Bypassing inside the
    // graph (GTCRN Enable=0, gate floor, compressor unity, HPF
    // pass-through) keeps the sink present and the streams attached.
    let out_path = pipewire_standalone_dir.join(OUTPUT_CONF_FILE);
    atomic_write(&out_path, output::build_output_conf(settings).as_bytes())?;
    info!("pipeline: wrote output config to {}", out_path.display());

    // Echo-cancel chain is opt-in (default off). Materialise the conf
    // only when wanted so the dedicated systemd unit can use the conf's
    // presence as a "should I run?" signal.
    let ec_path = pipewire_standalone_dir.join(ECHO_CANCEL_CONF_FILE);
    if echo_cancel::echo_cancel_wanted(settings) {
        atomic_write(
            &ec_path,
            echo_cancel::build_echo_cancel_conf(settings).as_bytes(),
        )?;
        info!(
            "pipeline: wrote echo-cancel config to {}",
            ec_path.display()
        );
    } else {
        remove_if_exists(&ec_path)?;
        info!("pipeline: echo-cancel idle, {} cleared", ec_path.display());
    }

    // Legacy cleanup: earlier versions wrote the output chain into the
    // filter-chain drop-in directory alongside the mic, and shipped a
    // WirePlumber rule for per-app routing. Both must be deleted so an
    // upgraded install doesn't load a phantom second GTCRN or pin
    // streams to a non-existent target.
    remove_if_exists(&pipewire_dropin_dir.join("20-biglinux-output.conf"))?;
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

/// Remove every file [`apply`] would have written. Used on uninstall and
/// from the UI's "reset to defaults" action. Missing files are not an
/// error.
pub fn remove_all() -> io::Result<()> {
    let legacy_routing = wireplumber_drop_in_dir().join(LEGACY_ROUTING_CONF_FILE);
    for path in [
        mic_conf_path(),
        output_conf_path(),
        echo_cancel_conf_path(),
        legacy_routing,
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
            t.path().join("pw-dropin"),
            t.path().join("pw-standalone"),
            t.path().join("wp"),
        )
    }

    #[test]
    fn apply_writes_mic_and_output_when_enabled() {
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        let settings = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };

        apply_to_dirs(&settings, &pw_dropin, &pw_standalone, &wp).unwrap();

        assert!(pw_dropin.join(MIC_CONF_FILE).exists());
        assert!(pw_standalone.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_uses_atomic_temp_files() {
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        apply_to_dirs(&AppSettings::default(), &pw_dropin, &pw_standalone, &wp).unwrap();

        for scan in [&pw_dropin, &pw_standalone] {
            if !scan.exists() {
                continue;
            }
            for entry in fs::read_dir(scan).unwrap() {
                let p = entry.unwrap().path();
                assert_ne!(p.extension().and_then(|s| s.to_str()), Some("tmp"));
            }
        }
    }

    #[test]
    fn apply_is_idempotent_across_runs() {
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        let settings = AppSettings::default();
        apply_to_dirs(&settings, &pw_dropin, &pw_standalone, &wp).unwrap();
        let first = fs::read_to_string(pw_dropin.join(MIC_CONF_FILE)).unwrap();

        apply_to_dirs(&settings, &pw_dropin, &pw_standalone, &wp).unwrap();
        let second = fs::read_to_string(pw_dropin.join(MIC_CONF_FILE)).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn apply_writes_output_conf_even_when_master_off() {
        // The conf is written unconditionally so the standalone unit
        // can come up in bypass mode without ever needing a stop/start
        // round trip while audio is playing.
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        let settings = AppSettings::default();
        assert!(!settings.output_filter.enabled);
        apply_to_dirs(&settings, &pw_dropin, &pw_standalone, &wp).unwrap();

        assert!(pw_dropin.join(MIC_CONF_FILE).exists());
        assert!(
            pw_standalone.join(OUTPUT_CONF_FILE).exists(),
            "output conf must always be present so the unit can run in bypass"
        );
    }

    #[test]
    fn apply_keeps_output_conf_after_disable() {
        // Going from enabled → disabled inside a session must not
        // remove the conf — see `apply_writes_output_conf_even_when_master_off`
        // for the rationale.
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        let enabled = AppSettings {
            output_filter: crate::config::OutputFilterSettings {
                enabled: true,
                ..crate::config::OutputFilterSettings::default()
            },
            ..AppSettings::default()
        };
        apply_to_dirs(&enabled, &pw_dropin, &pw_standalone, &wp).unwrap();
        assert!(pw_standalone.join(OUTPUT_CONF_FILE).exists());

        apply_to_dirs(&AppSettings::default(), &pw_dropin, &pw_standalone, &wp).unwrap();
        assert!(pw_standalone.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_removes_legacy_routing_drop_in() {
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);
        fs::create_dir_all(&wp).unwrap();
        let legacy = wp.join(LEGACY_ROUTING_CONF_FILE);
        fs::write(&legacy, b"# stale per-app rule\n").unwrap();

        apply_to_dirs(&AppSettings::default(), &pw_dropin, &pw_standalone, &wp).unwrap();

        assert!(
            !legacy.exists(),
            "legacy WirePlumber routing rule must be deleted on apply"
        );
    }

    #[test]
    fn apply_clears_mic_conf_when_all_filters_off() {
        use crate::config::{
            CompressorConfig, EchoCancelConfig, EqualizerConfig, GateConfig, HpfConfig,
            NoiseReductionConfig, StereoConfig,
        };
        let dir = tempdir().unwrap();
        let (pw_dropin, pw_standalone, wp) = dirs(&dir);

        apply_to_dirs(&AppSettings::default(), &pw_dropin, &pw_standalone, &wp).unwrap();
        assert!(pw_dropin.join(MIC_CONF_FILE).exists());

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
        apply_to_dirs(&off, &pw_dropin, &pw_standalone, &wp).unwrap();
        assert!(
            !pw_dropin.join(MIC_CONF_FILE).exists(),
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
