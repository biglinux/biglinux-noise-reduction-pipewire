// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shortcut specs, override parsing, and accelerator resolution (display-free).
//!
//! The gdk keyval helpers and the GTK-backed `display_accel` label formatter
//! live in `big_app_kit::keybindings`.

use std::collections::HashMap;

/// Compile-time description of one keyboard shortcut the app declares.
///
/// Apps assemble a `&'static [ShortcutDef]` and feed it through
/// [`resolve_accels`] to combine defaults with any user overrides.
pub struct ShortcutDef {
    /// Scoped action name (`"win.play-pause"`).
    pub action: &'static str,
    /// Localized title for the shortcut dialog row.
    pub display_name: &'static str,
    /// Grouping label shown above the row in the shortcut dialog.
    pub category: &'static str,
    /// Canonical GTK accelerator (`"<Ctrl>m"`, `"space"`) used when
    /// the user has not customised it.
    pub default_accel: &'static str,
    /// `true` when the binding is a bare key (no modifiers) and must
    /// be suppressed while a text entry has focus.
    pub single_key: bool,
}

/// Pair each [`ShortcutDef`] with its effective accelerator string.
///
/// Parses `custom` via [`parse_custom`] and overlays the overrides on top
/// of each shortcut's `default_accel`. Returned vector preserves the input
/// order so it can drive a settings UI directly.
#[must_use]
pub fn resolve_accels<'a>(
    shortcuts: &'a [ShortcutDef],
    custom: &str,
) -> Vec<(&'a ShortcutDef, String)> {
    let overrides: HashMap<String, String> = parse_custom(custom).into_iter().collect();
    shortcuts
        .iter()
        .map(|shortcut| {
            let accel = overrides
                .get(shortcut.action)
                .cloned()
                .unwrap_or_else(|| shortcut.default_accel.to_string());
            (shortcut, accel)
        })
        .collect()
}

/// Build an accel-string -> action-id map for the active shortcut set.
///
/// When `only_single_key` is true, only shortcuts marked `single_key` are
/// included (used for typing-mode bindings that should not fire while a
/// text entry has focus).
#[must_use]
pub fn build_accel_map(
    shortcuts: &[ShortcutDef],
    custom: &str,
    only_single_key: bool,
) -> HashMap<String, String> {
    resolve_accels(shortcuts, custom)
        .into_iter()
        .filter(|(shortcut, _)| !only_single_key || shortcut.single_key)
        .map(|(shortcut, accel)| (accel, shortcut.action.to_string()))
        .collect()
}

/// Upgrade a legacy `Ctrl+Shift+f`-style accel to canonical `<Ctrl><Shift>f`.
///
/// Returns the input unchanged when it already contains `<` or carries no
/// `+` separators. Unknown modifier tokens fall back to the original
/// string rather than producing a broken accelerator.
#[must_use]
pub fn migrate_legacy_accel(accel: &str) -> String {
    if accel.contains('<') || !accel.contains('+') {
        return accel.to_string();
    }
    let parts: Vec<&str> = accel.split('+').collect();
    let (key, mods) = match parts.split_last() {
        Some((key, mods)) => (*key, mods),
        None => return accel.to_string(),
    };
    let mut out = String::new();
    for modifier in mods {
        match *modifier {
            "Ctrl" | "ctrl" | "Control" | "control" => out.push_str("<Ctrl>"),
            "Shift" | "shift" => out.push_str("<Shift>"),
            "Alt" | "alt" => out.push_str("<Alt>"),
            _ => return accel.to_string(),
        }
    }
    out.push_str(key);
    out
}

/// Decode the serialized custom-overrides string into `(action, accel)` pairs.
///
/// Expected format is `action=accel;action=accel`. Each accel passes through
/// [`migrate_legacy_accel`], so on-disk values from older releases are
/// silently upgraded.
#[must_use]
pub fn parse_custom(value: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for entry in value.split(';') {
        let entry = entry.trim();
        if let Some((action, accel)) = entry.split_once('=') {
            let action = action.trim().to_string();
            let accel = migrate_legacy_accel(accel.trim());
            if !action.is_empty() && !accel.is_empty() {
                result.push((action, accel));
            }
        }
    }
    result
}

/// Encode `(action, accel)` overrides as the on-disk `action=accel;...` form.
///
/// Inverse of [`parse_custom`]; safe to feed back through it.
#[must_use]
pub fn serialize_custom(overrides: &[(String, String)]) -> String {
    overrides
        .iter()
        .map(|(action, key)| format!("{action}={key}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Drop any override for `action_id` from the in-memory list.
///
/// No-op when the action is not currently customised. Paired with
/// [`set_custom_override`] to implement the "reset to default" path.
pub fn remove_custom_override(overrides: &mut Vec<(String, String)>, action_id: &str) {
    overrides.retain(|(action, _)| action != action_id);
}

/// Update the custom override field in place.
pub fn set_custom_override(
    overrides: &mut Vec<(String, String)>,
    action_id: impl Into<String>,
    accel: impl Into<String>,
    default_accel: &str,
) {
    let action_id = action_id.into();
    let accel = accel.into();
    remove_custom_override(overrides, &action_id);
    if accel != default_accel {
        overrides.push((action_id, accel));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORTCUTS: &[ShortcutDef] = &[
        ShortcutDef {
            action: "play-pause",
            display_name: "Play / Pause",
            category: "Playback",
            default_accel: "space",
            single_key: true,
        },
        ShortcutDef {
            action: "mute",
            display_name: "Mute",
            category: "Playback",
            default_accel: "<Ctrl>m",
            single_key: false,
        },
    ];

    #[test]
    fn parse_custom_migrates_legacy_format() {
        let result = parse_custom("mute=Ctrl+m;fullscreen=Ctrl+Shift+f");
        assert_eq!(result[0], ("mute".into(), "<Ctrl>m".into()));
        assert_eq!(result[1], ("fullscreen".into(), "<Ctrl><Shift>f".into()));
    }

    #[test]
    fn migrate_legacy_accel_supports_alt_modifier() {
        assert_eq!(migrate_legacy_accel("Alt+Return"), "<Alt>Return");
        assert_eq!(migrate_legacy_accel("alt+space"), "<Alt>space");
    }

    #[test]
    fn legacy_accel_with_angle_marker_stays_unmigrated() {
        assert_eq!(migrate_legacy_accel("Ctrl+<m"), "Ctrl+<m");
    }

    #[test]
    fn parse_custom_rejects_empty_action_or_accel() {
        let result = parse_custom("=p;mute=;play-pause=space");

        assert_eq!(result, vec![("play-pause".into(), "space".into())]);
    }

    #[test]
    fn serialize_custom_roundtrips() {
        let original = vec![
            ("play-pause".into(), "p".into()),
            ("volume-up".into(), "Up".into()),
        ];
        assert_eq!(parse_custom(&serialize_custom(&original)), original);
    }

    #[test]
    fn build_accel_map_only_single_key_filters() {
        let map = build_accel_map(SHORTCUTS, "", true);
        assert_eq!(map.get("space"), Some(&"play-pause".to_string()));
        assert_eq!(map.get("<Ctrl>m"), None);
    }

    #[test]
    fn custom_override_applies_to_matching_action() {
        let resolved = resolve_accels(SHORTCUTS, "play-pause=p");
        assert_eq!(resolved[0].1, "p");
        assert_eq!(resolved[1].1, "<Ctrl>m");
    }

    #[test]
    fn set_custom_override_replaces_and_removes_default() {
        let mut entries = vec![("play-pause".to_string(), "space".to_string())];

        set_custom_override(&mut entries, "play-pause", "p", "space");
        assert_eq!(entries, vec![("play-pause".to_string(), "p".to_string())]);

        set_custom_override(&mut entries, "play-pause", "space", "space");
        assert!(entries.is_empty());
    }

    #[test]
    fn remove_custom_override_keeps_other_actions() {
        let mut entries = vec![
            ("play-pause".to_string(), "p".to_string()),
            ("mute".to_string(), "m".to_string()),
        ];

        remove_custom_override(&mut entries, "play-pause");
        assert_eq!(entries, vec![("mute".to_string(), "m".to_string())]);
    }
}
