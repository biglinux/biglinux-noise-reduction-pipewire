from pathlib import Path
from _common import done, replace, commit

TITLE = 'fix(ui): close independently of audio availability and offer explicit save recovery'
if not done(TITLE):
    state = 'src/ui/state.rs'
    replace(state, 'impl AppState {', '''/// Owned close task: saves preferences and stops only GUI-owned resources.
/// It never queries PipeWire or starts/restarts an audio service.
pub(super) struct CloseWork {
    baseline: AppSettings,
    desired: AppSettings,
    loopback: Option<Loopback>,
}

impl CloseWork {
    pub(super) fn run(self) -> Result<(), String> {
        drop(self.loopback);
        let _guard = crate::config::storage::SettingsLock::acquire()
            .map_err(|error| error.to_string())?;
        let latest = AppSettings::load_strict().map_err(|error| error.to_string())?;
        let merged = crate::config::storage::merge(&self.baseline, &self.desired, &latest)
            .map_err(|error| error.to_string())?;
        merged.save().map_err(|error| error.to_string())
    }
}

impl AppState {
    pub(super) fn close_work(&self) -> CloseWork {
        CloseWork {
            baseline: self.last_persisted.borrow().clone()
                .unwrap_or_else(|| self.settings.borrow().clone()),
            desired: self.settings.borrow().clone(),
            loopback: self.loopback.borrow_mut().take(),
        }
    }
''')
    shell = 'src/ui/mic_shell.rs'
    replace(shell, '    CloseRequested,', '''    CloseRequested,
    CloseSaveRetry,
    CloseWithoutSaving,
    CloseCancelled,''')
    replace(shell, '    MonitorStopped,', '''    MonitorStopped,
    ClosePreferencesSaved(Result<(), String>),''')
    replace(shell, '    is_closing: bool,', '''    is_closing: bool,
    close_save_in_flight: bool,''')
    replace(shell, '            is_closing: false,', '''            is_closing: false,
            close_save_in_flight: false,''')
    replace(shell, '''        if self.is_closing {
            return;
        }
        match message {''', '''        if self.is_closing && !matches!(message,
            MicInput::CloseSaveRetry | MicInput::CloseWithoutSaving | MicInput::CloseCancelled) {
            return;
        }
        match message {''')
    replace(shell, '''            MicInput::CloseRequested => self.close(widgets, root, &sender),''', '''            MicInput::CloseRequested => self.close(widgets, root, &sender),
            MicInput::CloseSaveRetry => self.save_before_close(&sender),
            MicInput::CloseWithoutSaving => self.stop_monitor(widgets, &sender),
            MicInput::CloseCancelled => {
                self.is_closing = false;
                self.applies = ApplyTracker::default();
                self.should_probe_after_apply = true;
                widgets.banner.set_title(&i18n("Changes have not been saved. You can keep editing or close without saving."));
                widgets.banner.set_button_label(None);
                widgets.banner.set_revealed(true);
                widgets.body.set_sensitive(true);
                self.banner_action = BannerAction::None;
            }''')
    replace(shell, '''                let result = self.state.finish_apply(*completion);''', '''                let result = self.state.finish_apply(*completion);
                if self.is_closing {
                    // Finish the worker already running, but never gate closing
                    // on an audio retry or dispatch another queued graph update.
                    self.save_before_close(&sender);
                    return;
                }''')
    text = Path(shell).read_text()
    start = text.index('                if self.is_closing {\n                    match result {')
    end = text.index('\n                match result {', start+1)
    text = text[:start] + text[end:]
    Path(shell).write_text(text)
    replace(shell, '''            MicCommandOutput::MonitorStopped => {''', '''            MicCommandOutput::ClosePreferencesSaved(result) => {
                self.close_save_in_flight = false;
                if !self.is_closing { return; }
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
                        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
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
            MicCommandOutput::MonitorStopped => {''')
    text = Path(shell).read_text()
    start = text.index('    fn close(\n')
    end = text.index('    fn stop_monitor(', start)
    text = text[:start] + '''    fn close(
        &mut self,
        widgets: &MicShellWidgets,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        if self.is_closing { return; }
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
        if self.close_save_in_flight { return; }
        self.close_save_in_flight = true;
        let work = self.state.close_work();
        sender.oneshot_command(async move {
            let result = relm4::spawn_blocking(move || work.run()).await
                .unwrap_or_else(|error| Err(error.to_string()));
            MicCommandOutput::ClosePreferencesSaved(result)
        });
    }

''' + text[end:]
    Path(shell).write_text(text)
    # Failure banners explain the problem but leave controls available to
    # disable a broken optional model or select a working one.
    replace(shell, '''        widgets.body.set_sensitive(false);
    }

    fn show_checking''', '''        widgets.body.set_sensitive(true);
    }

    fn show_checking''')
    replace(shell, '''                widgets.body.set_sensitive(false);
            }
        }
    }
}''', '''                widgets.body.set_sensitive(true);
            }
        }
    }
}''')
    commit(TITLE, [state, shell])
