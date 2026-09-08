//! GTK4 / libadwaita user interface.
//!
//! Structure:
//!
//! * `mic_shell::MicShell` — the sole standalone Relm4 root. Its model owns
//!   background services for the app's lifetime.
//! * `window` — composes the root view from Mic, Output, and Spectrum parts.
//! * `state::AppState` — shared settings snapshot + typed apply work, scheduled
//!   and correlated by the `MicShell` model.

mod app_kit_edges;
mod health;
mod i18n;
mod mic_shell;
mod mic_shell_tracker;
mod state;
mod views;
mod widgets;
mod window;

pub use mic_shell::run;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use adw::prelude::*;
    use gtk::gio;
    use relm4::{Component, ComponentController};

    use crate::config::{AppSettings, GateConfig, NoiseModel, WindowConfig};
    use crate::services::audio_monitor::{AudioMonitor, Event, SpectrumFrame};
    use crate::services::pipewire::user_tweaks::UserTweaks;
    use crate::ui::state::AppState;

    #[cfg(not(miri))]
    #[test]
    fn gtk_display_contracts_cover_dialogs_and_model_picker() {
        let session_mode = std::env::var("BIGLINUX_UI_SESSION_MODE").unwrap_or_default();
        if !matches!(session_mode.as_str(), "headless" | "vm" | "disposable") {
            eprintln!("skip: GTK display contracts require KWin headless, VM, or disposable UI");
            return;
        }
        gtk::init().expect("isolated GTK session is available");
        assert_window_reset_dialog_contract();
        assert_tuning_reset_dialog_contract();
        assert_model_picker_contract();
        assert_spectrum_visibility_contract();
        assert_populate_body_contract();
        assert_window_actions_contract();
        assert_window_build_contract();
        assert_monitor_binding_contract();
        assert_factory_reset_preserves_window_and_ui_preferences();
        assert_tuning_dropdown_and_banner_contract();
        assert_tuning_page_populates_expected_sections();
        assert_tuning_apply_button_contract();
    }

    #[cfg(not(miri))]
    fn assert_window_reset_dialog_contract() {
        let dialog = super::window::reset_confirmation_dialog();

        assert_eq!(
            dialog.heading().as_deref(),
            Some("Restore default settings?")
        );
        assert_eq!(
            dialog.body().as_str(),
            "All audio processing settings will return to their factory values. \
             Your window size and view preferences are kept."
        );
        assert_eq!(dialog.default_response().as_deref(), Some("cancel"));
        assert_eq!(dialog.close_response().as_str(), "cancel");
        assert_eq!(
            dialog.response_appearance("reset"),
            adw::ResponseAppearance::Destructive
        );
    }

    #[cfg(not(miri))]
    fn assert_tuning_reset_dialog_contract() {
        let dialog = super::views::advanced::reset_audio_settings_dialog();

        assert_eq!(
            dialog.heading().as_deref(),
            Some("Restore default audio settings?")
        );
        assert_eq!(
            dialog.body().as_str(),
            "All your custom audio settings will be removed and the \
             audio service will restart with the defaults."
        );
        assert_eq!(dialog.default_response().as_deref(), Some("cancel"));
        assert_eq!(dialog.close_response().as_str(), "cancel");
        assert_eq!(
            dialog.response_appearance("reset"),
            adw::ResponseAppearance::Destructive
        );
    }

    #[cfg(not(miri))]
    fn assert_model_picker_contract() {
        let picked_models = Rc::new(RefCell::new(Vec::new()));
        let picked_models_for_callback = Rc::clone(&picked_models);
        let dropdown = super::widgets::model_picker::build(NoiseModel::GtcrnVctk, move |model| {
            picked_models_for_callback.borrow_mut().push(model);
        });

        assert_eq!(dropdown.selected(), 1);
        let model = dropdown.model().expect("model picker exposes a list model");
        assert!(model.n_items() >= 3);

        dropdown.set_selected(0);
        assert_eq!(&*picked_models.borrow(), &[NoiseModel::GtcrnDns3]);
    }

    #[cfg(not(miri))]
    fn assert_spectrum_visibility_contract() {
        let spectrum_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let stack = adw::ViewStack::new();
        let mic = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let output = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let tuning = gtk::Box::new(gtk::Orientation::Vertical, 0);

        stack.add_titled(&mic, Some("mic"), "Microphone");
        stack.add_titled(&output, Some("output"), "Output filter");
        stack.add_titled(&tuning, Some("tuning"), "Tuning");

        stack.set_visible_child_name("mic");
        super::window::sync_spectrum_visibility(&spectrum_container, &stack);
        assert!(spectrum_container.is_visible());

        stack.set_visible_child_name("output");
        super::window::sync_spectrum_visibility(&spectrum_container, &stack);
        assert!(!spectrum_container.is_visible());

        stack.set_visible_child_name("tuning");
        super::window::sync_spectrum_visibility(&spectrum_container, &stack);
        assert!(!spectrum_container.is_visible());
    }

    #[cfg(not(miri))]
    fn assert_populate_body_contract() {
        let state = AppState::new(AppSettings::default());
        let (sender, _receiver) = relm4::channel::<super::mic_shell::MicInput>();
        let header = adw::HeaderBar::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let spectrum_container = gtk::Box::new(gtk::Orientation::Vertical, 0);

        super::window::populate_body(
            &state,
            &sender,
            &header,
            &body,
            &spectrum_container,
            super::views::Mode::Simple,
        );

        assert_eq!(child_count(&body), 1);
        assert!(header.title_widget().is_none());
        assert!(spectrum_container.is_visible());

        super::window::populate_body(
            &state,
            &sender,
            &header,
            &body,
            &spectrum_container,
            super::views::Mode::Advanced,
        );

        assert_eq!(child_count(&body), 1);
        assert!(header.title_widget().is_some());

        let stack = body
            .first_child()
            .and_downcast::<adw::ViewStack>()
            .expect("advanced body contains a ViewStack");
        assert_eq!(stack.pages().n_items(), 3);
        assert!(spectrum_container.is_visible());

        stack.set_visible_child_name("output");
        assert!(!spectrum_container.is_visible());
    }

    #[cfg(not(miri))]
    fn assert_window_actions_contract() {
        let application = registered_test_application("br.com.biglinux.NoiseReduction.ActionsTest");
        let window = adw::ApplicationWindow::builder()
            .application(&application)
            .build();
        let receiver = super::window::install_window_actions_contract(&window);

        assert!(window.lookup_action("reset-defaults").is_some());
        assert!(window.lookup_action("about").is_some());

        window
            .lookup_action("reset-defaults")
            .expect("reset action exists")
            .activate(None);
        assert!(matches!(
            receiver.recv_sync().expect("reset action emits an input"),
            super::mic_shell::MicInput::RestoreDefaultsRequested
        ));
        window
            .lookup_action("about")
            .expect("about action exists")
            .activate(None);
        assert!(matches!(
            receiver.recv_sync().expect("about action emits an input"),
            super::mic_shell::MicInput::AboutRequested
        ));

        let about_dialog = super::window::present_about_dialog_contract(&window);
        assert_eq!(about_dialog.application_name().as_str(), "Filter noise");
        assert_eq!(about_dialog.developer_name().as_str(), "BigLinux Team");
    }

    #[cfg(not(miri))]
    fn assert_window_build_contract() {
        let application = registered_test_application("br.com.biglinux.NoiseReduction.BuildTest");
        let (monitor, _events_tx) = AudioMonitor::contract_handle();
        let state = AppState::new(AppSettings {
            window: WindowConfig {
                width: 900,
                height: 640,
                maximized: false,
            },
            ..AppSettings::default()
        });

        let controller = super::mic_shell::MicShell::builder()
            .launch(super::mic_shell::MicShellInit {
                state,
                monitor: Rc::new(monitor),
            })
            .detach();
        let window = controller.widget();
        window.set_application(Some(&application));

        assert_eq!(window.title().as_deref(), Some("Filter noise"));
        assert_eq!(window.default_width(), 900);
        assert_eq!(window.default_height(), 640);
        assert!(window.content().is_some());
        assert!(window.lookup_action("reset-defaults").is_some());
        assert!(window.lookup_action("about").is_some());
    }

    #[cfg(not(miri))]
    fn assert_monitor_binding_contract() {
        let (monitor, events_tx) = AudioMonitor::contract_handle();
        let monitor = Rc::new(monitor);
        let spectrum = super::widgets::spectrum::Spectrum::new();

        assert!(monitor.is_active_for_contract());
        super::window::bind_monitor_to_spectrum_visibility(&spectrum, &monitor);
        assert!(!monitor.is_active_for_contract());

        super::window::bind_spectrum_to_monitor(&spectrum, &monitor);
        events_tx
            .try_send(Event::Frame(SpectrumFrame {
                bands_db: vec![-12.0; 8],
                rms_db: -18.0,
                peak_db: -6.0,
                seq: 1,
            }))
            .expect("contract event channel accepts a frame");

        for _ in 0..20 {
            while gtk::glib::MainContext::default().iteration(false) {}
            if spectrum.target_peak_for_contract() > 0.0 {
                break;
            }
        }

        assert!(spectrum.target_peak_for_contract() > 0.0);
    }

    #[cfg(not(miri))]
    fn assert_factory_reset_preserves_window_and_ui_preferences() {
        let state = AppState::new(AppSettings {
            gate: GateConfig {
                intensity: 87,
                ..GateConfig::default()
            },
            window: WindowConfig {
                width: 1024,
                height: 768,
                maximized: true,
            },
            ui: crate::config::UiConfig {
                show_advanced: true,
                dismiss_wp_override_warning: true,
            },
            ..AppSettings::default()
        });

        super::window::apply_factory_defaults(&state);
        let settings = state.settings();

        assert_eq!(settings.window.width, 1024);
        assert_eq!(settings.window.height, 768);
        assert!(settings.window.maximized);
        assert!(settings.ui.show_advanced);
        assert!(settings.ui.dismiss_wp_override_warning);
        assert_eq!(settings.gate, GateConfig::default());
    }

    #[cfg(not(miri))]
    fn assert_tuning_dropdown_and_banner_contract() {
        let picked_values = Rc::new(RefCell::new(Vec::new()));
        let picked_values_for_callback = Rc::clone(&picked_values);
        let dropdown =
            super::views::advanced::build_headroom_contract_dropdown(Some(1024), move |value| {
                picked_values_for_callback.borrow_mut().push(value);
            });

        assert_eq!(dropdown.selected(), 4);
        let model = dropdown
            .model()
            .expect("headroom dropdown exposes a list model");
        assert_eq!(model.n_items(), 7);
        dropdown.set_selected(5);
        assert_eq!(&*picked_values.borrow(), &[Some(2048)]);

        let banner = adw::Banner::new("");
        super::views::advanced::refresh_banner_contract(&banner, UserTweaks::default());
        assert_eq!(
            banner.title().as_str(),
            "The standard audio settings are in use."
        );
        assert!(!banner.is_revealed());

        super::views::advanced::refresh_banner_contract(
            &banner,
            UserTweaks {
                quantum: Some(512),
                ..UserTweaks::default()
            },
        );
        assert_eq!(
            banner.title().as_str(),
            "Your audio settings are active."
        );
        assert!(banner.is_revealed());
    }

    #[cfg(not(miri))]
    fn assert_tuning_page_populates_expected_sections() {
        let content = super::views::advanced::build_tuning_page_contract(UserTweaks {
            quantum: Some(512),
            ..UserTweaks::default()
        });

        assert!(child_count(&content) >= 13);
    }

    #[cfg(not(miri))]
    fn assert_tuning_apply_button_contract() {
        let button = gtk::Button::with_label("Apply and restart audio");
        let banner = adw::Banner::new("");

        super::views::advanced::trigger_apply_button_contract(&button, &banner);

        assert!(!button.is_sensitive());
        assert_eq!(button.label().as_deref(), Some("Restarting audio…"));
    }

    #[cfg(not(miri))]
    fn child_count(container: &gtk::Box) -> usize {
        let mut count = 0;
        let mut child = container.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        count
    }

    #[cfg(not(miri))]
    fn registered_test_application(application_id: &str) -> adw::Application {
        use relm4::adw::Application as RelmApplication;

        let application = RelmApplication::builder()
            .application_id(application_id)
            .build();
        application
            .register(None::<&gio::Cancellable>)
            .expect("test application registers");
        application
    }
}
