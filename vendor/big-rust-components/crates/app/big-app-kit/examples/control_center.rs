use big_app_kit::forms::{BigFieldSpec, BigSettingsGroupSpec, BigSettingsPageSpec};
use big_app_kit::shell::BigShellSpec;

fn main() {
    let shell = BigShellSpec::control_center("br.com.biglinux.Settings", "Settings");
    let page = BigSettingsPageSpec::new("General").group(BigSettingsGroupSpec::new(
        "Paths",
        vec![
            BigFieldSpec::path_row("output", "Output folder"),
            BigFieldSpec::switch_row("delete-originals", "Delete original files"),
        ],
    ));

    assert!(shell.resolved().requires_navigation);
    assert_eq!(page.field_count(), 2);
}
