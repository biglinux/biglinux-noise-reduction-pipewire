// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Read-only inline controls for previews and review dialogs.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Kind of read-only inline control to render.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigReadOnlyInlineControlKind {
    /// Text entry echoing a value.
    Text {
        /// Text displayed by the entry.
        text: String,
        /// Hide entry contents when true.
        is_secret: bool,
        /// Entry width in characters.
        width_chars: i32,
    },
    /// Switch echoing a boolean value.
    Switch {
        /// Active state displayed by the switch.
        active: bool,
    },
}

/// Data used to build a read-only inline preview control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigReadOnlyInlineControlSpec {
    /// Control kind.
    pub kind: BigReadOnlyInlineControlKind,
}

impl BigReadOnlyInlineControlSpec {
    /// Create a read-only text control.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: BigReadOnlyInlineControlKind::Text {
                text: text.into(),
                is_secret: false,
                width_chars: 16,
            },
        }
    }

    /// Create a read-only password-like text control.
    #[must_use]
    pub fn password(text: impl Into<String>) -> Self {
        Self {
            kind: BigReadOnlyInlineControlKind::Text {
                text: text.into(),
                is_secret: true,
                width_chars: 16,
            },
        }
    }

    /// Create a read-only switch control.
    #[must_use]
    pub fn switch(active: bool) -> Self {
        Self {
            kind: BigReadOnlyInlineControlKind::Switch { active },
        }
    }

    /// Configure entry width for text controls.
    #[must_use]
    pub fn width_chars(mut self, width_chars: i32) -> Self {
        if let BigReadOnlyInlineControlKind::Text {
            width_chars: width, ..
        } = &mut self.kind
        {
            *width = width_chars;
        }
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigReadOnlyInlineControlResolved {
        match &self.kind {
            BigReadOnlyInlineControlKind::Text {
                text,
                is_secret,
                width_chars,
            } => BigReadOnlyInlineControlResolved {
                kind: BigReadOnlyInlineControlKind::Text {
                    text: text.clone(),
                    is_secret: *is_secret,
                    width_chars: (*width_chars).max(1),
                },
            },
            BigReadOnlyInlineControlKind::Switch { active } => BigReadOnlyInlineControlResolved {
                kind: BigReadOnlyInlineControlKind::Switch { active: *active },
            },
        }
    }
}

/// Pure resolved read-only control contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigReadOnlyInlineControlResolved {
    /// Control kind after clamping numeric values.
    pub kind: BigReadOnlyInlineControlKind,
}

/// Built read-only inline preview control.
#[derive(Debug, Clone)]
pub struct BigReadOnlyInlineControl {
    root: gtk::Widget,
}

impl BigReadOnlyInlineControl {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigReadOnlyInlineControlSpec) -> Self {
        let resolved = spec.resolved();
        let root = match resolved.kind {
            BigReadOnlyInlineControlKind::Text {
                text,
                is_secret,
                width_chars,
            } => {
                let entry = gtk::Entry::builder()
                    .text(text)
                    .visibility(!is_secret)
                    .valign(gtk::Align::Center)
                    .width_chars(width_chars)
                    .sensitive(false)
                    .can_focus(false)
                    .build();
                entry.upcast::<gtk::Widget>()
            }
            BigReadOnlyInlineControlKind::Switch { active } => {
                let switch = gtk::Switch::builder()
                    .active(active)
                    .valign(gtk::Align::Center)
                    .sensitive(false)
                    .can_focus(false)
                    .build();
                switch.upcast::<gtk::Widget>()
            }
        };
        Self { root }
    }

    /// Return a reference to the root widget.
    #[must_use]
    pub fn root(&self) -> &gtk::Widget {
        &self.root
    }

    /// Consume `self` and yield the underlying widget.
    #[must_use]
    pub fn into_root(self) -> gtk::Widget {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_control_defaults_to_visible_sixteen_char_width() {
        let resolved = BigReadOnlyInlineControlSpec::text("value").resolved();

        assert_eq!(
            resolved.kind,
            BigReadOnlyInlineControlKind::Text {
                text: "value".to_owned(),
                is_secret: false,
                width_chars: 16
            }
        );
    }

    #[test]
    fn password_control_marks_text_secret() {
        let resolved = BigReadOnlyInlineControlSpec::password("secret").resolved();

        assert_eq!(
            resolved.kind,
            BigReadOnlyInlineControlKind::Text {
                text: "secret".to_owned(),
                is_secret: true,
                width_chars: 16
            }
        );
    }

    #[test]
    fn width_is_clamped_to_one() {
        let resolved = BigReadOnlyInlineControlSpec::text("value")
            .width_chars(-10)
            .resolved();

        assert_eq!(
            resolved.kind,
            BigReadOnlyInlineControlKind::Text {
                text: "value".to_owned(),
                is_secret: false,
                width_chars: 1
            }
        );
    }
}
