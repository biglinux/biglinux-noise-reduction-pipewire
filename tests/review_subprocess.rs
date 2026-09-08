//! Real-child regressions for the shared subprocess boundary.
#![cfg(not(miri))]
use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessOutputMode, BigSubprocessSpec};
use std::io::Write;
use std::os::fd::AsFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FIXTURE: &str = "BIGMIC_SUBPROCESS_PIPE_FIXTURE";
const ROLE: &str = "BIGMIC_SUBPROCESS_PIPE_ROLE";
const ENTRY: &str = "inherited_pipe_lifetimes_are_bounded_too";

#[test]
fn an_explicit_empty_allow_list_denies_execution() {
    let spec = BigSubprocessSpec::builder()
        .program("true")
        .allow_list(Vec::<String>::new())
        .build();
    assert!(spec.try_resolved().is_err());
}

#[test]
fn a_child_that_never_reads_stdin_cannot_bypass_the_timeout() {
    let started = Instant::now();
    let result = BigSubprocessSpec::builder()
        .program("sleep")
        .arg("10")
        .stdin(vec![b'x'; 1024 * 1024])
        .timeout(Duration::from_millis(100))
        .build()
        .run();
    assert!(matches!(result, Err(BigSubprocessError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn full_duplex_pipes_do_not_deadlock() {
    let payload = vec![b'x'; 512 * 1024];
    let result = BigSubprocessSpec::builder()
        .program("cat")
        .stdin(payload.clone())
        .timeout(Duration::from_secs(3))
        .build()
        .run()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, payload);
}

fn fixture_spec(directory: &Path, mode: &str) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program(std::env::current_exe().unwrap().to_str().unwrap())
        .args(["--exact", ENTRY, "--nocapture"])
        .env(FIXTURE, directory)
        .env(ROLE, mode)
        .stdout(BigSubprocessOutputMode::Capture)
        .stderr(BigSubprocessOutputMode::Capture)
        .timeout(Duration::from_secs(2))
        .build()
}

// The subprocess deliberately exits with an unjoined descendant. That is the
// failure scenario under test, not an application lifecycle recommendation.
#[allow(clippy::zombie_processes)]
fn fixture_process(directory: &Path, mode: &str) -> ! {
    if mode == "holder" || mode == "writer" {
        // Own duplicate writer descriptors throughout the sleep. This does
        // not depend on a shell, coreutils or libtest's Rust output capture.
        let mut stdout =
            std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned().unwrap());
        let mut stderr =
            std::fs::File::from(std::io::stderr().as_fd().try_clone_to_owned().unwrap());
        std::fs::write(directory.join("ready"), std::process::id().to_string()).unwrap();
        std::thread::sleep(if mode == "holder" {
            Duration::from_secs(10)
        } else {
            Duration::from_millis(150)
        });
        stdout.write_all(b"late stdout\n").unwrap();
        stderr.write_all(b"late stderr\n").unwrap();
        std::process::exit(0);
    }
    let mut descendant = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", ENTRY, "--nocapture"])
        .env(ROLE, if mode == "timeout" { "holder" } else { "writer" })
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start the pipe-holding descendant");
    let deadline = Instant::now() + Duration::from_secs(1);
    while !directory.join("ready").exists() {
        if Instant::now() >= deadline {
            let _ = descendant.kill();
            let _ = descendant.wait();
            panic!("Descendant did not acquire its inherited writer descriptors");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::process::exit(0);
}

#[cfg(target_os = "linux")]
fn assert_descendant_stopped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Ok(stat) => {
                // A zombie can remain briefly until the container's init
                // reaps it. It must not remain runnable or hold live pipes.
                let state = stat
                    .rsplit_once(") ")
                    .and_then(|(_, tail)| tail.bytes().next());
                if matches!(state, Some(b'Z' | b'X')) {
                    return;
                }
            }
            Err(error) => panic!("Cannot inspect descendant {pid}: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "Descendant {pid} was not stopped"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn inherited_pipe_lifetimes_are_bounded_too() {
    if let Some(directory) = std::env::var_os(FIXTURE) {
        fixture_process(Path::new(&directory), &std::env::var(ROLE).unwrap());
    }
    let directory = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let result = fixture_spec(directory.path(), "timeout").run();
    let pid: u32 = std::fs::read_to_string(directory.path().join("ready"))
        .expect("The deadline must exercise inherited pipes, not failed startup")
        .parse()
        .unwrap();
    assert!(
        matches!(result, Err(BigSubprocessError::Timeout)),
        "Captured writers still open after child exit: {result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    #[cfg(target_os = "linux")]
    assert_descendant_stopped(pid);
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}

#[test]
fn descendant_output_is_not_truncated_after_direct_child_exits() {
    let directory = tempfile::tempdir().unwrap();
    let result = fixture_spec(directory.path(), "late-output").run().unwrap();
    assert!(result.status.success());
    assert!(result.stdout.ends_with(b"late stdout\n"));
    assert!(result.stderr.ends_with(b"late stderr\n"));
}

#[test]
fn completed_child_does_not_report_a_spurious_timeout() {
    let result = BigSubprocessSpec::builder()
        .program("printf")
        .arg("complete")
        .timeout(Duration::from_secs(1))
        .build()
        .run()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"complete");
}

#[test]
fn oversized_output_is_a_reported_error_not_silent_truncation() {
    let result = BigSubprocessSpec::builder()
        .program("head")
        .args(["-c", "17000000", "/dev/zero"])
        .timeout(Duration::from_secs(5))
        .build()
        .run();
    assert!(matches!(
        result,
        Err(BigSubprocessError::CaptureLimit { .. })
    ));
}

#[test]
fn non_utf8_output_cannot_disable_secret_redaction() {
    let result = BigSubprocessSpec::builder()
        .program("printf")
        .arg("\\377 test-token")
        .redact("test-token")
        .build()
        .run()
        .unwrap();
    assert_eq!(result.stdout, b"\xff [REDACTED]");
}
