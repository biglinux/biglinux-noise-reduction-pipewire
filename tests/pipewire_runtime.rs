//! Native module/lifecycle contracts, separate from string-based graph tests.
//! No sound hardware, session manager or real user services are used.
use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use biglinux_microphone::config::{AppSettings, OutputChannelMode};
use biglinux_microphone::pipeline::{self, MIC_CAPTURE_NODE_NAME, MIC_NODE_NAME, OUTPUT_NODE_NAME};
use serde_json::Value;

struct Process(Child);

impl Process {
    fn start(program: &str, arguments: &[&str], log: &Path) -> Self {
        Self(
            Command::new(program)
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(File::create(log).expect("create diagnostic log"))
                .spawn()
                .expect("start native test process"),
        )
    }

    fn stop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().expect("reap native test process");
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn snapshot() -> Option<Vec<Value>> {
    let output = Command::new("/usr/bin/timeout")
        .args(["--kill-after=1s", "2s", "/usr/bin/pw-dump"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn has_node(graph: &[Value], name: &str) -> bool {
    graph.iter().any(|object| {
        object["type"] == "PipeWire:Interface:Node" && object["info"]["props"]["node.name"] == name
    })
}

fn await_state(mic: bool, output: bool, logs: &Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(graph) = snapshot() {
            let mic_present = has_node(&graph, MIC_NODE_NAME);
            let capture_present = has_node(&graph, MIC_CAPTURE_NODE_NAME);
            if mic_present == mic
                && capture_present == mic
                && has_node(&graph, OUTPUT_NODE_NAME) == output
            {
                return;
            }
        }
        if Instant::now() >= deadline {
            for entry in fs::read_dir(logs).expect("read diagnostics directory") {
                let path = entry.expect("diagnostic entry").path();
                if path.extension().is_some_and(|extension| extension == "log") {
                    eprintln!(
                        "{}:\n{}",
                        path.display(),
                        fs::read_to_string(&path).unwrap_or_default()
                    );
                }
            }
            panic!("PipeWire did not reach mic={mic}, output={output}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn load_graph(path: &Path, log: &Path) -> Process {
    Process::start(
        env!("CARGO_BIN_EXE_biglinux-microphone-pwloader"),
        &[
            "libpipewire-module-filter-chain",
            path.to_str().expect("UTF-8 fixture path"),
        ],
        log,
    )
}

#[test]
#[ignore = "requires a private PipeWire session; run scripts/test-pipewire.sh"]
fn generated_graphs_load_and_recover_independently() {
    assert_eq!(
        std::env::var("BIGLINUX_AUDIO_SESSION_MODE").as_deref(),
        Ok("disposable")
    );
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("private runtime directory");
    assert_eq!(
        std::env::var_os("PIPEWIRE_RUNTIME_DIR"),
        Some(runtime.clone())
    );
    assert!(Path::new(&runtime).join("biglinux-test-session").is_file());
    assert!(
        !Path::new(&runtime).join("pipewire-0").exists(),
        "never reuse a running daemon"
    );

    let directory = tempfile::tempdir().expect("fixture directory");
    let root = directory.path();
    let _daemon = Process::start(
        "/usr/bin/pipewire",
        &["-c", "/usr/share/pipewire/pipewire.conf"],
        &root.join("daemon.log"),
    );
    await_state(false, false, root);

    let mut settings = AppSettings::default();
    pipeline::cascade_mic_off(&mut settings);
    settings.hpf.enabled = true;
    settings.equalizer.enabled = true;
    settings.output_filter.enabled = true;
    settings.output_filter.noise_reduction.enabled = false;
    settings.output_filter.gate.enabled = false;
    settings.output_filter.compressor.enabled = false;
    settings.output_filter.equalizer.enabled = true;
    let mic_path = root.join("mic.args");
    let output_path = root.join("output.args");
    fs::write(&mic_path, pipeline::build_mic_conf_for(&settings)).unwrap();

    let mut microphone = load_graph(&mic_path, &root.join("microphone.log"));
    await_state(true, false, root);
    for mode in [OutputChannelMode::Stereo, OutputChannelMode::Mono] {
        settings.output_filter.channel_mode = mode;
        fs::write(&output_path, pipeline::build_output_conf_for(&settings)).unwrap();
        let mut playback = load_graph(&output_path, &root.join("playback.log"));
        await_state(true, true, root);
        assert!(playback.0.try_wait().unwrap().is_none());

        microphone.stop();
        await_state(false, true, root);
        assert!(playback.0.try_wait().unwrap().is_none());
        microphone = load_graph(&mic_path, &root.join("microphone.log"));
        await_state(true, true, root);

        playback.stop();
        await_state(true, false, root);
        assert!(microphone.0.try_wait().unwrap().is_none());
    }
    microphone.stop();
    await_state(false, false, root);
}
