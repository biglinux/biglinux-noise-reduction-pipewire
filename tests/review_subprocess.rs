//! Real-child regressions for the shared subprocess boundary.
#![cfg(not(miri))]
use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

// This short-lived fixture deliberately exits without waiting: the object
// under test must terminate its descendant process group. No shell job-control
// policy or ignored test is involved. The PID file proves spawn succeeded.
#[allow(clippy::zombie_processes)]
fn exit_with_inherited_pipes(directory: &Path) -> ! {
    let descendant = Command::new("sleep")
        .arg("10")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start the pipe-holding descendant");
    std::fs::write(directory.join("pid"), descendant.id().to_string()).unwrap();
    std::process::exit(0);
}

#[cfg(target_os = "linux")]
fn assert_descendant_stopped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        match state {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Ok(stat) => {
                // The orphan can briefly be a zombie pending the container's
                // init reaper. It must not remain runnable or hold live pipes.
                let state = stat.rsplit_once(") ").map(|(_, tail)| tail.as_bytes()[0]);
                if matches!(state, Some(b'Z' | b'X')) {
                    return;
                }
            }
            Err(error) => panic!("Cannot inspect descendant {pid}: {error}"),
        }
        assert!(Instant::now() < deadline, "Descendant {pid} was not stopped");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn inherited_pipe_lifetimes_are_bounded_too() {
    const FIXTURE: &str = "BIGMIC_SUBPROCESS_PIPE_FIXTURE";
    if let Some(directory) = std::env::var_os(FIXTURE) {
        exit_with_inherited_pipes(Path::new(&directory));
    }
    let directory = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    let started = Instant::now();
    let result = BigSubprocessSpec::builder()
        .program(executable.to_str().unwrap())
        .args([
            "--exact",
            "inherited_pipe_lifetimes_are_bounded_too",
            "--nocapture",
        ])
        .env(FIXTURE, directory.path())
        .timeout(Duration::from_secs(1))
        .build()
        .run();
    let pid: u32 = std::fs::read_to_string(directory.path().join("pid"))
        .expect("The deadline must exercise inherited pipes, not failed startup")
        .parse()
        .unwrap();
    assert!(
        matches!(result, Err(BigSubprocessError::Timeout)),
        "Inherited captured pipes must not be reported as completed: {result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    #[cfg(target_os = "linux")]
    assert_descendant_stopped(pid);
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
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
