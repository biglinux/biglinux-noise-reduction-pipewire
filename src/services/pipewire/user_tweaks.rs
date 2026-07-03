//! User-scoped PipeWire / WirePlumber tunables.
//!
//! Bridges the Advanced-mode "Tuning" tab to a pair of drop-in files
//! under `~/.config`:
//!
//! * `~/.config/pipewire/pipewire.conf.d/99-biglinux-microphone-user.conf`
//!   — daemon defaults (`default.clock.quantum`,
//!   `default.clock.allowed-rates`).
//! * `~/.config/wireplumber/wireplumber.conf.d/99-biglinux-microphone-user.conf`
//!   — per-class WirePlumber rules (USB / PCI ALSA headroom, Bluetooth
//!   `node.latency`, BlueZ codec hint).
//!
//! ## Sentinel-`None` semantics
//!
//! Each field is `Option<…>`. `None` means "the user has not customised
//! this control" — we omit the key entirely from the drop-in so the
//! distro defaults shipped by `pipewire-biglinux-config` (and by this
//! package's own `61-biglinux-alsa-headroom.conf`) win unchanged. This
//! keeps the user file *additive*: no surprise overrides to undo when
//! the user toggles a single control back to "Distribution default".
//!
//! ## Atomic writes
//!
//! Every write goes through a `<file>.tmp` rename so a crash mid-write
//! cannot leave a half-rendered config that PipeWire / WirePlumber
//! would refuse to load. We fsync the temp file before the rename and
//! fsync the parent directory after, so the rename is durable on disk
//! before the GUI fires `restart_pipewire_user_stack` — a race we hit
//! in the field where systemctl restarted the daemons faster than the
//! page-cache flushed, and PipeWire re-read the previous file. Empty
//! drop-ins (no field set) are deleted entirely instead of left as
//! zero-byte files.
//!
//! ## Drop-in precedence
//!
//! `99-` prefix beats every distro file (`50-`, `51-`, `61-`) under
//! `~/.config`, and `~/.config` itself beats `/etc` and `/usr/share`
//! per the PipeWire / WirePlumber lookup rules. Removing the user file
//! restores the distro defaults atomically.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Sample-rate options the UI exposes. `Standard` keeps the upstream /
/// distro default of 48 kHz only — every other variant adds the listed
/// rates to `default.clock.allowed-rates` so audiophile DACs can run
/// natively (at the cost of a graph reload when devices switch family).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRates {
    /// `[ 48000 ]` — distro default.
    Standard,
    /// `[ 44100 48000 ]` — ride-along for CD-source streams.
    Cd,
    /// `[ 44100 48000 88200 96000 176400 192000 ]` — full audiophile
    /// family (preserves both the CD and 48 k chains plus their high
    /// multiples).
    HiRes,
}

impl SampleRates {
    fn rates(self) -> &'static [u32] {
        match self {
            Self::Standard => &[48000],
            Self::Cd => &[44100, 48000],
            Self::HiRes => &[44100, 48000, 88200, 96000, 176_400, 192_000],
        }
    }
}

/// Snapshot of every user-controlled tunable. `None` on any field means
/// the user has not customised it — the corresponding key is omitted
/// from the drop-in entirely, so the distro default takes effect.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UserTweaks {
    /// PipeWire `default.clock.quantum`. Frames at 48 kHz.
    pub quantum: Option<u32>,
    /// `api.alsa.headroom` for `alsa_*.usb-*` nodes. Frames.
    pub headroom_usb: Option<u32>,
    /// `api.alsa.headroom` for `alsa_*.pci-*` nodes. Frames.
    pub headroom_pci: Option<u32>,
    /// `node.latency` numerator on Bluetooth nodes; denominator stays
    /// at 48000.
    pub bt_latency: Option<u32>,
    /// `default.clock.allowed-rates` family.
    pub sample_rates: Option<SampleRates>,
    /// `bluez5.enable-sbc-xq` — explicitly true/false. WP 0.5 already
    /// defaults to true; the field exists so users can pin the value
    /// for hardware that negotiates the codec incorrectly.
    pub bt_sbc_xq: Option<bool>,
    /// `wireplumber.settings.bluetooth.autoswitch-to-headset-profile`.
    /// When `Some(true)` the headset flips to HFP/HSP (mono call mode)
    /// the moment any input stream opens. Distro default is `false` so
    /// A2DP stereo stays put; users that need the BT mic flip this on.
    pub bt_call_autoswitch: Option<bool>,
    /// When `Some(true)` set `session.suspend-timeout-seconds = 0` on
    /// every ALSA node so the codec never sleeps — eliminates the
    /// "click on first sound after silence" symptom on HDA / HDMI
    /// hardware. Costs a small amount of laptop battery, hence off by
    /// default.
    pub alsa_no_suspend: Option<bool>,
}

impl UserTweaks {
    /// Read both drop-in files and reconstruct the tweak set. Missing
    /// files / unparsable lines are silently ignored — the field stays
    /// `None`, matching the "user has not customised" semantics. We
    /// never error out: a hand-edited drop-in must not lock the user
    /// out of the UI.
    pub fn load_from_disk() -> Self {
        let mut t = Self::default();
        if let Ok(text) = fs::read_to_string(pipewire_drop_in()) {
            parse_pipewire(&text, &mut t);
        }
        if let Ok(text) = fs::read_to_string(wireplumber_drop_in()) {
            parse_wireplumber(&text, &mut t);
        }
        t
    }

    /// True when at least one field is `Some` — i.e. the user has
    /// customised something. Drives the "Modificado" banner in the UI.
    pub fn is_modified(&self) -> bool {
        self.quantum.is_some()
            || self.headroom_usb.is_some()
            || self.headroom_pci.is_some()
            || self.bt_latency.is_some()
            || self.sample_rates.is_some()
            || self.bt_sbc_xq.is_some()
            || self.bt_call_autoswitch.is_some()
            || self.alsa_no_suspend.is_some()
    }

    /// Write both drop-in files atomically. Files with no relevant
    /// fields are deleted instead of left empty so `wpctl` /
    /// `pw-metadata` listings stay clean.
    pub fn apply(&self) -> io::Result<()> {
        write_or_remove(&pipewire_drop_in(), &render_pipewire(self))?;
        write_or_remove(&wireplumber_drop_in(), &render_wireplumber(self))?;
        Ok(())
    }
}

// ── Path helpers ──────────────────────────────────────────────────────

fn config_root() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".config")
    })
}

fn pipewire_drop_in() -> PathBuf {
    config_root()
        .join("pipewire")
        .join("pipewire.conf.d")
        .join("99-biglinux-microphone-user.conf")
}

fn wireplumber_drop_in() -> PathBuf {
    config_root()
        .join("wireplumber")
        .join("wireplumber.conf.d")
        .join("99-biglinux-microphone-user.conf")
}

// ── Atomic write / delete ─────────────────────────────────────────────

fn write_or_remove(path: &Path, body: &str) -> io::Result<()> {
    if body.trim().is_empty() {
        return remove_if_exists(path);
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)?;

    let temporary_override_file = path.with_extension("conf.tmp");
    {
        let mut f = File::create(&temporary_override_file)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&temporary_override_file, path)?;

    // fsync the parent directory so the rename is durable before
    // systemctl restarts pipewire — otherwise the daemons can re-read
    // the previous file from page cache while the new one is still
    // unflushed. Best-effort: some filesystems return EINVAL on dir
    // fsync (legacy FAT, some FUSE backends); we accept that since the
    // `sync_all` on the temp file above already gives data durability.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// ── Renderers ─────────────────────────────────────────────────────────

const HEADER: &str = "# Generated by biglinux-microphone Tuning tab.\n\
# Edit via the GUI — manual edits are preserved when possible but the\n\
# UI is authoritative on next Apply.\n";

fn render_pipewire(t: &UserTweaks) -> String {
    let mut props: Vec<String> = Vec::new();
    if let Some(q) = t.quantum {
        props.push(format!("    default.clock.quantum = {q}"));
    }
    if let Some(family) = t.sample_rates {
        let rates = family
            .rates()
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        props.push(format!("    default.clock.allowed-rates = [ {rates} ]"));
    }
    if props.is_empty() {
        return String::new();
    }
    let mut out = String::from(HEADER);
    out.push_str("\ncontext.properties = {\n");
    for line in &props {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn render_wireplumber(t: &UserTweaks) -> String {
    let mut alsa_blocks: Vec<String> = Vec::new();
    if let Some(h) = t.headroom_usb {
        alsa_blocks.push(alsa_rule(
            "~alsa_input.usb-.*",
            "~alsa_output.usb-.*",
            "api.alsa.headroom",
            &h.to_string(),
        ));
    }
    if let Some(h) = t.headroom_pci {
        alsa_blocks.push(alsa_rule(
            "~alsa_input.pci-.*",
            "~alsa_output.pci-.*",
            "api.alsa.headroom",
            &h.to_string(),
        ));
    }
    if let Some(true) = t.alsa_no_suspend {
        alsa_blocks.push(alsa_rule(
            "~alsa_input.*",
            "~alsa_output.*",
            "session.suspend-timeout-seconds",
            "0",
        ));
    }

    let mut bluez_blocks: Vec<String> = Vec::new();
    if let Some(latency) = t.bt_latency {
        bluez_blocks.push(bluez_rule("node.latency", &format!("\"{latency}/48000\"")));
    }

    let bluez_props = t
        .bt_sbc_xq
        .map(|on| format!("monitor.bluez.properties = {{\n    bluez5.enable-sbc-xq = {on}\n}}\n"));

    let wp_settings = t.bt_call_autoswitch.map(|on| {
        format!(
            "wireplumber.settings = {{\n\
             \x20\x20\x20\x20bluetooth.autoswitch-to-headset-profile = {on}\n\
             }}\n"
        )
    });

    if alsa_blocks.is_empty()
        && bluez_blocks.is_empty()
        && bluez_props.is_none()
        && wp_settings.is_none()
    {
        return String::new();
    }

    let mut out = String::from(HEADER);
    if let Some(settings) = wp_settings {
        out.push('\n');
        out.push_str(&settings);
    }
    if !alsa_blocks.is_empty() {
        out.push_str("\nmonitor.alsa.rules = [\n");
        for block in &alsa_blocks {
            out.push_str(block);
        }
        out.push_str("]\n");
    }
    if !bluez_blocks.is_empty() {
        out.push_str("\nmonitor.bluez.rules = [\n");
        for block in &bluez_blocks {
            out.push_str(block);
        }
        out.push_str("]\n");
    }
    if let Some(props) = bluez_props {
        out.push('\n');
        out.push_str(&props);
    }
    out
}

fn alsa_rule(input: &str, output: &str, key: &str, value: &str) -> String {
    format!(
        "  {{\n\
         \x20\x20\x20\x20matches = [\n\
         \x20\x20\x20\x20\x20\x20{{ node.name = \"{input}\" }}\n\
         \x20\x20\x20\x20\x20\x20{{ node.name = \"{output}\" }}\n\
         \x20\x20\x20\x20]\n\
         \x20\x20\x20\x20actions = {{ update-props = {{ {key} = {value} }} }}\n\
         \x20\x20}}\n"
    )
}

fn bluez_rule(key: &str, value: &str) -> String {
    format!(
        "  {{\n\
         \x20\x20\x20\x20matches = [\n\
         \x20\x20\x20\x20\x20\x20{{ node.name = \"~bluez_input.*\" }}\n\
         \x20\x20\x20\x20\x20\x20{{ node.name = \"~bluez_output.*\" }}\n\
         \x20\x20\x20\x20]\n\
         \x20\x20\x20\x20actions = {{ update-props = {{ {key} = {value} }} }}\n\
         \x20\x20}}\n"
    )
}

// ── Parsers (round-trip only) ─────────────────────────────────────────
//
// Best-effort: we do not try to be a full SPA-JSON parser. We only need
// to round-trip whatever `apply()` wrote, which is a fixed shape, so a
// line-oriented scan is enough. Anything we don't recognise stays as
// `None` and the user can re-Apply to overwrite the file.

fn parse_pipewire(text: &str, out: &mut UserTweaks) {
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(v) = strip_kv(line, "default.clock.quantum") {
            out.quantum = v.parse().ok();
        } else if let Some(rest) = line.strip_prefix("default.clock.allowed-rates") {
            // `key = [ 44100 48000 ... ]`
            if let Some(inside) = rest
                .trim_start()
                .strip_prefix('=')
                .map(str::trim_start)
                .and_then(|s| s.strip_prefix('['))
                .and_then(|s| s.strip_suffix(']'))
            {
                let nums: Vec<u32> = inside
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                out.sample_rates = match nums.as_slice() {
                    [48000] => Some(SampleRates::Standard),
                    [44100, 48000] => Some(SampleRates::Cd),
                    [44100, 48000, 88200, 96000, 176_400, 192_000] => Some(SampleRates::HiRes),
                    _ => None,
                };
            }
        }
    }
}

fn parse_wireplumber(text: &str, out: &mut UserTweaks) {
    // Track which family (usb / pci / generic alsa / bluez) we are
    // inside. The renderer always emits the input pattern first, so we
    // use that as the section marker.
    let mut current: Option<&'static str> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.contains("\"~alsa_input.usb-.*\"") {
            current = Some("usb");
        } else if line.contains("\"~alsa_input.pci-.*\"") {
            current = Some("pci");
        } else if line.contains("\"~alsa_input.*\"") {
            current = Some("alsa_all");
        } else if line.contains("\"~bluez_input.*\"") {
            current = Some("bluez");
        }
        if let Some(headroom) =
            extract_action_value(line, "api.alsa.headroom").and_then(|s| s.parse::<u32>().ok())
        {
            match current {
                Some("usb") => out.headroom_usb = Some(headroom),
                Some("pci") => out.headroom_pci = Some(headroom),
                _ => {}
            }
        }
        if let Some(value) = extract_action_value(line, "session.suspend-timeout-seconds") {
            if matches!(current, Some("alsa_all")) && value == "0" {
                out.alsa_no_suspend = Some(true);
            }
        }
        if let Some(latency) = extract_action_value(line, "node.latency")
            .and_then(|s| s.trim_matches('"').split('/').next().map(str::to_owned))
            .and_then(|s| s.parse::<u32>().ok())
        {
            if matches!(current, Some("bluez")) {
                out.bt_latency = Some(latency);
            }
        }
        if let Some(v) = strip_kv(line, "bluez5.enable-sbc-xq") {
            out.bt_sbc_xq = match v {
                "true" => Some(true),
                "false" => Some(false),
                _ => out.bt_sbc_xq,
            };
        }
        if let Some(v) = strip_kv(line, "bluetooth.autoswitch-to-headset-profile") {
            out.bt_call_autoswitch = match v {
                "true" => Some(true),
                "false" => Some(false),
                _ => out.bt_call_autoswitch,
            };
        }
    }
}

fn strip_kv<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    Some(rest)
}

/// Pull `<key> = <value>` out of a line that might also wrap the
/// assignment in `update-props = { … }`. We don't try to handle multi-
/// line `{}` blocks because the renderer keeps each rule on one update-
/// props line.
fn extract_action_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    let end = rest.find(['}', ',']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantum_only() -> UserTweaks {
        UserTweaks {
            quantum: Some(2048),
            ..UserTweaks::default()
        }
    }

    fn fully_populated() -> UserTweaks {
        UserTweaks {
            quantum: Some(2048),
            headroom_usb: Some(1024),
            headroom_pci: Some(512),
            bt_latency: Some(2048),
            sample_rates: Some(SampleRates::HiRes),
            bt_sbc_xq: Some(true),
            bt_call_autoswitch: Some(true),
            alsa_no_suspend: Some(true),
        }
    }

    #[test]
    fn default_is_unmodified() {
        assert!(!UserTweaks::default().is_modified());
    }

    #[test]
    fn any_set_field_marks_modified() {
        assert!(quantum_only().is_modified());
    }

    #[test]
    fn empty_render_is_empty_string() {
        assert!(render_pipewire(&UserTweaks::default()).is_empty());
        assert!(render_wireplumber(&UserTweaks::default()).is_empty());
    }

    #[test]
    fn pipewire_render_round_trips() {
        let original = quantum_only();
        let body = render_pipewire(&original);
        assert!(body.contains("default.clock.quantum = 2048"));
        let mut parsed = UserTweaks::default();
        parse_pipewire(&body, &mut parsed);
        assert_eq!(parsed.quantum, Some(2048));
    }

    #[test]
    fn pipewire_render_emits_allowed_rates() {
        let t = UserTweaks {
            sample_rates: Some(SampleRates::HiRes),
            ..UserTweaks::default()
        };
        let body = render_pipewire(&t);
        assert!(body.contains("44100 48000 88200 96000 176400 192000"));
        let mut parsed = UserTweaks::default();
        parse_pipewire(&body, &mut parsed);
        assert_eq!(parsed.sample_rates, Some(SampleRates::HiRes));
    }

    #[test]
    fn wireplumber_render_emits_per_class_alsa_blocks() {
        let t = UserTweaks {
            headroom_usb: Some(1024),
            headroom_pci: Some(512),
            ..UserTweaks::default()
        };
        let body = render_wireplumber(&t);
        assert!(body.contains("~alsa_input.usb-.*"));
        assert!(body.contains("~alsa_input.pci-.*"));
        assert!(body.contains("api.alsa.headroom = 1024"));
        assert!(body.contains("api.alsa.headroom = 512"));
    }

    #[test]
    fn wireplumber_render_round_trips_full() {
        let original = fully_populated();
        let body = render_wireplumber(&original);
        let mut parsed = UserTweaks::default();
        parse_wireplumber(&body, &mut parsed);
        assert_eq!(parsed.headroom_usb, Some(1024));
        assert_eq!(parsed.headroom_pci, Some(512));
        assert_eq!(parsed.bt_latency, Some(2048));
        assert_eq!(parsed.bt_sbc_xq, Some(true));
        assert_eq!(parsed.bt_call_autoswitch, Some(true));
        assert_eq!(parsed.alsa_no_suspend, Some(true));
    }

    #[test]
    fn wireplumber_render_emits_call_autoswitch_setting() {
        let t = UserTweaks {
            bt_call_autoswitch: Some(true),
            ..UserTweaks::default()
        };
        let body = render_wireplumber(&t);
        assert!(body.contains("bluetooth.autoswitch-to-headset-profile = true"));
        let mut parsed = UserTweaks::default();
        parse_wireplumber(&body, &mut parsed);
        assert_eq!(parsed.bt_call_autoswitch, Some(true));
    }

    #[test]
    fn wireplumber_render_emits_alsa_no_suspend_block() {
        let t = UserTweaks {
            alsa_no_suspend: Some(true),
            ..UserTweaks::default()
        };
        let body = render_wireplumber(&t);
        assert!(body.contains("~alsa_input.*"));
        assert!(body.contains("session.suspend-timeout-seconds = 0"));
        let mut parsed = UserTweaks::default();
        parse_wireplumber(&body, &mut parsed);
        assert_eq!(parsed.alsa_no_suspend, Some(true));
    }

    #[test]
    fn is_modified_reflects_any_single_field() {
        assert!(!UserTweaks::default().is_modified());
        // A single set field anywhere in the OR chain marks it modified.
        assert!(UserTweaks {
            quantum: Some(1024),
            ..UserTweaks::default()
        }
        .is_modified());
        assert!(UserTweaks {
            alsa_no_suspend: Some(true),
            ..UserTweaks::default()
        }
        .is_modified());
    }

    #[test]
    fn parse_pipewire_standard_and_cd_rate_sets() {
        let mut standard = UserTweaks::default();
        parse_pipewire("default.clock.allowed-rates = [ 48000 ]", &mut standard);
        assert_eq!(standard.sample_rates, Some(SampleRates::Standard));

        let mut cd = UserTweaks::default();
        parse_pipewire("default.clock.allowed-rates = [ 44100 48000 ]", &mut cd);
        assert_eq!(cd.sample_rates, Some(SampleRates::Cd));

        // An unknown rate set stays None.
        let mut unknown = UserTweaks::default();
        parse_pipewire("default.clock.allowed-rates = [ 12345 ]", &mut unknown);
        assert_eq!(unknown.sample_rates, None);
    }

    #[test]
    fn parse_wireplumber_bluetooth_false_values() {
        let text = "bluez5.enable-sbc-xq = false\n\
                    bluetooth.autoswitch-to-headset-profile = false\n";
        let mut parsed = UserTweaks::default();
        parse_wireplumber(text, &mut parsed);
        assert_eq!(parsed.bt_sbc_xq, Some(false));
        assert_eq!(parsed.bt_call_autoswitch, Some(false));
    }

    #[test]
    fn parse_wireplumber_suspend_zero_only_in_alsa_all_section() {
        // suspend-timeout 0 inside a USB section must NOT enable alsa_no_suspend
        // (pins the `current == alsa_all && value == "0"` guard against `||`).
        let usb_section = "matches = \"~alsa_input.usb-.*\"\n\
                           session.suspend-timeout-seconds = 0\n";
        let mut parsed = UserTweaks::default();
        parse_wireplumber(usb_section, &mut parsed);
        assert_eq!(parsed.alsa_no_suspend, None);
    }

    #[test]
    fn wireplumber_render_omits_new_fields_when_none() {
        let body = render_wireplumber(&UserTweaks {
            bt_latency: Some(2048),
            ..UserTweaks::default()
        });
        assert!(!body.contains("autoswitch-to-headset-profile"));
        assert!(!body.contains("session.suspend-timeout-seconds"));
    }

    #[test]
    fn wireplumber_render_omits_blocks_for_unset_fields() {
        let t = UserTweaks {
            bt_latency: Some(2048),
            ..UserTweaks::default()
        };
        let body = render_wireplumber(&t);
        assert!(!body.contains("alsa_input"));
        assert!(body.contains("bluez_input"));
    }

    #[test]
    fn write_or_remove_creates_file_then_deletes_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("conf.conf");
        write_or_remove(&path, "body\n").unwrap();
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "body\n");
        write_or_remove(&path, "").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn write_or_remove_is_idempotent_on_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.conf");
        write_or_remove(&path, "").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn extract_action_value_handles_brace_terminator() {
        let line = "actions = { update-props = { api.alsa.headroom = 1024 } }";
        assert_eq!(
            extract_action_value(line, "api.alsa.headroom"),
            Some("1024")
        );
    }

    #[test]
    fn extract_action_value_returns_none_for_missing_key() {
        let line = "actions = { update-props = { node.latency = \"2048/48000\" } }";
        assert_eq!(extract_action_value(line, "api.alsa.headroom"), None);
    }
}
