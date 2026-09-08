// SPDX-License-Identifier: MIT

//! Async surface orchestrator.
//!
//! [`BigAsyncSurface`] is a `gtk::Stack` with four named pages —
//! `skeleton`, `loading`, `content`, `error` — that swaps the visible child on
//! [`set_state`](BigAsyncSurfaceInput::SetState). It composes the display-free
//! specs from the sibling `state` modules ([`BigSkeletonSpec`],
//! [`BigLoadingSpinnerSpec`], [`BigErrorStateSpec`]) into widgets so every slow
//! surface is born with an explicit, resolved state instead of guessing
//! `Ready`.
//!
//! Product-UX contract (`never guess Ready`): the surface only shows its
//! `content` page when the caller sends [`BigAsyncState::Content`] — a resolved
//! state. Until then it shows a skeleton, a spinner, or a typed error.
//!
//! Two behaviours make it more than a raw `Stack`:
//!
//! - **Anti-flash loading.** Entering [`BigAsyncState::Loading`] does not show
//!   the spinner immediately; it defers by the spec's
//!   [`appearance_delay_ms`](BigLoadingSpinnerSpec::appearance_delay_ms) via
//!   `glib::timeout`, so operations that finish within the delay never flash a
//!   spinner. A newer state supersedes a pending deferral.
//! - **Typed error page.** [`BigAsyncState::Error`] renders an
//!   `adw::StatusPage` with a retry button (labelled from
//!   [`BigErrorStateSpec::retry_label`]) that emits
//!   [`BigAsyncSurfaceOutput::RetryRequested`], plus a collapsed disclosure for
//!   [`BigErrorStateSpec::details`].

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use relm4::gtk;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::state::error::BigErrorStateSpec;
use crate::state::loading::{BigLoadingSize, BigLoadingSpinnerSpec};
use crate::state::skeleton::BigSkeletonSpec;

/// Stack page name for the skeleton placeholder state.
pub const PAGE_SKELETON: &str = "skeleton";
/// Stack page name for the (anti-flash) loading state.
pub const PAGE_LOADING: &str = "loading";
/// Stack page name for the resolved content state.
pub const PAGE_CONTENT: &str = "content";
/// Stack page name for the typed error state.
pub const PAGE_ERROR: &str = "error";

/// The resolved state of an [`BigAsyncSurface`], generic over the content
/// widget `W` shown once data resolves.
///
/// A surface is always in exactly one of these states; there is no implicit
/// "ready before the probe finished" state — that is the whole point.
#[derive(Debug, Clone)]
pub enum BigAsyncState<W> {
    /// Placeholder cards while the first data loads.
    Skeleton(BigSkeletonSpec),
    /// Work in progress; the spinner appears only after the spec's anti-flash
    /// delay.
    Loading(BigLoadingSpinnerSpec),
    /// Resolved content widget to display.
    Content(W),
    /// A typed failure with cause and (optionally) retry.
    Error(BigErrorStateSpec),
}

/// The stack page a state resolves to, ignoring the loading anti-flash timing
/// (that deferral is applied by the widget, not this mapping).
#[must_use]
pub fn page_name_for_state<W>(state: &BigAsyncState<W>) -> &'static str {
    match state {
        BigAsyncState::Skeleton(_) => PAGE_SKELETON,
        BigAsyncState::Loading(_) => PAGE_LOADING,
        BigAsyncState::Content(_) => PAGE_CONTENT,
        BigAsyncState::Error(_) => PAGE_ERROR,
    }
}

/// Input messages accepted by [`BigAsyncSurface`].
#[derive(Debug, Clone)]
pub enum BigAsyncSurfaceInput<W> {
    /// Replace the surface state; swaps the visible stack page.
    SetState(BigAsyncState<W>),
}

/// Output messages emitted by [`BigAsyncSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigAsyncSurfaceOutput {
    /// The retry button on the error page was activated. The host should
    /// re-run the failed operation (e.g. re-send its `Loading` state).
    RetryRequested,
}

/// Widget handles for the four stack pages. Held in the component's `Widgets`
/// so [`apply_state`] can swap and re-render them.
pub struct BigAsyncSurfaceWidgets {
    /// The root stack whose visible child is the current state's page.
    pub stack: gtk::Stack,
    /// Container re-rendered with skeleton placeholder rows.
    skeleton_page: gtk::Box,
    /// Spinner shown after the anti-flash delay.
    loading_spinner: gtk::Spinner,
    /// Optional label under the spinner.
    loading_label: gtk::Label,
    /// Type-erased holder for the resolved content widget.
    content_bin: adw::Bin,
    /// Error status page (icon/title/body).
    error_status: adw::StatusPage,
    /// Retry button on the error page; label set from the spec.
    pub error_retry_button: gtk::Button,
    /// Collapsed disclosure that reveals the error details.
    error_details_expander: gtk::Expander,
    /// The (copyable) details text inside the disclosure.
    error_details_label: gtk::Label,
}

/// Pixel size of the loading spinner for a [`BigLoadingSize`].
const fn spinner_px(size: BigLoadingSize) -> i32 {
    match size {
        BigLoadingSize::Inline => 16,
        BigLoadingSize::Embedded => 24,
        BigLoadingSize::Primary => 48,
    }
}

/// Add the four pages to `stack`, wire the retry button to `on_retry`, and
/// return handles to them. Shared by [`build_async_surface_widgets`] and the
/// [`BigAsyncSurface`] component's `init`.
fn populate_pages(stack: &gtk::Stack, on_retry: impl Fn() + 'static) -> BigAsyncSurfaceWidgets {
    // Skeleton page: a vertical box of placeholder rows, re-filled per spec.
    let skeleton_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    stack.add_named(&skeleton_page, Some(PAGE_SKELETON));

    // Loading page: centered spinner + optional label.
    let loading_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .build();
    let loading_spinner = gtk::Spinner::new();
    loading_spinner.set_spinning(true);
    let loading_label = gtk::Label::builder()
        .css_classes(["dim-label"])
        .visible(false)
        .build();
    loading_page.append(&loading_spinner);
    loading_page.append(&loading_label);
    stack.add_named(&loading_page, Some(PAGE_LOADING));

    // Content page: a type-erased bin the resolved widget is set into.
    let content_bin = adw::Bin::new();
    stack.add_named(&content_bin, Some(PAGE_CONTENT));

    // Error page: adw::StatusPage with a retry button + a details disclosure.
    let error_status = adw::StatusPage::new();
    let error_actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .build();

    let error_retry_button = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    error_retry_button.connect_clicked(move |_| on_retry());
    error_actions.append(&error_retry_button);

    let error_details_label = gtk::Label::builder()
        .selectable(true)
        .wrap(true)
        .xalign(0.0)
        .css_classes(["monospace", "dim-label"])
        .build();
    let error_details_expander = gtk::Expander::builder()
        .label(crate::i18n::t("Details"))
        .visible(false)
        .build();
    error_details_expander.set_child(Some(&error_details_label));
    error_actions.append(&error_details_expander);

    error_status.set_child(Some(&error_actions));
    stack.add_named(&error_status, Some(PAGE_ERROR));

    BigAsyncSurfaceWidgets {
        stack: stack.clone(),
        skeleton_page,
        loading_spinner,
        loading_label,
        content_bin,
        error_status,
        error_retry_button,
        error_details_expander,
        error_details_label,
    }
}

/// Build a self-contained async-surface widget tree (a fresh `gtk::Stack` with
/// the four pages) whose retry button calls `on_retry`. Raw-GTK call sites and
/// tests use this; Relm4 hosts should prefer the [`BigAsyncSurface`] component.
#[must_use]
pub fn build_async_surface_widgets(on_retry: impl Fn() + 'static) -> BigAsyncSurfaceWidgets {
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(120)
        .build();
    populate_pages(&stack, on_retry)
}

/// Render `state` into `widgets` and switch the visible page.
///
/// All states switch immediately **except** [`BigAsyncState::Loading`] with a
/// non-zero delay: it keeps the current page and defers the spinner via a
/// `glib::timeout`, cancelled when `generation` advances (a newer state). Pass
/// a per-surface monotonic `generation` that the caller bumps on every state
/// change.
pub fn apply_state<W: IsA<gtk::Widget>>(
    widgets: &BigAsyncSurfaceWidgets,
    state: &BigAsyncState<W>,
    generation: &Rc<Cell<u64>>,
) {
    match state {
        BigAsyncState::Skeleton(spec) => {
            render_skeleton(&widgets.skeleton_page, spec);
            widgets.stack.set_visible_child_name(PAGE_SKELETON);
        }
        BigAsyncState::Loading(spec) => {
            widgets
                .loading_spinner
                .set_size_request(spinner_px(spec.size), spinner_px(spec.size));
            widgets.loading_label.set_label(&spec.label);
            widgets.loading_label.set_visible(!spec.label.is_empty());
            if spec.appearance_delay_ms == 0 {
                widgets.stack.set_visible_child_name(PAGE_LOADING);
            } else {
                // Anti-flash: keep the current page; reveal the spinner only if
                // still on this generation once the delay elapses.
                let generation_at_schedule = generation.get();
                let generation = Rc::clone(generation);
                let stack = widgets.stack.clone();
                gtk::glib::timeout_add_local_once(
                    Duration::from_millis(u64::from(spec.appearance_delay_ms)),
                    move || {
                        if generation.get() == generation_at_schedule {
                            stack.set_visible_child_name(PAGE_LOADING);
                        }
                    },
                );
            }
        }
        BigAsyncState::Content(widget) => {
            widgets.content_bin.set_child(Some(widget));
            widgets.stack.set_visible_child_name(PAGE_CONTENT);
        }
        BigAsyncState::Error(spec) => {
            render_error(widgets, spec);
            widgets.stack.set_visible_child_name(PAGE_ERROR);
        }
    }
}

/// Re-fill the skeleton container with `spec.rows` placeholder rows.
fn render_skeleton(page: &gtk::Box, spec: &BigSkeletonSpec) {
    while let Some(child) = page.first_child() {
        page.remove(&child);
    }
    let row_height = 32;
    for _ in 0..spec.rows {
        let placeholder = gtk::Box::builder()
            .height_request(row_height)
            .hexpand(true)
            .css_classes(["card", "dim-label"])
            .build();
        page.append(&placeholder);
    }
}

/// Apply an error spec onto the error page: status text, retry button
/// visibility/label, and the details disclosure.
fn render_error(widgets: &BigAsyncSurfaceWidgets, spec: &BigErrorStateSpec) {
    widgets.error_status.set_icon_name(Some(&spec.icon_name));
    widgets.error_status.set_title(&spec.title);
    widgets.error_status.set_description(Some(&spec.body));

    match &spec.retry_label {
        Some(label) if spec.offers_retry() => {
            widgets.error_retry_button.set_label(label);
            widgets.error_retry_button.set_visible(true);
        }
        _ => widgets.error_retry_button.set_visible(false),
    }

    match &spec.details {
        Some(details) => {
            widgets.error_details_label.set_label(details);
            widgets.error_details_expander.set_expanded(false);
            widgets.error_details_expander.set_visible(true);
        }
        None => widgets.error_details_expander.set_visible(false),
    }
}

/// Relm4 orchestrator for a slow surface — see the [module docs](self).
///
/// `W` is the resolved content widget type.
///
/// # Examples
///
/// ```ignore
/// use big_relm4_components::state::async_surface::{
///     BigAsyncState, BigAsyncSurface, BigAsyncSurfaceInput, BigAsyncSurfaceOutput,
/// };
/// use big_relm4_components::state::skeleton::BigSkeletonSpec;
/// use relm4::{Component, ComponentController};
///
/// // Start on a skeleton; resolve later with SetState(Content(..)) / Error(..).
/// let surface = BigAsyncSurface::<gtk::Label>::builder()
///     .launch(BigAsyncState::Skeleton(BigSkeletonSpec::list_rows()))
///     .forward(sender.input_sender(), |out| match out {
///         BigAsyncSurfaceOutput::RetryRequested => Msg::Reload,
///     });
/// ```
pub struct BigAsyncSurface<W> {
    state: BigAsyncState<W>,
    /// Monotonic guard bumped on every state change so a stale loading
    /// deferral no-ops.
    generation: Rc<Cell<u64>>,
}

impl<W> Component for BigAsyncSurface<W>
where
    W: IsA<gtk::Widget> + std::fmt::Debug + Clone + 'static,
{
    type Init = BigAsyncState<W>;
    type Input = BigAsyncSurfaceInput<W>;
    type Output = BigAsyncSurfaceOutput;
    type CommandOutput = ();
    type Root = gtk::Stack;
    type Widgets = BigAsyncSurfaceWidgets;

    fn init_root() -> Self::Root {
        gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build()
    }

    fn init(
        state: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let output = sender.output_sender().clone();
        let widgets = populate_pages(&root, move || {
            // output channel, not a widget — no ref cycle.
            let _ = output.send(BigAsyncSurfaceOutput::RetryRequested);
        });
        let generation = Rc::new(Cell::new(0));
        apply_state(&widgets, &state, &generation);
        ComponentParts {
            model: Self { state, generation },
            widgets,
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            BigAsyncSurfaceInput::SetState(state) => self.state = state,
        }
        // Invalidate any pending loading deferral scheduled for the old state.
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        apply_state(widgets, &self.state, &self.generation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn page_name_maps_each_non_content_state() {
        assert_eq!(
            page_name_for_state::<gtk::Label>(&BigAsyncState::Skeleton(
                BigSkeletonSpec::list_rows()
            )),
            PAGE_SKELETON
        );
        assert_eq!(
            page_name_for_state::<gtk::Label>(&BigAsyncState::Loading(BigLoadingSpinnerSpec::new(
                BigLoadingSize::Primary
            ))),
            PAGE_LOADING
        );
        assert_eq!(
            page_name_for_state::<gtk::Label>(&BigAsyncState::Error(BigErrorStateSpec::transient(
                "t", "b"
            ))),
            PAGE_ERROR
        );
    }

    #[test]
    fn spinner_px_scales_with_size() {
        assert!(spinner_px(BigLoadingSize::Inline) < spinner_px(BigLoadingSize::Embedded));
        assert!(spinner_px(BigLoadingSize::Embedded) < spinner_px(BigLoadingSize::Primary));
    }

    /// Pump the default main context without blocking, up to `budget`.
    #[cfg(not(miri))]
    fn pump_until(budget: Duration, mut done: impl FnMut() -> bool) {
        let ctx = gtk::glib::MainContext::default();
        let deadline = Instant::now() + budget;
        while !done() && Instant::now() < deadline {
            while ctx.iteration(false) {}
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn apply_state_swaps_pages_and_retry_emits() {
        if gtk::init().is_err() {
            return;
        }
        let retried = Rc::new(Cell::new(false));
        let retried_cb = Rc::clone(&retried);
        let widgets = build_async_surface_widgets(move || retried_cb.set(true));
        let generation = Rc::new(Cell::new(0u64));

        // skeleton -> loading(0 delay) -> content -> error
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Skeleton(BigSkeletonSpec::list_rows()),
            &generation,
        );
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_SKELETON)
        );

        generation.set(1);
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Loading(
                BigLoadingSpinnerSpec::new(BigLoadingSize::Primary).with_delay(Duration::ZERO),
            ),
            &generation,
        );
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_LOADING)
        );

        generation.set(2);
        let content = gtk::Label::new(Some("ready"));
        apply_state(&widgets, &BigAsyncState::Content(content), &generation);
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_CONTENT)
        );

        generation.set(3);
        let error = BigErrorStateSpec::transient("Network down", "Check the connection.")
            .with_details("stack trace here");
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Error(error),
            &generation,
        );
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_ERROR)
        );
        assert!(widgets.error_retry_button.is_visible());
        assert_eq!(
            widgets.error_retry_button.label().as_deref(),
            Some("Try again")
        );
        assert!(widgets.error_details_expander.is_visible());
        assert!(!widgets.error_details_expander.is_expanded());

        widgets.error_retry_button.emit_clicked();
        assert!(retried.get(), "retry button must emit the retry callback");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn permanent_error_hides_retry_button() {
        if gtk::init().is_err() {
            return;
        }
        let widgets = build_async_surface_widgets(|| {});
        let generation = Rc::new(Cell::new(0u64));
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Error(BigErrorStateSpec::permanent(
                "Permission denied",
                "The file is read-only.",
            )),
            &generation,
        );
        assert!(
            !widgets.error_retry_button.is_visible(),
            "a permanent error offers no retry"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn loading_anti_flash_defers_spinner_until_delay() {
        if gtk::init().is_err() {
            return;
        }
        let widgets = build_async_surface_widgets(|| {});
        let generation = Rc::new(Cell::new(0u64));
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Skeleton(BigSkeletonSpec::list_rows()),
            &generation,
        );

        generation.set(1);
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Loading(
                BigLoadingSpinnerSpec::new(BigLoadingSize::Primary)
                    .with_delay(Duration::from_millis(120)),
            ),
            &generation,
        );
        // Immediately after: still on skeleton — the spinner has not flashed.
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_SKELETON),
            "loading must not flash before the anti-flash delay"
        );

        pump_until(Duration::from_millis(700), || {
            widgets.stack.visible_child_name().as_deref() == Some(PAGE_LOADING)
        });
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_LOADING),
            "loading page must appear after the anti-flash delay"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn newer_state_supersedes_pending_loading_deferral() {
        if gtk::init().is_err() {
            return;
        }
        let widgets = build_async_surface_widgets(|| {});
        let generation = Rc::new(Cell::new(0u64));
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Skeleton(BigSkeletonSpec::list_rows()),
            &generation,
        );

        generation.set(1);
        apply_state(
            &widgets,
            &BigAsyncState::<gtk::Label>::Loading(
                BigLoadingSpinnerSpec::new(BigLoadingSize::Primary)
                    .with_delay(Duration::from_millis(120)),
            ),
            &generation,
        );
        // Resolve to content before the delay elapses (newer generation).
        generation.set(2);
        apply_state(
            &widgets,
            &BigAsyncState::Content(gtk::Label::new(Some("done"))),
            &generation,
        );
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_CONTENT)
        );

        // Pump past the old delay; the stale deferral must NOT hijack the page.
        pump_until(Duration::from_millis(300), || false);
        assert_eq!(
            widgets.stack.visible_child_name().as_deref(),
            Some(PAGE_CONTENT),
            "a superseded loading deferral must not switch pages"
        );
    }
}
