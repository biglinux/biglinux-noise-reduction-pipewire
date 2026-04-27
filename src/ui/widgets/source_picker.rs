//! Hardware microphone picker + volume slider rows.
//!
//! Returns a pair of `[label | control]` rows (dropdown and volume
//! slider) wired to `wpctl`. State lives in the PipeWire graph, not in
//! `settings.json` — picking a different source moves the **system**
//! default, so every other app follows along.
//!
//! A 2 s GLib timer polls the graph so a microphone plugged in
//! mid-session shows up without restarting the app, and the dropdown
//! row hides itself when only a single source is available (no point
//! presenting a one-item picker).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{glib, Box as GtkBox};

use crate::services::pipewire::{set_default_source, set_source_volume, snapshot_sources, Source};

use super::super::i18n::i18n;
use super::didactic::{labelled_row, slider_row};

/// Re-snapshot interval. Short enough to feel live when plugging a USB
/// mic, long enough to keep `pw-cli` overhead negligible.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Two stacked rows ready to drop into any card. The dropdown row is
/// hidden when only a single source is visible — re-shown by the
/// background poller as soon as a second mic shows up.
pub struct PickerRows {
    pub dropdown_row: GtkBox,
    pub volume_row: GtkBox,
}

/// Build the rows. `volume_for` is injected so callers can stub it in
/// tests; production wires
/// [`crate::services::pipewire::sources::source_volume`].
pub fn build_rows(volume_for: impl Fn(u32) -> Option<f32> + 'static) -> PickerRows {
    let (sources, default_id) = snapshot_sources();
    let active = Rc::new(Cell::new(default_id));
    let sources_state = Rc::new(RefCell::new(sources.clone()));
    // Set whenever a programmatic change must not echo back into the
    // wpctl wiring (rebuilding the model, swapping the active source).
    let suppress = Rc::new(Cell::new(false));

    let dropdown = build_dropdown(&sources);
    select_initial(&dropdown, &sources, default_id);

    let initial_vol = active.get().and_then(&volume_for).unwrap_or(1.0);
    let (vol_row, vol_adj) = build_volume_row(initial_vol);

    let volume_for = Rc::new(volume_for);
    wire_dropdown_change(
        &dropdown,
        &sources_state,
        &active,
        &vol_adj,
        &volume_for,
        &suppress,
    );
    wire_volume_change(&vol_adj, &active, &suppress);

    let dropdown_row = labelled_row(&i18n("Microphone"), &dropdown);
    dropdown_row.set_visible(sources.len() > 1);

    spawn_refresh_poller(
        dropdown.clone(),
        dropdown_row.clone(),
        vol_adj,
        sources_state,
        active,
        volume_for,
        suppress,
    );

    PickerRows {
        dropdown_row,
        volume_row: vol_row,
    }
}

fn build_dropdown(sources: &[Source]) -> gtk::DropDown {
    let labels: Vec<&str> = if sources.is_empty() {
        vec!["—"]
    } else {
        sources.iter().map(|s| s.description.as_str()).collect()
    };
    gtk::DropDown::from_strings(&labels)
}

fn select_initial(dropdown: &gtk::DropDown, sources: &[Source], default_id: Option<u32>) {
    let Some(id) = default_id else {
        return;
    };
    if let Some(idx) = sources.iter().position(|s| s.node_id == id) {
        dropdown.set_selected(u32::try_from(idx).unwrap_or(0));
    }
}

fn build_volume_row(initial: f32) -> (GtkBox, gtk::Adjustment) {
    let percent = (f64::from(initial) * 100.0).clamp(0.0, 150.0);
    let adj = gtk::Adjustment::new(percent, 0.0, 150.0, 1.0, 10.0, 0.0);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adj));
    scale.add_mark(100.0, gtk::PositionType::Bottom, None);
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);
    let row = slider_row(&i18n("Volume"), &scale, &spin);
    (row, adj)
}

fn wire_dropdown_change<F>(
    dropdown: &gtk::DropDown,
    sources: &Rc<RefCell<Vec<Source>>>,
    active: &Rc<Cell<Option<u32>>>,
    vol_adj: &gtk::Adjustment,
    volume_for: &Rc<F>,
    suppress: &Rc<Cell<bool>>,
) where
    F: Fn(u32) -> Option<f32> + 'static,
{
    let sources = Rc::clone(sources);
    let active = Rc::clone(active);
    let vol_adj = vol_adj.clone();
    let volume_for = Rc::clone(volume_for);
    let suppress = Rc::clone(suppress);
    dropdown.connect_selected_notify(move |dd| {
        if suppress.get() {
            return;
        }
        let idx = dd.selected() as usize;
        let picked_id = match sources.borrow().get(idx) {
            Some(s) => s.node_id,
            None => return,
        };
        if active.get() == Some(picked_id) {
            return;
        }
        if let Err(e) = set_default_source(picked_id) {
            log::warn!("source picker: set-default failed: {e}");
            return;
        }
        active.set(Some(picked_id));
        let vol = (volume_for)(picked_id).unwrap_or(1.0);
        suppress.set(true);
        vol_adj.set_value((f64::from(vol) * 100.0).clamp(0.0, 150.0));
        suppress.set(false);
    });
}

fn wire_volume_change(
    vol_adj: &gtk::Adjustment,
    active: &Rc<Cell<Option<u32>>>,
    suppress: &Rc<Cell<bool>>,
) {
    let active = Rc::clone(active);
    let suppress = Rc::clone(suppress);
    vol_adj.connect_value_changed(move |a| {
        if suppress.get() {
            return;
        }
        let Some(id) = active.get() else {
            return;
        };
        let v = (a.value() / 100.0).clamp(0.0, 1.5) as f32;
        if let Err(e) = set_source_volume(id, v) {
            log::warn!("source picker: set-volume failed: {e}");
        }
    });
}

/// Poll the graph for hot-plug changes. Stops automatically once the
/// dropdown widget is dropped (window closed).
fn spawn_refresh_poller<F>(
    dropdown: gtk::DropDown,
    dropdown_row: GtkBox,
    vol_adj: gtk::Adjustment,
    sources_state: Rc<RefCell<Vec<Source>>>,
    active: Rc<Cell<Option<u32>>>,
    volume_for: Rc<F>,
    suppress: Rc<Cell<bool>>,
) where
    F: Fn(u32) -> Option<f32> + 'static,
{
    let dropdown_weak = dropdown.downgrade();
    let dropdown_row_weak = dropdown_row.downgrade();
    let vol_adj_weak = vol_adj.downgrade();
    glib::timeout_add_local(REFRESH_INTERVAL, move || {
        let (Some(dropdown), Some(row), Some(vol_adj)) = (
            dropdown_weak.upgrade(),
            dropdown_row_weak.upgrade(),
            vol_adj_weak.upgrade(),
        ) else {
            return glib::ControlFlow::Break;
        };
        refresh(
            &dropdown,
            &row,
            &vol_adj,
            &sources_state,
            &active,
            volume_for.as_ref(),
            &suppress,
        );
        glib::ControlFlow::Continue
    });
}

fn refresh<F>(
    dropdown: &gtk::DropDown,
    dropdown_row: &GtkBox,
    vol_adj: &gtk::Adjustment,
    sources_state: &Rc<RefCell<Vec<Source>>>,
    active: &Rc<Cell<Option<u32>>>,
    volume_for: &F,
    suppress: &Rc<Cell<bool>>,
) where
    F: Fn(u32) -> Option<f32>,
{
    let (new_sources, new_default) = snapshot_sources();
    let mut current = sources_state.borrow_mut();
    let topology_changed = current.len() != new_sources.len()
        || current
            .iter()
            .zip(new_sources.iter())
            .any(|(a, b)| a.node_id != b.node_id);

    if topology_changed {
        let labels: Vec<&str> = if new_sources.is_empty() {
            vec!["—"]
        } else {
            new_sources.iter().map(|s| s.description.as_str()).collect()
        };
        suppress.set(true);
        dropdown.set_model(Some(&gtk::StringList::new(&labels)));
        suppress.set(false);
        dropdown_row.set_visible(new_sources.len() > 1);
    }

    // If the previously-active source vanished, follow whatever
    // WirePlumber promoted to default and refresh the volume slider.
    let active_id = active.get();
    let active_alive = active_id.is_some_and(|id| new_sources.iter().any(|s| s.node_id == id));
    let target_id = if active_alive {
        active_id
    } else {
        active.set(new_default);
        new_default
    };

    if let Some(id) = target_id {
        if let Some(idx) = new_sources.iter().position(|s| s.node_id == id) {
            let idx_u32 = u32::try_from(idx).unwrap_or(0);
            if dropdown.selected() != idx_u32 {
                suppress.set(true);
                dropdown.set_selected(idx_u32);
                suppress.set(false);
            }
        }
    }

    if !active_alive {
        let vol = active.get().and_then(volume_for).unwrap_or(1.0);
        suppress.set(true);
        vol_adj.set_value((f64::from(vol) * 100.0).clamp(0.0, 150.0));
        suppress.set(false);
    }

    *current = new_sources;
}
