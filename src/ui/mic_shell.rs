//! `MicShell` — the microphone window as a Relm4 [`Component`].
//!
//! ADR-D14 mountable content (shared with bigiris/players): the component
//! `Root` is the window *content* (a vertical box of header + spectrum strip +
//! body), never an `adw::ApplicationWindow`. Each entry point ([`super::app`]
//! standalone, [`super::embed`] host) launches the component and mounts
//! `controller.widget()` into its own window.
//!
//! Model owns the shared [`AppState`] and the [`AudioMonitor`]; shell-level
//! user actions (the Advanced toggle, an external `settings.json` change, the
//! menu's "Restore defaults" rebuild) flow through typed [`MicInput`] messages
//! handled in [`Component::update`]. The custom-draw [`Spectrum`] `DrawingArea`
//! has no Relm4 equivalent and stays embedded inside the component (manual
//! view) — in-model, not a violation.
//!
//! Body views `views::{simple,mic,output}` are message-driven: their controls
//! emit typed [`MicInput`] messages and `update` owns every `AppState`
//! transition (no `state.mutate` in view handlers). They are built from the
//! shared `big_relm4_components` card/row primitives. `views::advanced` (the
//! Tuning tab) is a self-contained `UserTweaks` Apply/Reset flow, not part of
//! the `AppState` message path.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, Orientation};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::config::{settings_file, AppSettings, NoiseModel, StereoMode};
use crate::pipeline::cascade_mic_off;
use crate::services::audio_monitor::AudioMonitor;

use super::state::AppState;
use super::views::Mode;
use super::widgets::eq_card::{self, EqMutation};
use super::widgets::spectrum::Spectrum;
use super::window;

/// Domain services handed to the shell at launch.
pub(super) struct MicShellInit {
    pub state: Rc<AppState>,
    pub monitor: Rc<AudioMonitor>,
}

/// User actions routed through the component. Body controls emit these typed
/// messages (Stage 2); `update` owns every `AppState` transition.
#[derive(Debug)]
pub(super) enum MicInput {
    /// The header "Advanced" switch flipped.
    AdvancedToggled(bool),
    /// `settings.json` changed outside this process (CLI / plasmoid / edit) and
    /// the new snapshot was absorbed; reconcile the switch + rebuild the body.
    ExternalSettingsLoaded,
    /// "Restore default settings" was confirmed; rebuild the body for the
    /// (geometry/UI-preserving) factory snapshot already written to `AppState`.
    RebuildBody,
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
    MicEchoCancelToggled(bool),
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

/// The microphone window content as a Relm4 component (model owns the state).
pub(super) struct MicShell {
    state: Rc<AppState>,
    mode: Mode,
    header: adw::HeaderBar,
    body: gtk::Box,
    spectrum_container: gtk::Box,
    mode_switch: gtk::Switch,
    /// Input sender clone, used to rebuild the body (`populate_body` wires the
    /// freshly-built view controls back to these messages).
    input: relm4::Sender<MicInput>,
    /// Kept alive for the shell's lifetime; the monitor/visibility bindings hold
    /// weak refs to it (see [`window::bind_spectrum_to_monitor`]).
    _spectrum: Rc<Spectrum>,
    /// Kept alive so the watch keeps firing; dropped with the model on close.
    _monitor: Rc<AudioMonitor>,
    /// `settings.json` watch; held to keep the subscription alive.
    _settings_monitor: Option<gio::FileMonitor>,
}

impl Component for MicShell {
    type Init = MicShellInit;
    type Input = MicInput;
    type Output = ();
    type CommandOutput = ();
    /// Mountable content (ADR-D14): the root box, never an ApplicationWindow.
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let MicShellInit { state, monitor } = init;
        let initial_mode = window::current_mode(&state);

        // ── Header ───────────────────────────────────────────────────────
        let header = adw::HeaderBar::new();
        header.set_decoration_layout(Some(":minimize,maximize,close"));
        let mode_picker = window::build_mode_picker(initial_mode);
        header.pack_start(&mode_picker.container);
        header.pack_end(&window::build_primary_menu_button());

        // ── Body + spectrum strip ─────────────────────────────────────────
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

        root.append(&header);
        root.append(&spectrum_container);
        root.append(&body);

        // Advanced toggle → typed message (no direct mutate in the handler).
        {
            let sender = sender.clone();
            mode_picker.switch.connect_active_notify(move |sw| {
                sender.input(MicInput::AdvancedToggled(sw.is_active()));
            });
        }

        let settings_monitor = install_external_settings_watch(&state, sender.clone());

        let model = MicShell {
            state,
            mode: initial_mode,
            header,
            body,
            spectrum_container,
            mode_switch: mode_picker.switch,
            input,
            _spectrum: spectrum,
            _monitor: monitor,
            _settings_monitor: settings_monitor,
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, root: &Self::Root) {
        // Panic boundary (module contract): a panic in message handling reaps
        // this module's window cleanly (host closes it) instead of orphaning a
        // dead window; the host process and sibling modules survive.
        big_app_kit::containment::contain_embedded_update(root, "microphone", || {
            match msg {
                MicInput::AdvancedToggled(advanced) => {
                    self.state.mutate(|s| s.ui.show_advanced = advanced);
                    self.mode = Mode::from_advanced_flag(advanced);
                    self.repopulate();
                }
                MicInput::RebuildBody => {
                    self.mode = window::current_mode(&self.state);
                    self.repopulate();
                }
                MicInput::ExternalSettingsLoaded => {
                    let advanced = self.state.settings().ui.show_advanced;
                    if self.mode_switch.is_active() == advanced {
                        self.mode = Mode::from_advanced_flag(advanced);
                        self.repopulate();
                    } else {
                        // Flipping the switch re-enters update via AdvancedToggled,
                        // which mutates + rebuilds (matches the pre-migration path).
                        self.mode_switch.set_active(advanced);
                    }
                }
                // Body-control transitions. The widget already shows the new value;
                // the mutation triggers the debounced apply. No rebuild (the visible
                // Simple-view controls don't depend on these fields).
                MicInput::NoiseFilterToggled(on) => self.state.mutate(|s| {
                    s.noise_reduction.enabled = on;
                    if !on {
                        // Single Simple-view master: cascade so one click tears down
                        // every reason the mic worker would stay alive.
                        cascade_mic_off(s);
                    }
                }),
                MicInput::MicIntensityChanged(v) => {
                    self.state.mutate(|s| s.noise_reduction.strength = v);
                }
                MicInput::SelfListenToggled(on) => self.state.mutate(|s| s.monitor.enabled = on),
                MicInput::OutputFilterToggled(on) => {
                    self.state.mutate(|s| s.output_filter.enabled = on);
                }
                MicInput::OutputIntensityChanged(v) => {
                    self.state
                        .mutate(|s| s.output_filter.noise_reduction.strength = v);
                }

                // ── Advanced mic-chain ────────────────────────────────────
                MicInput::MicNrEnabled(on) => self.state.mutate(|s| s.noise_reduction.enabled = on),
                MicInput::MicEchoCancelToggled(on) => {
                    self.state.mutate(|s| s.echo_cancel.enabled = on);
                }
                MicInput::MicModelChanged(model) => {
                    self.state.mutate(|s| s.noise_reduction.model = model);
                }
                MicInput::MicVoiceRecoveryChanged(v) => {
                    self.state.mutate(|s| s.noise_reduction.voice_recovery = v);
                }
                MicInput::MicHpfToggled(on) => self.state.mutate(|s| s.hpf.enabled = on),
                MicInput::MicGateToggled(on) => self.state.mutate(|s| s.gate.enabled = on),
                MicInput::MicGateIntensityChanged(v) => self.state.mutate(|s| s.gate.intensity = v),
                MicInput::MicCompressorToggled(on) => {
                    self.state.mutate(|s| s.compressor.enabled = on);
                }
                MicInput::MicCompressorIntensityChanged(v) => {
                    self.state.mutate(|s| s.compressor.intensity = v);
                }
                MicInput::MicEq(mutation) => {
                    self.state
                        .mutate(|s| eq_card::apply_eq_mutation(&mut s.equalizer, mutation));
                }
                MicInput::MicVoiceChangerToggled(on) => self.state.mutate(|s| {
                    s.stereo.enabled = on;
                    s.stereo.mode = if on {
                        StereoMode::VoiceChanger
                    } else {
                        StereoMode::Mono
                    };
                }),
                MicInput::MicPitchChanged(v) => self.state.mutate(|s| s.stereo.width = v),
                MicInput::MicSelfListenDelayChanged(v) => {
                    self.state.mutate(|s| s.monitor.delay_ms = v);
                }

                // ── Advanced output-chain ─────────────────────────────────
                MicInput::OutputModelChanged(model) => {
                    self.state
                        .mutate(|s| s.output_filter.noise_reduction.model = model);
                }
                MicInput::OutputVoiceRecoveryChanged(v) => {
                    self.state
                        .mutate(|s| s.output_filter.noise_reduction.voice_recovery = v);
                }
                MicInput::OutputHpfToggled(on) => {
                    self.state.mutate(|s| s.output_filter.hpf.enabled = on);
                }
                MicInput::OutputGateToggled(on) => {
                    self.state.mutate(|s| s.output_filter.gate.enabled = on);
                }
                MicInput::OutputGateIntensityChanged(v) => {
                    self.state.mutate(|s| s.output_filter.gate.intensity = v);
                }
                MicInput::OutputCompressorToggled(on) => {
                    self.state
                        .mutate(|s| s.output_filter.compressor.enabled = on);
                }
                MicInput::OutputCompressorIntensityChanged(v) => {
                    self.state
                        .mutate(|s| s.output_filter.compressor.intensity = v);
                }
                MicInput::OutputEq(mutation) => {
                    self.state.mutate(|s| {
                        eq_card::apply_eq_mutation(&mut s.output_filter.equalizer, mutation);
                    });
                }
            }
        });
    }
}

impl MicShell {
    fn repopulate(&self) {
        window::populate_body(
            &self.state,
            &self.input,
            &self.header,
            &self.body,
            &self.spectrum_container,
            self.mode,
        );
    }
}

/// Watch `settings.json` and forward external changes as [`MicInput`] messages.
/// Cheap by design: one `gio::FileMonitor`, re-read on each event, and
/// [`AppState::external_replace`] turns our own atomic-rename writes into a
/// no-op compare. The returned monitor is held by the model so the watch lives
/// exactly as long as the shell.
fn install_external_settings_watch(
    state: &Rc<AppState>,
    sender: ComponentSender<MicShell>,
) -> Option<gio::FileMonitor> {
    let file = gio::File::for_path(settings_file());
    let monitor =
        match file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("settings file monitor unavailable: {e}");
                return None;
            }
        };

    let state = Rc::clone(state);
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
        if state.external_replace(AppSettings::load()) {
            sender.input(MicInput::ExternalSettingsLoaded);
        }
    });

    Some(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

    // Pure reducer-style checks on the shell's mode mapping. Building the full
    // component requires a GTK display; these cover the message→mode logic that
    // `update` applies (Layer-1 no-display validation per relm4-ui).
    #[test]
    fn advanced_flag_maps_to_mode() {
        assert_eq!(Mode::from_advanced_flag(true), Mode::Advanced);
        assert_eq!(Mode::from_advanced_flag(false), Mode::Simple);
    }

    #[test]
    fn advanced_toggle_persists_into_settings() {
        // `AdvancedToggled` mutates `AppState.ui.show_advanced`; verify the
        // state side effect a rendered toggle would drive (no widgets needed).
        let state = AppState::new(AppSettings::default());
        state.mutate(|s| s.ui.show_advanced = true);
        assert!(state.settings().ui.show_advanced);
        state.mutate(|s| s.ui.show_advanced = false);
        assert!(!state.settings().ui.show_advanced);
    }
}
