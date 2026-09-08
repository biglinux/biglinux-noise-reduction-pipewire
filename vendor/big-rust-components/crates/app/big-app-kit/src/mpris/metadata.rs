use glib::Variant;
use gtk::glib;

pub(super) fn build_metadata(
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: f64,
    art_url: Option<&str>,
) -> Variant {
    let dur_us = (duration_secs * 1_000_000.0) as i64;
    let dict = glib::VariantDict::new(None);
    let trackid = glib::Variant::parse(
        Some(glib::VariantTy::OBJECT_PATH),
        "'/org/mpris/MediaPlayer2/Track/0'",
    )
    .expect("valid MPRIS trackid object path");
    dict.insert_value("mpris:trackid", &trackid);
    dict.insert("xesam:title", title);
    if !artist.is_empty() {
        let artists: Vec<String> = vec![artist.to_string()];
        dict.insert("xesam:artist", &artists);
    }
    if !album.is_empty() {
        dict.insert("xesam:album", album);
    }
    dict.insert("mpris:length", dur_us);
    if let Some(url) = art_url
        && !url.is_empty()
    {
        dict.insert("mpris:artUrl", url);
    }
    dict.end()
}

#[cfg(test)]
mod tests {
    use gtk::glib::VariantDict;
    use gtk::glib::VariantTy;
    use gtk::glib::variant::FromVariant;

    use super::build_metadata;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn metadata_includes_required_fields_and_art() {
        let meta = build_metadata("Song", "Artist", "Album", 12.5, Some("file:///tmp/art.png"));
        let dict = VariantDict::from_variant(&meta).expect("metadata dict");
        assert_eq!(
            dict.lookup::<String>("xesam:title").expect("title"),
            Some("Song".into())
        );
        assert_eq!(
            dict.lookup::<i64>("mpris:length").expect("length"),
            Some(12_500_000)
        );
        assert_eq!(
            dict.lookup::<String>("mpris:artUrl").expect("art"),
            Some("file:///tmp/art.png".into())
        );
        assert!(
            dict.lookup_value("mpris:trackid", Some(VariantTy::OBJECT_PATH))
                .is_some()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn metadata_omits_empty_optional_fields() {
        let meta = build_metadata("Song", "", "", 0.0, None);
        let dict = VariantDict::from_variant(&meta).expect("metadata dict");
        assert_eq!(
            dict.lookup::<String>("xesam:title").expect("title"),
            Some("Song".into())
        );
        assert_eq!(dict.lookup_value("xesam:artist", None), None);
        assert_eq!(dict.lookup_value("xesam:album", None), None);
        assert_eq!(dict.lookup_value("mpris:artUrl", None), None);
    }
}
