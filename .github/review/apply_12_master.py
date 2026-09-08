from pathlib import Path
import re
from _common import done, replace, write, commit

TITLE = 'feat(settings): pause microphone effects without erasing user preferences'
if not done(TITLE):
    path = 'src/config.rs'
    replace(path, 'pub struct AppSettings {', '''pub struct AppSettings {
    /// Temporary master bypass; individual effect preferences are retained.
    pub mic_bypass: bool,''')
    replace(path, 'impl AppSettings {', '''impl AppSettings {
    pub fn set_microphone_enabled(&mut self, enabled: bool) {
        self.mic_bypass = !enabled;
        if enabled && !crate::pipeline::mic_chain_wanted(self) {
            self.noise_reduction.enabled = true;
        }
    }

    /// Materialize effective flags without destroying the saved choices.
    pub fn runtime_settings(&self) -> Self {
        let mut effective = self.clone();
        if self.mic_bypass {
            crate::pipeline::cascade_mic_off(&mut effective);
        }
        effective
    }
''')
    replace(path, 'if !self.quality.decides_the_model() {', 'if !self.quality.decides_the_model() || self.filters_running() == 0 {')
    replace(path, '        usize::from(self.noise_reduction.enabled) + usize::from(self.output_filter.enabled)', '''        let microphone = usize::from(!self.mic_bypass && self.noise_reduction.enabled);
        let output = usize::from(self.output_filter.enabled && self.output_filter.noise_reduction.enabled);
        let channels = if self.output_filter.channel_mode == OutputChannelMode::Stereo { 2 } else { 1 };
        microphone + output * channels''')
    path = 'src/pipeline/mic.rs'
    text = Path(path).read_text()
    start = text.index('pub fn mic_chain_wanted('); end = text.index('\n}', start)
    part = text[start:end]
    expr = part.index('    settings.noise_reduction.enabled')
    part = part[:expr] + '    !settings.mic_bypass && (\n' + part[expr:] + ')'
    part = part.replace('|| settings.stereo.enabled', '|| (settings.stereo.enabled && settings.stereo.mode == StereoMode::VoiceChanger)')
    text = text[:start] + part + text[end:]
    start = text.index('pub fn ai_node_in_mic_chain('); end = text.index('\n}', start)
    part = text[start:end]
    expr = part.index('    settings.noise_reduction.enabled')
    part = part[:expr] + '    !settings.mic_bypass && (\n' + part[expr:] + ')'
    text = text[:start] + part + text[end:]
    Path(path).write_text(text)
    path = 'src/services/reconcile.rs'
    replace(path, 'pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {', '''pub fn apply(settings: &AppSettings, force: bool) -> io::Result<()> {
    let effective = settings.runtime_settings();
    let settings = &effective;''')
    path = 'src/ui/mic_shell.rs'
    text = Path(path).read_text()
    start = text.index('            MicInput::NoiseFilterToggled(on) =>')
    end = text.index('            MicInput::MicIntensityChanged', start)
    text = text[:start] + '''            MicInput::NoiseFilterToggled(on) => {
                self.mutate_settings(&sender, |settings| settings.set_microphone_enabled(on));
            }
''' + text[end:]
    text = text.replace('use crate::pipeline::cascade_mic_off;\n', '')
    for field in ['noise_reduction', 'hpf', 'gate', 'compressor']:
        text = text.replace(f'|s| s.{field}.enabled = on', f'''|s| {{
                    s.{field}.enabled = on;
                    if on {{ s.mic_bypass = false; }}
                }}''')
    text = text.replace('                s.stereo.enabled = on;', '                s.stereo.enabled = on;\n                if on { s.mic_bypass = false; }')
    text = text.replace('                    eq_card::apply_eq_mutation(&mut s.equalizer, mutation);', '                    eq_card::apply_eq_mutation(&mut s.equalizer, mutation);\n                    if s.equalizer.enabled { s.mic_bypass = false; }')
    Path(path).write_text(text)
    path = 'src/ui/views/simple.rs'
    replace(path, '.active(state.settings().noise_reduction.enabled)', '.active(crate::pipeline::mic_chain_wanted(&state.settings()))')
    replace(path, '"Removes background noise from your voice using AI. Higher \\\n             intensity = stronger cleanup."', '"Reduces background noise in your voice. Turning this off pauses microphone effects without forgetting your choices."')
    path = 'src/bin/cli.rs'
    text = Path(path).read_text()
    start = text.index('fn toggle_mic()'); end = text.index('\n}', start)
    part = text[start:end]
    a = part.index('    if settings.noise_reduction.enabled {')
    b = part.index('    match apply_settings', a)
    part = part[:a] + '''    let enabled = !pipeline::mic_chain_wanted(&settings);
    settings.set_microphone_enabled(enabled);
''' + part[b:]
    text = text[:start] + part + text[end:]
    text = text.replace('Some(true) => settings.noise_reduction.enabled = true,', 'Some(true) => settings.set_microphone_enabled(true),')
    text = text.replace('Some(false) => pipeline::cascade_mic_off(&mut settings),', 'Some(false) => settings.set_microphone_enabled(false),')
    text = text.replace('    let mic = s.noise_reduction.enabled;', '    let mic = pipeline::mic_chain_wanted(&s);')
    Path(path).write_text(text)
    write('tests/review_master.rs', r'''use biglinux_microphone::config::{AppSettings, EchoMode};
use biglinux_microphone::pipeline::mic_chain_wanted;

#[test]
fn pausing_and_resuming_does_not_erase_selected_effects() {
    let mut settings = AppSettings::default();
    settings.gate.enabled = true;
    settings.equalizer.enabled = true;
    settings.echo_cancel.mode = EchoMode::Automatic;
    let original = settings.clone();
    settings.set_microphone_enabled(false);
    assert!(!mic_chain_wanted(&settings));
    assert!(settings.gate.enabled);
    assert_eq!(settings.echo_cancel.mode, EchoMode::Automatic);
    let runtime = settings.runtime_settings();
    assert!(!runtime.echo_cancel.enabled);
    settings.set_microphone_enabled(true);
    assert_eq!(settings, original);
}
''')
    commit(TITLE, ['src/config.rs', 'src/pipeline/mic.rs', 'src/services/reconcile.rs',
        'src/ui/mic_shell.rs', 'src/ui/views/simple.rs', 'src/bin/cli.rs', 'tests/review_master.rs'])
