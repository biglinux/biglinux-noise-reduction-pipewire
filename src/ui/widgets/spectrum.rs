//! Premium spectrum analyser widget.
//!
//! Key design decisions that give it the "award-winning" feel:
//!
//! * **30 bands, 3-zone gradient** — green (−60 … −20 dB), orange
//!   (−20 … −10 dB), red (−10 … 0 dB). A dark-shade version of the
//!   same gradient renders as a background track so empty bars stay
//!   readable and keep the zone hint visible.
//! * **Sticky per-band peaks** — each bar draws a thin peak tick that
//!   holds for ~0.7 s then decays linearly.
//! * **30 Hz smoothing loop** — audio-monitor frames arrive at ~94 Hz;
//!   the widget interpolates current → target at 70 % per frame so
//!   transients stay responsive with less redraw work.
//! * **Horizontal peak meter** — LEVEL / PEAK numeric readout plus a
//!   bar with ruler marks every 10 dB and its own peak-hold indicator.
//! * **Segmented bars** — each column is cut every 10 dB, giving the
//!   classic LED-stack look without actually running many widgets.
//!
//! The widget owns its animation timer through an internal
//! `Rc<RefCell<SpectrumState>>`, so multiple [`Spectrum::push_frame`] calls
//! only update `target_*` fields while the timer handles the rest.

mod constants;
mod rendering;
mod state;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glib::SourceId;
use gtk::prelude::*;

use crate::services::audio_monitor::SpectrumFrame;
use crate::ui::i18n::i18n;

#[allow(unused_imports)]
pub use constants::BAND_COUNT;
use constants::{ANIMATION_FPS, WIDGET_HEIGHT};
use rendering::draw;
use state::SpectrumState;

const PEAK_METER_CAPTION_MSGID: &str = "LEVEL / PEAK";

#[cfg(test)]
use constants::{
    METER_HOLD_DECAY, METER_HOLD_TICKS, METER_PEAK_DECAY, PEAK_HOLD_TICKS, SMOOTH_FACTOR,
};
#[cfg(test)]
use state::{db_to_norm, resampled_band_targets};

/// Public handle. Hold one per window.
pub struct Spectrum {
    area: gtk::DrawingArea,
    root: gtk::Box,
    level: gtk::LevelBar,
    readout: gtk::Label,
    last_meter_update: Cell<Option<std::time::Instant>>,
    state: Rc<RefCell<SpectrumState>>,
    timer: Cell<Option<SourceId>>,
}

impl Spectrum {
    #[must_use]
    pub fn new() -> Rc<Self> {
        let area = gtk::DrawingArea::builder()
            .content_height(WIDGET_HEIGHT)
            .hexpand(true)
            .build();
        area.set_size_request(-1, WIDGET_HEIGHT);
        area.set_accessible_role(gtk::AccessibleRole::Presentation);
        area.update_property(&[
            gtk::accessible::Property::Label(&i18n("Microphone level meter")),
            gtk::accessible::Property::Description(&i18n(
                "Shows the live microphone input level and peak.",
            )),
        ]);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let readout = gtk::Label::builder().xalign(0.0).wrap(true).build();
        readout.set_text(&i18n("Speak normally to check your microphone level."));
        let level = gtk::LevelBar::for_interval(-60.0, 0.0);
        level.set_value(-60.0);
        level.add_offset_value("low", -30.0);
        level.add_offset_value("high", -6.0);
        level.add_offset_value("full", -1.0);
        level.update_property(&[
            gtk::accessible::Property::Label(&i18n("Microphone level meter")),
            gtk::accessible::Property::Description(&i18n(
                "Input level in decibels. Reduce microphone volume if it reaches zero.",
            )),
        ]);
        root.append(&readout);
        root.append(&level);
        root.append(&area);
        let state = Rc::new(RefCell::new(SpectrumState::default()));
        let peak_meter_caption = i18n(PEAK_METER_CAPTION_MSGID);

        // Draw callback reads the interpolated state.
        let draw_state = Rc::clone(&state);
        area.set_draw_func(move |_, cairo_context, w, h| {
            draw(
                cairo_context,
                w,
                h,
                &draw_state.borrow(),
                &peak_meter_caption,
            );
        });

        let widget = Rc::new(Self {
            area,
            root,
            level,
            readout,
            last_meter_update: Cell::new(None),
            state,
            timer: Cell::new(None),
        });
        widget.start_animation();
        widget
    }

    /// GTK widget handle for embedding in a container.
    #[must_use]
    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Push a new frame from the audio monitor. Only stores the target
    /// values — the 30 Hz timer drives the interpolation.
    pub fn push_frame(&self, frame: &SpectrumFrame) {
        self.state.borrow_mut().update_targets(frame);
        let now = std::time::Instant::now();
        if self
            .last_meter_update
            .get()
            .is_none_or(|previous| now.duration_since(previous) >= Duration::from_millis(250))
        {
            self.last_meter_update.set(Some(now));
            let rms = finite_db(frame.rms_db);
            let peak = finite_db(frame.peak_db);
            self.level.set_value(f64::from(rms));
            let text = i18n("Level: {level} dB · Peak: {peak} dB")
                .replace("{level}", &format!("{rms:.0}"))
                .replace("{peak}", &format!("{peak:.0}"));
            self.readout.set_text(&text);
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn target_peak_for_contract(&self) -> f32 {
        self.state.borrow().target_peak
    }

    /// Install the 30 Hz interpolation timer. Called once from `new`.
    fn start_animation(self: &Rc<Self>) {
        let period = Duration::from_millis(u64::from(1000 / ANIMATION_FPS.max(1)));
        // Weak, NOT strong: a strong capture in a repeating glib timer keeps
        // the widget alive forever — `Drop` (which removes the timer) can
        // never run, so window close leaks the whole spectrum subtree.
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local(period, move || match weak.upgrade() {
            Some(me) => {
                me.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        self.timer.set(Some(id));
    }

    fn tick(&self) {
        // Skip the entire animation pass while the widget is off-screen
        // (different page in the view stack, window minimised, etc.).
        // The pw-cat capture path is paused in tandem from window.rs so
        // there is nothing meaningful to interpolate towards anyway.
        if !self.area.is_mapped() {
            return;
        }
        let animate =
            gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations());
        if !animate {
            self.area.queue_draw();
            return;
        }
        if self.state.borrow_mut().advance_animation(true) {
            self.area.queue_draw();
        }
    }
}

fn finite_db(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-60.0, 0.0)
    } else {
        -60.0
    }
}

impl Drop for Spectrum {
    fn drop(&mut self) {
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }
}

#[cfg(test)]
mod tests;
