//! Real-child regressions for the shared subprocess boundary.
#![cfg(not(miri))]
use std::time::{Duration, Instant};
use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

#[test]
fn an_explicit_empty_allow_list_denies_execution() {
    let spec = BigSubprocessSpec::builder().program("true").allow_list(Vec::<String>::new()).build();
    assert!(spec.try_resolved().is_err());
}

#[test]
fn a_child_that_never_reads_stdin_cannot_bypass_the_timeout() {
    let started = Instant::now();
    let result = BigSubprocessSpec::builder().program("sleep").arg("10")
        .stdin(vec![b'x'; 1024 * 1024]).timeout(Duration::from_millis(100)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn full_duplex_pipes_do_not_deadlock() {
    let payload = vec![b'x'; 512 * 1024];
    let result = BigSubprocessSpec::builder().program("cat")
        .stdin(payload.clone()).timeout(Duration::from_secs(3)).build().run().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, payload);
}

#[test]
fn inherited_pipe_lifetimes_are_bounded_too() {
    let started = Instant::now();
    // Literal test fixture; no untrusted value is passed to the shell.
    let result = BigSubprocessSpec::builder().program("sh").args(["-c", "sleep 10 & exit 0"])
        .timeout(Duration::from_millis(100)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn oversized_output_is_a_reported_error_not_silent_truncation() {
    let result = BigSubprocessSpec::builder().program("head").args(["-c", "17000000", "/dev/zero"])
        .timeout(Duration::from_secs(5)).build().run();
    assert!(matches!(result, Err(BigSubprocessError::CaptureLimit { .. })));
}

#[test]
fn non_utf8_output_cannot_disable_secret_redaction() {
    let result = BigSubprocessSpec::builder().program("printf")
        .arg("\\377 test-token").redact("test-token").build().run().unwrap();
    assert_eq!(result.stdout, b"\xff [REDACTED]");
}
