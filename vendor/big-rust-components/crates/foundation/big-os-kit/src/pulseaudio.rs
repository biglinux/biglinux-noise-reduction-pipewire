// SPDX-License-Identifier: MIT

//! PulseAudio/PipeWire device discovery and control through `pactl`.
//!
//! PipeWire exposes the PulseAudio-compatible control surface on BigLinux
//! desktops, so media apps can share one small GTK-free contract for listing
//! microphone sources, finding the default desktop-audio monitor, and applying
//! mute/volume changes without duplicating subprocess parsing.

use crate::subprocess::BigSubprocessSpec;

/// A microphone/source entry parsed from `pactl list sources`.
#[derive(Debug, Clone, PartialEq)]
pub struct PulseAudioSource {
    /// Stable PulseAudio/PipeWire source name.
    pub name: String,
    /// Human-readable device description, when provided by the server.
    pub description: String,
    /// Whether the source is currently muted.
    pub muted: bool,
    /// Linear volume ratio where `1.0` is 100%.
    pub volume: f32,
}

impl PulseAudioSource {
    /// User-facing label, preferring the hardware description.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }
}

/// A sink entry parsed from `pactl list sinks`.
#[derive(Debug, Clone, PartialEq)]
pub struct PulseAudioSink {
    /// Stable PulseAudio/PipeWire sink name.
    pub name: String,
    /// Human-readable sink description, when provided by the server.
    pub description: String,
    /// Monitor source that captures desktop audio for this sink.
    pub monitor_source: String,
    /// Whether the sink is currently muted.
    pub muted: bool,
    /// Linear volume ratio where `1.0` is 100%.
    pub volume: f32,
}

impl PulseAudioSink {
    /// User-facing label, preferring the hardware description.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }
}

/// Validation error for PulseAudio/PipeWire object names passed to `pactl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PulseAudioObjectNameError {
    /// The name is empty.
    #[error("empty PulseAudio object name")]
    Empty,
    /// The name contains an ASCII control character.
    #[error("PulseAudio object name contains a control character")]
    ControlChar,
    /// The name starts with `-` and could be interpreted as a `pactl` flag.
    #[error("PulseAudio object name looks like a command-line flag")]
    FlagLike,
    /// The name is longer than the allowed local safety cap.
    #[error("PulseAudio object name is too long")]
    TooLong,
    /// The name contains a byte outside the accepted `pactl` identifier set.
    #[error("PulseAudio object name contains an invalid byte")]
    InvalidByte,
}

/// PulseAudio/PipeWire sink/source identifier validated for argv use.
///
/// Names parsed from `pactl list` normally look like
/// `alsa_input.pci-0000_00_1f.3.analog-stereo` or
/// `bluez_sink.AA_BB_CC_DD_EE_FF.a2dp_sink`. The validator accepts ASCII
/// alphanumeric plus `.`, `_`, `-`, `:`, caps length at 256 bytes, and rejects
/// leading `-` to prevent argv smuggling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseAudioObjectName(String);

impl PulseAudioObjectName {
    /// Maximum accepted byte length for a `pactl` object name.
    pub const MAX_LEN: usize = 256;

    /// Validate a PulseAudio/PipeWire object name for safe `pactl` argv use.
    ///
    /// # Errors
    ///
    /// Returns [`PulseAudioObjectNameError`] when the name is empty, flag-like,
    /// too long, contains control characters, or contains unsupported bytes.
    pub fn parse(value: &str) -> Result<Self, PulseAudioObjectNameError> {
        if value.is_empty() {
            return Err(PulseAudioObjectNameError::Empty);
        }
        if value.bytes().any(is_argv_control_byte) {
            return Err(PulseAudioObjectNameError::ControlChar);
        }
        if value.starts_with('-') {
            return Err(PulseAudioObjectNameError::FlagLike);
        }
        if value.len() > Self::MAX_LEN {
            return Err(PulseAudioObjectNameError::TooLong);
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
        {
            return Err(PulseAudioObjectNameError::InvalidByte);
        }
        Ok(Self(value.to_string()))
    }

    /// Validated name as a borrowed string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// List non-monitor microphone sources.
#[must_use]
pub fn list_sources() -> Vec<PulseAudioSource> {
    let Some(text) = run_pactl(&["list", "sources"]) else {
        return Vec::new();
    };
    parse_sources_blocks(&text)
}

/// Return the default sink, including its monitor source for desktop audio.
#[must_use]
pub fn default_sink() -> Option<PulseAudioSink> {
    let default_name = run_pactl(&["get-default-sink"])?.trim().to_string();
    let text = run_pactl(&["list", "sinks"])?;
    parse_sink_blocks(&text)
        .into_iter()
        .find(|sink| sink.name == default_name)
}

/// Set source mute state. Returns `false` for invalid names or failed `pactl`.
pub fn set_source_muted(source: &str, muted: bool) -> bool {
    let Ok(name) = PulseAudioObjectName::parse(source) else {
        return false;
    };
    run_pactl_status(&[
        "set-source-mute",
        name.as_str(),
        if muted { "1" } else { "0" },
    ])
}

/// Set source volume. Values are clamped to `0%..=150%`.
pub fn set_source_volume(source: &str, volume: f32) -> bool {
    let Ok(name) = PulseAudioObjectName::parse(source) else {
        return false;
    };
    let volume_arg = volume_percent_arg(volume);
    run_pactl_status(&["set-source-volume", name.as_str(), &volume_arg])
}

/// Set sink mute state. Returns `false` for invalid names or failed `pactl`.
pub fn set_sink_muted(sink: &str, muted: bool) -> bool {
    let Ok(name) = PulseAudioObjectName::parse(sink) else {
        return false;
    };
    run_pactl_status(&[
        "set-sink-mute",
        name.as_str(),
        if muted { "1" } else { "0" },
    ])
}

/// Set sink volume. Values are clamped to `0%..=150%`.
pub fn set_sink_volume(sink: &str, volume: f32) -> bool {
    let Ok(name) = PulseAudioObjectName::parse(sink) else {
        return false;
    };
    let volume_arg = volume_percent_arg(volume);
    run_pactl_status(&["set-sink-volume", name.as_str(), &volume_arg])
}

fn run_pactl(args: &[&str]) -> Option<String> {
    let output = BigSubprocessSpec::builder()
        .program("pactl")
        .args(args)
        .allow_list(["pactl"])
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout_lossy())
}

fn run_pactl_status(args: &[&str]) -> bool {
    BigSubprocessSpec::builder()
        .program("pactl")
        .args(args)
        .allow_list(["pactl"])
        .build()
        .run()
        .is_ok_and(|output| output.status.success())
}

fn parse_sources_blocks(text: &str) -> Vec<PulseAudioSource> {
    let mut sources = Vec::new();
    let mut current_source: Option<PartialSource> = None;
    for line in text.lines() {
        if line.starts_with("Source #") {
            if let Some(source) = current_source.take().and_then(PartialSource::into_source) {
                sources.push(source);
            }
            current_source = Some(PartialSource::default());
            continue;
        }

        let Some(entry) = current_source.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Name: ") {
            entry.name = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Description: ") {
            entry.description = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Mute: ") {
            entry.muted = Some(rest == "yes");
        } else if trimmed.starts_with("Volume:") {
            entry.volume = parse_volume_percent(trimmed);
        }
    }

    if let Some(source) = current_source.and_then(PartialSource::into_source) {
        sources.push(source);
    }
    sources.retain(|source| !source.name.ends_with(".monitor"));
    sources
}

#[derive(Default)]
struct PartialSource {
    name: Option<String>,
    description: Option<String>,
    muted: Option<bool>,
    volume: Option<f32>,
}

impl PartialSource {
    fn into_source(self) -> Option<PulseAudioSource> {
        let name = self.name?;
        Some(PulseAudioSource {
            name,
            description: self.description.unwrap_or_default(),
            muted: self.muted.unwrap_or(false),
            volume: self.volume.unwrap_or(1.0),
        })
    }
}

fn parse_sink_blocks(text: &str) -> Vec<PulseAudioSink> {
    let mut sinks = Vec::new();
    let mut current_sink: Option<PartialSink> = None;
    for line in text.lines() {
        if line.starts_with("Sink #") {
            if let Some(sink) = current_sink.take().and_then(PartialSink::into_sink) {
                sinks.push(sink);
            }
            current_sink = Some(PartialSink::default());
            continue;
        }

        let Some(entry) = current_sink.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Name: ") {
            entry.name = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Description: ") {
            entry.description = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Monitor Source: ") {
            entry.monitor_source = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Mute: ") {
            entry.muted = Some(rest == "yes");
        } else if trimmed.starts_with("Volume:") {
            entry.volume = parse_volume_percent(trimmed);
        }
    }

    if let Some(sink) = current_sink.and_then(PartialSink::into_sink) {
        sinks.push(sink);
    }
    sinks
}

#[derive(Default)]
struct PartialSink {
    name: Option<String>,
    description: Option<String>,
    monitor_source: Option<String>,
    muted: Option<bool>,
    volume: Option<f32>,
}

impl PartialSink {
    fn into_sink(self) -> Option<PulseAudioSink> {
        let name = self.name?;
        Some(PulseAudioSink {
            name,
            description: self.description.unwrap_or_default(),
            monitor_source: self.monitor_source.unwrap_or_default(),
            muted: self.muted.unwrap_or(false),
            volume: self.volume.unwrap_or(1.0),
        })
    }
}

fn parse_volume_percent(line: &str) -> Option<f32> {
    // Format: "Volume: front-left: 65536 / 100% / 0.00 dB, front-right: ..."
    let percent = line.split('/').nth(1)?.trim().trim_end_matches('%');
    let parsed: f32 = percent.parse().ok()?;
    Some(parsed / 100.0)
}

fn volume_percent_arg(volume: f32) -> String {
    let percent = (volume.clamp(0.0, 1.5) * 100.0).round() as u32;
    format!("{percent}%")
}

fn is_argv_control_byte(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pactl_block_with_description_and_volume() {
        let input = "\
Source #0
\tState: SUSPENDED
\tName: alsa_input.pci-0000_00_1f.3.analog-stereo
\tDescription: Built-in Audio Analog Stereo
\tMute: no
\tVolume: front-left: 42598 /  65% / -11.41 dB,   front-right: 42598 /  65% / -11.41 dB
";
        let sources = parse_sources_blocks(input);
        assert_eq!(sources.len(), 1);
        let source = &sources[0];
        assert_eq!(source.name, "alsa_input.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(source.description, "Built-in Audio Analog Stereo");
        assert!(!source.muted);
        assert!((source.volume - 0.65).abs() < 0.01);
    }

    #[test]
    fn filters_monitor_sources() {
        let input = "\
Source #0
\tName: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
\tDescription: Monitor of Built-in Audio
\tMute: no
\tVolume: front-left: 65536 / 100% / 0.00 dB
Source #1
\tName: alsa_input.pci-0000_00_1f.3.analog-stereo
\tDescription: Built-in Audio
\tMute: yes
\tVolume: front-left: 0 / 0% / -inf dB
";
        let sources = parse_sources_blocks(input);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "alsa_input.pci-0000_00_1f.3.analog-stereo");
        assert!(sources[0].muted);
    }

    #[test]
    fn source_display_name_prefers_description() {
        let source = PulseAudioSource {
            name: "alsa_input.whatever".into(),
            description: "Logitech Webcam C920".into(),
            muted: false,
            volume: 1.0,
        };
        assert_eq!(source.display_name(), "Logitech Webcam C920");
    }

    #[test]
    fn source_display_name_falls_back_to_name() {
        let source = PulseAudioSource {
            name: "alsa_input.whatever".into(),
            description: String::new(),
            muted: false,
            volume: 0.5,
        };
        assert_eq!(source.display_name(), "alsa_input.whatever");
    }

    #[test]
    fn parses_sink_block_with_monitor_source() {
        let input = "\
Sink #0
\tState: RUNNING
\tName: alsa_output.pci-0000_00_1f.3.analog-stereo
\tDescription: Built-in Audio Analog Stereo
\tMonitor Source: alsa_output.pci-0000_00_1f.3.analog-stereo.monitor
\tMute: no
\tVolume: front-left: 49151 /  75% / -6.24 dB,   front-right: 49151 /  75% / -6.24 dB
";
        let sinks = parse_sink_blocks(input);
        assert_eq!(sinks.len(), 1);
        let sink = &sinks[0];
        assert_eq!(sink.name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(sink.description, "Built-in Audio Analog Stereo");
        assert_eq!(
            sink.monitor_source,
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"
        );
        assert!(!sink.muted);
        assert!((sink.volume - 0.75).abs() < 0.01);
    }

    #[test]
    fn object_name_accepts_typical() {
        assert!(PulseAudioObjectName::parse("alsa_input.pci-0000_00_1f.3.analog-stereo").is_ok());
        assert!(PulseAudioObjectName::parse("alsa_output.usb-046d_C920-00.iec958-stereo").is_ok());
        assert!(PulseAudioObjectName::parse("bluez_sink.AA_BB_CC_DD_EE_FF.a2dp_sink").is_ok());
    }

    #[test]
    fn object_name_rejects_hostile() {
        for hostile in [
            "",
            "-evil",
            "name with space",
            "name;rm",
            "name\nrm",
            "name$evil",
            "name/with/slash",
            "foo\x7fbar",
        ] {
            assert!(
                PulseAudioObjectName::parse(hostile).is_err(),
                "should reject {hostile:?}"
            );
        }
    }

    #[test]
    fn object_name_boundary() {
        let ok = "a".repeat(PulseAudioObjectName::MAX_LEN);
        assert!(PulseAudioObjectName::parse(&ok).is_ok());
        let too_long = "a".repeat(PulseAudioObjectName::MAX_LEN + 1);
        assert_eq!(
            PulseAudioObjectName::parse(&too_long),
            Err(PulseAudioObjectNameError::TooLong)
        );
    }

    #[test]
    fn volume_arg_clamps_to_supported_range() {
        assert_eq!(volume_percent_arg(-1.0), "0%");
        assert_eq!(volume_percent_arg(0.625), "63%");
        assert_eq!(volume_percent_arg(2.0), "150%");
    }

    #[test]
    fn invalid_names_fail_before_pactl() {
        assert!(!set_source_muted("-rf", true));
        assert!(!set_source_volume("name$evil", 0.5));
        assert!(!set_sink_muted("name/with/slash", false));
        assert!(!set_sink_volume("name;rm", 0.5));
    }
}
