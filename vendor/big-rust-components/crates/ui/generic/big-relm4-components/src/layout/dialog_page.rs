//! Shared dialog page shell for BigLinux Relm4/libadwaita apps.

use adw::prelude::*;
use relm4::gtk;

/// Standard margins for dialog content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigDialogMargins {
    /// Start.
    pub start: i32,
    /// End.
    pub end: i32,
    /// Top.
    pub top: i32,
    /// Bottom.
    pub bottom: i32,
}

impl Default for BigDialogMargins {
    fn default() -> Self {
        Self {
            start: 16,
            end: 16,
            top: 12,
            bottom: 12,
        }
    }
}

/// Reusable dialog shell: `AdwToolbarView`, top `AdwHeaderBar`, content box.
///
/// Use this when app dialogs share the same visual structure. Put app-specific
/// controls in [`BigDialogPage::content`].
#[derive(Debug, Clone)]
pub struct BigDialogPage {
    root: adw::ToolbarView,
    header: adw::HeaderBar,
    content: gtk::Box,
}

impl BigDialogPage {
    /// Construct a [`BigDialogPage`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn plain() -> Self {
        Self::new(ScrollMode::Plain)
    }

    /// Construct a [`BigDialogPage`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn scrolled() -> Self {
        Self::new(ScrollMode::Scrolled)
    }

    /// Construct a [`BigDialogPage`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn scrolled_clamped(maximum_size: i32, tightening_threshold: i32) -> Self {
        Self::new(ScrollMode::ScrolledClamped {
            maximum_size,
            tightening_threshold,
        })
    }

    fn new(scroll_mode: ScrollMode) -> Self {
        let root = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        root.add_top_bar(&header);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        apply_margins(&content, BigDialogMargins::default());

        match scroll_mode {
            ScrollMode::Plain => root.set_content(Some(&content)),
            ScrollMode::Scrolled => {
                let scroll = scrolled_window();
                scroll.set_child(Some(&content));
                root.set_content(Some(&scroll));
            }
            ScrollMode::ScrolledClamped {
                maximum_size,
                tightening_threshold,
            } => {
                let scroll = scrolled_window();
                let clamp = adw::Clamp::new();
                clamp.set_maximum_size(maximum_size);
                clamp.set_tightening_threshold(tightening_threshold);
                clamp.set_child(Some(&content));
                scroll.set_child(Some(&clamp));
                root.set_content(Some(&scroll));
            }
        }

        Self {
            root,
            header,
            content,
        }
    }

    /// Return a reference to the `root` exposed by this [`BigDialogPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ToolbarView {
        &self.root
    }

    /// Return a reference to the `header` exposed by this [`BigDialogPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// Return a reference to the `content` exposed by this [`BigDialogPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn content(&self) -> &gtk::Box {
        &self.content
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> adw::ToolbarView {
        self.root
    }

    /// Return a reference to the `append description` exposed by this [`BigDialogPage`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    pub fn append_description(&self, text: &str) -> gtk::Label {
        let label = dialog_description(text);
        self.content.append(&label);
        label
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollMode {
    Plain,
    Scrolled,
    ScrolledClamped {
        maximum_size: i32,
        tightening_threshold: i32,
    },
}

/// Build a wrapped, left-aligned dim label suitable for placing above
/// the form on a dialog page.
#[must_use]
pub fn dialog_description(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .wrap(true)
        .margin_bottom(16)
        .build();
    label.add_css_class("dim-label");
    label
}

fn scrolled_window() -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}

fn apply_margins(content: &gtk::Box, margins: BigDialogMargins) {
    content.set_margin_start(margins.start);
    content.set_margin_end(margins.end);
    content.set_margin_top(margins.top);
    content.set_margin_bottom(margins.bottom);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_margins_match_biglinux_default() {
        assert_eq!(
            BigDialogMargins::default(),
            BigDialogMargins {
                start: 16,
                end: 16,
                top: 12,
                bottom: 12,
            }
        );
    }
}
