// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Preview and mini-editor contracts.

/// Sort of media the preview pane should render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPreviewKind {
    /// Still image (PNG, JPEG, ...).
    Image,
    /// Video stream with optional timeline scrubbing.
    Video,
    /// Audio stream with optional waveform.
    Audio,
    /// Metadata-only view (no rendered media).
    Metadata,
    /// In-place plain-text editor.
    MiniTextEditor,
    /// In-place image editor (paint, annotate).
    MiniImageEditor,
    /// Crop/resize/rotate transformation tool.
    CropResizeRotate,
}

/// Preview pane spec — image / video / audio / metadata / mini text editor / crop tool.
///
/// # Capabilities
///
/// `preview`, `preview+timeline`, `preview+selection`, `preview+editor`
///
/// # Archetypes
///
/// `media-player`, `media-converter`, `file-manager`, `screenshot`, `editor`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPreviewSpec {
    /// Sort of media being previewed.
    pub kind: BigPreviewKind,
    /// `true` when the pane only views the file; `false` enables
    /// editor affordances.
    pub read_only: bool,
    /// `true` when the pane needs a toolbar.
    pub shows_toolbar: bool,
    /// `true` for video/audio panes that need a timeline scrubber.
    pub shows_timeline: bool,
    /// `true` for panes that let the user select a region (crop,
    /// text selection, ...).
    pub supports_selection: bool,
}

impl BigPreviewSpec {
    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn new(kind: BigPreviewKind) -> Self {
        let mut spec = Self {
            kind,
            read_only: true,
            shows_toolbar: false,
            shows_timeline: false,
            supports_selection: false,
        };
        spec.apply_kind_defaults();
        spec
    }

    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn image() -> Self {
        Self::new(BigPreviewKind::Image)
    }

    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn video() -> Self {
        Self::new(BigPreviewKind::Video)
    }

    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn audio() -> Self {
        Self::new(BigPreviewKind::Audio)
    }

    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn mini_text_editor() -> Self {
        Self::new(BigPreviewKind::MiniTextEditor)
    }

    /// Build a preview spec; defaults are applied per-kind via
    /// `apply_kind_defaults`.
    #[must_use]
    pub fn mini_image_editor() -> Self {
        Self::new(BigPreviewKind::MiniImageEditor)
    }

    /// Flip the spec into editor mode: `read_only = false` and toolbar
    /// visible.
    #[must_use]
    pub fn editable(mut self) -> Self {
        self.read_only = false;
        self.shows_toolbar = true;
        self
    }

    /// Compute the affordances an adapter must wire (media surface,
    /// timeline, editor, metadata).
    #[must_use]
    pub fn resolved(&self) -> BigPreviewResolved {
        BigPreviewResolved {
            media_surface_required: matches!(
                self.kind,
                BigPreviewKind::Video | BigPreviewKind::Audio
            ),
            timeline_required: self.shows_timeline,
            editor_required: !self.read_only,
            metadata_required: matches!(self.kind, BigPreviewKind::Metadata),
        }
    }

    fn apply_kind_defaults(&mut self) {
        match self.kind {
            BigPreviewKind::Video | BigPreviewKind::Audio => {
                self.shows_toolbar = true;
                self.shows_timeline = true;
            }
            BigPreviewKind::MiniTextEditor
            | BigPreviewKind::MiniImageEditor
            | BigPreviewKind::CropResizeRotate => {
                self.read_only = false;
                self.shows_toolbar = true;
                self.supports_selection = true;
            }
            BigPreviewKind::Image | BigPreviewKind::Metadata => {}
        }
    }
}

/// Adapter-facing flags computed from a [`BigPreviewSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPreviewResolved {
    /// `true` when the adapter must spin up a media playback surface
    /// (GstPlay, mpv, ...).
    pub media_surface_required: bool,
    /// `true` when the adapter must place a timeline scrubber under
    /// the media surface.
    pub timeline_required: bool,
    /// `true` when the adapter must wire in editor affordances
    /// (toolbar, save).
    pub editor_required: bool,
    /// `true` for metadata-only views.
    pub metadata_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_preview_requires_media_surface_and_timeline() {
        let resolved = BigPreviewSpec::video().resolved();
        assert!(resolved.media_surface_required);
        assert!(resolved.timeline_required);
    }

    #[test]
    fn mini_image_editor_is_editable_by_default() {
        let resolved = BigPreviewSpec::new(BigPreviewKind::MiniImageEditor).resolved();
        assert!(resolved.editor_required);
    }
}
