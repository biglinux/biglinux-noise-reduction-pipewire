from pathlib import Path
from _common import done, replace, commit

TITLE = 'fix(ui): preserve tuning drafts, active tabs and focus during synchronization'
if not done(TITLE):
    path = 'src/ui/state.rs'
    replace(path, 'pub struct AppState {', '''pub struct AppState {
    // The tuning page owns an independent draft and no Rc<AppState>. Keeping
    // this widget avoids losing unapplied choices on external JSON updates.
    tuning_page: RefCell<Option<gtk::Widget>>,
    active_page: RefCell<String>,''')
    replace(path, '        Rc::new(Self {', '''        Rc::new(Self {
            tuning_page: RefCell::new(None),
            active_page: RefCell::new("mic".to_owned()),''')
    replace(path, 'impl AppState {', '''impl AppState {
    pub(super) fn tuning_page(&self) -> gtk::Widget {
        self.tuning_page.borrow_mut().get_or_insert_with(super::views::advanced::build).clone()
    }
    pub(super) fn active_page(&self) -> String { self.active_page.borrow().clone() }
    pub(super) fn remember_page(&self, name: &str) { *self.active_page.borrow_mut() = name.to_owned(); }
''')
    path = 'src/ui/window.rs'
    text = Path(path).read_text().replace('Mode, advanced, mic, output, simple', 'Mode, mic, output, simple')
    text = text.replace('    while let Some(child) = body.first_child() {', '''    let previous_focus = body.root().and_then(|root| root.focus());
    let focus_path = previous_focus.as_ref().and_then(|focus| child_path(body.upcast_ref(), focus));
    let scroll_positions = collect_scroll_positions(body.upcast_ref());
    let tuning = state.tuning_page();
    if let Some(old_stack) = body.first_child().and_downcast::<adw::ViewStack>() {
        if let Some(name) = old_stack.visible_child_name() { state.remember_page(&name); }
        if tuning.parent().as_ref() == Some(old_stack.upcast_ref()) { old_stack.remove(&tuning); }
    }
    while let Some(child) = body.first_child() {''', 1)
    text = text.replace('                &advanced::build(),', '                &tuning,')
    text = text.replace('.policy(adw::ViewSwitcherPolicy::Wide)', '.policy(adw::ViewSwitcherPolicy::Narrow)')
    text = text.replace('            sync_spectrum_visibility(spectrum_container, &stack);', '''            stack.set_visible_child_name(&state.active_page());
            sync_spectrum_visibility(spectrum_container, &stack);''', 1)
    text = text.replace('                let spectrum_container = spectrum_container.clone();', '''                let spectrum_container = spectrum_container.clone();
                let weak_state = Rc::downgrade(state);''', 1)
    text = text.replace('                    sync_spectrum_visibility(&spectrum_container, stack);', '''                    if let (Some(state), Some(name)) = (weak_state.upgrade(), stack.visible_child_name()) {
                        state.remember_page(&name);
                    }
                    sync_spectrum_visibility(&spectrum_container, stack);''', 1)
    ending = '''            body.append(&stack);
        }
    }
}'''
    assert ending in text
    text = text.replace(ending, '''            body.append(&stack);
        }
    }
    let weak_body = body.downgrade();
    glib::idle_add_local_once(move || {
        let Some(body) = weak_body.upgrade() else { return; };
        for (path, value) in scroll_positions {
            if let Some(scroll) = child_at(body.upcast_ref(), &path).and_downcast::<gtk::ScrolledWindow>() {
                scroll.vadjustment().set_value(value);
            }
        }
        if let Some(focus) = previous_focus.filter(|focus| focus.is_ancestor(&body)) {
            focus.grab_focus();
        } else if let Some((path, widget_type)) = focus_path {
            if let Some(widget) = child_at(body.upcast_ref(), &path).filter(|widget| widget.type_() == widget_type) {
                widget.grab_focus();
            }
        }
    });
}''', 1)
    pos = text.index('/// The spectrum reflects')
    text = text[:pos] + '''fn child_path(root: &gtk::Widget, target: &gtk::Widget) -> Option<(Vec<usize>, glib::Type)> {
    let mut path = Vec::new();
    let mut widget = target.clone();
    while widget != *root {
        let parent = widget.parent()?;
        let mut child = parent.first_child();
        let mut index = 0;
        while child.as_ref() != Some(&widget) {
            child = child?.next_sibling();
            index += 1;
        }
        path.push(index);
        widget = parent;
    }
    path.reverse();
    Some((path, target.type_()))
}

fn child_at(root: &gtk::Widget, path: &[usize]) -> Option<gtk::Widget> {
    let mut widget = root.clone();
    for &index in path {
        let mut child = widget.first_child()?;
        for _ in 0..index { child = child.next_sibling()?; }
        widget = child;
    }
    Some(widget)
}

fn collect_scroll_positions(root: &gtk::Widget) -> Vec<(Vec<usize>, f64)> {
    fn visit(widget: &gtk::Widget, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, f64)>) {
        if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
            out.push((path.clone(), scroll.vadjustment().value()));
        }
        let mut child = widget.first_child();
        let mut index = 0;
        while let Some(widget) = child {
            path.push(index);
            visit(&widget, path, out);
            path.pop();
            child = widget.next_sibling();
            index += 1;
        }
    }
    let mut positions = Vec::new();
    visit(root, &mut Vec::new(), &mut positions);
    positions
}

''' + text[pos:]
    Path(path).write_text(text)
    path = 'src/ui/mic_shell.rs'
    text = Path(path).read_text().replace('        header.set_decoration_layout(Some(":minimize,maximize,close"));\n', '')
    # A result that was in flight when close began must not start queued work.
    start = text.index('            MicCommandOutput::HealthResolved { request, health } => {')
    text = text[:start] + text[start:].replace('                if let Some(next) = tracking.next {', '                if !self.is_closing && let Some(next) = tracking.next {', 1)
    Path(path).write_text(text)
    commit(TITLE, ['src/ui/state.rs', 'src/ui/window.rs', path])
