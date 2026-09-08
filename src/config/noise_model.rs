// SPDX-License-Identifier: MIT

//! Persisted denoiser identities and the installed LADSPA runtime contract.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Filesystem directory that hosts BigLinux LADSPA plugins.
pub const LADSPA_DIR_PATH: &str = "/usr/lib/ladspa";
/// Absolute path of the GTCRN LADSPA plugin shared object.
pub const GTCRN_LADSPA_PATH: &str = "/usr/lib/ladspa/libgtcrn_ladspa.so";
/// Absolute path of the DeepFilterNet LADSPA plugin shared object.
pub const DEEPFILTER_LADSPA_PATH: &str = "/usr/lib/ladspa/libdeep_filter_ladspa.so";
/// Absolute path of the high-resolution 48 kHz DPDFNet v2 plugin.
const DPDFNET_V2_HR_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet2_48khz_hr_ladspa.so";
/// Absolute path of the high-resolution 48 kHz DPDFNet v8 plugin.
const DPDFNET_V8_HR_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet8_48khz_hr_ladspa.so";
/// Absolute path of the DPDFNet baseline (16 kHz) plugin.
const DPDFNET_BASELINE_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_baseline_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v2 plugin.
const DPDFNET_V2_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet2_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v4 plugin.
const DPDFNET_V4_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet4_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v8 plugin.
const DPDFNET_V8_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet8_ladspa.so";

/// LADSPA plugin label advertised by the GTCRN mono noise reducer.
pub const LABEL_GTCRN_MONO: &str = "gtcrn_mono";
/// LADSPA plugin label advertised by the DeepFilterNet mono reducer.
const LABEL_DEEPFILTER_MONO: &str = "deep_filter_mono";
/// LADSPA plugin label for high-resolution 48 kHz DPDFNet v2 mono.
const LABEL_DPDFNET_V2_HR_MONO: &str = "dpdfnet2_48khz_hr_mono";
/// LADSPA plugin label for high-resolution 48 kHz DPDFNet v8 mono.
const LABEL_DPDFNET_V8_HR_MONO: &str = "dpdfnet8_48khz_hr_mono";
/// LADSPA plugin label for the DPDFNet baseline mono reducer.
const LABEL_DPDFNET_BASELINE_MONO: &str = "baseline_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v2 mono reducer.
const LABEL_DPDFNET_V2_16K_MONO: &str = "dpdfnet2_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v4 mono reducer.
const LABEL_DPDFNET_V4_16K_MONO: &str = "dpdfnet4_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v8 mono reducer.
const LABEL_DPDFNET_V8_16K_MONO: &str = "dpdfnet8_mono";

/// Models exposed to the realtime LAVFI/PipeWire playback chain.
///
/// The order here is the order shown to users in the dropdown and stored
/// in their settings file; the persisted discriminants are part of the current
/// settings contract and must not be reshuffled.
pub const REALTIME_LAVFI_MODELS: [NoiseModel; 5] = [
    NoiseModel::GtcrnDns3,
    NoiseModel::GtcrnVctk,
    NoiseModel::DeepFilterNet3,
    NoiseModel::DpdfnetV2Hr,
    NoiseModel::DpdfnetV8Hr,
];

/// Human-readable labels matching [`REALTIME_LAVFI_MODELS`].
pub const REALTIME_LAVFI_MODEL_LABELS: [&str; 5] = [
    "GTCRN - DNS3",
    "GTCRN - VCTK",
    "DeepFilterNet3",
    "DPDFNet-2 HR",
    "DPDFNet-8 HR",
];

/// Stable identifier of a noise-reduction model.
///
/// The discriminants are serialized as `u8` via `serde` and stored in
/// user-facing settings files, so existing values must keep their numeric
/// meaning forever. Add new variants by appending; never renumber.
///
/// # Examples
///
/// ```
/// use biglinux_microphone::config::noise_model::NoiseModel;
///
/// let model = NoiseModel::GtcrnDns3;
/// assert_eq!(model.ladspa_control(), 0.0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum NoiseModel {
    /// GTCRN model trained on the DNS3 corpus. Default selection.
    #[default]
    GtcrnDns3 = 0,
    /// GTCRN model trained on the VCTK corpus.
    GtcrnVctk = 1,
    /// DeepFilterNet3, attenuation-only realtime path.
    DeepFilterNet3 = 2,
    /// High-resolution 48 kHz DPDFNet v2.
    DpdfnetV2Hr = 3,
    /// High-resolution 48 kHz DPDFNet v8.
    DpdfnetV8Hr = 4,
    /// Baseline 16 kHz DPDFNet model.
    DpdfnetBaseline = 5,
    /// 16 kHz DPDFNet v2 model (offline pipelines only).
    DpdfnetV2 = 6,
    /// 16 kHz DPDFNet v4 model (offline pipelines only).
    DpdfnetV4 = 7,
    /// 16 kHz DPDFNet v8 model (offline pipelines only).
    DpdfnetV8 = 8,
}

impl TryFrom<u8> for NoiseModel {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::GtcrnDns3),
            1 => Ok(Self::GtcrnVctk),
            2 => Ok(Self::DeepFilterNet3),
            3 => Ok(Self::DpdfnetV2Hr),
            4 => Ok(Self::DpdfnetV8Hr),
            5 => Ok(Self::DpdfnetBaseline),
            6 => Ok(Self::DpdfnetV2),
            7 => Ok(Self::DpdfnetV4),
            8 => Ok(Self::DpdfnetV8),
            other => Err(format!("unknown NoiseModel value: {other}")),
        }
    }
}

impl From<NoiseModel> for u8 {
    fn from(model: NoiseModel) -> Self {
        model as Self
    }
}

impl NoiseModel {
    /// Resolve an arbitrary `i32` (e.g. from a settings file) to a model.
    ///
    /// Values that do not match any variant fall back to
    /// [`NoiseModel::default`] so corrupt or out-of-range settings never
    /// crash the UI.
    #[must_use]
    pub fn from_i32_or_default(value: i32) -> Self {
        u8::try_from(value)
            .ok()
            .and_then(|v| Self::try_from(v).ok())
            .unwrap_or_default()
    }

    /// Indicates the model can be driven from the realtime LAVFI
    /// playback graph (vs offline-only ffmpeg pipelines); the UI uses
    /// this to filter the dropdown shown in the live denoise panel.
    #[must_use]
    pub fn is_realtime_lavfi_supported(self) -> bool {
        REALTIME_LAVFI_MODELS.contains(&self)
    }

    /// LADSPA `model` control-port value to select this variant.
    ///
    /// For dual-model GTCRN plugins this picks DNS3 (`0.0`) vs VCTK
    /// (`1.0`); for single-model plugins the value is irrelevant and is
    /// returned as `0.0`.
    #[must_use]
    pub fn ladspa_control(self) -> f32 {
        match self {
            Self::GtcrnVctk => 1.0,
            Self::GtcrnDns3
            | Self::DeepFilterNet3
            | Self::DpdfnetV2Hr
            | Self::DpdfnetV8Hr
            | Self::DpdfnetBaseline
            | Self::DpdfnetV2
            | Self::DpdfnetV4
            | Self::DpdfnetV8 => 0.0,
        }
    }

    /// Whether this is one of the 16 kHz DPDFNet variants.
    #[must_use]
    fn is_dpdfnet_16khz(self) -> bool {
        matches!(
            self,
            Self::DpdfnetBaseline | Self::DpdfnetV2 | Self::DpdfnetV4 | Self::DpdfnetV8
        )
    }

    /// Host sample rate this model's LADSPA plugin requires.
    ///
    /// The 16 kHz DPDFNet plugins hard-require a 16 kHz host and abort
    /// (observed as an ffmpeg segfault) at any other rate; every other
    /// model is a 48 kHz full-band network. Callers must `aresample`
    /// to this rate before the `ladspa=` filter.
    #[must_use]
    pub fn lavfi_sample_rate(self) -> u32 {
        if self.is_dpdfnet_16khz() {
            16_000
        } else {
            48_000
        }
    }

    /// The inference runtime this plugin loads at run time, if any.
    ///
    /// Both plugin families resolve their runtime with `dlopen` rather than
    /// recording it in the ELF header — GTCRN through ONNX Runtime's dynamic
    /// loading, DPDFNet through the OpenVINO bindings. DeepFilterNet3 links
    /// `tract` statically and needs nothing. Names are unversioned on purpose:
    /// that is the symlink the loader looks for, and depending on a soname is
    /// exactly what used to break on every distro update.
    #[must_use]
    pub fn runtime_library(self) -> Option<&'static str> {
        match self {
            Self::GtcrnDns3 | Self::GtcrnVctk => Some("libonnxruntime.so"),
            Self::DeepFilterNet3 => None,
            Self::DpdfnetV2Hr
            | Self::DpdfnetV8Hr
            | Self::DpdfnetBaseline
            | Self::DpdfnetV2
            | Self::DpdfnetV4
            | Self::DpdfnetV8 => Some("libopenvino_c.so"),
        }
    }

    /// Whether the plugin file is present **and** the runtime it will load is
    /// resolvable, i.e. selecting this model will actually denoise.
    ///
    /// [`NoiseModel::plugin_available`] only stats the `.so`. That used to be
    /// paired with an `ldd` check, which stopped meaning anything once both
    /// families moved to loading their runtime dynamically: the ELF header no
    /// longer names it, so `ldd` is clean whether or not the runtime exists.
    /// Asking the dynamic loader directly is the only truthful test, and it is
    /// the same lookup the plugin itself will perform.
    ///
    /// A missing runtime is no longer fatal — the plugins pass audio through
    /// instead of aborting — so this predicate exists to stop us *offering* a
    /// model that would silently do nothing.
    ///
    /// Runs a `dlopen`; callers on a hot path must cache the result.
    #[must_use]
    pub fn plugin_loadable(self) -> bool {
        self.plugin_available() && self.runtime_library().is_none_or(runtime_loadable)
    }

    /// Whether the plugin uses a single attenuation control port.
    ///
    /// DeepFilterNet and DPDFNet plugins expose a single attenuation
    /// (`c0`) port instead of the GTCRN multi-port contract, so the
    /// filter-string formatters branch on this predicate.
    #[must_use]
    pub fn is_attenuation_only(self) -> bool {
        !matches!(self, Self::GtcrnDns3 | Self::GtcrnVctk)
    }

    /// Return the on-disk plugin path and its advertised label.
    #[must_use]
    pub fn plugin_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::GtcrnDns3 | Self::GtcrnVctk => (GTCRN_LADSPA_PATH, LABEL_GTCRN_MONO),
            Self::DeepFilterNet3 => (DEEPFILTER_LADSPA_PATH, LABEL_DEEPFILTER_MONO),
            Self::DpdfnetV2Hr => (DPDFNET_V2_HR_LADSPA_PATH, LABEL_DPDFNET_V2_HR_MONO),
            Self::DpdfnetV8Hr => (DPDFNET_V8_HR_LADSPA_PATH, LABEL_DPDFNET_V8_HR_MONO),
            Self::DpdfnetBaseline => (DPDFNET_BASELINE_LADSPA_PATH, LABEL_DPDFNET_BASELINE_MONO),
            Self::DpdfnetV2 => (DPDFNET_V2_16K_LADSPA_PATH, LABEL_DPDFNET_V2_16K_MONO),
            Self::DpdfnetV4 => (DPDFNET_V4_16K_LADSPA_PATH, LABEL_DPDFNET_V4_16K_MONO),
            Self::DpdfnetV8 => (DPDFNET_V8_16K_LADSPA_PATH, LABEL_DPDFNET_V8_16K_MONO),
        }
    }

    /// Whether the plugin file is present on disk right now.
    ///
    /// Performs a `stat` of the path returned by
    /// [`NoiseModel::plugin_and_label`]; callers should cache the result
    /// if they query it on a hot path.
    #[must_use]
    pub fn plugin_available(self) -> bool {
        Path::new(self.plugin_and_label().0).exists()
    }
}

pub fn deepfilter_attenuation_db(strength: f32) -> f64 {
    let strength = if strength.is_nan() {
        0.0
    } else {
        strength.clamp(0.0, 1.0)
    };
    f64::from(strength * strength) * 100.0
}

fn runtime_loadable(name: &str) -> bool {
    let Ok(cname) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: `cname` is a valid NUL-terminated string that outlives the call.
    // `dlopen` returns either null or a handle this function owns and closes;
    // nothing else observes the handle.
    unsafe {
        let handle = libc::dlopen(cname.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return false;
        }
        libc::dlclose(handle);
    }
    true
}

#[cfg(test)]
mod loader_tests {
    #[test]
    fn runtime_loader_accepts_existing_and_rejects_missing_or_nul_names() {
        assert!(super::runtime_loadable("libc.so.6"));
        assert!(!super::runtime_loadable(
            "lib-missing-microphone-fixture.so"
        ));
        assert!(!super::runtime_loadable("libc.so.6\0suffix"));
    }
}
