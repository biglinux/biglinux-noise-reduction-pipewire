// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Stable settings-document keys for the shared text-editor surface.
//!
//! These string keys name the editor toggles inside an app's persisted
//! settings JSON. Kept as consts so producers and consumers agree byte-for-byte.

/// Settings key: whether the editor soft-wraps long lines.
pub const EDITOR_LINE_WRAPPING_KEY: &str = "editor_line_wrapping";
/// Settings key: whether the editor highlights Markdown syntax.
pub const EDITOR_MARKDOWN_HIGHLIGHTING_KEY: &str = "editor_markdown_highlighting";
/// Settings key: whether the editor shows the line-number gutter.
pub const EDITOR_SHOW_LINE_NUMBERS_KEY: &str = "editor_show_line_numbers";
