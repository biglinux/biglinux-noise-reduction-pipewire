use big_relm4_components::list::action_nav_row::BigActionNavRowSpec;

#[test]
fn action_nav_row_contract_escapes_plain_text_without_gtk() {
    let resolved = BigActionNavRowSpec::new("<b>Audio</b>")
        .subtitle("A&B")
        .prefix_icon_name("audio-volume-high-symbolic")
        .resolved();

    assert_eq!(resolved.title, "&lt;b&gt;Audio&lt;/b&gt;");
    assert_eq!(resolved.subtitle.as_deref(), Some("A&amp;B"));
    assert_eq!(
        resolved.prefix_icon_name.as_deref(),
        Some("audio-volume-high-symbolic")
    );
    assert_eq!(resolved.action_icon_name, "go-next-symbolic");
    assert_eq!(resolved.action_label, "<b>Audio</b>");
}

#[test]
fn action_nav_row_contract_allows_explicit_markup_without_gtk() {
    let resolved = BigActionNavRowSpec::new("<b>Audio</b>")
        .subtitle("<i>Settings</i>")
        .action_icon_name("document-edit-symbolic")
        .action_label("Edit audio settings")
        .allow_markup()
        .resolved();

    assert_eq!(resolved.title, "<b>Audio</b>");
    assert_eq!(resolved.subtitle.as_deref(), Some("<i>Settings</i>"));
    assert_eq!(resolved.action_icon_name, "document-edit-symbolic");
    assert_eq!(resolved.action_label, "Edit audio settings");
}
