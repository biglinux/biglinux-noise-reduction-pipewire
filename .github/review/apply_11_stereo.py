from pathlib import Path
from _common import done, replace, write, commit

TITLE = 'feat(audio): preserve stereo playback and offer a clearly labeled mono economy mode'
if not done(TITLE):
    path = 'src/config/output_filter.rs'
    text = Path(path).read_text()
    pos = text.index('/// Output') if '/// Output' in text else text.index('#[derive(')
    text = text[:pos] + '''/// Spatial audio is preserved unless the user explicitly chooses mono.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputChannelMode {
    #[default]
    Stereo,
    Mono,
}

''' + text[pos:]
    text = text.replace('pub struct OutputFilterSettings {', 'pub struct OutputFilterSettings {\n    pub channel_mode: OutputChannelMode,')
    text = text.replace('        Self {\n', '        Self {\n            channel_mode: OutputChannelMode::Stereo,', 1)
    Path(path).write_text(text)
    replace('src/config.rs', 'pub use output_filter::OutputFilterSettings;', 'pub use output_filter::{OutputChannelMode, OutputFilterSettings};')
    path = 'src/pipeline/output.rs'
    text = Path(path).read_text()
    text = text.replace('pub fn build_output_conf(settings: &AppSettings) -> String {', '''pub fn build_output_conf(settings: &AppSettings) -> String {
    if settings.output_filter.channel_mode == crate::config::OutputChannelMode::Stereo {
        return build_stereo_conf(settings);
    }''')
    pos = text.index('/// True when GTCRN should')
    text = text[:pos] + '''/// Two independent mono processors preserve left/right phase and panning.
/// The economical mono variant below is an explicit user choice, not a hidden
/// side effect of enabling an equalizer or turning neural processing off.
fn build_stereo_conf(settings: &AppSettings) -> String {
    let mono: Vec<Node> = output_nodes(settings).into_iter()
        .filter(|node| !matches!(node.name.as_str(), "mixer" | "copy_l" | "copy_r"))
        .collect();
    let mut nodes = Vec::with_capacity(mono.len() * 2);
    let mut links = Vec::with_capacity(mono.len().saturating_sub(1) * 2);
    let mut inputs = Vec::with_capacity(2);
    let mut outputs = Vec::with_capacity(2);
    for prefix in ["", "right_"] {
        let chain: Vec<Node> = mono.iter().cloned().map(|mut node| {
            node.name = format!("{prefix}{}", node.name);
            node
        }).collect();
        if let (Some(first), Some(last)) = (chain.first(), chain.last()) {
            inputs.push(format!("{}:{}", first.name, first.input_port));
            outputs.push(format!("{}:{}", last.name, last.output_port));
        }
        for pair in chain.windows(2) {
            links.push(Link::new(format!("{}:{}", pair[0].name, pair[0].output_port),
                format!("{}:{}", pair[1].name, pair[1].input_port)));
        }
        nodes.extend(chain);
    }
    Graph {
        description: OUTPUT_DESCRIPTION.into(), media_name: OUTPUT_DESCRIPTION.into(),
        nodes, links, inputs, outputs, capture_props: capture_props(), playback_props: playback_props(),
    }.render()
}

''' + text[pos:]
    text = text.replace('    let denoiser = if nr.model.is_attenuation_only() {', '''    let denoiser = if !nr.enabled {
        Node::builtin("ai", LABEL_COPY)
    } else if nr.model.is_attenuation_only() {''')
    # Remove inaccurate explanation: async changes scheduling, not the clock
    # or resampler. Keep established behavior until measured on target devices.
    text = text.replace('it lets PipeWire insert an adaptive resampler if a downstream', 'it changes scheduling and can add one graph cycle of latency. A downstream')
    text = text.replace('sink ends up with a slightly different negotiated rate (Bluetooth', 'sink with a different negotiated rate (Bluetooth')
    text = text.replace('links and some USB mics renegotiate when codecs switch), without', 'or a USB device) relies on separate adapter/resampler policy, not this flag;')
    text = text.replace('any cost in the common case where rates agree.', 'do not describe this scheduling trade-off as cost-free.')
    Path(path).write_text(text)
    path = 'src/services/pipewire/live.rs'
    replace(path, 'fn output_params(s: &AppSettings) -> Vec<(String, f64)> {', '''fn output_params(s: &AppSettings) -> Vec<(String, f64)> {
    let mut parameters = output_params_mono(s);
    if s.output_filter.channel_mode == crate::config::OutputChannelMode::Stereo {
        let right: Vec<_> = parameters.iter().map(|(key, value)| (format!("right_{key}"), *value)).collect();
        parameters.extend(right);
    }
    parameters
}

fn output_params_mono(s: &AppSettings) -> Vec<(String, f64)> {''')
    # Retain safe bypass pushes during transitions; unknown copy-node controls
    # are ignored and the shared planner handles the topology change.
    path = 'src/services/reconcile.rs'
    text = Path(path).read_text()
    start = text.index('pub fn output_topology_changed(')
    tail = text[start:]
    tail = tail.replace('prev.is_some_and(|p| {', 'prev.is_none_or(|p| {', 1)
    tail = tail.replace('        p.output_filter.equalizer.bands', '''        p.output_filter.channel_mode != now.output_filter.channel_mode
            || p.output_filter.noise_reduction.enabled != now.output_filter.noise_reduction.enabled
            || p.output_filter.equalizer.bands''', 1)
    Path(path).write_text(text[:start] + tail)
    path = 'src/ui/mic_shell.rs'
    replace(path, '    OutputModelChanged(NoiseModel),', '    OutputModelChanged(NoiseModel),\n    OutputChannelsChanged(crate::config::OutputChannelMode),')
    replace(path, '            MicInput::OutputModelChanged(model) => {', '''            MicInput::OutputChannelsChanged(mode) => {
                self.mutate_settings(&sender, |settings| settings.output_filter.channel_mode = mode);
            }
            MicInput::OutputModelChanged(model) => {''')
    path = 'src/ui/views/output.rs'
    text = Path(path).read_text()
    at = text.index('    content.append(&master);') + len('    content.append(&master);')
    text = text[:at] + '''
    let channels = adw::ComboRow::builder()
        .title(i18n("Stereo or lower CPU use"))
        .subtitle(i18n("Stereo keeps left and right separate for music and video. Mono uses fewer resources but combines both sides; use it mainly for speech."))
        .model(&gtk::StringList::new(&[
            &i18n("Keep stereo (recommended)"), &i18n("Combine to mono (lower CPU use)"),
        ]))
        .build();
    channels.set_selected(u32::from(state.settings().output_filter.channel_mode == crate::config::OutputChannelMode::Mono));
    let sender = input.clone();
    channels.connect_selected_notify(move |row| {
        let mode = if row.selected() == 1 { crate::config::OutputChannelMode::Mono } else { crate::config::OutputChannelMode::Stereo };
        let _ = sender.send(MicInput::OutputChannelsChanged(mode));
    });
    let group = adw::PreferencesGroup::new();
    group.add(&channels);
    content.append(&group);
''' + text[at:]
    Path(path).write_text(text)
    write('tests/review_stereo.rs', r'''use biglinux_microphone::config::{AppSettings, OutputChannelMode};
use biglinux_microphone::pipeline::build_output_conf_for;

#[test]
fn default_playback_keeps_two_independent_channels() {
    let settings = AppSettings::default();
    let graph = build_output_conf_for(&settings);
    assert!(graph.contains("right_hpf"));
    assert!(graph.contains("right_eq"));
    assert!(!graph.contains("mixer:In"));
}

#[test]
fn mono_requires_an_explicit_setting() {
    let mut settings = AppSettings::default();
    settings.output_filter.channel_mode = OutputChannelMode::Mono;
    let graph = build_output_conf_for(&settings);
    assert!(graph.contains("mixer:In 1"));
    assert!(!graph.contains("right_hpf"));
}

#[test]
fn equalizer_only_does_not_require_a_neural_runtime() {
    let mut settings = AppSettings::default();
    settings.output_filter.enabled = true;
    settings.output_filter.noise_reduction.enabled = false;
    settings.output_filter.equalizer.enabled = true;
    let graph = build_output_conf_for(&settings);
    assert!(!graph.contains("libgtcrn"));
    assert!(graph.contains("right_eq"));
}
''')
    # Existing output tests describe the legacy mono topology. Run those in
    # that explicit mode and cover the new default separately above.
    path = 'src/pipeline/output_tests.rs'
    text = Path(path).read_text()
    text = text.replace('AppSettings::default()', 'mono_settings()')
    text += '''
fn mono_settings() -> AppSettings {
    let mut settings = AppSettings::default();
    settings.output_filter.channel_mode = crate::config::OutputChannelMode::Mono;
    settings
}
'''
    Path(path).write_text(text)
    commit(TITLE, ['src/config.rs', 'src/config/output_filter.rs', 'src/pipeline/output.rs',
        'src/services/pipewire/live.rs', 'src/services/reconcile.rs', 'src/ui/mic_shell.rs',
        'src/ui/views/output.rs', 'tests/review_stereo.rs', path])
