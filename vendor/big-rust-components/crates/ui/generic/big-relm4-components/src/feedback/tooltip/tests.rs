use super::*;

#[test]
fn config_defaults_match_biglinux_policy() {
    assert_eq!(
        BigTooltipConfig::default(),
        BigTooltipConfig {
            show_delay_ms: 150,
            fade_out_ms: 300,
            css_fade_ms: 200,
            max_width_chars: 80,
        }
    );
}

#[test]
fn config_builder_updates_values() {
    let config = BigTooltipConfig::default()
        .show_delay_ms(1)
        .fade_out_ms(2)
        .css_fade_ms(3)
        .max_width_chars(4);
    assert_eq!(config.show_delay_ms, 1);
    assert_eq!(config.fade_out_ms, 2);
    assert_eq!(config.css_fade_ms, 3);
    assert_eq!(config.max_width_chars, 4);
}

#[test]
fn adjust_dark_background_lightens() {
    assert_eq!(adjust_tooltip_background("#1a1a1a"), "#424242");
}

#[test]
fn adjust_light_background_darkens() {
    assert_eq!(adjust_tooltip_background("#ffffff"), "#ebebeb");
}

#[test]
fn adjust_clamps_lighten_overflow() {
    assert_eq!(adjust_tooltip_background("#ff0000"), "#ff2828");
}

#[test]
fn adjust_handles_hash_prefix() {
    assert_eq!(
        adjust_tooltip_background("#1a1a1a"),
        adjust_tooltip_background("1a1a1a")
    );
}

#[test]
fn adjust_returns_original_on_invalid_input() {
    assert_eq!(adjust_tooltip_background("#abc"), "#abc");
    assert_eq!(adjust_tooltip_background("#gggggg"), "#gggggg");
}

// GtkLabel markup routing needs gtk_init; ordinary tests cover it while Miri
// stays focused on display-free Rust-owned invariants.
#[test]
#[cfg_attr(miri, ignore)]
fn apply_label_content_routes_plain_text_and_markup() {
    if gtk::init().is_err() {
        return;
    }

    let plain_label = gtk::Label::new(None);
    apply_label_content(&plain_label, "<b>bold</b>", LabelContentKind::PlainText);
    assert!(!plain_label.uses_markup());
    assert_eq!(plain_label.text().as_str(), "<b>bold</b>");

    let markup_label = gtk::Label::new(None);
    apply_label_content(&markup_label, "<b>bold</b>", LabelContentKind::Markup);
    assert!(markup_label.uses_markup());
    // Pango strips markup from the visible text.
    assert_eq!(markup_label.text().as_str(), "bold");
}

#[test]
fn dark_color_classification_is_conservative() {
    assert!(is_dark_color("#000000"));
    assert!(is_dark_color("#1a1a1a"));
    assert!(is_dark_color("#abc"));
    assert!(!is_dark_color("#ffffff"));
    assert!(!is_dark_color("#fafafa"));
}
