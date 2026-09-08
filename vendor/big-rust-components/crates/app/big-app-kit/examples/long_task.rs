use big_app_kit::dialogs::BigDialogSpec;
use big_app_kit::tasks::{BigTaskProgress, BigTaskSpec, BigTaskState};

fn main() {
    let task = BigTaskSpec::new("scan", "Scan files");
    let progress = BigTaskProgress::new(BigTaskState::Running, "Scanning").fraction(0.5);
    let error = BigDialogSpec::error_with_details("Scan failed", "log path");

    assert!(task.resolved().needs_cancel_output);
    assert_eq!(progress.fraction, Some(0.5));
    assert!(error.resolved().requires_copy_button);
}
