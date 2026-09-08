//! Security regression tests for `BigSubprocessSpec` argv validation
//! and `BigAtomicJsonStore` crash-mid-rename safety.

use std::fs;

use big_app_kit::storage::BigAtomicJsonStore;
use big_app_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};
use serde::{Deserialize, Serialize};

// ── subprocess argv ────────────────────────────────────────────────

fn assert_rejects(byte: u8) {
    let argument_with_forbidden_byte = format!("safe-prefix{}suffix", byte as char);
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg(argument_with_forbidden_byte.clone())
        .build();
    let validation_error = subprocess_spec
        .try_resolved()
        .expect_err("argv should be rejected");
    assert!(
        matches!(validation_error, BigSubprocessError::ForbiddenArgvByte { byte: rejected_byte, .. } if rejected_byte == byte),
        "expected ForbiddenArgvByte({byte:#x}), got {validation_error:?}"
    );
}

#[test]
fn argv_rejects_nul() {
    assert_rejects(0x00);
}

#[test]
fn argv_rejects_newline() {
    assert_rejects(0x0a);
}

#[test]
fn argv_rejects_carriage_return() {
    assert_rejects(0x0d);
}

#[test]
fn argv_rejects_del() {
    assert_rejects(0x7f);
}

#[test]
fn program_with_nul_rejected() {
    let subprocess_spec = BigSubprocessSpec::builder().program("ec\0ho").build();
    let validation_error = subprocess_spec.try_resolved().unwrap_err();
    assert!(matches!(
        validation_error,
        BigSubprocessError::ForbiddenArgvByte { index: 0, byte: 0 }
    ));
}

#[test]
fn argv_accepts_normal_strings() {
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("ffprobe")
        .arg("-of")
        .arg("json")
        .arg("/tmp/file with spaces.mp4")
        .arg("--utf8=café")
        .build();
    assert!(subprocess_spec.try_resolved().is_ok());
}

#[test]
fn argv_accepts_tab() {
    // Tab is allowed — only NUL/LF/CR/DEL rejected.
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("printf")
        .arg("a\tb")
        .build();
    assert!(subprocess_spec.try_resolved().is_ok());
}

// ── atomic JSON store crash-mid-rename simulation ──────────────────

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Prefs {
    volume: u8,
    theme: String,
}

#[test]
fn atomic_json_store_round_trip() {
    let prefs_temp_dir = tempfile::tempdir().unwrap();
    let prefs_path = prefs_temp_dir.path().join("prefs.json");
    let prefs_store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&prefs_path);
    let written_prefs = Prefs {
        volume: 73,
        theme: "neon".into(),
    };
    prefs_store.write(&written_prefs).unwrap();
    let read_prefs = prefs_store.read().unwrap().unwrap();
    assert_eq!(read_prefs, written_prefs);
}

// Simulate a crash that happened after the temp file was written but
// before rename. On next read the *original* file must still be the
// previous-known-good content (or NotFound if nothing existed).
#[test]
fn atomic_json_store_crash_before_rename_keeps_previous() {
    let prefs_temp_dir = tempfile::tempdir().unwrap();
    let prefs_path = prefs_temp_dir.path().join("prefs.json");
    let prefs_store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&prefs_path);

    // 1. write a known-good baseline
    let first_prefs = Prefs {
        volume: 1,
        theme: "first".into(),
    };
    prefs_store.write(&first_prefs).unwrap();

    // 2. simulate the crash: drop a stray sibling temp file (as `write`
    //    would have created) WITHOUT renaming it over path.
    let orphan_temp_payload_path = prefs_temp_dir.path().join(".prefs.json.tmp.fake.0");
    fs::write(
        &orphan_temp_payload_path,
        br#"{"volume":42,"theme":"corrupted"}"#,
    )
    .unwrap();

    // 3. read — must observe first_prefs, not the orphaned sibling.
    let read_prefs = prefs_store.read().unwrap().unwrap();
    assert_eq!(
        read_prefs, first_prefs,
        "crash before rename leaked partial write"
    );

    // 4. cleanup orphan + write second_prefs normally; verify it lands.
    fs::remove_file(&orphan_temp_payload_path).unwrap();
    let second_prefs = Prefs {
        volume: 99,
        theme: "second".into(),
    };
    prefs_store.write(&second_prefs).unwrap();
    assert_eq!(prefs_store.read().unwrap().unwrap(), second_prefs);
}

// If the rename happens but the temp file is *replaced* by a truncated
// payload (simulating partial flush before rename), the round-trip must
// still surface a JSON error rather than silently returning corruption.
#[test]
fn atomic_json_store_truncated_payload_errors() {
    let prefs_temp_dir = tempfile::tempdir().unwrap();
    let prefs_path = prefs_temp_dir.path().join("prefs.json");
    // hand-write a truncated file as if a previous process crashed.
    fs::write(&prefs_path, b"{\"volume\": 12, \"theme\": \"hal").unwrap();
    let prefs_store: BigAtomicJsonStore<Prefs> = BigAtomicJsonStore::new(&prefs_path);
    let read_result = prefs_store.read();
    assert!(
        read_result.is_err(),
        "expected JSON error, got {read_result:?}"
    );
}
