use super::*;

#[test]
fn banner_title_message_matches_modified_state() {
    assert_eq!(
        banner_title_message_for_modified_state(false),
        "The standard audio settings are in use."
    );
    assert_eq!(
        banner_title_message_for_modified_state(true),
        "Custom settings active. Click Apply to enable them."
    );
}

#[test]
#[cfg(not(miri))]
fn localized_banner_title_matches_modified_state() {
    assert_eq!(
        banner_title_for_modified_state(false),
        "The standard audio settings are in use."
    );
    assert_eq!(
        banner_title_for_modified_state(true),
        "Custom settings active. Click Apply to enable them."
    );
}

#[test]
fn reset_response_only_accepts_reset_id() {
    assert!(is_reset_response("reset"));
    assert!(!is_reset_response("cancel"));
    assert!(!is_reset_response(""));
}

#[test]
#[cfg(not(miri))]
fn headroom_options_keep_default_and_curated_values() {
    let options = headroom_options();
    let labels = options
        .iter()
        .map(|option| option.label.as_str())
        .collect::<Vec<_>>();
    let values = options
        .iter()
        .map(|option| option.value)
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        [
            "Default",
            "None",
            "Small (~5 ms)",
            "Medium (~11 ms)",
            "Standard (~21 ms)",
            "Large (virtual machines, ~42 ms)",
            "Maximum (last resort, ~85 ms)",
        ]
    );
    assert_eq!(
        values,
        [
            None,
            Some(0),
            Some(256),
            Some(512),
            Some(1024),
            Some(2048),
            Some(4096),
        ]
    );
}
