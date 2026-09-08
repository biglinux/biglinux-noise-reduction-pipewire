//! Accelerator display label.
//!
//! Replaces deprecated `GtkShortcutLabel` with a plain, testable label.

use relm4::gtk;

/// Display-free specification describing big accelerator label behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAcceleratorLabelSpec {
    accelerator: String,
    empty_text: String,
}

impl BigAcceleratorLabelSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(accelerator: impl Into<String>) -> Self {
        Self {
            accelerator: accelerator.into(),
            empty_text: String::new(),
        }
    }

    /// Configure the `empty_text` setting and return the updated builder.
    ///
    /// The supplied `text` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigAcceleratorLabelSpec`].
    #[must_use]
    pub fn empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = text.into();
        self
    }

    /// Return a reference to the `display text` exposed by this [`BigAcceleratorLabelSpec`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn display_text(&self) -> String {
        accelerator_display_text(&self.accelerator, &self.empty_text)
    }
}

/// Render a GTK accelerator string (`"<Ctrl><Shift>f"`) as a human
/// label (`"Ctrl+Shift+F"`). Empty input falls back to `empty_text`;
/// unrecognised input is returned verbatim.
#[must_use]
pub fn accelerator_display_text(accelerator: &str, empty_text: &str) -> String {
    let mut rest = accelerator.trim();
    if rest.is_empty() {
        return empty_text.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for (token, label) in [
        ("<Primary>", "Ctrl"),
        ("<Control>", "Ctrl"),
        ("<Ctrl>", "Ctrl"),
        ("<Shift>", "Shift"),
        ("<Alt>", "Alt"),
        ("<Super>", "Super"),
        ("<Meta>", "Meta"),
    ] {
        if let Some(tail) = rest.strip_prefix(token) {
            parts.push(label.to_string());
            rest = tail;
        }
    }
    let key = display_key(rest);
    if parts.is_empty() && key == rest {
        return accelerator.to_string();
    }
    parts.push(key);
    parts.join("+")
}

fn display_key(key: &str) -> String {
    match key {
        "space" => "Space".to_string(),
        "plus" => "+".to_string(),
        "minus" => "-".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "Page_Up" => "Page Up".to_string(),
        "Page_Down" => "Page Down".to_string(),
        "BackSpace" => "Backspace".to_string(),
        one if one.chars().count() == 1 => one.to_uppercase(),
        other => other.replace('_', " "),
    }
}

/// Reusable `<Ctrl>+Q`-style label widget pre-styled with the
/// `accelerator` CSS class.
#[derive(Debug, Clone)]
pub struct BigAcceleratorLabel {
    label: gtk::Label,
}

impl BigAcceleratorLabel {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigAcceleratorLabelSpec) -> Self {
        let label = gtk::Label::builder()
            .label(spec.display_text())
            .css_classes(["accelerator", "caption"])
            .valign(gtk::Align::Center)
            .build();
        Self { label }
    }

    /// Return a reference to the `widget` exposed by this [`BigAcceleratorLabel`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn widget(&self) -> &gtk::Label {
        &self.label
    }

    /// Sets accelerator.
    pub fn set_accelerator(&self, accelerator: &str) {
        self.label
            .set_text(&accelerator_display_text(accelerator, ""));
    }
}

/// Convenience: build a styled [`gtk::Label`] from a GTK accelerator
/// string and detach it from the wrapper. Equivalent to
/// [`BigAcceleratorLabel::new`] + [`BigAcceleratorLabel::widget`].
#[must_use]
pub fn accelerator_label(accelerator: &str) -> gtk::Label {
    BigAcceleratorLabel::new(BigAcceleratorLabelSpec::new(accelerator))
        .widget()
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_accelerator_uses_empty_text() {
        assert_eq!(accelerator_display_text("", "Not set"), "Not set");
    }

    #[test]
    fn invalid_accelerator_falls_back_to_raw_text() {
        assert_eq!(
            accelerator_display_text("not a real accel", ""),
            "not a real accel"
        );
    }

    #[test]
    fn spec_resolves_display_text_without_display_server() {
        let spec = BigAcceleratorLabelSpec::new("<Ctrl>q").empty_text("Not set");
        assert_eq!(spec.display_text(), "Ctrl+Q");
    }
}
