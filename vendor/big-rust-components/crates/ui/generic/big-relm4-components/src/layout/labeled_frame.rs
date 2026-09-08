// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Labeled framed sections for previews and compact editors.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Data used to build a label plus framed child section.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigLabeledFrameSpec {
    /// Section title.
    pub title: String,
    /// Vertical spacing between label and frame.
    pub spacing: i32,
    /// Bottom margin applied to the frame.
    pub margin_bottom: i32,
    /// CSS classes applied to the frame.
    pub frame_css_classes: Vec<String>,
}

impl BigLabeledFrameSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            spacing: 4,
            margin_bottom: 2,
            frame_css_classes: vec!["card".to_owned()],
        }
    }

    /// Configure vertical spacing.
    #[must_use]
    pub fn spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Configure bottom margin.
    #[must_use]
    pub fn margin_bottom(mut self, margin_bottom: i32) -> Self {
        self.margin_bottom = margin_bottom;
        self
    }

    /// Configure frame CSS classes.
    #[must_use]
    pub fn frame_css_classes(
        mut self,
        frame_css_classes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.frame_css_classes = frame_css_classes.into_iter().map(Into::into).collect();
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigLabeledFrameResolved {
        BigLabeledFrameResolved {
            title: self.title.clone(),
            spacing: self.spacing.max(0),
            margin_bottom: self.margin_bottom.max(0),
            frame_css_classes: self.frame_css_classes.clone(),
        }
    }
}

/// Pure resolved labeled-frame contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigLabeledFrameResolved {
    /// Section title.
    pub title: String,
    /// Vertical spacing after clamping.
    pub spacing: i32,
    /// Bottom margin after clamping.
    pub margin_bottom: i32,
    /// CSS classes applied to the frame.
    pub frame_css_classes: Vec<String>,
}

/// Built label plus framed child section.
#[derive(Debug, Clone)]
pub struct BigLabeledFrame {
    root: gtk::Box,
    frame: gtk::Frame,
}

impl BigLabeledFrame {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigLabeledFrameSpec, child: &impl IsA<gtk::Widget>) -> Self {
        let resolved = spec.resolved();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(resolved.spacing)
            .build();
        root.append(
            &gtk::Label::builder()
                .label(&resolved.title)
                .xalign(0.0)
                .css_classes(["caption-heading"])
                .build(),
        );

        let css_classes: Vec<&str> = resolved
            .frame_css_classes
            .iter()
            .map(String::as_str)
            .collect();
        let frame = gtk::Frame::builder()
            .child(child)
            .css_classes(css_classes)
            .margin_bottom(resolved.margin_bottom)
            .build();
        root.append(&frame);

        Self { root, frame }
    }

    /// Return a reference to the root container.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the frame.
    #[must_use]
    pub fn frame(&self) -> &gtk::Frame {
        &self.frame
    }

    /// Consume `self` and yield the underlying root container.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labeled_frame_defaults_to_card_frame() {
        let resolved = BigLabeledFrameSpec::new("Preview").resolved();

        assert_eq!(resolved.title, "Preview");
        assert_eq!(resolved.spacing, 4);
        assert_eq!(resolved.margin_bottom, 2);
        assert_eq!(resolved.frame_css_classes, ["card"]);
    }

    #[test]
    fn resolved_clamps_spacing_and_margin() {
        let resolved = BigLabeledFrameSpec::new("Preview")
            .spacing(-8)
            .margin_bottom(-4)
            .resolved();

        assert_eq!(resolved.spacing, 0);
        assert_eq!(resolved.margin_bottom, 0);
    }
}
