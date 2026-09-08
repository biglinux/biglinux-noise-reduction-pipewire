from _common import done, replace, commit

TITLE = 'fix(subprocess): document capture failures and scope platform imports'
if not done(TITLE):
    path = 'vendor/big-rust-components/crates/foundation/big-os-kit/src/subprocess.rs'
    replace(path, 'use std::sync::atomic::{AtomicBool, Ordering};', 'use std::sync::atomic::AtomicBool;\n#[cfg(any(test, not(unix)))]\nuse std::sync::atomic::Ordering;')
    replace(path, "    CaptureLimit { stream: &'static str, limit: usize },", """    CaptureLimit {
        /// Captured stream that exceeded the limit: stdout or stderr.
        stream: &'static str,
        /// Maximum number of bytes permitted for that stream.
        limit: usize,
    },""")
    commit(TITLE, [path])
