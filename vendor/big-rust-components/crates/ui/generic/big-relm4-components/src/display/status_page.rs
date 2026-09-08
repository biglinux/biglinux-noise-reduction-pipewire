//! Relm4 wrapper for `adw::StatusPage`, driven by [`BigEmptyStateSpec`].
//!
//! This is the single widget builder for the display-free empty-state spec
//! in [`crate::state::empty`]: feed it a [`BigEmptyStateSpec`] and it renders
//! the icon/title/body plus the optional primary call-to-action, emitting
//! [`BigStatusPageOutput::ActionTriggered`] when the CTA is pressed. Use it for
//! every welcome / empty surface (empty library, empty queue, no results, …)
//! instead of hand-rolling a per-app `adw::StatusPage` component.

use adw::prelude::*;
use relm4::gtk;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::state::empty::BigEmptyStateSpec;

/// Messages accepted by [`BigStatusPage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigStatusPageInput {
    /// Replace the whole spec (icon/title/body/action) and re-render in one
    /// pass — e.g. transition an empty library into a "no search results"
    /// state without rebuilding the component.
    SetSpec(BigEmptyStateSpec),
}

/// Messages emitted by [`BigStatusPage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigStatusPageOutput {
    /// The primary CTA was activated; carries the spec's
    /// [`crate::state::empty::BigEmptyStateAction::action_id`].
    ActionTriggered(String),
}

/// Reusable BigLinux status / empty-state component.
///
/// Wraps `adw::StatusPage` and renders a [`BigEmptyStateSpec`] with typed
/// Relm4 updates. The optional CTA button is built inside the component and
/// reports activations through [`BigStatusPageOutput::ActionTriggered`], so
/// the host stays decoupled from the widget tree.
///
/// # Examples
///
/// ```ignore
/// use big_relm4_components::display::status_page::{BigStatusPage, BigStatusPageOutput};
/// use big_relm4_components::state::empty::BigEmptyStateSpec;
/// use relm4::{Component, ComponentController, RelmApp};
///
/// fn main() {
///     let spec = BigEmptyStateSpec::new("folder-music-symbolic", "No Audio Files")
///         .with_body("Drop files here to begin.")
///         .with_action("Add files", "win.add-files");
///     let app = RelmApp::new("br.com.biglinux.example");
///     app.run::<BigStatusPage>(spec);
/// }
///
/// // Forward CTA activations from a host component:
/// # fn forward(ctrl: &relm4::Controller<BigStatusPage>) {
/// ctrl.connect_receiver(|_sender, msg| match msg {
///     BigStatusPageOutput::ActionTriggered(id) => println!("cta: {id}"),
/// });
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigStatusPage {
    spec: BigEmptyStateSpec,
}

impl BigStatusPage {
    /// The spec currently rendered by this status page.
    #[must_use]
    pub fn spec(&self) -> &BigEmptyStateSpec {
        &self.spec
    }
}

impl Component for BigStatusPage {
    type Init = BigEmptyStateSpec;
    type Input = BigStatusPageInput;
    type Output = BigStatusPageOutput;
    type CommandOutput = ();
    type Root = adw::StatusPage;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::StatusPage::new()
    }

    fn init(
        spec: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        render(&root, &spec, &sender);
        ComponentParts {
            model: Self { spec },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            BigStatusPageInput::SetSpec(spec) => self.spec = spec,
        }
        render(root, &self.spec, &sender);
    }
}

/// Apply `spec` onto an existing `adw::StatusPage`: set icon/title/body and
/// (re)build the optional CTA button, returned **unbound** so the caller wires
/// its `connect_clicked`. A spec with no action clears any prior child, so this
/// is safe to call repeatedly to re-render in place.
///
/// Raw-GTK call sites that already own a status page use this; Relm4 hosts that
/// want the page built for them use [`build_empty_state`] or the
/// [`BigStatusPage`] component.
#[must_use]
pub fn apply_empty_state(page: &adw::StatusPage, spec: &BigEmptyStateSpec) -> Option<gtk::Button> {
    page.set_icon_name(Some(&spec.icon_name));
    page.set_title(&spec.title);
    page.set_description(spec.body.as_deref());

    match &spec.action {
        Some(action) => {
            let button = gtk::Button::builder()
                .label(&action.label)
                .halign(gtk::Align::Center)
                .build();
            button.add_css_class("pill");
            if action.suggested {
                button.add_css_class("suggested-action");
            }
            page.set_child(Some(&button));
            Some(button)
        }
        None => {
            page.set_child(None::<&gtk::Widget>);
            None
        }
    }
}

/// Build a fresh vexpanding `adw::StatusPage` from `spec` plus its optional
/// unbound CTA button. For raw-GTK call sites that embed the page directly and
/// toggle its visibility; Relm4 hosts should prefer the [`BigStatusPage`]
/// component, which wires the CTA to [`BigStatusPageOutput::ActionTriggered`].
#[must_use]
pub fn build_empty_state(spec: &BigEmptyStateSpec) -> (adw::StatusPage, Option<gtk::Button>) {
    let page = adw::StatusPage::new();
    page.set_vexpand(true);
    let button = apply_empty_state(&page, spec);
    (page, button)
}

/// Render `spec` into the component root and wire the CTA (if any) to the
/// component's output sender.
fn render(
    root: &adw::StatusPage,
    spec: &BigEmptyStateSpec,
    sender: &ComponentSender<BigStatusPage>,
) {
    if let (Some(button), Some(action)) = (apply_empty_state(root, spec), spec.action.as_ref()) {
        let action_id = action.action_id.clone();
        // output channel (ComponentSender), not a widget — no ref cycle.
        let output = sender.output_sender().clone();
        button.connect_clicked(move |_| {
            let _ = output.send(BigStatusPageOutput::ActionTriggered(action_id.clone()));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_holds_the_latest_spec() {
        let mut model = BigStatusPage {
            spec: BigEmptyStateSpec::new("folder-symbolic", "Empty"),
        };
        assert!(model.spec().action.is_none());

        // The render path needs a display; exercise the state transition only.
        model.spec = BigEmptyStateSpec::new("folder-music-symbolic", "No Audio Files")
            .with_body("Add files to continue")
            .with_action("Add files", "win.add-files");

        assert_eq!(model.spec().title, "No Audio Files");
        assert_eq!(model.spec().body.as_deref(), Some("Add files to continue"));
        assert_eq!(
            model.spec().action.as_ref().map(|a| a.action_id.as_str()),
            Some("win.add-files")
        );
    }
}
