//! Premium spectrum analyser widget.
//!
//! Ported from the BigLinux Microphone Python legacy. Key design
//! decisions that give it the "award-winning" feel:
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
//! * **Horizontal peak meter** — VAL / PEAK numeric readout plus a
//!   bar with ruler marks every 10 dB and its own peak-hold indicator.
//! * **Segmented bars** — each column is cut every 10 dB, giving the
//!   classic LED-stack look without actually running many widgets.
//!
//! The widget owns its animation timer through an internal
//! `Rc<RefCell<SpectrumState>>`, so multiple [`Spectrum::push_frame`] calls
//! only update `target_*` fields while the timer handles the rest.

mod constants;
mod geometry;
mod rendering;
mod state;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glib::SourceId;
use gtk::prelude::*;

use crate::services::audio_monitor::SpectrumFrame;

#[allow(unused_imports)]
pub use constants::BAND_COUNT;
use constants::{ANIMATION_FPS, WIDGET_HEIGHT};
use rendering::draw;
use state::SpectrumState;

#[cfg(test)]
use constants::{
    METER_HOLD_DECAY, METER_HOLD_TICKS, METER_PEAK_DECAY, PEAK_DECAY, PEAK_HOLD_TICKS,
    PEAK_METER_TICK_VALUES, SMOOTH_FACTOR,
};
#[cfg(test)]
use geometry::*;
#[cfg(test)]
use rendering::*;

/// Public handle. Hold one per window.
pub struct Spectrum {
    area: gtk::DrawingArea,
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

        let state = Rc::new(RefCell::new(SpectrumState::default()));

        // Draw callback reads the interpolated state.
        let draw_state = Rc::clone(&state);
        area.set_draw_func(move |_, cairo_context, w, h| {
            draw(cairo_context, w, h, &draw_state.borrow());
        });

        let widget = Rc::new(Self {
            area,
            state,
            timer: Cell::new(None),
        });
        widget.start_animation();
        widget
    }

    /// GTK widget handle for embedding in a container.
    #[must_use]
    pub fn widget(&self) -> &gtk::Widget {
        self.area.upcast_ref()
    }

    /// Push a new frame from the audio monitor. Only stores the target
    /// values — the 30 Hz timer drives the interpolation.
    pub fn push_frame(&self, frame: &SpectrumFrame) {
        push_frame_to_state(&self.state, frame);
    }

    #[cfg(test)]
    pub(in crate::ui) fn target_peak_for_contract(&self) -> f32 {
        self.state.borrow().target_peak
    }

    /// Install the 30 Hz interpolation timer. Called once from `new`.
    fn start_animation(self: &Rc<Self>) {
        let period = Duration::from_millis(animation_frame_period_millis().max(1));
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
        if should_skip_animation_tick(self.area.is_mapped()) {
            return;
        }
        if self.state.borrow_mut().advance_animation(true) {
            self.area.queue_draw();
        }
    }
}

fn animation_frame_period_millis() -> u64 {
    u64::from(1000 / ANIMATION_FPS.max(1))
}

fn should_skip_animation_tick(is_area_mapped: bool) -> bool {
    !is_area_mapped
}

fn push_frame_to_state(state: &Rc<RefCell<SpectrumState>>, frame: &SpectrumFrame) {
    state.borrow_mut().update_targets(frame);
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
