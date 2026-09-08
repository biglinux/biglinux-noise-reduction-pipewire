use big_app_kit::collections::BigCollectionSpec;
use big_app_kit::files::{BigFileEntry, BigFileWorkflowSpec};
use big_app_kit::shell::BigShellSpec;
use big_app_kit::tasks::BigTaskSpec;

fn main() {
    let shell = BigShellSpec::file_queue("br.com.biglinux.FileQueue", "File Queue");
    let files = BigFileWorkflowSpec::open_files("Add files");
    let queue = BigCollectionSpec::file_queue("No files");
    let task = BigTaskSpec::new("process", "Process files");
    let file_entry = BigFileEntry::file("/tmp/example.mp3");

    assert!(shell.resolved().regions.contains(&"footer"));
    assert!(files.picker.resolved().multiple);
    assert!(queue.resolved().requires_drag_handles);
    assert!(task.resolved().needs_worker);
    assert_eq!(file_entry.display_name, "example.mp3");
}
