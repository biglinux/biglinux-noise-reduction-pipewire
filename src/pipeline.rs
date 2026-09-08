//! Filter-chain configuration pipeline.
//!
//! Each loader hosts a module in its own client context. PipeWire schedules
//! the exported nodes as part of the shared graph; the clients retain their
//! own processing threads. Generated arguments follow XDG_CONFIG_HOME.

mod echo_cancel;
mod graph;
mod mic;
mod migration;
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

#[must_use]
pub fn pwloader_args_dir() -> PathBuf {
    xdg_config_root().join("biglinux-microphone")
}

#[must_use]
pub fn mic_conf_path() -> PathBuf {
    pwloader_args_dir().join(MIC_CONF_FILE)
}

#[must_use]
pub fn output_conf_path() -> PathBuf {
    pwloader_args_dir().join(OUTPUT_CONF_FILE)
}

#[must_use]
pub fn echo_cancel_conf_path() -> PathBuf {
    pwloader_args_dir().join(ECHO_CANCEL_CONF_FILE)
}

pub const LEGACY_ROUTING_CONF_FILE: &str = "50-biglinux-output-routing.conf";

/// Only application-owned names belong here. In particular,
/// `pipewire/filter-chain.conf` is a user configuration, not ours to remove.
const LEGACY_FILES: &[&str] = &[
    "pipewire/filter-chain.conf.d/source-gtcrn-smart.conf",
    "pipewire/filter-chain.conf.d/source-ulunas-smart.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise-smart.conf",
    "pipewire/filter-chain.conf.d/source-rnnoise-config.conf",
    "pipewire/filter-chain.conf.d/big-output-filter.conf",
    "pipewire/big-output-filter.conf",
    "pipewire/filter-chain.conf.d/20-biglinux-output.conf",
    "pipewire/pipewire.conf.d/50-biglinux-microphone-realtime.conf",
    "wireplumber/wireplumber.conf.d/50-biglinux-output-routing.conf",
    "wireplumber/wireplumber.conf.d/50-biglinux-microphone-routing.conf",
    "pipewire/biglinux-microphone-echocancel.conf",
    "pipewire/filter-chain.conf.d/05-biglinux-echocancel.conf",
    "pipewire/filter-chain.conf.d/10-biglinux-microphone.conf",
    "pipewire/biglinux-microphone-output.conf",
];

fn archive_legacy_under(root: &Path) {
    for rel in LEGACY_FILES {
        let path = root.join(rel);
        if let Err(error) = migration::archive(&path) {
            log::warn!("pipeline: leaving {} unchanged: {error}", path.display());
        }
    }
}

/// Preserve a backup before retiring each old application-owned file.
/// A failed backup leaves the original in place and is reported in the log.
pub fn purge_legacy_files() {
    archive_legacy_under(&xdg_config_root());
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = Path::new(&runtime).join("gtcrn-ladspa-controls");
        let _ = fs::remove_file(&path);
    }
    purge_legacy_services();
}

fn purge_legacy_services() {
    const LEGACY_USER_UNITS: &[&str] = &["biglinux-microphone-echocancel.service"];
    for unit in LEGACY_USER_UNITS {
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

pub fn apply(settings: &AppSettings) -> io::Result<()> {
    let root = xdg_config_root();
    apply_to_dirs(
        settings,
        &pwloader_args_dir(),
        &root.join("pipewire/filter-chain.conf.d"),
        &root.join("wireplumber/wireplumber.conf.d"),
    )
}

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
        migration::archive(&pipewire_dir.join(name))?;
    }
    migration::archive(&wireplumber_dir.join(LEGACY_ROUTING_CONF_FILE))
}

/// Avoid needless atomic replacements, fsyncs and file-monitor events.
fn write_if_changed(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents.as_bytes()) {
        return Ok(());
    }
    atomic_write(path, contents.as_bytes())
}

pub fn apply_to_dir(settings: &AppSettings, args_dir: &Path) -> io::Result<()> {
    let mic_path = args_dir.join(MIC_CONF_FILE);
    if mic::mic_chain_wanted(settings) {
        write_if_changed(&mic_path, &mic::build_mic_conf(settings))?;
    } else {
        remove_file_if_exists(&mic_path)?;
    }

    // Keep output arguments ready, even while the output unit is disabled.
    write_if_changed(
        &args_dir.join(OUTPUT_CONF_FILE),
        &output::build_output_conf(settings),
    )?;

    let ec_path = args_dir.join(ECHO_CANCEL_CONF_FILE);
    if settings.echo_cancel.enabled {
        write_if_changed(&ec_path, &echo_cancel::build_echo_cancel_conf())?;
    } else {
        remove_file_if_exists(&ec_path)?;
    }
    Ok(())
}

pub fn remove_all() -> io::Result<()> {
    for path in [mic_conf_path(), output_conf_path(), echo_cancel_conf_path()] {
        remove_file_if_exists(&path)?;
    }
    Ok(())
}

pub(crate) fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
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
    fn migration_preserves_generic_pipewire_configuration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pipewire/filter-chain.conf");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "my independent filter chain\n").unwrap();
        archive_legacy_under(dir.path());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "my independent filter chain\n"
        );
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
        assert!(fs::read_dir(&args).unwrap().all(|entry| {
            !entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")
        }));
    }

    #[test]
    fn apply_is_idempotent_across_runs() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);
        let settings = AppSettings::default();
        apply_to_dir(&settings, &args).unwrap();
        let path = args.join(MIC_CONF_FILE);
        let first = read_to_string(&path).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        apply_to_dir(&settings, &args).unwrap();
        assert_eq!(first, read_to_string(&path).unwrap());
        assert_eq!(inode, fs::metadata(path).unwrap().ino());
    }

    #[test]
    fn apply_writes_output_conf_even_when_master_off() {
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);
        let settings = AppSettings::default();
        assert!(!settings.output_filter.enabled);
        apply_to_dir(&settings, &args).unwrap();
        assert!(args.join(MIC_CONF_FILE).exists());
        assert!(args.join(OUTPUT_CONF_FILE).exists());
    }

    #[test]
    fn apply_keeps_output_conf_after_disable() {
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
        let dir = tempdir().unwrap();
        let args = args_dir(&dir);
        apply_to_dir(&AppSettings::default(), &args).unwrap();
        assert!(args.join(MIC_CONF_FILE).exists());
        let mut off = AppSettings::default();
        cascade_mic_off(&mut off);
        apply_to_dir(&off, &args).unwrap();
        assert!(!args.join(MIC_CONF_FILE).exists());
    }
}
