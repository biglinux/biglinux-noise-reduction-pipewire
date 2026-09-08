use big_relm4_components::input::switch_row::BigSwitchRowSpec;

#[test]
fn switch_row_contract_escapes_plain_text_without_gtk() {
    let resolved = BigSwitchRowSpec::new("<b>Lockdown</b>", true)
        .subtitle("A&B")
        .resolved();

    assert_eq!(resolved.title, "&lt;b&gt;Lockdown&lt;/b&gt;");
    assert_eq!(resolved.subtitle.as_deref(), Some("A&amp;B"));
    assert!(resolved.active);
}

#[test]
fn switch_row_contract_allows_explicit_markup_without_gtk() {
    let resolved = BigSwitchRowSpec::new("<b>Lockdown</b>", false)
        .subtitle("<i>Read-only</i>")
        .allow_markup()
        .resolved();

    assert_eq!(resolved.title, "<b>Lockdown</b>");
    assert_eq!(resolved.subtitle.as_deref(), Some("<i>Read-only</i>"));
    assert!(!resolved.active);
}
