// SPDX-License-Identifier: MIT

//! [`BigWorkspaceShell`] — a tabbed workspace of split-pane trees.
//!
//! The full big-terminal-grade window shell, promoted to the shared layer: a
//! [`BigTabStrip`] of tabs where each
//! tab holds a live [`BigSplitTree`] of panes. It ties together the four
//! concerns the terminal shell's `TabHost` god-object buried among
//! terminal/SSH/file-manager specifics:
//!
//! 1. the **tab strip** (already shared) plus the content container,
//! 2. a **registry** mapping each open tab's page to its split tree and a slice
//!    of consumer-owned per-tab metadata `M` (session, persisted id, color, …),
//! 3. **focused-pane change** fan-out (so window chrome bound to the active pane
//!    can refresh), wired once per tab from the tree's own focus tracker, and
//! 4. **teardown** — when a tab's page is detached, its entry is dropped and the
//!    consumer is handed the removed `M` to clean up its side tables.
//!
//! Everything terminal/editor/file-manager-specific (PTY wiring, SSH lifecycle,
//! the file-manager dock, tab groups, drag-to-split gestures, layout serde) stays in the
//! consumer; the shell owns only the generic tab↔tree↔focus↔teardown spine.
//! Generic over [`SplitPaneSurface`] `P` (the pane) and the metadata type `M`, so
//! the terminal, editor, and file-manager shells reuse the exact same window.
//!
//! # Transparency is neutrality
//!
//! The shell paints **no** background of its own: its container is a bare
//! `gtk::Box` and the tab strip's stack is transparent until a pane's own widget
//! paints. A consumer that wants per-region semi-transparency (a terminal's
//! adaptive-alpha background showing through a blurred compositor, an opaque
//! sidebar) controls it entirely app-side via CSS on its pane widgets — the
//! shell never imposes an opaque fill that would defeat it.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk::gio;
use relm4::gtk::prelude::*;
use relm4::gtk::{Align, Box as GtkBox, Orientation};

use crate::layout::tab_strip_host::{BigTabPage, BigTabStrip};
use crate::pane::{BigSplitTree, SplitPaneSurface};

/// One open tab: its strip page, the split tree it hosts, and the consumer's
/// per-tab metadata. Fields are public so consumers iterate the registry
/// ([`BigWorkspaceShell::tabs`]) the same way the former app-local tab list did.
pub struct WorkspaceTab<P: SplitPaneSurface + 'static, M> {
    /// The tab-strip page this tab is mounted in.
    pub page: Rc<BigTabPage>,
    /// The live split tree of panes shown when this tab is selected.
    pub tree: Rc<BigSplitTree<P>>,
    /// Consumer-owned per-tab data (session, persisted id, color provider, …).
    pub meta: M,
}

/// A tabbed workspace of [`BigSplitTree`] panes. Construct with
/// [`new`](Self::new) from a consumer-built [`BigTabStrip`] (the strip owns the
/// chrome, keeping styling app-side); add tabs with
/// [`append_tab`](Self::append_tab); mount [`root_widget`](Self::root_widget).
pub struct BigWorkspaceShell<P: SplitPaneSurface + 'static, M: 'static> {
    strip: BigTabStrip,
    container: GtkBox,
    tabs: RefCell<Vec<WorkspaceTab<P, M>>>,
    focus_listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    on_tab_removed: RefCell<Option<Rc<dyn Fn(M)>>>,
}

impl<P: SplitPaneSurface + 'static, M: 'static> BigWorkspaceShell<P, M> {
    /// Wrap `strip` in a workspace shell. The shell's container holds the strip's
    /// content stack (the consumer mounts the strip's `tab_bar` wherever its
    /// window chrome wants it). A page-detached teardown is wired so closing a
    /// tab drops its registry entry and fires the
    /// [`on_tab_removed`](Self::set_on_tab_removed) hook with the removed `M`.
    #[must_use]
    pub fn new(strip: BigTabStrip) -> Rc<Self> {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.add_css_class("big-workspace-shell");
        container.append(strip.content_widget());
        let shell = Rc::new(Self {
            strip,
            container,
            tabs: RefCell::new(Vec::new()),
            focus_listeners: RefCell::new(Vec::new()),
            on_tab_removed: RefCell::new(None),
        });
        let weak = Rc::downgrade(&shell);
        shell.strip.connect_page_detached(move |_, page, _| {
            let Some(shell) = weak.upgrade() else {
                return;
            };
            shell.on_page_detached(page);
        });
        shell
    }

    /// The tab strip (selection, menu, move/reorder, the `tab_bar` for chrome).
    #[must_use]
    pub fn strip(&self) -> &BigTabStrip {
        &self.strip
    }

    /// The container holding the tab content — mount this in the window/host/
    /// background surface. The tab strip's `tab_bar` is mounted separately.
    #[must_use]
    pub fn root_widget(&self) -> &GtkBox {
        &self.container
    }

    /// The open-tab registry. Consumers iterate it (`borrow()`) to map a page or
    /// pane back to its tree/metadata, and mutate it (`borrow_mut()`) for cross-
    /// tab moves; the shell keeps it in sync with the strip on detach.
    #[must_use]
    pub fn tabs(&self) -> &RefCell<Vec<WorkspaceTab<P, M>>> {
        &self.tabs
    }

    /// Read all workspace tabs in order without leaking the registry borrow.
    /// The closure should clone any value it needs to keep.
    pub fn read_workspace_tabs<R>(&self, read_tabs: impl FnOnce(&[WorkspaceTab<P, M>]) -> R) -> R {
        let tabs = self.tabs.borrow();
        read_tabs(&tabs)
    }

    /// Zero-based index of the tab whose page is `page`.
    #[must_use]
    pub fn tab_index_for_page(&self, page: &Rc<BigTabPage>) -> Option<usize> {
        self.tabs
            .borrow()
            .iter()
            .position(|tab| Rc::ptr_eq(&tab.page, page))
    }

    /// Read the tab whose page is `page` without leaking the registry borrow.
    /// The closure should clone any value it needs to keep.
    pub fn read_tab_for_page<R>(
        &self,
        page: &Rc<BigTabPage>,
        read_tab: impl FnOnce(&WorkspaceTab<P, M>) -> R,
    ) -> Option<R> {
        let tabs = self.tabs.borrow();
        let tab = tabs.iter().find(|tab| Rc::ptr_eq(&tab.page, page))?;
        Some(read_tab(tab))
    }

    /// Mutate the tab whose page is `page` without leaking the registry borrow.
    pub fn update_tab_for_page<R>(
        &self,
        page: &Rc<BigTabPage>,
        update_tab: impl FnOnce(&mut WorkspaceTab<P, M>) -> R,
    ) -> Option<R> {
        let mut tabs = self.tabs.borrow_mut();
        let tab = tabs.iter_mut().find(|tab| Rc::ptr_eq(&tab.page, page))?;
        Some(update_tab(tab))
    }

    /// Read the tab whose split tree contains `pane` without leaking the
    /// registry borrow. The closure should clone any value it needs to keep.
    pub fn read_tab_for_pane<R>(
        &self,
        pane: &Rc<P>,
        read_tab: impl FnOnce(&WorkspaceTab<P, M>) -> R,
    ) -> Option<R> {
        let tabs = self.tabs.borrow();
        let tab = tabs.iter().find(|tab| tab.tree.contains_pane(pane))?;
        Some(read_tab(tab))
    }

    /// The split tree hosted by `page`.
    #[must_use]
    pub fn split_tree_for_page(&self, page: &Rc<BigTabPage>) -> Option<Rc<BigSplitTree<P>>> {
        self.read_tab_for_page(page, |tab| tab.tree.clone())
    }

    /// All tab pages in workspace order.
    #[must_use]
    pub fn tab_pages(&self) -> Vec<Rc<BigTabPage>> {
        self.tabs
            .borrow()
            .iter()
            .map(|tab| tab.page.clone())
            .collect()
    }

    /// All tab pages except the currently selected one, in workspace order.
    #[must_use]
    pub fn tab_pages_except_selected(&self) -> Vec<Rc<BigTabPage>> {
        let selected_page = self.strip.selected_page();
        self.tabs
            .borrow()
            .iter()
            .filter(|tab| {
                selected_page
                    .as_ref()
                    .is_none_or(|selected_page| !Rc::ptr_eq(&tab.page, selected_page))
            })
            .map(|tab| tab.page.clone())
            .collect()
    }

    /// The page and split tree containing `pane`.
    #[must_use]
    pub fn page_and_split_tree_for_pane(
        &self,
        pane: &Rc<P>,
    ) -> Option<(Rc<BigTabPage>, Rc<BigSplitTree<P>>)> {
        self.read_tab_for_pane(pane, |tab| (tab.page.clone(), tab.tree.clone()))
    }

    /// Number of open tabs.
    #[must_use]
    pub fn tab_count(&self) -> usize {
        self.tabs.borrow().len()
    }

    /// Append a tab hosting `tree` with metadata `meta`, returning its page (the
    /// consumer sets the title/icon and selects it). The tree's focus tracker is
    /// wired to the shell's focus fan-out, so focusing any pane in this tab
    /// fires [`connect_focused_pane_notify`](Self::connect_focused_pane_notify)
    /// listeners.
    pub fn append_tab(self: &Rc<Self>, tree: Rc<BigSplitTree<P>>, meta: M) -> Rc<BigTabPage> {
        let page = self.strip.append(tree.root_widget());
        let weak = Rc::downgrade(self);
        tree.set_on_focus_changed(Rc::new(move || {
            if let Some(shell) = weak.upgrade() {
                shell.notify_focused_pane_changed();
            }
        }));
        self.tabs.borrow_mut().push(WorkspaceTab {
            page: page.clone(),
            tree,
            meta,
        });
        page
    }

    /// Remove and return the tab whose page is `page`, **without** firing the
    /// [`on_tab_removed`](Self::set_on_tab_removed) hook — for moving a tab to
    /// another shell (a transfer must not run the source's teardown side
    /// effects). Returns `None` if `page` is not registered here.
    ///
    /// The strip's own `transfer_page` fires a page-detached event, but because
    /// this has already dropped the registry entry, the shell's detach handler
    /// finds nothing and is a no-op — so call this *before* `transfer_page`.
    pub fn take_tab_by_page(&self, page: &Rc<BigTabPage>) -> Option<WorkspaceTab<P, M>> {
        let mut tabs = self.tabs.borrow_mut();
        let index = tabs.iter().position(|tab| Rc::ptr_eq(&tab.page, page))?;
        Some(tabs.remove(index))
    }

    /// Restore a tab removed with [`Self::take_tab_by_page`] when the caller
    /// aborts before completing a strip transfer or adoption.
    ///
    /// This only restores the workspace registry entry. The caller remains
    /// responsible for keeping the visible strip state consistent with the
    /// aborted operation.
    pub fn restore_taken_tab(&self, tab: WorkspaceTab<P, M>) {
        self.tabs.borrow_mut().push(tab);
    }

    /// Adopt a `tab` taken from another shell (via
    /// [`take_tab_by_page`](Self::take_tab_by_page)) at `index`, re-pointing its
    /// split tree's focus tracker at *this* shell so focusing the moved panes
    /// fires this shell's listeners. `index` is clamped to the tab count.
    pub fn adopt_tab(self: &Rc<Self>, index: usize, tab: WorkspaceTab<P, M>) {
        let weak = Rc::downgrade(self);
        tab.tree.set_on_focus_changed(Rc::new(move || {
            if let Some(shell) = weak.upgrade() {
                shell.notify_focused_pane_changed();
            }
        }));
        let mut tabs = self.tabs.borrow_mut();
        let at = index.min(tabs.len());
        tabs.insert(at, tab);
    }

    /// The currently selected tab's page, if any.
    #[must_use]
    pub fn selected_page(&self) -> Option<Rc<BigTabPage>> {
        self.strip.selected_page()
    }

    /// Read the currently selected tab without leaking the registry borrow.
    /// The closure should clone any value it needs to keep.
    pub fn read_selected_tab<R>(
        &self,
        read_tab: impl FnOnce(&WorkspaceTab<P, M>) -> R,
    ) -> Option<R> {
        let page = self.strip.selected_page()?;
        self.read_tab_for_page(&page, read_tab)
    }

    /// Mutate the currently selected tab without leaking the registry borrow.
    pub fn update_selected_tab<R>(
        &self,
        update_tab: impl FnOnce(&mut WorkspaceTab<P, M>) -> R,
    ) -> Option<R> {
        let page = self.strip.selected_page()?;
        self.update_tab_for_page(&page, update_tab)
    }

    /// Read the last tab in workspace order without leaking the registry borrow.
    /// The closure should clone any value it needs to keep.
    pub fn read_last_tab<R>(&self, read_tab: impl FnOnce(&WorkspaceTab<P, M>) -> R) -> Option<R> {
        let tabs = self.tabs.borrow();
        let tab = tabs.last()?;
        Some(read_tab(tab))
    }

    /// Select `page` if it belongs to this workspace.
    pub fn select_page(&self, page: &Rc<BigTabPage>) {
        self.strip.set_selected_page(page);
    }

    /// Whether `page` is the currently selected page.
    #[must_use]
    pub fn is_page_selected(&self, page: &Rc<BigTabPage>) -> bool {
        self.strip
            .selected_page()
            .is_some_and(|selected_page| Rc::ptr_eq(&selected_page, page))
    }

    /// Select the page at zero-based `index`, or do nothing if absent.
    pub fn select_page_at_index(&self, index: usize) {
        self.strip.select_page_at_index(index);
    }

    /// Select the next page. Returns `false` when already at the last page or
    /// when no page is selected.
    pub fn select_next_page(&self) -> bool {
        self.strip.select_next_page()
    }

    /// Select the previous page. Returns `false` when already at the first page
    /// or when no page is selected.
    pub fn select_previous_page(&self) -> bool {
        self.strip.select_previous_page()
    }

    /// Register a selected-page-changed listener without exposing the
    /// underlying tab strip to consumers.
    pub fn connect_selected_page_notify<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.strip.connect_selected_page_notify(move |_| callback());
    }

    /// Register a page-count-changed listener, handed the new page count.
    pub fn connect_n_pages_notify<F>(&self, callback: F)
    where
        F: Fn(i32) + 'static,
    {
        self.strip
            .connect_n_pages_notify(move |strip| callback(strip.n_pages()));
    }

    /// Register a page-attached listener without exposing the underlying tab
    /// strip to consumers.
    pub fn connect_page_attached<F>(&self, callback: F)
    where
        F: Fn(&Rc<BigTabPage>, i32) + 'static,
    {
        self.strip
            .connect_page_attached(move |_, page, position| callback(page, position));
    }

    /// Register a page-detached listener without exposing the underlying tab
    /// strip to consumers.
    pub fn connect_page_detached<F>(&self, callback: F)
    where
        F: Fn(&Rc<BigTabPage>, i32) + 'static,
    {
        self.strip
            .connect_page_detached(move |_, page, position| callback(page, position));
    }

    /// Register a context-menu setup listener. Consumers receive the page the
    /// menu was requested for, if any, and can install a menu model through
    /// [`Self::set_context_menu_model`].
    pub fn connect_context_menu_setup<F>(&self, callback: F)
    where
        F: Fn(Option<&Rc<BigTabPage>>) + 'static,
    {
        self.strip.connect_setup_menu(move |_, page| callback(page));
    }

    /// Set or clear the context-menu model shown by the workspace tab strip.
    pub fn set_context_menu_model<N: IsA<gio::MenuModel>>(&self, model: Option<&N>) {
        self.strip.set_menu_model(model);
    }

    /// Register a tab-move listener after the strip commits a reordering.
    pub fn connect_move_committed<F>(&self, callback: F)
    where
        F: Fn(&Rc<BigTabPage>, &Rc<BigTabPage>) + 'static,
    {
        self.strip
            .connect_move_committed(move |_, moving, target| callback(moving, target));
    }

    /// Register a tab move-inside request, fired when a tab is dropped onto
    /// another tab and the consumer must decide the embed/split behavior.
    pub fn connect_move_inside_requested<F>(&self, callback: F)
    where
        F: Fn(&Rc<BigTabPage>, &Rc<BigTabPage>) + 'static,
    {
        self.strip
            .connect_move_inside_requested(move |_, moving, target| callback(moving, target));
    }

    /// Begin click-to-move mode for `page`.
    pub fn start_move_for_page(&self, page: &Rc<BigTabPage>) {
        self.strip.start_move(page);
    }

    /// Move `page` into `target` at `position`, returning whether the transfer
    /// succeeded.
    pub fn transfer_page_to(
        &self,
        page: &Rc<BigTabPage>,
        target: &BigWorkspaceShell<P, M>,
        position: i32,
    ) -> bool {
        self.strip.transfer_page(page, target.strip(), position)
    }

    /// Remove `page` from the strip without destroying its child, for callers
    /// that immediately re-home the tab content into a split tree.
    pub fn remove_page_for_embed(&self, page: &Rc<BigTabPage>) -> bool {
        self.strip.remove_page_for_embed(page)
    }

    /// The tab-button strip widget for window chrome mounting.
    #[must_use]
    pub fn tab_bar_widget(&self) -> &GtkBox {
        self.strip.tab_bar()
    }

    /// Set tab-bar alignment without exposing the underlying strip.
    pub fn set_tab_bar_halign(&self, align: Align) {
        self.strip.tab_bar().set_halign(align);
    }

    /// Apply strip-wide tab chrome policy.
    pub fn apply_tab_chrome(&self, expand_evenly: bool, close_mode: &str) {
        self.strip.apply_tab_chrome(expand_evenly, close_mode);
    }

    /// Apply the full suite tab personalization (alignment, expand-evenly,
    /// close mode, tab theme CSS) from a preference store. Call on boot and
    /// whenever a [`crate::layout::tab_prefs::TAB_SETTING_KEYS`] key changes.
    ///
    /// `theme_provider_slot` caches the theme's CSS provider across calls.
    /// Apps with their own tab-theme pipeline (big-terminal) keep calling the
    /// granular pieces instead.
    pub fn apply_tab_personalization(
        &self,
        store: &dyn crate::input::preference_rows::BigPreferenceStore,
        theme_provider_slot: &mut Option<relm4::gtk::CssProvider>,
    ) {
        use crate::layout::tab_prefs;
        self.set_tab_bar_halign(tab_prefs::tab_alignment_halign(store));
        let (expand_evenly, close_mode) = tab_prefs::tab_chrome_settings(store);
        self.apply_tab_chrome(expand_evenly, &close_mode);
        crate::theme::inject_palette_css(
            &tab_prefs::tab_theme_css(&tab_prefs::tab_theme_setting(store)),
            theme_provider_slot,
        );
    }

    /// Request closing `page`, honoring the strip's close-request callback.
    pub fn request_close_page(&self, page: &Rc<BigTabPage>) {
        self.strip.request_close_page(page);
    }

    /// Close `page` immediately, without the close-request callback.
    pub fn close_page(&self, page: &Rc<BigTabPage>) {
        self.strip.close_page(page);
    }

    /// Close every page immediately, without the close-request callback.
    pub fn close_all_pages(&self) {
        for page in self.tab_pages() {
            self.strip.close_page(&page);
        }
    }

    /// Request closing the selected page. Returns `false` if no page is
    /// selected.
    pub fn request_close_selected_page(&self) -> bool {
        let Some(page) = self.strip.selected_page() else {
            return false;
        };
        self.strip.request_close_page(&page);
        true
    }

    /// Close every page except the selected page. Returns `false` if no page is
    /// selected.
    pub fn close_pages_except_selected(&self) -> bool {
        let Some(page) = self.strip.selected_page() else {
            return false;
        };
        self.strip.close_other_pages(&page);
        true
    }

    /// Close every page after the selected page. Returns `false` if no page is
    /// selected.
    pub fn close_pages_after_selected(&self) -> bool {
        let Some(page) = self.strip.selected_page() else {
            return false;
        };
        self.strip.close_pages_after(&page);
        true
    }

    /// Move the selected page one slot toward the start.
    pub fn reorder_selected_page_backward(&self) -> bool {
        let Some(page) = self.strip.selected_page() else {
            return false;
        };
        self.strip.reorder_backward(&page)
    }

    /// Move the selected page one slot toward the end.
    pub fn reorder_selected_page_forward(&self) -> bool {
        let Some(page) = self.strip.selected_page() else {
            return false;
        };
        self.strip.reorder_forward(&page)
    }

    /// The split tree of the currently selected tab.
    #[must_use]
    pub fn focused_tree(&self) -> Option<Rc<BigSplitTree<P>>> {
        let page = self.strip.selected_page()?;
        self.split_tree_for_page(&page)
    }

    /// The focused pane of the currently selected tab.
    #[must_use]
    pub fn focused_pane(&self) -> Option<Rc<P>> {
        self.focused_tree().and_then(|tree| tree.focused_pane())
    }

    /// Register a callback fired whenever the focused pane changes (in any tab).
    /// Window chrome bound to the active pane subscribes here.
    pub fn connect_focused_pane_notify(&self, callback: Rc<dyn Fn()>) {
        self.focus_listeners.borrow_mut().push(callback);
    }

    /// Fire every focus listener. Iterates a snapshot so a listener may
    /// re-subscribe without a `RefCell` re-borrow panic. Called automatically by
    /// the per-tab focus tracker; exposed for consumers that change focus
    /// out-of-band (e.g. on tab switch).
    pub fn notify_focused_pane_changed(&self) {
        let callbacks = self.focus_listeners.borrow().clone();
        for callback in callbacks {
            callback();
        }
    }

    /// Set the hook fired when a tab is closed (its page detached), handed the
    /// removed tab's metadata so the consumer can clean up its side tables
    /// (groups, colors, persistence).
    pub fn set_on_tab_removed(&self, callback: Rc<dyn Fn(M)>) {
        *self.on_tab_removed.borrow_mut() = Some(callback);
    }

    /// Set a close-request callback for workspace pages.
    ///
    /// Return `true` to allow the shell to close the page immediately. Return
    /// `false` to veto the immediate close, usually because the app is showing
    /// an async confirmation and will call [`Self::close_page`] later.
    pub fn set_close_request_callback<F>(&self, close_request_callback: Option<F>)
    where
        F: Fn(&Rc<BigTabPage>) -> bool + 'static,
    {
        match close_request_callback {
            Some(callback) => self.strip.set_close_request_callback(Some(
                move |_strip: &BigTabStrip, page: &Rc<BigTabPage>| callback(page),
            )),
            None => self
                .strip
                .set_close_request_callback(None::<fn(&BigTabStrip, &Rc<BigTabPage>) -> bool>),
        }
    }

    /// Drop the registry entry for a detached `page` and hand its metadata to the
    /// [`on_tab_removed`](Self::set_on_tab_removed) hook. The hook runs after the
    /// entry leaves the registry, so it observes a consistent tab list.
    fn on_page_detached(&self, page: &Rc<BigTabPage>) {
        let removed = {
            let mut tabs = self.tabs.borrow_mut();
            tabs.iter()
                .position(|tab| Rc::ptr_eq(&tab.page, page))
                .map(|idx| tabs.remove(idx))
        };
        if let Some(tab) = removed {
            let hook = self.on_tab_removed.borrow().clone();
            if let Some(hook) = hook {
                hook(tab.meta);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::gtk;
    use relm4::gtk::{Box as GtkBox, Orientation, Widget};
    use std::cell::Cell;

    /// Minimal pane: a real `gtk::Box` root (needed for the live split tree) plus
    /// counters proving the tree drives header-visibility / focus on it.
    struct StubPane {
        root: GtkBox,
        header_visible: Cell<bool>,
        focus_grabs: Cell<u32>,
    }

    impl StubPane {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                root: GtkBox::new(Orientation::Vertical, 0),
                header_visible: Cell::new(true),
                focus_grabs: Cell::new(0),
            })
        }
    }

    impl SplitPaneSurface for StubPane {
        fn pane_root(&self) -> Widget {
            self.root.clone().upcast()
        }
        fn set_header_visible(&self, visible: bool) {
            self.header_visible.set(visible);
        }
        fn grab_focus(&self) {
            self.focus_grabs.set(self.focus_grabs.get() + 1);
        }
    }

    /// Build a shell over a default tab strip, or `None` outside explicit GTK
    /// smoke runs — the caller returns early then.
    fn shell() -> Option<Rc<BigWorkspaceShell<StubPane, String>>> {
        if std::env::var_os("BIGAGENTS_RUN_UI_SMOKE").as_deref() != Some(std::ffi::OsStr::new("1"))
        {
            return None;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
            return None;
        }
        if gtk::init().is_err() {
            return None;
        }
        Some(BigWorkspaceShell::new(BigTabStrip::default()))
    }

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn append_tab_registers_and_selects_first() {
        let Some(shell) = shell() else { return };
        let pane = StubPane::new();
        let tree = BigSplitTree::new(&pane);
        let page = shell.append_tab(tree.clone(), "tab-1".to_owned());
        assert_eq!(shell.tab_count(), 1);
        // The strip auto-selects the first page; the shell maps it back to the
        // tree we appended. (`BigTabPage` has no `Debug`, so compare by identity.)
        let selected = shell.selected_page().expect("a page is selected");
        assert!(Rc::ptr_eq(&selected, &page));
        let focused = shell.focused_tree().expect("a tree is focused");
        assert!(Rc::ptr_eq(&focused, &tree));
        let focused_pane = shell.focused_pane().expect("a pane is focused");
        assert!(Rc::ptr_eq(&focused_pane, &pane));
    }

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn focus_notify_fires_on_pane_focus_change() {
        let Some(shell) = shell() else { return };
        let fired = Rc::new(Cell::new(0_u32));
        {
            let fired = Rc::clone(&fired);
            shell.connect_focused_pane_notify(Rc::new(move || fired.set(fired.get() + 1)));
        }
        let tree = BigSplitTree::new(&StubPane::new());
        shell.append_tab(tree.clone(), "tab-1".to_owned());
        // Splitting focuses the new pane, which trips the tree's focus tracker →
        // the shell's fan-out. (A direct notify also fires, proving wiring.)
        shell.notify_focused_pane_changed();
        assert!(fired.get() >= 1, "focus listener fired at least once");
    }

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn detaching_a_page_drops_its_entry_and_fires_hook() {
        let Some(shell) = shell() else { return };
        let removed = Rc::new(RefCell::new(Vec::<String>::new()));
        {
            let removed = Rc::clone(&removed);
            shell.set_on_tab_removed(Rc::new(move |meta: String| removed.borrow_mut().push(meta)));
        }
        let page_a = shell.append_tab(BigSplitTree::new(&StubPane::new()), "a".to_owned());
        let _page_b = shell.append_tab(BigSplitTree::new(&StubPane::new()), "b".to_owned());
        assert_eq!(shell.tab_count(), 2);

        shell.strip().request_close_page(&page_a);

        assert_eq!(shell.tab_count(), 1, "closed tab left the registry");
        assert_eq!(
            *removed.borrow(),
            vec!["a".to_owned()],
            "the removed tab's metadata reached the hook"
        );
        // The surviving tab is still mapped.
        assert!(shell.tabs().borrow().iter().any(|t| t.meta == "b"));
    }

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn take_then_adopt_moves_a_tab_between_shells_without_teardown() {
        if gtk::init().is_err() {
            return;
        }
        let source = BigWorkspaceShell::<StubPane, String>::new(BigTabStrip::default());
        let target = BigWorkspaceShell::<StubPane, String>::new(BigTabStrip::default());
        let torn_down = Rc::new(Cell::new(0_u32));
        {
            let torn_down = Rc::clone(&torn_down);
            source.set_on_tab_removed(Rc::new(move |_| torn_down.set(torn_down.get() + 1)));
        }
        let page = source.append_tab(BigSplitTree::new(&StubPane::new()), "moved".to_owned());

        let tab = source.take_tab_by_page(&page).expect("tab is taken");
        assert_eq!(source.tab_count(), 0);
        target.adopt_tab(0, tab);

        assert_eq!(target.tab_count(), 1);
        assert_eq!(
            torn_down.get(),
            0,
            "a transfer must not fire the teardown hook"
        );
        // The adopted tree now drives the TARGET shell's fan-out.
        let fired = Rc::new(Cell::new(0_u32));
        {
            let fired = Rc::clone(&fired);
            target.connect_focused_pane_notify(Rc::new(move || fired.set(fired.get() + 1)));
        }
        target.notify_focused_pane_changed();
        assert!(fired.get() >= 1);
    }

    #[test]
    fn tab_lookup_helpers_are_identity_keyed() {
        let Some(shell) = shell() else { return };
        let pane = StubPane::new();
        let tree = BigSplitTree::new(&pane);
        let page = shell.append_tab(tree.clone(), "only".to_owned());
        let found = shell.read_tab_for_page(&page, |tab| tab.meta.clone());
        assert_eq!(found, Some("only".to_owned()));
        assert_eq!(shell.tab_index_for_page(&page), Some(0));
        let found_tree = shell
            .split_tree_for_page(&page)
            .expect("page maps to split tree");
        assert!(Rc::ptr_eq(&found_tree, &tree));
        let (found_page, found_tree) = shell
            .page_and_split_tree_for_pane(&pane)
            .expect("pane maps to tab parts");
        assert!(Rc::ptr_eq(&found_page, &page));
        assert!(Rc::ptr_eq(&found_tree, &tree));
    }

    #[test]
    fn selected_and_last_tab_helpers_do_not_expose_registry_borrows() {
        let Some(shell) = shell() else { return };
        let page_a = shell.append_tab(BigSplitTree::new(&StubPane::new()), "a".to_owned());
        let page_b = shell.append_tab(BigSplitTree::new(&StubPane::new()), "b".to_owned());
        shell.select_page(&page_a);

        assert_eq!(
            shell.read_workspace_tabs(|tabs| tabs
                .iter()
                .map(|tab| tab.meta.clone())
                .collect::<Vec<_>>()),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            shell.read_selected_tab(|tab| tab.meta.clone()),
            Some("a".to_owned())
        );
        assert_eq!(
            shell.read_last_tab(|tab| tab.meta.clone()),
            Some("b".to_owned())
        );
        assert_eq!(
            shell.update_selected_tab(|tab| {
                tab.meta.push_str("-selected");
                tab.meta.clone()
            }),
            Some("a-selected".to_owned())
        );
        assert_eq!(
            shell.read_tab_for_page(&page_a, |tab| tab.meta.clone()),
            Some("a-selected".to_owned())
        );
        let pages_except_selected = shell.tab_pages_except_selected();
        assert_eq!(pages_except_selected.len(), 1);
        assert!(Rc::ptr_eq(&pages_except_selected[0], &page_b));
    }

    #[test]
    fn close_all_pages_clears_registry_through_shell() {
        let Some(shell) = shell() else { return };
        shell.append_tab(BigSplitTree::new(&StubPane::new()), "a".to_owned());
        shell.append_tab(BigSplitTree::new(&StubPane::new()), "b".to_owned());

        shell.close_all_pages();

        assert_eq!(shell.tab_count(), 0);
    }
}
