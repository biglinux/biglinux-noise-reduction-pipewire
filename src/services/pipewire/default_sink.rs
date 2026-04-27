//! Read the system default audio sink.
//!
//! Used at filter-chain enable time to capture the user's chosen
//! hardware sink so it can be pinned via `filter.smart.target` on the
//! output filter conf. We don't write the default — the user's
//! original sink stays the visible default in every volume control;
//! WirePlumber's smart-filter policy transparently routes streams
//! through `output-biglinux` first.
//!
//! Shells out to `pw-metadata` rather than using the pipewire crate
//! directly: the rest of `services::pipewire` already owns the long-
//! running PW client connection (worker thread) and this helper runs
//! from the UI thread. Keeping it as a one-shot subprocess call avoids
//! cross-thread synchronisation.

use std::process::{Command, Stdio};

/// `node.name` of the current default sink, or `None` when
/// `pw-metadata` returns no value (fresh session, daemon down, or
/// metadata never written).
#[must_use]
pub fn default_sink_name() -> Option<String> {
    let output = Command::new("pw-metadata")
        .args(["0", "default.audio.sink"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_default_sink(&String::from_utf8_lossy(&output.stdout))
}

fn parse_default_sink(stdout: &str) -> Option<String> {
    let needle = "\"name\":";
    let pos = stdout.find(needle)?;
    let after = &stdout[pos + needle.len()..];
    let after = after.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_sink_extracts_name_field() {
        let stdout = "Found \"settings\" metadata 30\nupdate: id:0 \
                      key:'default.audio.sink' value:'{ \"name\": \
                      \"alsa_output.pci-0000_00_1f.3.analog-stereo\" }' \
                      type:'Spa:String:JSON'\n";
        assert_eq!(
            parse_default_sink(stdout),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo".into())
        );
    }

    #[test]
    fn parse_default_sink_handles_missing_value() {
        assert_eq!(parse_default_sink("nothing here"), None);
    }
}
