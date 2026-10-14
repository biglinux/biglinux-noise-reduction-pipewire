//! Saving and switching scenarios, as one transaction each.
//!
//! Switching a scenario changes a dozen settings at once. Done as a dozen
//! `set` calls it would write the settings file a dozen times, wake the path
//! unit a dozen times, and restart the filter chain under whatever is playing
//! — the same failure that made calibration re-plumb the graph, multiplied.
//! So: one lock, one write, one reconcile.
//!
//! Saving is the opposite: it writes a section of the settings file and
//! reconciles nothing, because remembering what you already have changes no
//! audio at all.

use serde::Serialize;

use crate::config::storage::SettingsLock;
use crate::config::{AppSettings, Profile, ProfileAudio, ProfilesConfig};
use crate::services::pipewire::{set_default_source, snapshot_sources};
use crate::services::reconcile;

#[derive(Debug, Serialize)]
pub struct MicrophoneOutcome {
    pub wanted: Option<String>,
    pub wanted_description: Option<String>,
    pub present: bool,
    pub used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Applied {
    pub applied: String,
    pub label: String,
    pub microphone: MicrophoneOutcome,
}

#[derive(Debug, Serialize)]
pub struct Listed {
    pub name: String,
    pub label: String,
    pub updated: String,
    pub microphone: Option<String>,
    pub microphone_description: Option<String>,
    /// Whether the microphone this scenario wants is connected right now.
    pub microphone_present: bool,
    pub active: bool,
}

/// Everything saved, with the device each one wants and whether it is here.
#[must_use]
pub fn list(settings: &AppSettings) -> Vec<Listed> {
    let (sources, _default) = snapshot_sources();
    let present: Vec<&str> = sources
        .iter()
        .map(|source| source.node_name.as_str())
        .collect();
    settings
        .profiles
        .profiles
        .iter()
        .map(|(name, profile)| Listed {
            name: name.clone(),
            label: profile.label.clone(),
            updated: profile.updated.clone(),
            microphone: profile.microphone.clone(),
            microphone_description: profile.microphone_description.clone(),
            microphone_present: profile
                .microphone
                .as_deref()
                .is_none_or(|wanted| present.contains(&wanted)),
            active: settings.profiles.active.as_deref() == Some(name.as_str()),
        })
        .collect()
}

/// Remember the current preferences under a name. Writes settings, nothing else.
pub fn save(label: &str) -> Result<String, String> {
    let _lock = SettingsLock::acquire().map_err(|error| error.to_string())?;
    let mut settings = AppSettings::load_strict().map_err(|error| error.to_string())?;
    let name = ProfilesConfig::slug(label);
    let (sources, default_id) = snapshot_sources();
    let current = sources
        .iter()
        .find(|source| Some(source.node_id) == default_id);

    let profile = Profile {
        label: label.trim().to_owned(),
        updated: now_rfc3339(),
        microphone: current.map(|source| source.node_name.clone()),
        microphone_description: current.map(|source| source.description.clone()),
        audio: ProfileAudio {
            mic_bypass: settings.mic_bypass,
            noise_reduction: settings.noise_reduction.clone(),
            gate: settings.gate,
            compressor: settings.compressor,
            hpf: settings.hpf.clone(),
            stereo: settings.stereo.clone(),
            equalizer: settings.equalizer.clone(),
            output_filter: settings.output_filter.clone(),
            echo_cancel: settings.echo_cancel.clone(),
            gain_safety: settings.gain_safety,
        },
    };
    settings.profiles.profiles.insert(name.clone(), profile);
    settings.profiles.active = Some(name.clone());
    settings.save().map_err(|error| error.to_string())?;
    Ok(name)
}

/// Forget a scenario. Writes settings, nothing else.
pub fn delete(name: &str) -> Result<(), String> {
    let _lock = SettingsLock::acquire().map_err(|error| error.to_string())?;
    let mut settings = AppSettings::load_strict().map_err(|error| error.to_string())?;
    if settings.profiles.profiles.remove(name).is_none() {
        return Err(format!("there is no scenario called `{name}`"));
    }
    if settings.profiles.active.as_deref() == Some(name) {
        settings.profiles.active = None;
    }
    settings.save().map_err(|error| error.to_string())
}

/// Switch to a scenario: resolve its microphone, copy its preferences, and
/// reconcile the graph once.
///
/// A scenario whose microphone is unplugged is not a failure. Everything else
/// is applied and the missing device is named in the result, because somebody
/// whose interface is switched off still wants their filter settings back.
pub fn apply(name: &str) -> Result<Applied, String> {
    let _lock = SettingsLock::acquire().map_err(|error| error.to_string())?;
    let mut settings = AppSettings::load_strict().map_err(|error| error.to_string())?;
    let profile = settings
        .profiles
        .profiles
        .get(name)
        .cloned()
        .ok_or_else(|| format!("there is no scenario called `{name}`"))?;

    let microphone = switch_microphone(&profile);

    let audio = profile.audio.clone();
    settings.mic_bypass = audio.mic_bypass;
    settings.noise_reduction = audio.noise_reduction;
    settings.gate = audio.gate;
    settings.compressor = audio.compressor;
    settings.hpf = audio.hpf;
    settings.stereo = audio.stereo;
    settings.equalizer = audio.equalizer;
    settings.output_filter = audio.output_filter;
    settings.echo_cancel = audio.echo_cancel;
    settings.gain_safety = audio.gain_safety;
    settings.profiles.active = Some(name.to_owned());

    crate::services::echo::settle(&mut settings.echo_cancel);
    settings.save().map_err(|error| error.to_string())?;
    reconcile::apply(&settings, false).map_err(|error| error.to_string())?;

    Ok(Applied {
        applied: name.to_owned(),
        label: profile.label,
        microphone,
    })
}

/// Point the session at the scenario's microphone, if it is here.
fn switch_microphone(profile: &Profile) -> MicrophoneOutcome {
    let (sources, default_id) = snapshot_sources();
    let current = sources
        .iter()
        .find(|source| Some(source.node_id) == default_id)
        .map(|source| source.node_name.clone());
    let Some(wanted) = profile.microphone.clone() else {
        return MicrophoneOutcome {
            wanted: None,
            wanted_description: None,
            present: true,
            used: current,
            note: None,
        };
    };
    let matches: Vec<_> = sources.iter().filter(|s| s.node_name == wanted).collect();
    let mut outcome = MicrophoneOutcome {
        wanted: Some(wanted),
        wanted_description: profile.microphone_description.clone(),
        present: !matches.is_empty(),
        used: current,
        note: None,
    };
    let [source] = matches.as_slice() else {
        outcome.note = Some(
            if matches.is_empty() {
                "the requested microphone is not connected; everything else was applied"
            } else {
                "multiple microphones have the requested name; none was selected"
            }
            .into(),
        );
        return outcome;
    };
    if Some(source.node_id) == default_id {
        return outcome;
    }
    let changed = set_default_source(source.node_id);
    // set-default changes routing policy for new streams, not an atomic switch
    // of every existing stream. Report the observed default, never the request.
    let (after, default_after) = snapshot_sources();
    let selected = after.iter().find(|s| Some(s.node_id) == default_after);
    outcome.used = selected.map(|s| s.node_name.clone());
    outcome.note = match changed {
        Err(error) => Some(format!(
            "the microphone could not be selected: {error}; everything else was applied"
        )),
        Ok(()) if selected.is_some_and(|s| source.same_generation(s)) => None,
        Ok(()) => Some(
            "the requested microphone selection was not confirmed; everything else was applied"
                .into(),
        ),
    };
    outcome
}

fn now_rfc3339() -> String {
    // No chrono in this crate: seconds since the epoch, formatted by `date`'s
    // rules would need a process. A plain UTC stamp is enough to order a list.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    let days = secs / 86_400;
    let time = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// Howard Hinnant's civil-from-days, the standard branch-free version.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timestamp_is_a_date_somebody_can_read() {
        let stamp = now_rfc3339();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'));
        assert!(stamp[..4].parse::<u32>().unwrap() >= 2026, "{stamp}");
    }

    #[test]
    fn the_epoch_is_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }
}
