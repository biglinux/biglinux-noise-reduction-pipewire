from pathlib import Path
from _common import done, replace, commit

TITLE = 'refactor(ui): remove strict lint violations without suppressing checks'
if not done(TITLE):
    path = 'src/ui/mic_shell.rs'
    for field in ['prefer_fast_cpus', 'reserve_memory']:
        replace(path, f'settings.runtime.{field} = enabled\n', f'settings.runtime.{field} = enabled;\n')
    replace('src/ui/state.rs', '*self.active_page.borrow_mut() = name.to_owned();', 'name.clone_into(&mut self.active_page.borrow_mut());')
    replace('src/ui/window.rs', '''        } else if let Some((path, widget_type)) = focus_path {
            if let Some(widget) =
                child_at(body.upcast_ref(), &path).filter(|widget| widget.type_() == widget_type)
            {
                widget.grab_focus();
            }
        }''', '''        } else if let Some((path, widget_type)) = focus_path
            && let Some(widget) = child_at(body.upcast_ref(), &path)
                .filter(|widget| widget.type_() == widget_type)
        {
            widget.grab_focus();
        }''')
    replace('src/pipeline.rs', '''            !entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")''', '''            entry.unwrap().path().extension().is_none_or(|extension| extension != "tmp")''')
    path = Path('src/ui/widgets/source_picker.rs')
    source = path.read_text()
    old = '''        let mut pending = Pending::default();
        pending.refresh = true;
        pending.volume = Some((12, 0.2));'''
    assert source.count(old) == 1
    path.write_text(source.replace(old, '''        let mut pending = Pending {
            refresh: true,
            volume: Some((12, 0.2)),
            ..Pending::default()
        };'''))
    commit(TITLE, ['src/ui/mic_shell.rs','src/ui/state.rs','src/ui/window.rs','src/pipeline.rs',str(path)])

TITLE = 'test(audio): retain explicit mono contracts alongside stereo and bypass regressions'
if not done(TITLE):
    path = Path('src/pipeline/output_tests.rs')
    text = path.read_text()
    old = '            ..crate::config::OutputFilterSettings::default()'
    assert text.count(old) >= 5
    # An outer ..mono_settings() cannot override a nested output_filter
    # initializer. These legacy tests deliberately exercise the mono chain;
    # tests/review_stereo.rs independently covers the default stereo graph.
    text = text.replace(old, '            channel_mode: crate::config::OutputChannelMode::Mono,\n' + old)
    path.write_text(text)
    path = 'src/pipeline/mic/tests.rs'
    replace(path, '("stereo", |s| s.stereo.enabled = true),', '''("voice changer", |s| {
            s.stereo.enabled = true;
            s.stereo.mode = crate::config::StereoMode::VoiceChanger;
        }),''')
    path = Path('src/ui/state_tests.rs')
    text = path.read_text()
    start = text.index('fn output_topology_unchanged_when_only_nr_enabled_toggles()')
    end = text.index('\n#[test]', start)
    block = text[start:end]
    block = block.replace('fn output_topology_unchanged_when_only_nr_enabled_toggles()', 'fn mono_output_topology_unchanged_when_only_nr_enabled_toggles()')
    old = '            ..OutputFilterSettings::default()'
    assert block.count(old) == 1
    block = block.replace(old, '            channel_mode: crate::config::OutputChannelMode::Mono,\n' + old)
    text = text[:start] + block + text[end:]
    text += '''
#[test]
fn stereo_nr_toggle_rebuilds_the_conditional_neural_stage() {
    let prev = AppSettings {
        output_filter: OutputFilterSettings {
            enabled: true,
            ..OutputFilterSettings::default()
        },
        ..AppSettings::default()
    };
    let mut next = prev.clone();
    next.output_filter.noise_reduction.enabled = !prev.output_filter.noise_reduction.enabled;
    assert!(output_topology_changed(Some(&prev), &next));
}
'''
    path.write_text(text)
    commit(TITLE, ['src/pipeline/output_tests.rs', 'src/pipeline/mic/tests.rs', str(path)])
