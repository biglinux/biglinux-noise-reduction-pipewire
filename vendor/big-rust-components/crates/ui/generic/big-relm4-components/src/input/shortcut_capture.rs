// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Keyboard shortcut capture dialog.

use adw::prelude::*;
use relm4::gtk;
use relm4::gtk::{gdk, glib};
use std::cell::RefCell;
use std::rc::Rc;

/// Big Shortcut Validator alias.
pub type BigShortcutValidator = dyn Fn(&str) -> Option<String>;

fn no_shortcut_validation(_: &str) -> Option<String> {
    None
}

/// Localized strings the shortcut capture dialog needs at build
/// time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShortcutCaptureLabels {
    /// Title.
    pub title: String,
    /// Instruction.
    pub instruction: String,
    /// Waiting.
    pub waiting: String,
    /// Reset to default.
    pub reset_to_default: String,
}

impl BigShortcutCaptureLabels {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        instruction: impl Into<String>,
        waiting: impl Into<String>,
        reset_to_default: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            instruction: instruction.into(),
            waiting: waiting.into(),
            reset_to_default: reset_to_default.into(),
        }
    }
}

/// Closure bag wiring the shortcut capture dialog to the host
/// (binding formatting, validation, reset, apply).
pub struct BigShortcutCaptureHandlers<FK, FF, FR, FA> {
    /// Key to binding.
    pub key_to_binding: FK,
    /// Format binding.
    pub format_binding: FF,
    /// Validate binding.
    pub validate_binding: Rc<BigShortcutValidator>,
    /// On reset.
    pub on_reset: FR,
    /// On apply.
    pub on_apply: FA,
}

/// Present the shortcut capture dialog surface.
pub fn show_shortcut_capture_dialog<FK, FF, FR, FA>(
    transient_for: &impl IsA<gtk::Window>,
    display_name: &str,
    labels: BigShortcutCaptureLabels,
    key_to_binding: FK,
    format_binding: FF,
    on_reset: FR,
    on_apply: FA,
) where
    FK: Fn(gdk::Key, gdk::ModifierType) -> String + 'static,
    FF: Fn(&str) -> String + 'static,
    FR: Fn() + 'static,
    FA: Fn(String) + 'static,
{
    show_shortcut_capture_dialog_with_validator(
        transient_for,
        display_name,
        labels,
        BigShortcutCaptureHandlers {
            key_to_binding,
            format_binding,
            validate_binding: Rc::new(no_shortcut_validation),
            on_reset,
            on_apply,
        },
    );
}

/// Present the shortcut capture dialog with validator surface.
pub fn show_shortcut_capture_dialog_with_validator<FK, FF, FR, FA>(
    transient_for: &impl IsA<gtk::Window>,
    display_name: &str,
    labels: BigShortcutCaptureLabels,
    handlers: BigShortcutCaptureHandlers<FK, FF, FR, FA>,
) where
    FK: Fn(gdk::Key, gdk::ModifierType) -> String + 'static,
    FF: Fn(&str) -> String + 'static,
    FR: Fn() + 'static,
    FA: Fn(String) + 'static,
{
    let capture_dialog = adw::Window::builder()
        .title(&labels.title)
        .default_width(360)
        .default_height(200)
        .transient_for(transient_for)
        .modal(true)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    let title = gtk::Label::builder().label(display_name).build();
    title.add_css_class("title-3");
    content.append(&title);

    let instruction = gtk::Label::builder().label(&labels.instruction).build();
    instruction.add_css_class("dim-label");
    content.append(&instruction);

    let current_display = gtk::Label::builder().label(&labels.waiting).build();
    current_display.add_css_class("monospace");
    current_display.add_css_class("title-2");
    content.append(&current_display);

    let validation_label = gtk::Label::builder().visible(false).wrap(true).build();
    validation_label.add_css_class("error");
    content.append(&validation_label);

    let button_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .spacing(8)
        .margin_top(8)
        .build();
    let reset_button = gtk::Button::builder()
        .label(&labels.reset_to_default)
        .build();
    button_box.append(&reset_button);
    content.append(&button_box);

    toolbar_view.set_content(Some(&content));
    capture_dialog.set_content(Some(&toolbar_view));

    let captured_binding: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let BigShortcutCaptureHandlers {
        key_to_binding,
        format_binding,
        validate_binding,
        on_reset,
        on_apply,
    } = handlers;
    let key_to_binding: Rc<dyn Fn(gdk::Key, gdk::ModifierType) -> String> = Rc::new(key_to_binding);
    let format_binding: Rc<dyn Fn(&str) -> String> = Rc::new(format_binding);

    let key_ctrl = gtk::EventControllerKey::new();
    {
        let captured_binding = captured_binding.clone();
        let current_display = current_display.clone();
        let validation_label = validation_label.clone();
        let key_to_binding = key_to_binding.clone();
        let format_binding = format_binding.clone();
        let validate_binding = validate_binding.clone();
        key_ctrl.connect_key_pressed(move |_, keyval, _keycode, state| {
            if is_modifier_key(keyval) {
                return glib::Propagation::Proceed;
            }
            let binding = key_to_binding(keyval, state);
            current_display.set_label(&(format_binding)(&binding));
            if let Some(message) = validate_binding(&binding) {
                validation_label.set_label(&message);
                validation_label.set_visible(true);
            } else {
                validation_label.set_visible(false);
            }
            *captured_binding.borrow_mut() = Some(binding);
            glib::Propagation::Stop
        });
    }
    capture_dialog.add_controller(key_ctrl);

    {
        let dialog_weak = capture_dialog.downgrade();
        reset_button.connect_clicked(move |_| {
            on_reset();
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });
    }

    capture_dialog.connect_close_request(move |_| {
        if let Some(binding) = captured_binding.borrow().as_ref() {
            on_apply(binding.clone());
        }
        glib::Propagation::Proceed
    });

    capture_dialog.present();
}

/// Returns `true` if modifier key.
#[must_use]
pub fn is_modifier_key(keyval: gdk::Key) -> bool {
    matches!(
        keyval,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_keys_are_ignored() {
        assert!(is_modifier_key(gdk::Key::Shift_L));
        assert!(is_modifier_key(gdk::Key::Control_R));
        assert!(is_modifier_key(gdk::Key::Alt_L));
        assert!(is_modifier_key(gdk::Key::Super_R));
        assert!(!is_modifier_key(gdk::Key::a));
    }
}
