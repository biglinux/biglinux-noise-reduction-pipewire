use big_app_kit::collections::BigCollectionSpec;
use big_app_kit::previews::BigPreviewSpec;
use big_app_kit::shell::BigShellSpec;

fn main() {
    let shell = BigShellSpec::sidebar_detail("br.com.biglinux.Gallery", "Gallery");
    let grid = BigCollectionSpec::preview_grid("No previews").searchable(true);
    let preview = BigPreviewSpec::image();

    assert!(shell.resolved().requires_navigation);
    assert_eq!(grid.resolved().layout_role, "grid");
    assert!(!preview.resolved().editor_required);
}
