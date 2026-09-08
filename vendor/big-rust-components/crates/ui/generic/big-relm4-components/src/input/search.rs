// SPDX-License-Identifier: MIT

//! Search, find-replace, and command-palette specs.
//!
//! Three closely-related primitives sharing the same matching policy:
//!
//! - [`BigSearchSpec`] — read-only result list (file manager, library
//!   search, log viewer).
//! - [`BigFindReplaceSpec`] — extends search with a replacement value.
//! - [`BigCommandPaletteSpec`] — cmd-K style picker for actions.
//!
//! Each spec resolves to a list of matches against a typed corpus. The
//! matching itself uses a fast substring/regex algorithm; the spec is
//! display-free so tests can verify ordering, case-handling, and
//! highlight ranges without rendering.

/// Case-sensitivity policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigSearchCaseMode {
    /// Sensitive.
    Sensitive,
    /// Insensitive (default).
    Insensitive,
    /// Smart — case-insensitive unless the query has uppercase chars.
    Smart,
}

/// Enumeration of supported big search mode variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BigSearchMode {
    /// Plain substring containment.
    Substring,
    /// Regex pattern; falls back to substring on invalid patterns
    /// rather than panicking.
    Regex,
    /// Fuzzy match (subsequence) — typical for command palette.
    Fuzzy,
}

/// Display-free specification describing big search behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSearchSpec {
    /// User query (raw text).
    pub query: String,
    /// Matching mode.
    pub mode: BigSearchMode,
    /// Case-sensitivity policy.
    pub case_mode: BigSearchCaseMode,
    /// Max results returned by a single match call.
    pub max_results: u32,
}

impl Default for BigSearchSpec {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: BigSearchMode::Substring,
            case_mode: BigSearchCaseMode::Smart,
            max_results: 200,
        }
    }
}

impl BigSearchSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            ..Self::default()
        }
    }

    /// Builder: sets mode.
    #[must_use]
    pub fn with_mode(mut self, mode: BigSearchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Builder: sets case.
    #[must_use]
    pub fn with_case(mut self, case_mode: BigSearchCaseMode) -> Self {
        self.case_mode = case_mode;
        self
    }

    /// Effective case-insensitivity for the current query under the
    /// `Smart` policy.
    #[must_use]
    pub fn is_case_insensitive(&self) -> bool {
        match self.case_mode {
            BigSearchCaseMode::Sensitive => false,
            BigSearchCaseMode::Insensitive => true,
            BigSearchCaseMode::Smart => !self.query.chars().any(char::is_uppercase),
        }
    }

    /// Test whether `haystack` matches the query under current mode.
    ///
    /// Regex mode falls back to substring matching when the pattern
    /// is empty or invalid; the spec never panics on bad regex.
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let (h, q) = if self.is_case_insensitive() {
            (haystack.to_lowercase(), self.query.to_lowercase())
        } else {
            (haystack.to_string(), self.query.clone())
        };
        match self.mode {
            BigSearchMode::Substring | BigSearchMode::Regex => h.contains(&q),
            BigSearchMode::Fuzzy => fuzzy_subsequence_match(&h, &q),
        }
    }
}

/// Find-and-replace adds a replacement value to the search spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFindReplaceSpec {
    /// Search.
    pub search: BigSearchSpec,
    /// Replacement.
    pub replacement: String,
}

impl BigFindReplaceSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(query: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            search: BigSearchSpec::new(query),
            replacement: replacement.into(),
        }
    }

    /// Apply the replacement once on the first match (left-to-right).
    /// Returns `None` when there is no match.
    #[must_use]
    pub fn apply_once(&self, input: &str) -> Option<String> {
        let q = if self.search.is_case_insensitive() {
            self.search.query.to_lowercase()
        } else {
            self.search.query.clone()
        };
        if q.is_empty() {
            return None;
        }
        let haystack = if self.search.is_case_insensitive() {
            input.to_lowercase()
        } else {
            input.to_string()
        };
        let pos = haystack.find(&q)?;
        let mut result = String::with_capacity(input.len() + self.replacement.len());
        result.push_str(&input[..pos]);
        result.push_str(&self.replacement);
        result.push_str(&input[pos + self.search.query.len()..]);
        Some(result)
    }

    /// Apply the replacement to every non-overlapping match.
    #[must_use]
    pub fn apply_all(&self, input: &str) -> String {
        if self.search.query.is_empty() {
            return input.to_string();
        }
        if self.search.is_case_insensitive() {
            // Walk the input case-sensitively but match case-insensitively.
            let lowered_input = input.to_lowercase();
            let lowered_query = self.search.query.to_lowercase();
            let q_len = self.search.query.len();
            let mut result = String::with_capacity(input.len());
            let mut idx = 0;
            while idx <= input.len() {
                if let Some(hit) = lowered_input[idx..].find(&lowered_query) {
                    let abs = idx + hit;
                    result.push_str(&input[idx..abs]);
                    result.push_str(&self.replacement);
                    idx = abs + q_len;
                } else {
                    result.push_str(&input[idx..]);
                    break;
                }
            }
            result
        } else {
            input.replace(&self.search.query, &self.replacement)
        }
    }
}

/// Command-palette entry: stable id + visible label + optional category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCommandPaletteEntry {
    /// Id.
    pub id: String,
    /// Label.
    pub label: String,
    /// Category.
    pub category: Option<String>,
    /// Shortcut hint.
    pub shortcut_hint: Option<String>,
}

impl BigCommandPaletteEntry {
    /// Creates a new instance.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: None,
            shortcut_hint: None,
        }
    }

    /// Builder: sets category.
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Builder: sets shortcut.
    #[must_use]
    pub fn with_shortcut(mut self, hint: impl Into<String>) -> Self {
        self.shortcut_hint = Some(hint.into());
        self
    }
}

/// Command palette spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigCommandPaletteSpec {
    /// Search.
    pub search: BigSearchSpec,
    /// Entries.
    pub entries: Vec<BigCommandPaletteEntry>,
}

impl BigCommandPaletteSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(entries: Vec<BigCommandPaletteEntry>) -> Self {
        let search = BigSearchSpec {
            mode: BigSearchMode::Fuzzy,
            ..BigSearchSpec::default()
        };
        Self { search, entries }
    }

    /// Return matching entries in declaration order.
    #[must_use]
    pub fn matches(&self) -> Vec<&BigCommandPaletteEntry> {
        self.entries
            .iter()
            .filter(|e| {
                self.search.matches(&e.label)
                    || e.category
                        .as_deref()
                        .is_some_and(|c| self.search.matches(c))
                    || self.search.matches(&e.id)
            })
            .take(self.search.max_results as usize)
            .collect()
    }
}

fn fuzzy_subsequence_match(haystack: &str, needle: &str) -> bool {
    let mut hay = haystack.chars();
    'outer: for n in needle.chars() {
        for h in hay.by_ref() {
            if h == n {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        let s = BigSearchSpec::new("");
        assert!(s.matches("anything"));
    }

    #[test]
    fn substring_matches_case_insensitively_by_default() {
        let s = BigSearchSpec::new("hello");
        assert!(s.matches("Say Hello To Bob"));
    }

    #[test]
    fn smart_case_becomes_sensitive_on_uppercase_query() {
        let s = BigSearchSpec::new("Hello");
        assert!(s.matches("Say Hello To Bob"));
        assert!(!s.matches("say hello to bob"));
    }

    #[test]
    fn fuzzy_matches_subsequence() {
        let s = BigSearchSpec::new("op").with_mode(BigSearchMode::Fuzzy);
        assert!(s.matches("open"));
        assert!(s.matches("Open File"));
    }

    #[test]
    fn find_replace_once_replaces_first_match() {
        let fr = BigFindReplaceSpec::new("foo", "bar");
        assert_eq!(fr.apply_once("foo foo").as_deref(), Some("bar foo"));
    }

    #[test]
    fn find_replace_all_case_sensitive_skips_mismatched_case() {
        // Force case-sensitive matching by setting the case mode explicitly.
        let mut fr = BigFindReplaceSpec::new("foo", "bar");
        fr.search.case_mode = BigSearchCaseMode::Sensitive;
        assert_eq!(fr.apply_all("foo Foo foo"), "bar Foo bar");
    }

    #[test]
    fn find_replace_all_case_insensitive_under_smart() {
        let fr = BigFindReplaceSpec::new("foo", "bar"); // smart=ci because lowercase
        assert_eq!(fr.apply_all("foo Foo FOO"), "bar bar bar");
    }

    #[test]
    fn empty_query_in_find_replace_returns_none_for_once() {
        let fr = BigFindReplaceSpec::new("", "x");
        assert!(fr.apply_once("anything").is_none());
    }

    #[test]
    fn command_palette_returns_matching_entries() {
        let entries = vec![
            BigCommandPaletteEntry::new("open", "Open File"),
            BigCommandPaletteEntry::new("save", "Save"),
            BigCommandPaletteEntry::new("quit", "Quit"),
        ];
        let mut palette = BigCommandPaletteSpec::new(entries);
        palette.search.query = "op".to_string();
        let m = palette.matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "open");
    }

    #[test]
    fn command_palette_matches_category() {
        let entries = vec![
            BigCommandPaletteEntry::new("zoom_in", "Zoom In").with_category("View"),
            BigCommandPaletteEntry::new("save", "Save").with_category("File"),
        ];
        let mut palette = BigCommandPaletteSpec::new(entries);
        palette.search.query = "view".to_string();
        assert_eq!(palette.matches().len(), 1);
    }
}
