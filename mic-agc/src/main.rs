//! BigLinux Microphone AGC Service (Native PipeWire Version)
//!
//! Analyzes microphone audio and dynamically adjusts PipeWire capture volume.
//!
//! This version uses native PipeWire Rust bindings instead of spawning pw-cat.
//! Volume control still uses wpctl for simplicity (SPA POD construction is complex).

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;
use serde::Deserialize;
use std::convert::TryInto;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Audio parameters
const FRAMES_PER_DECISION: usize = 50;

// Config file path
const CONFIG_RELPATH: &str = ".config/biglinux-microphone/settings.json";
const CONFIG_RELOAD_SECS: u64 = 5;

// AGC defaults
const DEFAULT_TARGET_LEVEL: i32 = 73;
const DEFAULT_MIN_VOLUME: i32 = 20;
const DEFAULT_MAX_VOLUME: i32 = 100;
const DEFAULT_REACTIVITY: i32 = 50;

// AGC tuning
const DEADZONE_DB: f64 = 2.0;
const VOICE_RATIO_THRESHOLD: f64 = 0.20;  // 20% of frames must be voiced (was 0.10)
const VOICEBAND_ENERGY_RATIO: f64 = 0.30;

// Clipping detection
const CLIP_PEAK_THRESHOLD: f32 = 0.999;
const CLIP_RATIO_THRESHOLD: f64 = 0.10;
const CLIP_STEP: i32 = 3;

// Pitch detection
const PITCH_MIN_LAG: usize = 120;
const PITCH_MAX_LAG: usize = 240;
const PITCH_THRESHOLD: f64 = 0.55;  // Stricter pitch detection (was 0.4)

// Filter output voice detection (post noise-reduction, so can use lower threshold)
const FILTER_VOICE_THRESHOLD: f32 = 0.005;  // ~-46 dBFS, fine since NR removes noise

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct AppSettings {
    #[serde(default)]
    agc: AgcSettings,
}

#[derive(Deserialize, Clone)]
struct AgcSettings {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_target")]
    target_level_dbfs: i32,
    #[serde(default = "default_min")]
    min_volume: i32,
    #[serde(default = "default_max")]
    max_volume: i32,
    #[serde(default = "default_reactivity")]
    reactivity: i32,
}

impl Default for AgcSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            target_level_dbfs: DEFAULT_TARGET_LEVEL,
            min_volume: DEFAULT_MIN_VOLUME,
            max_volume: DEFAULT_MAX_VOLUME,
            reactivity: DEFAULT_REACTIVITY,
        }
    }
}

fn default_true() -> bool { true }
fn default_target() -> i32 { DEFAULT_TARGET_LEVEL }
fn default_min() -> i32 { DEFAULT_MIN_VOLUME }
fn default_max() -> i32 { DEFAULT_MAX_VOLUME }
fn default_reactivity() -> i32 { DEFAULT_REACTIVITY }

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(CONFIG_RELPATH)
}

fn load_config() -> AgcSettings {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<AppSettings>(&content)
            .map(|s| s.agc)
            .unwrap_or_else(|e| {
                eprintln!("biglinux-mic-agc: config parse error: {e}");
                AgcSettings::default()
            }),
        Err(_) => AgcSettings::default(),
    }
}

// ---------------------------------------------------------------------------
// Volume control (uses wpctl)
// ---------------------------------------------------------------------------

fn set_pw_volume(node_id: u32, pct: i32) {
    let vol = f64::from(pct.clamp(0, 100)) / 100.0;
    let _ = Command::new("wpctl")
        .args(["set-volume", &node_id.to_string(), &format!("{vol:.2}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn read_pw_volume(node_id: u32) -> Option<i32> {
    let output = Command::new("wpctl")
        .args(["get-volume", &node_id.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Volume:") {
            let token = rest.split_whitespace().next()?;
            let vol: f64 = token.parse().ok()?;
            return Some((vol * 100.0).round() as i32);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Audio analysis
// ---------------------------------------------------------------------------

fn frame_peak(samples: &[f32]) -> f32 {
    samples.iter().filter(|s| s.is_finite()).map(|s| s.abs()).fold(0.0f32, f32::max)
}

fn has_pitch(samples: &[f32]) -> bool {
    let n = samples.len();
    if n < PITCH_MAX_LAG + 1 {
        return false;
    }

    let mut energy = 0.0f64;
    for &s in samples {
        if s.is_finite() {
            energy += (s as f64) * (s as f64);
        }
    }
    if energy < 1e-10 {
        return false;
    }

    let max_lag = PITCH_MAX_LAG.min(n / 2);
    let mut max_corr = 0.0f64;
    for lag in PITCH_MIN_LAG..=max_lag {
        let mut corr = 0.0f64;
        for i in 0..n - lag {
            let a = samples[i];
            let b = samples[i + lag];
            if a.is_finite() && b.is_finite() {
                corr += (a as f64) * (b as f64);
            }
        }
        if corr > max_corr {
            max_corr = corr;
        }
    }

    max_corr / energy > PITCH_THRESHOLD
}

fn is_voiceband_dominant(samples: &[f32]) -> bool {
    let n = samples.len();
    if n < 4 {
        return false;
    }

    // 2nd-order Butterworth LPF at fc=3000/fs=48000
    const B0: f64 = 0.029955;
    const B1: f64 = 0.059910;
    const B2: f64 = 0.029955;
    const A1: f64 = -1.454540;
    const A2: f64 = 0.574360;

    let mut total_energy = 0.0f64;
    let mut voiceband_energy = 0.0f64;
    let mut x1 = 0.0f64;
    let mut x2 = 0.0f64;
    let mut y1 = 0.0f64;
    let mut y2 = 0.0f64;

    for &s in samples {
        if !s.is_finite() {
            continue;
        }
        let x = s as f64;
        total_energy += x * x;

        let y = B0 * x + B1 * x1 + B2 * x2 - A1 * y1 - A2 * y2;
        x2 = x1;
        x1 = x;
        y2 = y1;
        y1 = y;

        voiceband_energy += y * y;
    }

    if total_energy < 1e-20 {
        return false;
    }

    voiceband_energy / total_energy >= VOICEBAND_ENERGY_RATIO
}

fn target_rms_db(pct: i32) -> f64 {
    let p = pct.clamp(0, 100) as f64;
    -50.0 + p * 0.44
}

fn rms_db_from_energy(total_energy: f64, sample_count: usize) -> f64 {
    if sample_count == 0 || total_energy <= 0.0 {
        return -100.0;
    }
    let rms = (total_energy / sample_count as f64).sqrt();
    20.0 * rms.log10()
}

// ---------------------------------------------------------------------------
// Reactivity parameters
// ---------------------------------------------------------------------------

struct ReactivityParams {
    speech_decisions_required: u32,
    max_step: i32,
    level_smoothing: f64,
}

fn compute_reactivity(reactivity: i32) -> ReactivityParams {
    let r = (reactivity.clamp(0, 100) as f64) / 100.0;
    ReactivityParams {
        speech_decisions_required: (5.0 - 4.0 * r).round().max(1.0) as u32,
        max_step: (2.0 + 8.0 * r).round() as i32,
        level_smoothing: 0.15 + 0.65 * r,
    }
}

// ---------------------------------------------------------------------------
// AGC state machine
// ---------------------------------------------------------------------------

struct AgcState {
    current_volume: i32,
    smoothed_rms_db: f64,
    speech_decisions: u32,
}

impl AgcState {
    fn new(volume: i32) -> Self {
        Self {
            current_volume: volume,
            smoothed_rms_db: -60.0,
            speech_decisions: 0,
        }
    }

    fn sync_volume(&mut self, actual_volume: i32) {
        self.current_volume = actual_volume;
    }

    fn decide(
        &mut self,
        voice_rms_db: f64,
        voiced_frames: usize,
        total_frames: usize,
        clipped_frames: usize,
        cfg: &AgcSettings,
    ) -> bool {
        if self.current_volume < cfg.min_volume {
            self.current_volume = cfg.min_volume;
            return true;
        }
        if self.current_volume > cfg.max_volume {
            self.current_volume = cfg.max_volume;
            return true;
        }

        if total_frames > 0 {
            let clip_ratio = clipped_frames as f64 / total_frames as f64;
            if clip_ratio > CLIP_RATIO_THRESHOLD {
                let new_vol = (self.current_volume - CLIP_STEP).max(cfg.min_volume);
                if new_vol != self.current_volume {
                    self.current_volume = new_vol;
                    self.speech_decisions = 0;
                    return true;
                }
            }
        }

        let rp = compute_reactivity(cfg.reactivity);

        if voice_rms_db < -90.0 {
            self.speech_decisions = 0;
            return false;
        }

        let voice_ratio = if total_frames > 0 {
            voiced_frames as f64 / total_frames as f64
        } else {
            0.0
        };
        if voice_ratio < VOICE_RATIO_THRESHOLD {
            self.speech_decisions = 0;
            return false;
        }

        self.smoothed_rms_db =
            rp.level_smoothing * voice_rms_db + (1.0 - rp.level_smoothing) * self.smoothed_rms_db;

        self.speech_decisions += 1;
        if self.speech_decisions < rp.speech_decisions_required {
            return false;
        }

        let target_db = target_rms_db(cfg.target_level_dbfs);
        let error_db = target_db - self.smoothed_rms_db;

        if error_db.abs() < DEADZONE_DB {
            return false;
        }

        let effective_error = error_db - error_db.signum() * DEADZONE_DB;
        let max_increase = (rp.max_step as f64 * 0.5).max(1.0);
        let max_decrease = rp.max_step as f64;
        let step = if effective_error > 0.0 {
            (effective_error / 3.0).round().clamp(1.0, max_increase) as i32
        } else {
            (effective_error / 3.0).round().clamp(-max_decrease, -1.0) as i32
        };

        if step == 0 {
            return false;
        }

        let new_vol = (self.current_volume + step).clamp(cfg.min_volume, cfg.max_volume);
        if new_vol != self.current_volume {
            self.current_volume = new_vol;
            self.speech_decisions = 0;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Stream user data
// ---------------------------------------------------------------------------

struct StreamData {
    format: spa::param::audio::AudioInfoRaw,
    alsa_node_id: u32,
    agc_state: AgcState,
    cfg: AgcSettings,
    frames_in_cycle: usize,
    voiced_energy_sum: f64,
    voiced_sample_count: usize,
    voiced_frames_in_cycle: usize,
    clipped_frames_in_cycle: usize,
    last_cfg_check: Instant,
    /// If Some, voice detection comes from filter output thread; else use pitch on raw mic
    voice_active: Option<Arc<AtomicBool>>,
}

// ---------------------------------------------------------------------------
// Parse pw-cli ls Node output into (id, props) pairs.
// Much lighter than pw-dump and immune to the JSON corruption bug.
// ---------------------------------------------------------------------------

struct PwNode {
    id: u32,
    media_class: String,
    node_name: String,
}

fn list_pw_nodes() -> Vec<PwNode> {
    let output = match Command::new("pw-cli")
        .args(["ls", "Node"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut nodes = Vec::new();
    let mut current_id: Option<u32> = None;
    let mut media_class = String::new();
    let mut node_name = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("id ") {
            // Flush previous node
            if let Some(id) = current_id.take() {
                nodes.push(PwNode { id, media_class: std::mem::take(&mut media_class), node_name: std::mem::take(&mut node_name) });
            }
            current_id = rest.split(',').next().and_then(|s| s.trim().parse().ok());
            media_class.clear();
            node_name.clear();
        } else if let Some(val) = trimmed.strip_prefix("media.class = ") {
            media_class = val.trim_matches('"').to_string();
        } else if let Some(val) = trimmed.strip_prefix("node.name = ") {
            node_name = val.trim_matches('"').to_string();
        }
    }
    // Flush last node
    if let Some(id) = current_id {
        nodes.push(PwNode { id, media_class, node_name });
    }
    nodes
}

// ---------------------------------------------------------------------------
// Find ALSA source node via pw-cli
// ---------------------------------------------------------------------------

fn find_alsa_source_node() -> Option<u32> {
    list_pw_nodes().iter().find(|n| {
        n.media_class == "Audio/Source" && n.node_name.contains("alsa")
    }).map(|n| n.id)
}

// ---------------------------------------------------------------------------
// Find filter output node (big-noise-canceling-output) for voice detection
// ---------------------------------------------------------------------------

fn find_filter_output_node() -> Option<u32> {
    list_pw_nodes().iter().find(|n| {
        n.media_class == "Audio/Source" && n.node_name == "big-noise-canceling-output"
    }).map(|n| n.id)
}

// ---------------------------------------------------------------------------
// Single-instance lock
// ---------------------------------------------------------------------------

fn acquire_lock() -> Option<File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let lock_path = PathBuf::from(runtime_dir).join("biglinux-mic-agc.lock");
    
    // Open without truncate - we truncate AFTER getting the lock
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(&lock_path)
        .ok()?;
    
    // Try exclusive lock (non-blocking)
    let fd = file.as_raw_fd();
    let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    
    if result == 0 {
        // We have the lock - now truncate and write our PID
        let mut f = file;
        let _ = f.set_len(0); // Truncate
        let _ = writeln!(f, "{}", process::id());
        Some(f)
    } else {
        eprintln!("biglinux-mic-agc: another instance is already running");
        None
    }
}

// ---------------------------------------------------------------------------
// Voice detection thread for filter output
// ---------------------------------------------------------------------------

/// Spawns a background thread that monitors the filter output and updates voice_active.
/// Returns the voice_active flag if successful.
fn spawn_filter_voice_detection(filter_node_id: u32) -> Option<Arc<AtomicBool>> {
    let voice_active = Arc::new(AtomicBool::new(false));
    let flag = voice_active.clone();

    std::thread::spawn(move || {
        // Initialize PipeWire for this thread
        pw::init();
        
        loop {
            if let Err(e) = run_filter_detection_loop(filter_node_id, &flag) {
                eprintln!("biglinux-mic-agc: filter stream error: {e}");
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });

    Some(voice_active)
}

/// Runs the filter output stream for voice detection.
fn run_filter_detection_loop(filter_node_id: u32, voice_active: &Arc<AtomicBool>) -> Result<(), pw::Error> {
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Monitor",
        *pw::keys::TARGET_OBJECT => filter_node_id.to_string(),
    };

    let stream = pw::stream::StreamBox::new(&core, "biglinux-mic-agc-voice", props)?;

    struct FilterData {
        format_ready: bool,
        voice_active: Arc<AtomicBool>,
    }

    let data = FilterData {
        format_ready: false,
        voice_active: voice_active.clone(),
    };

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, _param| {
            if id == spa::param::ParamType::Format.as_raw() {
                user_data.format_ready = true;
            }
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => {}
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() || !user_data.format_ready {
                    return;
                }

                let data = &mut datas[0];
                let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);

                if let Some(raw_samples) = data.data() {
                    let samples: Vec<f32> = (0..n_samples as usize)
                        .filter_map(|n| {
                            let start = n * mem::size_of::<f32>();
                            let end = start + mem::size_of::<f32>();
                            raw_samples.get(start..end)
                                .and_then(|s| s.try_into().ok())
                                .map(f32::from_le_bytes)
                        })
                        .collect();

                    let peak = frame_peak(&samples);

                    // Voice detection on filtered output:
                    // - Peak must exceed threshold (noise gate open)
                    // - Signal must have pitch AND dominant voiceband energy
                    // Using AND to be strict: both periodic structure and voiceband required
                    let is_voice = peak > FILTER_VOICE_THRESHOLD
                        && has_pitch(&samples)
                        && is_voiceband_dominant(&samples);
                    
                    user_data.voice_active.store(is_voice, Ordering::Relaxed);
                }
            }
        })
        .register()?;

    // Build audio format POD
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();
    let pod = Pod::from_bytes(&values).unwrap();

    stream.connect(
        pw::spa::utils::Direction::Input,
        Some(filter_node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut [pod],
    )?;

    eprintln!("biglinux-mic-agc: filter voice detection connected to node {}", filter_node_id);

    mainloop.run();
    Ok(())
}

// ---------------------------------------------------------------------------
// Main application
// ---------------------------------------------------------------------------

fn main() -> Result<(), pw::Error> {
    // Ensure single instance
    let _lock = match acquire_lock() {
        Some(lock) => lock,
        None => {
            std::process::exit(1);
        }
    };
    
    eprintln!("biglinux-mic-agc: starting (native PipeWire stream)");

    pw::init();

    // Try to find filter output node for voice detection
    let voice_active: Option<Arc<AtomicBool>> = match find_filter_output_node() {
        Some(filter_id) => {
            eprintln!("biglinux-mic-agc: found filter output node {}, using for voice detection", filter_id);
            spawn_filter_voice_detection(filter_id)
        }
        None => {
            eprintln!("biglinux-mic-agc: no filter chain found, using pitch detection on raw mic");
            None
        }
    };

    loop {
        let cfg = load_config();

        if !cfg.enabled {
            eprintln!("biglinux-mic-agc: disabled, waiting…");
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }

        let alsa_node_id = match find_alsa_source_node() {
            Some(id) => id,
            None => {
                eprintln!("biglinux-mic-agc: no ALSA source found, retrying…");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

        eprintln!("biglinux-mic-agc: found ALSA source node {}", alsa_node_id);

        if let Err(e) = run_agc_loop(alsa_node_id, &cfg, voice_active.clone()) {
            eprintln!("biglinux-mic-agc: error: {e}");
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

fn run_agc_loop(alsa_node_id: u32, initial_cfg: &AgcSettings, voice_active: Option<Arc<AtomicBool>>) -> Result<(), pw::Error> {
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let initial_vol = read_pw_volume(alsa_node_id).unwrap_or(70);

    // Setup stream properties to target specific node
    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Monitor",
        *pw::keys::TARGET_OBJECT => alsa_node_id.to_string(),
    };

    let stream = pw::stream::StreamBox::new(&core, "biglinux-mic-agc", props)?;

    let data = StreamData {
        format: Default::default(),
        alsa_node_id,
        agc_state: AgcState::new(initial_vol),
        cfg: initial_cfg.clone(),
        frames_in_cycle: 0,
        voiced_energy_sum: 0.0,
        voiced_sample_count: 0,
        voiced_frames_in_cycle: 0,
        clipped_frames_in_cycle: 0,
        last_cfg_check: Instant::now(),
        voice_active,
    };

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }

            user_data
                .format
                .parse(param)
                .expect("Failed to parse param to AudioInfoRaw");

            eprintln!(
                "biglinux-mic-agc: capturing rate:{} channels:{}",
                user_data.format.rate(),
                user_data.format.channels()
            );
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => {}
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let data = &mut datas[0];
                let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);

                if let Some(raw_samples) = data.data() {
                    // Parse f32 samples
                    let samples: Vec<f32> = (0..n_samples as usize)
                        .filter_map(|n| {
                            let start = n * mem::size_of::<f32>();
                            let end = start + mem::size_of::<f32>();
                            raw_samples.get(start..end)
                                .and_then(|s| s.try_into().ok())
                                .map(f32::from_le_bytes)
                        })
                        .collect();

                    if samples.is_empty() || !user_data.cfg.enabled {
                        return;
                    }

                    let peak = frame_peak(&samples);

                    // Track clipping
                    if peak > CLIP_PEAK_THRESHOLD {
                        user_data.clipped_frames_in_cycle += 1;
                    }

                    // Voice detection: prefer filter output if available, else pitch on raw mic
                    let is_voiced = match &user_data.voice_active {
                        Some(flag) => flag.load(Ordering::Relaxed),
                        None => peak > 0.01 && has_pitch(&samples),
                    };

                    if is_voiced {
                        user_data.voiced_frames_in_cycle += 1;
                        for &s in &samples {
                            if s.is_finite() {
                                user_data.voiced_energy_sum += (s as f64) * (s as f64);
                                user_data.voiced_sample_count += 1;
                            }
                        }
                    }

                    user_data.frames_in_cycle += 1;

                    // Decision every FRAMES_PER_DECISION frames (~500ms)
                    if user_data.frames_in_cycle >= FRAMES_PER_DECISION {
                        // Sync volume with external changes
                        if let Some(actual_vol) = read_pw_volume(user_data.alsa_node_id) {
                            user_data.agc_state.sync_volume(actual_vol);
                        }

                        let voice_rms_db = rms_db_from_energy(
                            user_data.voiced_energy_sum,
                            user_data.voiced_sample_count,
                        );

                        if user_data.agc_state.decide(
                            voice_rms_db,
                            user_data.voiced_frames_in_cycle,
                            user_data.frames_in_cycle,
                            user_data.clipped_frames_in_cycle,
                            &user_data.cfg,
                        ) {
                            set_pw_volume(user_data.alsa_node_id, user_data.agc_state.current_volume);
                            eprintln!(
                                "biglinux-mic-agc: vol → {}% (rms={:.1}dB target={:.1}dB)",
                                user_data.agc_state.current_volume,
                                user_data.agc_state.smoothed_rms_db,
                                target_rms_db(user_data.cfg.target_level_dbfs)
                            );
                        }

                        // Reset accumulators
                        user_data.frames_in_cycle = 0;
                        user_data.voiced_energy_sum = 0.0;
                        user_data.voiced_sample_count = 0;
                        user_data.voiced_frames_in_cycle = 0;
                        user_data.clipped_frames_in_cycle = 0;
                    }

                    // Periodic config reload
                    if user_data.last_cfg_check.elapsed() > Duration::from_secs(CONFIG_RELOAD_SECS) {
                        user_data.cfg = load_config();
                        user_data.last_cfg_check = Instant::now();
                    }
                }
            }
        })
        .register()?;

    // Build audio format POD: F32LE, accept native rate/channels
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    let flags = pw::stream::StreamFlags::AUTOCONNECT
        | pw::stream::StreamFlags::MAP_BUFFERS
        | pw::stream::StreamFlags::RT_PROCESS;

    stream.connect(
        spa::utils::Direction::Input,
        None,
        flags,
        &mut params,
    )?;

    eprintln!("biglinux-mic-agc: stream connected to node {}", alsa_node_id);

    mainloop.run();

    Ok(())
}
