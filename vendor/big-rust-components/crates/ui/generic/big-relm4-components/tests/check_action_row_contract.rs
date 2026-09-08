use big_relm4_components::list::check_action_row::{BigCheckActionRowSpec, BigCheckRowActivation};

#[test]
fn check_action_row_contract_defaults_to_toggle_check_without_gtk() {
    let resolved = BigCheckActionRowSpec::new("<b>Universal</b>")
        .subtitle("A&B")
        .resolved();

    assert_eq!(resolved.title, "&lt;b&gt;Universal&lt;/b&gt;");
    assert_eq!(resolved.subtitle.as_deref(), Some("A&amp;B"));
    assert_eq!(resolved.activation, BigCheckRowActivation::ToggleCheck);
}

#[test]
fn check_action_row_contract_can_open_detail_without_gtk() {
    let resolved = BigCheckActionRowSpec::new("Customize")
        .trailing_icon_name("go-next-symbolic")
        .activation(BigCheckRowActivation::OpenDetail)
        .resolved();

    assert_eq!(
        resolved.trailing_icon_name.as_deref(),
        Some("go-next-symbolic")
    );
    assert_eq!(resolved.activation, BigCheckRowActivation::OpenDetail);
}
