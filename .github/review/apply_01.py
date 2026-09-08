from _common import done, replace, write, commit

TITLE = 'fix(audio): decouple standalone gates from neural processing'
if not done(TITLE):
    path = 'src/pipeline/mic.rs'
    replace(path, 'settings.noise_reduction.enabled || settings.gate.enabled', '''settings.noise_reduction.enabled
        || (settings.gate.enabled && !settings.noise_reduction.model.is_attenuation_only())''')
    replace(path, '''        // DFN3 has no integrated gate, unlike GTCRN. When the user wants
        // the silence gate alongside DFN3 we wire a standalone SWH gate
        // immediately after it (same plugin the output chain uses).
        if settings.noise_reduction.model.is_attenuation_only() && settings.gate.enabled {
            nodes.push(standalone_gate_node(settings));
        }
    }''', '''    }
    // Attenuation-only backends have an independent gate. Keeping the gate
    // enabled must not keep the neural network running after NR is disabled.
    if settings.noise_reduction.model.is_attenuation_only() && settings.gate.enabled {
        nodes.push(standalone_gate_node(settings));
    }''')
    path = 'src/services/pipewire/live.rs'
    old = '''            if s.gate.enabled {
                params.extend([
                    ("gate:Threshold (dB)".to_owned(), gate_derived.threshold_db),
                    ("gate:Attack (ms)".to_owned(), gate_derived.attack_ms),
                    ("gate:Hold (ms)".to_owned(), gate_derived.hold_ms),
                    ("gate:Decay (ms)".to_owned(), gate_derived.release_ms),
                    ("gate:Range (dB)".to_owned(), gate_derived.range_db),
                ]);
            }
'''
    replace(path, old, '')
    replace(path, '''    append_compressor_params(
        &mut params,
        "compressor",
        s.compressor,''', '''    if nr.model.is_attenuation_only() && gate.enabled {
        params.extend([
            ("gate:Threshold (dB)".to_owned(), gate_derived.threshold_db),
            ("gate:Attack (ms)".to_owned(), gate_derived.attack_ms),
            ("gate:Hold (ms)".to_owned(), gate_derived.hold_ms),
            ("gate:Decay (ms)".to_owned(), gate_derived.release_ms),
            ("gate:Range (dB)".to_owned(), gate_derived.range_db),
        ]);
    }
    append_compressor_params(
        &mut params,
        "compressor",
        s.compressor,''')
    replace(path, '''    #[test]
    fn mic_params_includes_prefixed_controls()''', '''    #[test]
    fn attenuation_gate_updates_without_a_neural_node() {
        let mut settings = AppSettings::default();
        settings.noise_reduction.model = crate::config::NoiseModel::DeepFilterNet3;
        settings.noise_reduction.enabled = false;
        settings.gate.enabled = true;
        let controls = mic_params(&settings);
        assert!(!controls.iter().any(|(key, _)| key.starts_with("ai:")));
        assert!(controls.iter().any(|(key, _)| key == "gate:Threshold (dB)"));
    }

    #[test]
    fn mic_params_includes_prefixed_controls()''')
    write('tests/review_audio.rs', '''//! Regressions for independently switchable microphone effects.
use biglinux_microphone::config::{AppSettings, NoiseModel};
use biglinux_microphone::pipeline::{ai_node_in_mic_chain, build_mic_conf_for};

#[test]
fn standalone_gate_does_not_reenable_a_disabled_denoiser() {
    for id in 0..=8_u8 {
        let Ok(model) = NoiseModel::try_from(id) else { continue };
        for denoise in [false, true] {
            for gate in [false, true] {
                let mut settings = AppSettings::default();
                settings.noise_reduction.model = model;
                settings.noise_reduction.enabled = denoise;
                settings.gate.enabled = gate;
                let expected_ai = denoise || (gate && !model.is_attenuation_only());
                assert_eq!(ai_node_in_mic_chain(&settings), expected_ai);
                let graph = build_mic_conf_for(&settings);
                assert_eq!(graph.contains("name = \\\"ai\\\""), expected_ai, "{model:?}: {graph}");
                assert_eq!(graph.contains("name = \\\"gate\\\""), gate && model.is_attenuation_only());
            }
        }
    }
}
''')
    commit(TITLE, ['src/pipeline/mic.rs', 'src/services/pipewire/live.rs', 'tests/review_audio.rs'])

TITLE = 'fix(models): check runtime loadability when selecting automatic quality'
if not done(TITLE):
    replace('src/config/quality.rs', 'if model.plugin_available() {', 'if model.plugin_loadable_cached() {')
    replace('src/config/quality.rs', '#[serde(rename = "auto")]', '#[serde(rename = "auto", alias = "automatic")]')
    replace('src/config/plugin_cost.rs', '''            && let Ok(share) = value.parse::<f32>()
        {''', '''            && let Ok(share) = value.parse::<f32>()
            && share.is_finite()
            && share > 0.0
        {''')
    commit(TITLE, ['src/config/quality.rs', 'src/config/plugin_cost.rs'])
