//! Displayless PipeWire probe for the realtime audio pipeline.

use std::process::ExitCode;
use std::time::Duration;

use biglinux_microphone::config::{AppSettings, config_dir};
use biglinux_microphone::pipeline::mic_chain_wanted;
use pipewire as pw;
use pw::loop_::Signal;
use pw::types::ObjectType;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("biglinux-microphone-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(duration) = parse_duration(&args)? else {
        // `--help` printed the usage; there is nothing to watch.
        return Ok(());
    };
    let settings = AppSettings::load();
    println!(
        "probe pipeline config_dir={} mic_wanted={} output_wanted={} echo_cancel={}",
        config_dir().display(),
        mic_chain_wanted(&settings),
        settings.output_filter.enabled,
        settings.echo_cancel.enabled
    );
    println!("probe watch duration_ms={}", duration.as_millis());

    pw::init();
    let result = run_pipewire_probe(duration);
    // SAFETY: this binary calls `pw::init()` once on this thread and all
    // PipeWire handles have been dropped before deinit returns to main.
    unsafe {
        pw::deinit();
    }
    result
}

/// `Ok(None)` means `--help` was asked for and printed.
///
/// Two flags do not need a state machine walking the iterator, and the help
/// case does not need a zero-duration sentinel that a second function has to
/// know means "do not run".
fn parse_duration(args: &[String]) -> Result<Option<Duration>, String> {
    const DEFAULT: Duration = Duration::from_secs(5);
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Ok(Some(DEFAULT)),
        ["--help" | "-h"] => {
            print_help();
            Ok(None)
        }
        ["--duration-ms", value] => value
            .parse::<u64>()
            .map(|millis| Some(Duration::from_millis(millis)))
            .map_err(|error| format!("--duration-ms value: {error}")),
        ["--duration-ms"] => Err("--duration-ms requires a value".to_owned()),
        [other, ..] => Err(format!("unknown argument: {other}")),
    }
}

fn print_help() {
    println!(
        "biglinux-microphone-probe [--duration-ms N]\n\n\
         Connects to PipeWire without GTK and prints node state, stream events, \
         and common negotiated latency/quantum properties."
    );
}

fn run_pipewire_probe(duration: Duration) -> Result<(), String> {
    let main_loop =
        pw::main_loop::MainLoopRc::new(None).map_err(|error| format!("MainLoop::new: {error}"))?;
    let main_loop_weak = main_loop.downgrade();
    let _sig_int = main_loop.loop_().add_signal_local(Signal::INT, move || {
        if let Some(main_loop) = main_loop_weak.upgrade() {
            main_loop.quit();
        }
    });
    let main_loop_weak = main_loop.downgrade();
    let _sig_term = main_loop.loop_().add_signal_local(Signal::TERM, move || {
        if let Some(main_loop) = main_loop_weak.upgrade() {
            main_loop.quit();
        }
    });

    let main_loop_weak = main_loop.downgrade();
    let timer = main_loop.loop_().add_timer(move |_| {
        if let Some(main_loop) = main_loop_weak.upgrade() {
            main_loop.quit();
        }
    });
    timer
        .update_timer(Some(duration), None)
        .into_result()
        .map_err(|error| format!("arm duration timer: {error}"))?;

    let context = pw::context::ContextRc::new(&main_loop, None)
        .map_err(|error| format!("Context::new: {error}"))?;
    let core = context
        .connect_rc(None)
        .map_err(|error| format!("connect: {error}"))?;
    let registry = core
        .get_registry_rc()
        .map_err(|error| format!("get_registry: {error}"))?;

    let _registry_listener = registry
        .add_listener_local()
        .global(|global| {
            if global.type_ != ObjectType::Node {
                return;
            }
            print_node_global(global.id, global.props);
        })
        .global_remove(|id| {
            println!("probe node_removed id={id}");
        })
        .register();

    println!("probe connected=true");
    main_loop.run();
    println!("probe disconnected=true");
    Ok(())
}

fn print_node_global(id: u32, props: Option<&pw::spa::utils::dict::DictRef>) {
    let media_class = prop(props, "media.class");
    let event = if media_class.starts_with("Stream/") {
        "stream"
    } else {
        "node"
    };
    println!(
        "probe {event}_appeared id={id} name={} media_class={} state={} quantum={} rate={} latency={} driver={} target={}",
        prop(props, "node.name"),
        empty_as_dash(media_class),
        prop(props, "node.state"),
        first_present(
            props,
            &["clock.quantum", "node.quantum", "api.alsa.period-size"]
        ),
        first_present(props, &["clock.rate", "node.rate", "audio.rate"]),
        first_present(props, &["node.latency", "api.alsa.headroom"]),
        prop(props, "node.driver"),
        first_present(props, &["target.object", "node.target"]),
    );
}

fn prop<'a>(props: Option<&'a pw::spa::utils::dict::DictRef>, key: &str) -> &'a str {
    props.and_then(|props| props.get(key)).unwrap_or("-")
}

fn first_present<'a>(props: Option<&'a pw::spa::utils::dict::DictRef>, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| props.and_then(|props| props.get(key)))
        .unwrap_or("-")
}

fn empty_as_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}
