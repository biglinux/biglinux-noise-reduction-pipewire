//! `PaneContent` — one mountable content unit, three mount points.
//!
//! The keystone of the unified-shell restructure (`RESTRUCTURE-UNIFIED-SHELL.md`
//! §4.3.1, ADR D13): a single small trait every piece of content implements so
//! the generic chrome can host it in a standalone window, a `big-host` multicall
//! module, or the shell background surface — and arrange several of them in a
//! split. The chrome depends on this trait, never on a content crate
//! (`vte4`/`libmpv`/…); each content crate implements the trait for its own type
//! (the adapters land per phase: terminal, editor, files, image, camera, audio,
//! video).
//!
//! This module is the **API skeleton only** — the base trait, its optional
//! extension traits, the typed activity/focus enums, the owner [`PaneGuard`],
//! the [`PaneFactory`] bridge, and the [`PaneNode`] split tree. No adapter and
//! no assembled tabbed widget live here yet (K2-T2+).
//!
//! # Design (why these shapes)
//!
//! - **Small, object-safe base.** Adding a required method to a public trait is a
//!   SemVer break, so [`PaneContent`] carries only what *every* container needs;
//!   each optional capability is a separate trait ([`PanePreferences`],
//!   [`PaneCommands`]) added additively. The base is object-safe, so a container
//!   holds a `Box<dyn PaneContent>`.
//! - **Layout lives in the container, not the content.** Content is a
//!   layout-ignorant *leaf*; the tab set ([`crate::layout::tab_workspace`]) and the
//!   split arrangement ([`PaneNode`]) are data the container owns. This keeps
//!   layout serde-friendly and testable, and stops content from depending on where
//!   it is mounted.
//! - **Typed activity, never a `bool`.** [`PaneActivity`] distinguishes a *job*
//!   (a running command, audio playback, a recording, a download) from mere
//!   occlusion: a job MUST keep running when [`PaneActivity::Occluded`]; only
//!   animation / redraw / preview should pause. A `set_active(bool)` would invite
//!   the "occluded ⇒ kill the job" bug.
//! - **Ownership = a single strong owner that tears down on drop.** [`PaneGuard`]
//!   is the sole strong owner of a mounted pane (generalizing the shipping
//!   `big-terminal-ui` window guards); dropping it drops the content, whose own `Drop`
//!   must unparent its widget subtree. Implementors MUST NOT hold a strong `Rc`
//!   back to their own widgets — weak-ify every closure⇄widget edge (INVARIANTS
//!   H9). A debug finalization census must show zero survivors after unmount.
//!
//! `PaneState` (layout persistence) is deliberately **not** here yet — it needs
//! `serde_json` and is only used in the persistence phase (P5); being an extension
//! trait, it is added then without breaking this API.

pub mod companion;
pub mod controller;
pub use companion::CompanionPane;
pub use controller::ControllerPane;
pub mod split_tree;
pub use split_tree::{BigSplitTree, SplitPaneSurface};
pub mod split_tree_layout;
pub mod tabbed;
pub use tabbed::{BigPaneTabs, render_pane_node};
pub mod workspace_shell;
pub use workspace_shell::{BigWorkspaceShell, WorkspaceTab};

pub use crate::layout::split_panes::BigSplitOrientation;
use crate::layout::split_panes::BigSplitPlacement;
use crate::layout::tab_workspace::BigTabId;
use relm4::gtk;

/// A unit of content mountable in any container: a tab, a split-pane leaf, a
/// host-module window, or the shell background surface. One trait, three mount
/// points. Small and object-safe (`Box<dyn PaneContent>`); optional capabilities
/// are the separate traits below.
///
/// # Leak contract (mandatory — INVARIANTS H9)
///
/// The container owns the boxed content via a [`PaneGuard`] and drops it on
/// unmount; the implementor's `Drop` MUST tear the whole widget subtree down
/// (unparent `set_parent` children, drop controllers). Never hold a strong `Rc`
/// back to your own widgets — weak-ify every closure⇄widget edge. A debug
/// finalization census must show full finalization (zero survivors).
///
/// # Examples
///
/// ```no_run
/// use big_relm4_components::pane::{PaneContent, KeyboardNeed};
/// use relm4::gtk;
/// use relm4::gtk::prelude::Cast;
///
/// struct EmptyPane;
/// impl PaneContent for EmptyPane {
///     fn root(&self) -> gtk::Widget {
///         gtk::Label::new(Some("nothing here")).upcast()
///     }
///     fn title(&self) -> String {
///         "Empty".to_owned()
///     }
///     fn connect_title_changed(&self, _on_change: Box<dyn Fn(&str)>) {}
///     fn keyboard_need(&self) -> KeyboardNeed {
///         KeyboardNeed::None
///     }
/// }
/// let boxed: Box<dyn PaneContent> = Box::new(EmptyPane);
/// assert_eq!(boxed.title(), "Empty");
/// ```
pub trait PaneContent {
    /// The widget to mount. Called once; the container parents/unparents it.
    fn root(&self) -> gtk::Widget;

    /// Title for the tab/header (feeds `crate::layout::tab_workspace::BigTab::title`).
    fn title(&self) -> String;

    /// Subscribe to later title changes (editor dirty-marker, terminal working
    /// directory, player now-playing). The container re-reads [`title`](Self::title)
    /// and updates the tab/header when `on_change` fires.
    fn connect_title_changed(&self, on_change: Box<dyn Fn(&str)>);

    /// Keyboard need — drives the container focus policy, especially the
    /// background surface's `gtk4_layer_shell::KeyboardMode` escalation. Static
    /// content returns [`KeyboardNeed::None`].
    fn keyboard_need(&self) -> KeyboardNeed;

    /// Activity transition. The reason is typed (see [`PaneActivity`]).
    /// Idempotent; defaulted to a no-op for static content.
    fn set_activity(&self, _activity: PaneActivity) {}
}

/// Why the container is changing a pane's activity.
///
/// The distinction this enum encodes is load-bearing: a running **job** (a
/// terminal command, audio playback, a recording, a download, a transcode) MUST
/// keep running when merely [`Occluded`](Self::Occluded) or [`Hidden`](Self::Hidden);
/// only [`UserPaused`](Self::UserPaused) (and, opt-in, [`OnBattery`](Self::OnBattery))
/// may suspend work. Animation / redraw / polling / camera *preview* SHOULD stop on
/// occlusion; jobs MUST NOT. A `bool` would lose this distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaneActivity {
    /// Mapped and unobstructed — render and run normally.
    Visible,
    /// Covered by another surface. Pause animation/redraw/preview; keep jobs running.
    Occluded,
    /// Unmapped (e.g. on an inactive tab). Same rule as `Occluded` for jobs.
    Hidden,
    /// The user explicitly paused this content. Work may suspend.
    UserPaused,
    /// Running on battery; opt-in suspension of nonessential work.
    OnBattery,
}

/// Container-neutral focus requirement. Maps onto `gtk4_layer_shell::KeyboardMode`
/// for the background surface; toplevel windows ignore it (they always accept focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardNeed {
    /// Never takes keyboard focus (static content — wallpaper, a thumbnail grid).
    None,
    /// Takes focus on demand (click/hover), releases it otherwise (desktop icons).
    OnDemand,
    /// Holds keyboard focus while mapped (terminal, editor, text entry).
    Exclusive,
}

/// Optional capability: a content-specific preferences page (terminal color
/// scheme, player aspect, file-manager view options). The generic *chrome* theme
/// lives in the shell (the theme split, D5) — this is only the content's own knobs.
/// Object-safe; access it through a downcast or an explicit registration, never on
/// the base trait.
pub trait PanePreferences {
    /// The preferences page widget to embed in the shell's settings surface.
    fn preferences_page(&self) -> gtk::Widget;
}

/// Optional capability: the named commands this content exposes, each tagged with
/// a disclosure level so the chrome shows basic commands by default and reveals
/// advanced ones (progressive disclosure, §7). Feeds the unified introspectable
/// action layer and the command palette. Object-safe.
pub trait PaneCommands {
    /// The commands this content currently offers.
    fn commands(&self) -> Vec<PaneCommand>;
}

/// One named command exposed by a [`PaneCommands`] content type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneCommand {
    /// Stable action id (`win.`-style or content-local), used to activate it.
    pub id: String,
    /// Translated, human-readable label for menus / the command palette.
    pub title: String,
    /// Whether the command is surfaced by default or behind an "Advanced" reveal.
    pub disclosure: PaneDisclosure,
}

impl PaneCommand {
    /// Build a basic (default-visible) command.
    #[must_use]
    pub fn basic(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            disclosure: PaneDisclosure::Basic,
        }
    }

    /// Build an advanced (reveal-behind-disclosure) command.
    #[must_use]
    pub fn advanced(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            disclosure: PaneDisclosure::Advanced,
        }
    }
}

/// Progressive-disclosure level for a [`PaneCommand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneDisclosure {
    /// Shown by default — a core command for the content's basic task.
    Basic,
    /// Hidden behind an "Advanced" reveal / escalation — power-user command.
    Advanced,
}

/// Sole strong owner of one mounted [`PaneContent`].
///
/// The container holds the guard for the pane's lifetime and drops it on unmount;
/// because the content weak-refs its own widgets (the leak contract on
/// [`PaneContent`]), this guard is the only thing keeping the subtree alive — so
/// dropping it tears the subtree down. Generalizes the shipping `big-terminal-ui`
/// `FileManagerWindowGuard` / `TerminalWindowGuard` / `DesktopIconViewGuard`.
pub struct PaneGuard(Box<dyn PaneContent>);

impl PaneGuard {
    /// Take ownership of a boxed content.
    #[must_use]
    pub fn new(content: Box<dyn PaneContent>) -> Self {
        Self(content)
    }

    /// Borrow the owned content (to mount its [`root`](PaneContent::root), read
    /// its [`title`](PaneContent::title), or drive its activity).
    #[must_use]
    pub fn content(&self) -> &dyn PaneContent {
        &*self.0
    }
}

/// Builds a pane on demand from its persisted id.
///
/// The bridge from a serde-friendly [`BigTabId`] (what
/// [`crate::layout::tab_workspace`] and [`PaneNode`] store) to live content. Used
/// for lazy spawn (the background host builds content only when switched to it)
/// and rebuild-on-restore (layout persistence). Object-safe; returns `None` for
/// an id this factory does not recognize.
pub trait PaneFactory {
    /// Build the content for `id`, or `None` if unknown.
    fn build(&self, id: &str) -> Option<PaneGuard>;
}

/// A split arrangement of panes within one container, as data.
///
/// Leaves reference content by [`BigTabId`] (the live content lives in the
/// container's id→[`PaneGuard`] table), so the tree stays pure data — testable and
/// persistable. A single tab's content can itself be a [`Split`](Self::Split) of
/// several leaves (terminal + editor + files side by side), which is exactly what
/// "split inside a window" means. Reuses [`BigSplitOrientation`] from
/// [`crate::layout::split_panes`].
#[derive(Debug, Clone, PartialEq)]
pub enum PaneNode {
    /// A single pane, referenced by its content id.
    Leaf(BigTabId),
    /// Two child nodes separated by a divider.
    Split {
        /// Direction the divider runs in.
        orientation: BigSplitOrientation,
        /// Divider position as a fraction `0.0..=1.0` of the split axis.
        ratio: f32,
        /// The first child (left/top).
        first: Box<PaneNode>,
        /// The second child (right/bottom).
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// A leaf referencing `id`.
    #[must_use]
    pub fn leaf(id: impl Into<BigTabId>) -> Self {
        Self::Leaf(id.into())
    }

    /// A split of two children at `ratio` along `orientation`.
    #[must_use]
    pub fn split(orientation: BigSplitOrientation, ratio: f32, first: Self, second: Self) -> Self {
        Self::Split {
            orientation,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// Collect the content ids referenced by every leaf, left-to-right /
    /// top-to-bottom. The container uses this to know which [`PaneGuard`]s a node
    /// needs mounted.
    #[must_use]
    pub fn leaf_ids(&self) -> Vec<&BigTabId> {
        let mut ids = Vec::new();
        self.collect_leaf_ids(&mut ids);
        ids
    }

    fn collect_leaf_ids<'a>(&'a self, out: &mut Vec<&'a BigTabId>) {
        match self {
            Self::Leaf(id) => out.push(id),
            Self::Split { first, second, .. } => {
                first.collect_leaf_ids(out);
                second.collect_leaf_ids(out);
            }
        }
    }

    /// Whether any leaf references `id`.
    #[must_use]
    pub fn contains(&self, id: &BigTabId) -> bool {
        match self {
            Self::Leaf(leaf) => leaf == id,
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// Split the leaf referencing `target` into two, inserting `new_id` on the
    /// side given by `placement` at the given divider `ratio`. The new split's
    /// orientation follows the placement (Left/Right → horizontal, Top/Bottom →
    /// vertical). Returns `false` (tree unchanged) if `target` is not a leaf in
    /// this tree.
    pub fn split_leaf(
        &mut self,
        target: &BigTabId,
        new_id: BigTabId,
        placement: BigSplitPlacement,
        ratio: f32,
    ) -> bool {
        match self {
            Self::Leaf(id) if id == target => {
                let kept = Self::Leaf(id.clone());
                let added = Self::Leaf(new_id);
                *self = match placement {
                    BigSplitPlacement::Left => {
                        Self::split(BigSplitOrientation::Horizontal, ratio, added, kept)
                    }
                    BigSplitPlacement::Right => {
                        Self::split(BigSplitOrientation::Horizontal, ratio, kept, added)
                    }
                    BigSplitPlacement::Top => {
                        Self::split(BigSplitOrientation::Vertical, ratio, added, kept)
                    }
                    BigSplitPlacement::Bottom => {
                        Self::split(BigSplitOrientation::Vertical, ratio, kept, added)
                    }
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, new_id.clone(), placement, ratio)
                    || second.split_leaf(target, new_id, placement, ratio)
            }
        }
    }

    /// Remove the leaf referencing `target`, collapsing its parent split into
    /// the surviving sibling. Returns `false` (tree unchanged) if `target` is
    /// not found OR is the lone root leaf (a tree always keeps ≥1 leaf — the
    /// caller closes the whole container instead).
    pub fn remove_leaf(&mut self, target: &BigTabId) -> bool {
        let Self::Split { first, second, .. } = self else {
            return false;
        };
        if matches!(first.as_ref(), Self::Leaf(id) if id == target) {
            *self = std::mem::replace(second.as_mut(), Self::Leaf(BigTabId::new()));
            return true;
        }
        if matches!(second.as_ref(), Self::Leaf(id) if id == target) {
            *self = std::mem::replace(first.as_mut(), Self::Leaf(BigTabId::new()));
            return true;
        }
        first.remove_leaf(target) || second.remove_leaf(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::gtk::prelude::Cast;

    /// Minimal stub used to prove the trait is object-safe and the guard works.
    /// `root()` is compiled but never executed (no display in tests).
    struct StubPane;

    impl PaneContent for StubPane {
        fn root(&self) -> gtk::Widget {
            gtk::Label::new(None).upcast()
        }
        fn title(&self) -> String {
            "Stub".to_owned()
        }
        fn connect_title_changed(&self, _on_change: Box<dyn Fn(&str)>) {}
        fn keyboard_need(&self) -> KeyboardNeed {
            KeyboardNeed::Exclusive
        }
    }

    #[test]
    fn pane_content_is_object_safe() {
        // Compiles only if the base trait is object-safe.
        fn assert_object_safe(_: &dyn PaneContent) {}
        let stub = StubPane;
        assert_object_safe(&stub);
        let boxed: Box<dyn PaneContent> = Box::new(StubPane);
        assert_eq!(boxed.title(), "Stub");
        assert_eq!(boxed.keyboard_need(), KeyboardNeed::Exclusive);
    }

    #[test]
    fn pane_guard_owns_and_borrows_content() {
        let guard = PaneGuard::new(Box::new(StubPane));
        assert_eq!(guard.content().title(), "Stub");
    }

    #[test]
    fn set_activity_default_is_noop() {
        // The defaulted method is callable on a trait object without panicking.
        let boxed: Box<dyn PaneContent> = Box::new(StubPane);
        boxed.set_activity(PaneActivity::Occluded);
    }

    #[test]
    fn command_constructors_set_disclosure() {
        assert_eq!(
            PaneCommand::basic("win.save", "Save").disclosure,
            PaneDisclosure::Basic
        );
        assert_eq!(
            PaneCommand::advanced("win.reload-config", "Reload Config").disclosure,
            PaneDisclosure::Advanced
        );
    }

    #[test]
    fn pane_node_collects_leaf_ids_in_order() {
        // terminal | (editor / files) — a tab split three ways.
        let tree = PaneNode::split(
            BigSplitOrientation::Horizontal,
            0.5,
            PaneNode::leaf("terminal"),
            PaneNode::split(
                BigSplitOrientation::Vertical,
                0.6,
                PaneNode::leaf("editor"),
                PaneNode::leaf("files"),
            ),
        );
        let ids: Vec<&str> = tree.leaf_ids().iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["terminal", "editor", "files"]);
    }

    #[test]
    fn split_leaf_right_keeps_target_first() {
        let mut tree = PaneNode::leaf("a");
        assert!(tree.split_leaf(
            &"a".to_owned(),
            "b".to_owned(),
            BigSplitPlacement::Right,
            0.5
        ));
        assert_eq!(
            tree,
            PaneNode::split(
                BigSplitOrientation::Horizontal,
                0.5,
                PaneNode::leaf("a"),
                PaneNode::leaf("b"),
            )
        );
    }

    #[test]
    fn split_leaf_top_puts_new_first_vertical() {
        let mut tree = PaneNode::leaf("a");
        assert!(tree.split_leaf(&"a".to_owned(), "b".to_owned(), BigSplitPlacement::Top, 0.3));
        assert_eq!(
            tree,
            PaneNode::split(
                BigSplitOrientation::Vertical,
                0.3,
                PaneNode::leaf("b"),
                PaneNode::leaf("a"),
            )
        );
    }

    #[test]
    fn split_leaf_targets_nested_leaf() {
        let mut tree = PaneNode::split(
            BigSplitOrientation::Horizontal,
            0.5,
            PaneNode::leaf("a"),
            PaneNode::leaf("b"),
        );
        assert!(tree.split_leaf(
            &"b".to_owned(),
            "c".to_owned(),
            BigSplitPlacement::Bottom,
            0.5
        ));
        let ids: Vec<&str> = tree.leaf_ids().iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_leaf_unknown_target_is_noop() {
        let mut tree = PaneNode::leaf("a");
        assert!(!tree.split_leaf(
            &"zzz".to_owned(),
            "b".to_owned(),
            BigSplitPlacement::Left,
            0.5
        ));
        assert_eq!(tree, PaneNode::leaf("a"));
    }

    #[test]
    fn remove_leaf_collapses_parent_to_sibling() {
        let mut tree = PaneNode::split(
            BigSplitOrientation::Horizontal,
            0.5,
            PaneNode::leaf("a"),
            PaneNode::leaf("b"),
        );
        assert!(tree.remove_leaf(&"a".to_owned()));
        assert_eq!(tree, PaneNode::leaf("b"));
    }

    #[test]
    fn remove_leaf_nested_keeps_other_branch() {
        let mut tree = PaneNode::split(
            BigSplitOrientation::Horizontal,
            0.5,
            PaneNode::leaf("a"),
            PaneNode::split(
                BigSplitOrientation::Vertical,
                0.5,
                PaneNode::leaf("b"),
                PaneNode::leaf("c"),
            ),
        );
        assert!(tree.remove_leaf(&"b".to_owned()));
        assert_eq!(
            tree,
            PaneNode::split(
                BigSplitOrientation::Horizontal,
                0.5,
                PaneNode::leaf("a"),
                PaneNode::leaf("c"),
            )
        );
    }

    #[test]
    fn remove_lone_root_leaf_is_noop() {
        let mut tree = PaneNode::leaf("a");
        assert!(!tree.remove_leaf(&"a".to_owned()));
        assert_eq!(tree, PaneNode::leaf("a"));
    }

    #[test]
    fn remove_unknown_leaf_is_noop() {
        let mut tree = PaneNode::split(
            BigSplitOrientation::Horizontal,
            0.5,
            PaneNode::leaf("a"),
            PaneNode::leaf("b"),
        );
        assert!(!tree.remove_leaf(&"zzz".to_owned()));
        assert_eq!(tree.leaf_ids().len(), 2);
    }

    #[test]
    fn contains_finds_nested_leaf() {
        let tree = PaneNode::split(
            BigSplitOrientation::Vertical,
            0.5,
            PaneNode::leaf("a"),
            PaneNode::leaf("b"),
        );
        assert!(tree.contains(&"b".to_owned()));
        assert!(!tree.contains(&"zzz".to_owned()));
    }
}
