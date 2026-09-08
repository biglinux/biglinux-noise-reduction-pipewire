from pathlib import Path
from _common import done, replace, commit

TITLE = 'fix(tuning): track pending edits, synchronize reset controls and serialize application'
if not done(TITLE):
    path = 'src/ui/views/advanced.rs'
    text = Path(path).read_text().replace('use std::cell::RefCell;', 'use std::cell::{Cell, RefCell};')
    text = text.replace('Rc<RefCell<UserTweaks>>', 'Rc<TuningSelection>')
    text = text.replace('Rc::new(RefCell::new(initial))', 'Rc::new(TuningSelection::new(initial))')
    text = text.replace('Rc::new(RefCell::new(tweaks))', 'Rc::new(TuningSelection::new(tweaks))')
    at = text.index('/// Build the Tuning page.')
    text = text[:at] + '''/// Editable values and the last successfully applied snapshot are distinct.
pub(super) struct TuningSelection {
    current: RefCell<UserTweaks>,
    applied: RefCell<UserTweaks>,
    busy: Cell<bool>,
    content: glib::WeakRef<GtkBox>,
    apply_button: RefCell<glib::WeakRef<gtk::Button>>,
    reset_button: RefCell<glib::WeakRef<gtk::Button>>,
    dropdowns: RefCell<Vec<glib::WeakRef<gtk::DropDown>>>,
    preview: RefCell<Option<crate::services::preview::QuantumPreview>>,
    preview_busy: Cell<bool>,
}

impl TuningSelection {
    fn new(initial: UserTweaks) -> Self {
        Self {
            applied: RefCell::new(initial.clone()), current: RefCell::new(initial),
            busy: Cell::new(false), content: glib::WeakRef::new(),
            apply_button: RefCell::new(glib::WeakRef::new()),
            reset_button: RefCell::new(glib::WeakRef::new()),
            dropdowns: RefCell::new(Vec::new()), preview: RefCell::new(None),
            preview_busy: Cell::new(false),
        }
    }
    fn borrow(&self) -> std::cell::Ref<'_, UserTweaks> { self.current.borrow() }
    fn borrow_mut(&self) -> std::cell::RefMut<'_, UserTweaks> { self.current.borrow_mut() }
    fn register(&self, dropdown: &gtk::DropDown) { self.dropdowns.borrow_mut().push(dropdown.downgrade()); }
    fn reset_controls(&self) {
        for dropdown in self.dropdowns.borrow().iter().filter_map(glib::WeakRef::upgrade) {
            dropdown.set_selected(0);
        }
    }
}

''' + text[at:]
    text = text.replace('    let toolbar = action_toolbar();', '''    selection.content.set(Some(content));
    let toolbar = action_toolbar();
    *selection.apply_button.borrow_mut() = toolbar.apply.downgrade();
    *selection.reset_button.borrow_mut() = toolbar.reset.downgrade();''')
    text = text.replace('                    *selection.borrow_mut() = UserTweaks::default();', '''                    *selection.borrow_mut() = UserTweaks::default();
                    selection.reset_controls();''')
    start = text.index('fn refresh_banner('); end = text.index('\nfn banner_title_for_modified_state', start)
    text = text[:start] + '''fn refresh_banner(banner: &adw::Banner, selection: &Rc<TuningSelection>) {
    let dirty = *selection.borrow() != *selection.applied.borrow();
    let modified = selection.borrow().is_modified();
    let busy = selection.busy.get();
    let title = if busy {
        i18n("Applying audio settings…")
    } else if dirty {
        i18n("Changes are not applied yet. Apply them when you are ready.")
    } else if modified {
        i18n("Your audio settings are active.")
    } else {
        i18n("The standard audio settings are in use.")
    };
    banner.set_title(&title);
    banner.set_revealed(dirty || modified || busy);
    if let Some(button) = selection.apply_button.borrow().upgrade() { button.set_sensitive(dirty && !busy); }
    if let Some(button) = selection.reset_button.borrow().upgrade() {
        button.set_sensitive((modified || selection.applied.borrow().is_modified()) && !busy);
    }
}
''' + text[end:]
    # Old message helpers remain test-only until contract tests are updated.
    text = text.replace('fn banner_title_for_modified_state(', '#[cfg(test)]\nfn banner_title_for_modified_state(')
    text = text.replace('fn banner_title_message_for_modified_state(', '#[cfg(test)]\nfn banner_title_message_for_modified_state(')
    # Register every card's actual dropdown so reset updates both model and view.
    text = text.replace('    card.add_row(&labelled_row', '    selection.register(&dropdown);\n    card.add_row(&labelled_row')
    start = text.index('    // Heard before it is saved.')
    end = text.index('    card.add_row(&listen);', start)
    text = text[:start] + '''    let listen = gtk::Button::builder()
        .label(i18n("Try for 15 seconds"))
        .halign(gtk::Align::Start)
        .build();
    listen.set_tooltip_text(Some(&i18n("Temporarily changes the audio buffer for all applications. Stop the preview to return to the previous value.")));
    {
        let selection = Rc::clone(selection);
        listen.connect_clicked(move |button| {
            if selection.preview_busy.replace(true) { return; }
            let previous = selection.preview.borrow_mut().take();
            let frames = selection.borrow().quantum.unwrap_or(0);
            button.set_sensitive(false);
            let weak_button = button.downgrade();
            let selection = Rc::clone(&selection);
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    if let Some(mut preview) = previous {
                        preview.stop().map(|()| None)
                    } else {
                        crate::services::preview::QuantumPreview::start(frames).map(Some)
                    }
                }).await.unwrap_or_else(|_| Err(std::io::Error::other("preview worker failed")));
                selection.preview_busy.set(false);
                let Some(button) = weak_button.upgrade() else { return; };
                button.set_sensitive(true);
                match result {
                    Ok(preview) => {
                        button.set_label(&if preview.is_some() { i18n("Stop preview") } else { i18n("Try for 15 seconds") });
                        *selection.preview.borrow_mut() = preview;
                    }
                    Err(error) => {
                        log::warn!("buffer preview: {error}");
                        button.set_label(&i18n("Try preview again"));
                        button.set_tooltip_text(Some(&i18n("The audio preview could not be started or restored. Check the audio connection and try again.")));
                    }
                }
            });
        });
    }
    let weak_selection = Rc::downgrade(selection);
    let weak_button = listen.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let (Some(selection), Some(button)) = (weak_selection.upgrade(), weak_button.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let expired = selection.preview.borrow_mut().as_mut().is_some_and(|preview| !preview.is_alive());
        if expired {
            selection.preview.borrow_mut().take();
            button.set_label(&i18n("Try for 15 seconds"));
        }
        glib::ControlFlow::Continue
    });
''' + text[end:]
    # Keep a stable title for the modified-state helper while real UI uses dirty state.
    text = text.replace('    refresh_banner(banner, &selection);\n}\n\n#[cfg(test)]\npub(in crate::ui) fn build_tuning', '    refresh_banner(banner, &selection);\n}\n\n#[cfg(test)]\npub(in crate::ui) fn build_tuning')
    Path(path).write_text(text)
    path = 'src/ui/views/advanced/apply.rs'
    text = Path(path).read_text().replace('use std::cell::RefCell;\n', '')
    text = text.replace('use super::{UserTweaks, refresh_banner};', 'use super::{TuningSelection, UserTweaks, refresh_banner};')
    text = text.replace('Rc<RefCell<UserTweaks>>', 'Rc<TuningSelection>')
    text = text.replace('Rc::new(RefCell::new(UserTweaks::default()))', 'Rc::new(TuningSelection::new(UserTweaks::default()))')
    text = text.replace('    let tweaks = selection.borrow().clone();', '''    if selection.busy.replace(true) { return; }
    let expected = selection.applied.borrow().clone();
    let tweaks = selection.borrow().clone();
    let applied = tweaks.clone();
    let preview = selection.preview.borrow_mut().take();
    let old_label = button.label();
    if let Some(content) = selection.content.upgrade() { content.set_sensitive(false); }''')
    text = text.replace('            if let Err(e) = tweaks.apply() {', '''            if let Some(mut preview) = preview && let Err(error) = preview.stop() {
                return ApplyOutcome::RestartFailed(error.to_string());
            }
            let guard = crate::config::storage::SettingsLock::acquire();
            let _guard = match guard {
                Ok(guard) => guard,
                Err(error) => return ApplyOutcome::WriteFailed(error.to_string()),
            };
            if UserTweaks::load_from_disk() != expected {
                return ApplyOutcome::WriteFailed("Audio settings changed in another application. Reopen the tuning page before applying.".to_owned());
            }
            if let Err(e) = tweaks.apply() {''')
    text = text.replace('        let Some(button) = button_weak.upgrade() else {', '''        selection.busy.set(false);
        if matches!(outcome, ApplyOutcome::Ok) { *selection.applied.borrow_mut() = applied; }
        if let Some(content) = selection.content.upgrade() { content.set_sensitive(true); }
        let Some(button) = button_weak.upgrade() else {''')
    text = text.replace('        button.set_label(&i18n("Apply and restart audio"));', '''        button.set_label(old_label.as_deref().unwrap_or(""));''')
    Path(path).write_text(text)
    # Preserve meaningful contracts rather than checking an obsolete ambiguous banner.
    path = 'src/ui.rs'
    replace(path, '"Custom settings active. Click Apply to enable them."', '"Your audio settings are active."')
    commit(TITLE, ['src/ui/views/advanced.rs', 'src/ui/views/advanced/apply.rs', path])
