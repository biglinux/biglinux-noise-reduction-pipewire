//! Regressions for independently switchable microphone effects.
use biglinux_microphone::config::{AppSettings, NoiseModel};
use biglinux_microphone::pipeline::{ai_node_in_mic_chain, build_mic_conf_for};

#[test]
fn standalone_gate_does_not_reenable_a_disabled_denoiser() {
    for model in NoiseModel::ALL {
        for denoise in [false, true] {
            for gate in [false, true] {
                let mut settings = AppSettings::default();
                settings.noise_reduction.model = model;
                settings.noise_reduction.enabled = denoise;
                settings.gate.enabled = gate;
                assert_eq!(ai_node_in_mic_chain(&settings), denoise);
                let graph = build_mic_conf_for(&settings);
                assert_eq!(
                    graph.contains("name = \"ai\""),
                    denoise,
                    "{model:?}: {graph}"
                );
                assert_eq!(graph.contains("name = \"gate\""), gate);
            }
        }
    }
}
