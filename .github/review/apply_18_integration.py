from pathlib import Path
from _common import done, replace, commit

TITLE = 'fix(review): complete configuration fixtures and resolve integration lint errors'
if not done(TITLE):
    replace('src/pipeline/output_tests.rs', '            target_sink_name: None,', '            target_sink_name: None,\n            channel_mode: crate::config::OutputChannelMode::Mono,')
    replace('src/config/storage.rs', 'Value::Object(Default::default())', 'Value::Object(serde_json::Map::default())')
    replace('src/config.rs', '    pub fn runtime_settings(&self) -> Self {', '    #[must_use]\n    pub fn runtime_settings(&self) -> Self {')
    replace('src/services/audio_monitor/capture.rs', 'self.read_buf.chunks_exact(4)', 'self.read_buf.as_chunks::<4>().0')
    path = 'src/services/reconcile.rs'
    text = Path(path).read_text()
    text = text.replace('dirs::runtime_dir()\n        .map(|path| path.join("biglinux-microphone"))\n        .unwrap_or_else(crate::config::config_dir)', 'dirs::runtime_dir().map_or_else(crate::config::config_dir, |path| path.join("biglinux-microphone"))')
    Path(path).write_text(text)
    path = 'src/services/preview.rs'
    text = Path(path).read_text().replace('let _ = gio::spawn_blocking(move || {', 'drop(gio::spawn_blocking(move || {')
    text = text.replace('log::warn!("preview cleanup: {error}");\n                }\n            });', 'log::warn!("preview cleanup: {error}");\n                }\n            }));')
    Path(path).write_text(text)
    path = 'src/ui/mic_shell.rs'
    text = Path(path).read_text().replace('settings.output_filter.channel_mode = mode\n', 'settings.output_filter.channel_mode = mode;\n')
    Path(path).write_text(text)
    path = 'src/pwloader/denoise_watch.rs'
    text = Path(path).read_text().replace('_handle', 'handle')
    Path(path).write_text(text)
    path = 'src/services/settings_watch.rs'
    text = Path(path).read_text()
    old = '''                let monitor = match gio::File::for_path(parent)'''
    if old in text:
        text = text.replace(old, '''                if let Err(error) = std::fs::create_dir_all(parent) {
                    log::error!("settings watcher directory unavailable: {error}");
                    return;
                }
                let monitor = match gio::File::for_path(parent)''')
    Path(path).write_text(text)
    commit(TITLE, ['src/pipeline/output_tests.rs', 'src/config/storage.rs', 'src/config.rs',
        'src/services/audio_monitor/capture.rs', 'src/services/reconcile.rs', 'src/services/preview.rs',
        'src/ui/mic_shell.rs', 'src/pwloader/denoise_watch.rs', path])
