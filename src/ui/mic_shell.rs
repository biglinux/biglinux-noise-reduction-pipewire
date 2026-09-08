//! `MicShell` — the microphone application's Relm4 root.
//!
//! The model owns [`AppState`], [`AudioMonitor`], apply/health workers, and the
//! debounce. Views emit typed [`MicInput`] messages; the custom-draw
//! [`Spectrum`] remains a concrete widget owned by this root.

use std::ffi::OsString;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::layout::hamburger_menu::{
    BigActionPopoverButton, build_flat_action_popover_button,
};
use glib::SourceId;
use gtk::{Orientation, gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::config::{AppSettings, NoiseModel, StereoMode, app_id, app_version, settings_file};
use crate::pipeline::cascade_mic_off;
use crate::services::audio_monitor::{AudioMonitor, MonitorConfig};

use super::health::{self, Health};
use super::i18n::{i18n, init_gettext};
use super::mic_shell_tracker::{ApplyTracker, HealthRequest, SettingsLoadTracker};
use super::state::{AppState, ApplyCompletion, ApplyRequest, ApplyRevision, ApplyWork};
#[cfg(test)]
use super::views::Mode;
use super::widgets::eq_card::{self, EqMutation};
use super::widgets::spectrum::Spectrum;
use super::widgets::wp_override_warning;
use super::window;

/// Coalesce slider drags without delaying deliberate actions such as close.
const APPLY_DEBOUNCE: Duration = Duration::from_millis(150);

/// Run the microphone application through its sole Relm4 root.
#[must_use]
pub fn run() -> glib::ExitCode {
    if let Some(result) = early_cli(&std::env::args_os().collect::<Vec<_>>()) {
        return result;
    }
    init_gettext();
    let app = adw::Application::builder().application_id(app_id()).build();
    let state = AppState::new(AppSettings::load());
    let monitor = Rc::new(AudioMonitor::start(MonitorConfig::default()));
    relm4::RelmApp::from_app(app).run::<MicShell>(MicShellInit { state, monitor });
    glib::ExitCode::SUCCESS
}

/// Domain services handed to the shell at launch.
pub(super) struct MicShellInit {
    pub state: Rc<AppState>,
    pub monitor: Rc<AudioMonitor>,
}

/// User actions routed through the component. Body controls emit these typed
/// messages; `update_with_view` owns every `AppState` transition.
#[derive(Debug)]
pub(super) enum MicInput {
    /// The window manager requested a clean close.
    CloseRequested,
    CloseSaveRetry,
    CloseWithoutSaving,
    CloseCancelled,
    /// The main menu requested the reset confirmation dialog.
    RestoreDefaultsRequested,
    /// The reset confirmation dialog was accepted.
    RestoreDefaultsConfirmed,
    /// The main menu requested application information.
    AboutRequested,
    /// The stale WirePlumber override dialog returned a user decision.
    OverrideWarningDecided(wp_override_warning::OverrideWarningDecision),
    /// The header "Advanced" switch flipped.
    AdvancedToggled(bool),
    /// `settings.json` changed outside this process (CLI / plasmoid / edit).
    ExternalSettingsChanged,
    /// The apply debounce for this settings revision elapsed.
    ApplyDebounceElapsed(ApplyRevision),
    /// The status banner's action button was clicked; the model decides
    /// between re-probing health and retrying the failed apply.
    BannerActionClicked,
    /// Microphone noise-filter master switch (Simple view). Off cascades the
    /// whole mic chain down so the worker fully tears down.
    NoiseFilterToggled(bool),
    /// Microphone noise-reduction intensity (0.0–1.0).
    MicIntensityChanged(f32),
    /// "Hear my voice" self-listen toggle.
    SelfListenToggled(bool),
    /// System-sound (output) filter master switch.
    OutputFilterToggled(bool),
    /// Output filter noise-reduction intensity (0.0–1.0).
    OutputIntensityChanged(f32),

    // ── Advanced mic-chain controls (views::mic) ──────────────────────
    /// Mic noise-reduction master (Advanced view — no cascade, unlike Simple).
    MicNrEnabled(bool),
    /// WebRTC echo-cancellation toggle.
    MicEchoModeChanged(crate::config::EchoMode),
    QualityChanged(crate::config::Quality),
    /// Mic neural-model selection.
    MicModelChanged(NoiseModel),
    /// Mic voice-presence (high-frequency recovery) intensity.
    MicVoiceRecoveryChanged(f32),
    /// Mic high-pass filter toggle.
    MicHpfToggled(bool),
    /// Mic silence-gate toggle.
    MicGateToggled(bool),
    /// Mic silence-gate intensity (`0..=GATE_INTENSITY_MAX`).
    MicGateIntensityChanged(u8),
    /// Mic compressor toggle.
    MicCompressorToggled(bool),
    /// Mic compressor intensity (0.0–1.0).
    MicCompressorIntensityChanged(f32),
    /// Mic equalizer mutation (enable/preset/band).
    MicEq(EqMutation),
    /// Voice-changer toggle (pitch shift via stereo VoiceChanger mode).
    MicVoiceChangerToggled(bool),
    /// Voice-changer pitch width (0.0–1.0).
    MicPitchChanged(f32),
    /// Self-listen monitor delay in milliseconds.
    MicSelfListenDelayChanged(u32),

    // ── Advanced output-chain controls (views::output) ────────────────
    /// Output neural-model selection.
    OutputModelChanged(NoiseModel),
    /// Output voice-presence intensity.
    OutputVoiceRecoveryChanged(f32),
    /// Output high-pass filter toggle.
    OutputHpfToggled(bool),
    /// Output silence-gate toggle.
    OutputGateToggled(bool),
    /// Output silence-gate intensity.
    OutputGateIntensityChanged(u8),
    /// Output compressor toggle.
    OutputCompressorToggled(bool),
    /// Output compressor intensity (0.0–1.0).
    OutputCompressorIntensityChanged(f32),
    /// Output equalizer mutation.
    OutputEq(EqMutation),
}

#[derive(Debug)]
pub(super) enum MicCommandOutput {
    ExternalSettingsLoaded {
        generation: u64,
        settings: Box<AppSettings>,
    },
    ApplyCompleted(Box<ApplyCompletion>),
    HealthResolved {
        request: HealthRequest,
        health: Health,
    },
    /// The spectrum worker and its `pw-cat` child were reaped off the GTK
    /// thread after the final settings revision settled.
    MonitorStopped,
    ClosePreferencesSaved(Result<(), String>),
    OverrideRemovalCompleted {
        path: std::path::PathBuf,
        result: Result<wp_override_warning::OverrideRemoval, String>,
    },
}

/// What the status banner's single action button does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BannerAction {
    /// Banner hidden / informational — no button.
    None,
    /// Unavailable state — button re-runs the health probe.
    Recheck,
    /// Apply failure — button re-runs the apply pipeline.
    RetryApply,
    /// An explicit apply retry is running; health results from an older probe
    /// must not replace the honest in-progress state.
    RetryInProgress,
}

/// The microphone application model and lifecycle owner.
pub(super) struct MicShell {
    state: Rc<AppState>,
    banner_action: BannerAction,
    is_closing: bool,
    close_save_in_flight: bool,
    should_probe_after_apply: bool,
    applies: ApplyTracker,
    apply_debounce: Option<SourceId>,
    settings_loads: SettingsLoadTracker,
    settings_reload_pending: bool,
    monitor: Option<Rc<AudioMonitor>>,
    settings_monitor: Option<gio::FileMonitor>,
}

/// Concrete GTK view owned by the root component runtime.
pub(super) struct MicShellWidgets {
    header: adw::HeaderBar,
    body: gtk::Box,
    banner: adw::Banner,
    spectrum_container: gtk::Box,
    mode_switch: gtk::Switch,
    _spectrum: Rc<Spectrum>,
    _primary_menu: BigActionPopoverButton,
}

impl Component for MicShell {
    type Init = MicShellInit;
    type Input = MicInput;
    type Output = ();
    type CommandOutput = MicCommandOutput;
    type Root = adw::ApplicationWindow;
    type Widgets = MicShellWidgets;

    fn init_root() -> Self::Root {
        let window = adw::ApplicationWindow::builder().build();
        // Opt in to optional Big Gnome Center background styling. This used
        // to sit in `window::build`, which the Relm4 port removed — the
        // component owns the window now, so the class moves with it.
        window.add_css_class("biglinux-microphone");
        window
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let MicShellInit { state, monitor } = init;
        let initial_mode = window::current_mode(&state);
        let initial_window = state.settings().window.clone();
        root.set_title(Some(&i18n("Filter noise")));
        root.set_default_width(initial_window.width.try_into().unwrap_or(720));
        root.set_default_height(initial_window.height.try_into().unwrap_or(700));
        if initial_window.maximized {
            root.maximize();
        }

        let header = adw::HeaderBar::new();
        header.set_decoration_layout(Some(":minimize,maximize,close"));
        let mode_picker = window::build_mode_picker(initial_mode);
        header.pack_start(&mode_picker.container);
        let primary_menu = build_flat_action_popover_button(&window::primary_menu_spec());
        // Pack the component's root container, not the bare button: the
        // button already lives inside `root()` (which also anchors the
        // popover), and packing a parented child trips
        // `adw_header_bar_pack_end`'s assertion — the menu silently
        // never appears (AT-SPI tree had no "Main menu" button).
        header.pack_end(primary_menu.root());

        let body = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();

        let spectrum = Spectrum::new();
        window::bind_spectrum_to_monitor(&spectrum, &monitor);
        window::bind_monitor_to_spectrum_visibility(&spectrum, &monitor);
        let spectrum_container = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .margin_top(12)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        spectrum_container.append(spectrum.widget());

        let input = sender.input_sender().clone();
        window::populate_body(
            &state,
            &input,
            &header,
            &body,
            &spectrum_container,
            initial_mode,
        );
        body.set_sensitive(false);

        let banner = adw::Banner::new(&i18n("Checking the audio system…"));
        banner.set_revealed(true);
        {
            let sender = sender.clone();
            banner.connect_button_clicked(move |_| {
                let _ = sender.input_sender().send(MicInput::BannerActionClicked);
            });
        }

        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.append(&header);
        content.append(&banner);
        content.append(&spectrum_container);
        content.append(&body);
        root.set_content(Some(&content));

        // Advanced toggle → typed message (no direct mutate in the handler).
        {
            let sender = sender.clone();
            mode_picker.switch.connect_active_notify(move |sw| {
                let _ = sender
                    .input_sender()
                    .send(MicInput::AdvancedToggled(sw.is_active()));
            });
        }

        window::install_window_actions(&root, sender.input_sender().clone());
        {
            let sender = sender.clone();
            root.connect_close_request(move |_| {
                let _ = sender.input_sender().send(MicInput::CloseRequested);
                glib::Propagation::Stop
            });
        }

        let root_for_warning = root.clone();
        let should_suppress_warning = state.settings().ui.dismiss_wp_override_warning;
        let warning_sender = sender.clone();
        glib::idle_add_local_once(move || {
            wp_override_warning::maybe_show(
                &root_for_warning,
                should_suppress_warning,
                move |decision| {
                    let _ = warning_sender
                        .input_sender()
                        .send(MicInput::OverrideWarningDecided(decision));
                },
            );
        });

        let settings_monitor = install_external_settings_watch(sender.clone());
        let mut applies = ApplyTracker::default();
        if let Some(request) = applies.mark_current_ready() {
            spawn_apply_work(&sender, state.apply_work(request));
        }

        let model = MicShell {
            state,
            banner_action: BannerAction::None,
            is_closing: false,
            close_save_in_flight: false,
            should_probe_after_apply: true,
            applies,
            apply_debounce: None,
            settings_loads: SettingsLoadTracker::default(),
            settings_reload_pending: false,
            monitor: Some(monitor),
            settings_monitor,
        };
        let widgets = MicShellWidgets {
            header,
            body,
            banner,
            spectrum_container,
            mode_switch: mode_picker.switch,
            _spectrum: spectrum,
            _primary_menu: primary_menu,
        };
        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        if self.is_closing
            && !matches!(
                message,
                MicInput::CloseSaveRetry | MicInput::CloseWithoutSaving | MicInput::CloseCancelled
            )
        {
            return;
        }
        match message {
            MicInput::CloseRequested => self.close(widgets, root, &sender),
            MicInput::CloseSaveRetry => self.save_before_close(&sender),
            MicInput::CloseWithoutSaving => self.stop_monitor(widgets, &sender),
            MicInput::CloseCancelled => {
                self.is_closing = false;
                self.applies = ApplyTracker::default();
                self.should_probe_after_apply = true;
                widgets.banner.set_title(&i18n(
                    "Changes have not been saved. You can keep editing or close without saving.",
                ));
                widgets.banner.set_button_label(None);
                widgets.banner.set_revealed(true);
                widgets.body.set_sensitive(true);
                self.banner_action = BannerAction::None;
            }
            MicInput::RestoreDefaultsRequested => {
                let dialog = window::reset_confirmation_dialog();
                let sender = sender.clone();
                dialog.connect_response(None, move |dialog, response| {
                    if response == "reset" {
                        let _ = sender
                            .input_sender()
                            .send(MicInput::RestoreDefaultsConfirmed);
                    }
                    dialog.close();
                });
                dialog.present(Some(root));
            }
            MicInput::RestoreDefaultsConfirmed => {
                window::apply_factory_defaults(&self.state);
                self.settings_changed(&sender);
                self.repopulate(widgets, sender.input_sender());
            }
            MicInput::AboutRequested => {
                window::present_about_dialog(root);
            }
            MicInput::OverrideWarningDecided(decision) => match decision {
                wp_override_warning::OverrideWarningDecision::Keep { should_dismiss } => {
                    self.dismiss_override_warning(should_dismiss, &sender);
                }
                wp_override_warning::OverrideWarningDecision::Remove {
                    path,
                    should_dismiss,
                } => {
                    self.dismiss_override_warning(should_dismiss, &sender);
                    sender.spawn_oneshot_command(move || {
                        let result = wp_override_warning::remove_override(&path);
                        MicCommandOutput::OverrideRemovalCompleted { path, result }
                    });
                }
            },
            MicInput::AdvancedToggled(advanced) => {
                self.mutate_settings(&sender, |s| s.ui.show_advanced = advanced);
                self.repopulate(widgets, sender.input_sender());
            }
            MicInput::ApplyDebounceElapsed(revision) => {
                if !self.applies.is_current_revision(revision) {
                    return;
                }
                self.apply_debounce.take();
                if let Some(request) = self.applies.mark_ready(revision) {
                    self.spawn_apply_request(&sender, request);
                }
            }
            MicInput::BannerActionClicked => match self.banner_action {
                BannerAction::Recheck => {
                    if self.spawn_health_probe(&sender) {
                        self.show_checking(widgets);
                    }
                }
                BannerAction::RetryApply => {
                    self.show_checking(widgets);
                    self.banner_action = BannerAction::RetryInProgress;
                    self.should_probe_after_apply = true;
                    let revision = self.applies.settings_changed();
                    self.arm_apply_debounce(&sender, revision);
                }
                BannerAction::None | BannerAction::RetryInProgress => {}
            },
            MicInput::ExternalSettingsChanged => {
                if self.state.has_active_apply() {
                    self.settings_reload_pending = true;
                    return;
                }
                let Some(generation) = self.settings_loads.begin() else {
                    log::error!("settings reload generation exhausted");
                    return;
                };
                sender.spawn_oneshot_command(move || MicCommandOutput::ExternalSettingsLoaded {
                    generation,
                    settings: Box::new(AppSettings::load()),
                });
            }
            // Body-control transitions. The widget already shows the new value;
            // the mutation triggers the debounced apply. No rebuild (the visible
            // Simple-view controls don't depend on these fields).
            MicInput::NoiseFilterToggled(on) => self.mutate_settings(&sender, |s| {
                s.noise_reduction.enabled = on;
                if !on {
                    // Single Simple-view master: cascade so one click tears down
                    // every reason the mic worker would stay alive.
                    cascade_mic_off(s);
                }
            }),
            MicInput::MicIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| s.noise_reduction.strength = v);
            }
            MicInput::SelfListenToggled(on) => {
                self.mutate_settings(&sender, |s| s.monitor.enabled = on);
            }
            MicInput::OutputFilterToggled(on) => {
                self.mutate_settings(&sender, |s| s.output_filter.enabled = on);
            }
            MicInput::OutputIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| {
                    s.output_filter.noise_reduction.strength = v;
                });
            }

            // ── Advanced mic-chain ────────────────────────────────────
            MicInput::MicNrEnabled(on) => {
                self.mutate_settings(&sender, |s| s.noise_reduction.enabled = on);
            }
            MicInput::MicEchoModeChanged(mode) => {
                self.mutate_settings(&sender, |s| s.echo_cancel.mode = mode);
            }
            MicInput::QualityChanged(quality) => {
                self.mutate_settings(&sender, |s| s.quality = quality);
            }
            // Picking a model by name is the expert path, and it takes ownership of the
            // choice away from §38's policy. Without this the model reverts at the next
            // login, which reads as the list not working rather than as a policy winning.
            MicInput::MicModelChanged(model) => {
                self.mutate_settings(&sender, |s| {
                    s.noise_reduction.model = model;
                    s.quality = crate::config::Quality::Manual;
                });
            }
            MicInput::MicVoiceRecoveryChanged(v) => {
                self.mutate_settings(&sender, |s| s.noise_reduction.voice_recovery = v);
            }
            MicInput::MicHpfToggled(on) => {
                self.mutate_settings(&sender, |s| s.hpf.enabled = on);
            }
            MicInput::MicGateToggled(on) => {
                self.mutate_settings(&sender, |s| s.gate.enabled = on);
            }
            MicInput::MicGateIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| s.gate.intensity = v);
            }
            MicInput::MicCompressorToggled(on) => {
                self.mutate_settings(&sender, |s| s.compressor.enabled = on);
            }
            MicInput::MicCompressorIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| s.compressor.intensity = v);
            }
            MicInput::MicEq(mutation) => {
                self.mutate_settings(&sender, |s| {
                    eq_card::apply_eq_mutation(&mut s.equalizer, mutation);
                });
            }
            MicInput::MicVoiceChangerToggled(on) => self.mutate_settings(&sender, |s| {
                s.stereo.enabled = on;
                s.stereo.mode = if on {
                    StereoMode::VoiceChanger
                } else {
                    StereoMode::Mono
                };
            }),
            MicInput::MicPitchChanged(v) => {
                self.mutate_settings(&sender, |s| s.stereo.width = v);
            }
            MicInput::MicSelfListenDelayChanged(v) => {
                self.mutate_settings(&sender, |s| s.monitor.delay_ms = v);
            }

            // ── Advanced output-chain ─────────────────────────────────
            MicInput::OutputModelChanged(model) => {
                self.mutate_settings(&sender, |s| {
                    s.output_filter.noise_reduction.model = model;
                    s.quality = crate::config::Quality::Manual;
                });
            }
            MicInput::OutputVoiceRecoveryChanged(v) => {
                self.mutate_settings(&sender, |s| {
                    s.output_filter.noise_reduction.voice_recovery = v;
                });
            }
            MicInput::OutputHpfToggled(on) => {
                self.mutate_settings(&sender, |s| s.output_filter.hpf.enabled = on);
            }
            MicInput::OutputGateToggled(on) => {
                self.mutate_settings(&sender, |s| s.output_filter.gate.enabled = on);
            }
            MicInput::OutputGateIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| s.output_filter.gate.intensity = v);
            }
            MicInput::OutputCompressorToggled(on) => {
                self.mutate_settings(&sender, |s| {
                    s.output_filter.compressor.enabled = on;
                });
            }
            MicInput::OutputCompressorIntensityChanged(v) => {
                self.mutate_settings(&sender, |s| {
                    s.output_filter.compressor.intensity = v;
                });
            }
            MicInput::OutputEq(mutation) => {
                self.mutate_settings(&sender, |s| {
                    eq_card::apply_eq_mutation(&mut s.output_filter.equalizer, mutation);
                });
            }
        }
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match message {
            MicCommandOutput::ExternalSettingsLoaded {
                generation,
                settings,
            } => {
                if self.is_closing {
                    return;
                }
                if !self.settings_loads.accept(generation) {
                    return;
                }
                if !self.state.external_replace(*settings) {
                    return;
                }
                self.cancel_apply_debounce();
                self.show_checking(widgets);
                self.should_probe_after_apply = true;
                self.applies.settings_changed();
                if let Some(request) = self.applies.mark_current_ready() {
                    self.spawn_apply_request(&sender, request);
                }
                let advanced = self.state.settings().ui.show_advanced;
                if widgets.mode_switch.is_active() == advanced {
                    self.repopulate(widgets, sender.input_sender());
                } else {
                    // Flipping the switch re-enters via `AdvancedToggled`, which
                    // persists the already-loaded value and rebuilds the view.
                    widgets.mode_switch.set_active(advanced);
                }
            }
            MicCommandOutput::ApplyCompleted(completion) => {
                let request = completion.request();
                let Some(tracking) = self.applies.complete_apply(request) else {
                    return;
                };
                let result = self.state.finish_apply(*completion);
                if self.is_closing {
                    // Finish the worker already running, but never gate closing
                    // on an audio retry or dispatch another queued graph update.
                    self.save_before_close(&sender);
                    return;
                }
                if result.is_ok() {
                    self.applies.record_applied(request);
                }
                if let Some(next) = tracking.next {
                    self.spawn_apply_request(&sender, next);
                }
                if !tracking.is_settled {
                    return;
                }
                if std::mem::take(&mut self.settings_reload_pending) && !self.is_closing {
                    let _ = sender
                        .input_sender()
                        .send(MicInput::ExternalSettingsChanged);
                }

                match result {
                    Ok(()) if self.should_probe_after_apply => {
                        self.should_probe_after_apply = false;
                        self.show_checking(widgets);
                        self.spawn_health_probe(&sender);
                    }
                    Ok(()) => {}
                    Err(cause) => self.show_apply_failure(widgets, &cause),
                }
            }
            MicCommandOutput::HealthResolved { request, health } => {
                let Some(tracking) = self.applies.complete_health(request) else {
                    return;
                };
                if let Some(next) = tracking.next {
                    self.spawn_apply_request(&sender, next);
                }
                if tracking.is_current && tracking.next.is_none() && !self.is_closing {
                    self.show_health(widgets, &health);
                }
            }
            MicCommandOutput::ClosePreferencesSaved(result) => {
                self.close_save_in_flight = false;
                if !self.is_closing {
                    return;
                }
                match result {
                    Ok(()) => self.stop_monitor(widgets, &sender),
                    Err(error) => {
                        log::warn!("close: preferences were not saved: {error}");
                        let dialog = adw::AlertDialog::builder()
                            .heading(i18n("Settings could not be saved"))
                            .body(i18n("Another application may be changing the settings, or the settings file is not writable. Try again, keep this window open, or close without saving your latest changes."))
                            .build();
                        dialog.add_response("cancel", &i18n("Keep window open"));
                        dialog.add_response("discard", &i18n("Close without saving"));
                        dialog.add_response("retry", &i18n("Try again"));
                        dialog.set_default_response(Some("cancel"));
                        dialog.set_close_response("cancel");
                        dialog.set_response_appearance(
                            "discard",
                            adw::ResponseAppearance::Destructive,
                        );
                        let input = sender.input_sender().clone();
                        dialog.connect_response(None, move |_, response| {
                            let message = match response {
                                "retry" => MicInput::CloseSaveRetry,
                                "discard" => MicInput::CloseWithoutSaving,
                                _ => MicInput::CloseCancelled,
                            };
                            let _ = input.send(message);
                        });
                        dialog.present(Some(root));
                    }
                }
            }
            MicCommandOutput::MonitorStopped => {
                if self.is_closing {
                    root.destroy();
                }
            }
            MicCommandOutput::OverrideRemovalCompleted { path, result } => {
                wp_override_warning::present_removal_result(root.upcast_ref(), &path, &result);
            }
        }
    }

    fn shutdown(&mut self, _widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        self.cancel_apply_debounce();
        self.settings_monitor.take();
        self.monitor.take();
    }
}

fn early_cli(arguments: &[OsString]) -> Option<glib::ExitCode> {
    if arguments.iter().any(|argument| argument == "--version") {
        println!("biglinux-microphone {}", app_version());
        return Some(glib::ExitCode::SUCCESS);
    }
    if arguments.iter().any(|argument| argument == "--help") {
        println!("Usage: biglinux-microphone [--help] [--version]");
        return Some(glib::ExitCode::SUCCESS);
    }
    None
}

impl MicShell {
    fn repopulate(&self, widgets: &MicShellWidgets, input: &relm4::Sender<MicInput>) {
        window::populate_body(
            &self.state,
            input,
            &widgets.header,
            &widgets.body,
            &widgets.spectrum_container,
            window::current_mode(&self.state),
        );
    }

    fn show_apply_failure(&mut self, widgets: &MicShellWidgets, cause: &str) {
        widgets.banner.set_title(&apply_failure_title(cause));
        widgets.banner.set_button_label(Some(&i18n("Try again")));
        self.banner_action = BannerAction::RetryApply;
        self.should_probe_after_apply = true;
        widgets.banner.set_revealed(true);
        widgets.body.set_sensitive(true);
    }

    fn show_checking(&mut self, widgets: &MicShellWidgets) {
        widgets
            .banner
            .set_title(&i18n("Checking the audio system…"));
        widgets.banner.set_button_label(None);
        widgets.banner.set_revealed(true);
        widgets.body.set_sensitive(false);
        self.banner_action = BannerAction::None;
    }

    fn mutate_settings<F>(&mut self, sender: &ComponentSender<Self>, mutation: F)
    where
        F: FnOnce(&mut AppSettings),
    {
        self.state.mutate(mutation);
        self.settings_changed(sender);
    }

    fn settings_changed(&mut self, sender: &ComponentSender<Self>) {
        if self.applies.has_active_health() || self.banner_action == BannerAction::Recheck {
            self.should_probe_after_apply = true;
        }
        let revision = self.applies.settings_changed();
        self.arm_apply_debounce(sender, revision);
    }

    fn arm_apply_debounce(&mut self, sender: &ComponentSender<Self>, revision: ApplyRevision) {
        self.cancel_apply_debounce();
        let sender = sender.clone();
        self.apply_debounce = Some(glib::timeout_add_local_once(APPLY_DEBOUNCE, move || {
            let _ = sender
                .input_sender()
                .send(MicInput::ApplyDebounceElapsed(revision));
        }));
    }

    fn cancel_apply_debounce(&mut self) {
        if let Some(source) = self.apply_debounce.take() {
            source.remove();
        }
    }

    fn spawn_apply_request(&self, sender: &ComponentSender<Self>, request: ApplyRequest) {
        spawn_apply_work(sender, self.state.apply_work(request));
    }

    fn spawn_health_probe(&mut self, sender: &ComponentSender<Self>) -> bool {
        let Some(request) = self.applies.begin_health() else {
            return false;
        };
        sender.oneshot_command(async move {
            let health = relm4::spawn_blocking(health::probe)
                .await
                .unwrap_or_else(|error| {
                    log::error!("audio health worker failed: {error}");
                    Health::Unavailable {
                        cause: i18n("The audio system check failed unexpectedly."),
                        hint: i18n(
                            "Check again, or run biglinux-microphone-cli doctor in a terminal.",
                        ),
                    }
                });
            MicCommandOutput::HealthResolved { request, health }
        });
        true
    }

    fn dismiss_override_warning(&mut self, should_dismiss: bool, sender: &ComponentSender<Self>) {
        if should_dismiss {
            self.mutate_settings(sender, |settings| {
                settings.ui.dismiss_wp_override_warning = true;
            });
        }
    }

    fn close(
        &mut self,
        widgets: &MicShellWidgets,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        if self.is_closing {
            return;
        }
        self.is_closing = true;
        self.cancel_apply_debounce();
        widgets.banner.set_title(&i18n("Saving settings…"));
        widgets.banner.set_button_label(None);
        widgets.banner.set_revealed(true);
        widgets.body.set_sensitive(false);
        let window = self.state.settings().window.clone();
        let width = current_window_dimension(root.width(), window.width);
        let height = current_window_dimension(root.height(), window.height);
        self.state.mutate(|settings| {
            settings.monitor.enabled = false;
            settings.window.width = width;
            settings.window.height = height;
            settings.window.maximized = root.is_maximized();
        });
        if !self.state.has_active_apply() {
            self.save_before_close(sender);
        }
    }

    fn save_before_close(&mut self, sender: &ComponentSender<Self>) {
        if self.close_save_in_flight {
            return;
        }
        self.close_save_in_flight = true;
        let work = self.state.close_work();
        sender.oneshot_command(async move {
            let result = relm4::spawn_blocking(move || work.run())
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            MicCommandOutput::ClosePreferencesSaved(result)
        });
    }

    fn stop_monitor(&mut self, widgets: &MicShellWidgets, sender: &ComponentSender<Self>) {
        let Some(monitor) = self.monitor.take() else {
            sender.oneshot_command(async { MicCommandOutput::MonitorStopped });
            return;
        };
        let monitor = match Rc::try_unwrap(monitor) {
            Ok(monitor) => monitor,
            Err(monitor) => {
                log::error!(
                    "audio monitor still had {} strong owners during close",
                    Rc::strong_count(&monitor)
                );
                self.monitor = Some(monitor);
                self.is_closing = false;
                widgets.banner.set_title(&i18n(
                    "Could not stop the audio monitor safely. Close the window again to retry.",
                ));
                widgets.banner.set_button_label(None);
                widgets.banner.set_revealed(true);
                widgets.body.set_sensitive(true);
                return;
            }
        };
        sender.oneshot_command(async move {
            if let Err(error) = relm4::spawn_blocking(move || monitor.shutdown()).await {
                log::error!("audio monitor shutdown worker failed: {error}");
            }
            MicCommandOutput::MonitorStopped
        });
    }

    /// Render a resolved health probe. Ready hides the banner and
    /// re-enables the body; Unavailable explains cause + next action and
    /// makes the controls insensitive — a toggle that cannot reach
    /// PipeWire/the plugins must not pretend to work.
    fn show_health(&mut self, widgets: &MicShellWidgets, health: &Health) {
        match health {
            Health::Ready => {
                widgets.banner.set_revealed(false);
                self.banner_action = BannerAction::None;
                widgets.body.set_sensitive(true);
            }
            Health::Unavailable { cause, hint } => {
                widgets.banner.set_title(&format!("{cause} {hint}"));
                widgets.banner.set_button_label(Some(&i18n("Check again")));
                self.banner_action = BannerAction::Recheck;
                widgets.banner.set_revealed(true);
                widgets.body.set_sensitive(true);
            }
        }
    }
}

fn apply_failure_title(cause: &str) -> String {
    format!("{} {cause}", i18n("Could not apply the audio settings:"))
}

fn current_window_dimension(actual: i32, persisted: u32) -> u32 {
    u32::try_from(actual)
        .ok()
        .filter(|dimension| *dimension > 0)
        .unwrap_or(persisted)
}

fn spawn_apply_work(sender: &ComponentSender<MicShell>, work: ApplyWork) {
    let request = work.request();
    sender.oneshot_command(async move {
        let completion = relm4::spawn_blocking(move || work.run())
            .await
            .unwrap_or_else(|error| {
                log::error!("audio apply worker failed: {error}");
                ApplyCompletion::worker_failed(request)
            });
        MicCommandOutput::ApplyCompleted(Box::new(completion))
    });
}

/// Watch `settings.json` and forward external changes as [`MicInput`] messages.
/// The component model performs the read/compare transition; the callback does
/// not mutate application state.
fn install_external_settings_watch(sender: ComponentSender<MicShell>) -> Option<gio::FileMonitor> {
    let file = gio::File::for_path(settings_file());
    let monitor =
        match file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("settings file monitor unavailable: {e}");
                return None;
            }
        };

    monitor.connect_changed(move |_, _, _, event| {
        if !matches!(
            event,
            gio::FileMonitorEvent::Changed
                | gio::FileMonitorEvent::ChangesDoneHint
                | gio::FileMonitorEvent::Created
                | gio::FileMonitorEvent::Renamed
                | gio::FileMonitorEvent::MovedIn
        ) {
            return;
        }
        let _ = sender
            .input_sender()
            .send(MicInput::ExternalSettingsChanged);
    });

    Some(monitor)
}

#[cfg(test)]
#[path = "mic_shell_tests.rs"]
mod tests;
