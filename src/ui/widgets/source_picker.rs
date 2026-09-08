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

use adw::prelude::*;
use gtk::{Box as GtkBox, gio, glib};

use crate::services::pipewire::{Source, set_default_source, set_source_volume, snapshot_sources};

use super::super::i18n::i18n;
use super::didactic::{labelled_row, slider_row};

/// Re-snapshot interval. Short enough to feel live when plugging a USB
/// mic, long enough to keep `pw-cli` overhead negligible.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Trailing-edge debounce for the volume slider. A drag fires
/// `value_changed` dozens of times per second; coalescing to the last
/// value keeps `wpctl set-volume` off the live drag path (one
/// subprocess per settle, not per tick) and avoids out-of-order
/// completions leaving a stale level.
const VOLUME_DEBOUNCE: Duration = Duration::from_millis(150);

/// Two stacked rows ready to drop into any card. The dropdown row is
/// hidden when only a single source is visible — re-shown by the
/// background poller as soon as a second mic shows up.
pub struct PickerRows {
    pub dropdown_row: GtkBox,
    pub volume_row: GtkBox,
}

/// Build the rows. `volume_for` is injected so callers can stub it in
/// tests; production wires
/// [`crate::services::pipewire::source_volume`].
pub fn build_rows(volume_for: impl Fn(u32) -> Option<f32> + 'static) -> PickerRows {
    // Nothing is asked of PipeWire here. `snapshot_sources` shells out to
    // `pw-cli` and `pw-metadata` three to six times, and this runs while the
    // window is being built -- before the first frame, so the panel stayed
    // blank for as long as PipeWire took to answer. The rows start empty and
    // the first refresh fills them, which is the path every later change
    // already takes.
    let sources: Vec<Source> = Vec::new();
    let default_id = None;
    let active = Rc::new(Cell::new(default_id));
    let sources_state = Rc::new(RefCell::new(sources.clone()));
    // Set whenever a programmatic change must not echo back into the
    // wpctl wiring (rebuilding the model, swapping the active source).
    let suppress = Rc::new(Cell::new(false));

    let dropdown = build_dropdown(&sources);
    select_initial(&dropdown, &sources, default_id);

    // Unity until the first refresh says otherwise: reading the real volume is
    // another blocking `wpctl` call, and `apply_refresh` sets it below.
    let initial_vol = 1.0;
    let (vol_row, vol_adj) = build_volume_row(initial_vol);
    let error_banner = adw::Banner::builder().revealed(false).build();

    let volume_for = Rc::new(volume_for);
    // Shared trailing-debounce timer for the volume slider — also cancelled on
    // a source switch so a half-finished drag for the source being left never
    // lands after the user picked a different one.
    let vol_pending: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    wire_dropdown_change(
        &dropdown,
        &sources_state,
        &active,
        &vol_adj,
        &volume_for,
        &suppress,
        &vol_pending,
        &error_banner,
    );
    wire_volume_change(&vol_adj, &active, &suppress, &vol_pending, &error_banner);

    let dropdown_row = GtkBox::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    let microphone_row = labelled_row(&i18n("Microphone"), &dropdown);
    dropdown_row.append(&error_banner);
    dropdown_row.append(&microphone_row);
    microphone_row.set_visible(sources.len() > 1);

    refresh_once(
        dropdown.clone(),
        microphone_row.clone(),
        vol_adj.clone(),
        Rc::clone(&sources_state),
        Rc::clone(&active),
        Rc::clone(&volume_for),
        Rc::clone(&suppress),
    );
    spawn_refresh_poller(
        dropdown.clone(),
        microphone_row,
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
    vol_pending: &Rc<Cell<Option<glib::SourceId>>>,
    error_banner: &adw::Banner,
) where
    F: Fn(u32) -> Option<f32> + 'static,
{
    let sources = Rc::clone(sources);
    let active = Rc::clone(active);
    let vol_adj = vol_adj.clone();
    let volume_for = Rc::clone(volume_for);
    let suppress = Rc::clone(suppress);
    let vol_pending = Rc::clone(vol_pending);
    let error_banner = error_banner.clone();
    dropdown.connect_selected_notify(move |dd| {
        if suppress.get() {
            return;
        }
        let idx = dd.selected() as usize;
        let picked_id = match sources.borrow().get(idx) {
            Some(s) => s.node_id,
            None => return,
        };
        let previous_id = active.get();
        if previous_id == Some(picked_id) {
            return;
        }
        // Cancel a pending volume write for the source we're leaving — its
        // value is about to be replaced by the new source's, and the
        // suppressed set_value below would otherwise let that timer fire.
        if let Some(timer) = vol_pending.take() {
            timer.remove();
        }
        // Optimistically record the pick so a rapid second selection
        // supersedes this one: the async continuation only applies its
        // volume side-effect when `active` still points at `picked_id`.
        active.set(Some(picked_id));
        let active = Rc::clone(&active);
        let dropdown = dd.clone();
        let sources = Rc::clone(&sources);
        let vol_adj = vol_adj.clone();
        let volume_for = Rc::clone(&volume_for);
        let suppress = Rc::clone(&suppress);
        let error_banner = error_banner.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || set_default_source(picked_id))
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("worker thread panicked")));
            if let Err(e) = result {
                log::warn!("source picker: set-default failed: {e}");
                error_banner.set_title(&i18n(
                    "Could not switch microphone. Try again or run diagnostics.",
                ));
                error_banner.set_revealed(true);
                if active.get() == Some(picked_id) {
                    active.set(previous_id);
                    if let Some(previous_id) = previous_id
                        && let Some(idx) = sources
                            .borrow()
                            .iter()
                            .position(|source| source.node_id == previous_id)
                    {
                        suppress.set(true);
                        dropdown.set_selected(u32::try_from(idx).unwrap_or(0));
                        let vol = (volume_for)(previous_id).unwrap_or(1.0);
                        vol_adj.set_value((f64::from(vol) * 100.0).clamp(0.0, 150.0));
                        suppress.set(false);
                    }
                }
                return;
            }
            error_banner.set_revealed(false);
            // A newer selection may have landed while the subprocess ran;
            // don't clobber its volume with this stale one.
            if active.get() != Some(picked_id) {
                return;
            }
            let vol = (volume_for)(picked_id).unwrap_or(1.0);
            suppress.set(true);
            vol_adj.set_value((f64::from(vol) * 100.0).clamp(0.0, 150.0));
            suppress.set(false);
        });
    });
}

fn wire_volume_change(
    vol_adj: &gtk::Adjustment,
    active: &Rc<Cell<Option<u32>>>,
    suppress: &Rc<Cell<bool>>,
    pending: &Rc<Cell<Option<glib::SourceId>>>,
    error_banner: &adw::Banner,
) {
    let active = Rc::clone(active);
    let suppress = Rc::clone(suppress);
    // Pending trailing-debounce timer (shared with the dropdown so a source
    // switch can cancel it); replaced on every tick so only the final value
    // reaches `wpctl`.
    let pending = Rc::clone(pending);
    let error_banner = error_banner.clone();
    vol_adj.connect_value_changed(move |a| {
        if suppress.get() {
            return;
        }
        let Some(id) = active.get() else {
            return;
        };
        let v = (a.value() / 100.0).clamp(0.0, 1.5) as f32;

        if let Some(source) = pending.take() {
            source.remove();
        }
        let pending_inner = Rc::clone(&pending);
        let error_banner = error_banner.clone();
        let source = glib::timeout_add_local_once(VOLUME_DEBOUNCE, move || {
            pending_inner.set(None);
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || set_source_volume(id, v))
                    .await
                    .unwrap_or_else(|_| Err(std::io::Error::other("worker thread panicked")));
                if let Err(e) = result {
                    log::warn!("source picker: set-volume failed: {e}");
                    error_banner.set_title(&i18n(
                        "Could not change microphone volume. Try again or run diagnostics.",
                    ));
                    error_banner.set_revealed(true);
                } else {
                    error_banner.set_revealed(false);
                }
            });
        });
        pending.set(Some(source));
    });
}

/// Poll the graph for hot-plug changes. Stops automatically once the
/// dropdown widget is dropped (window closed).
/// Ask PipeWire for the first time, without the window waiting for the answer.
#[allow(clippy::too_many_arguments)]
fn refresh_once<F>(
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
    glib::spawn_future_local(async move {
        let Ok((new_sources, new_default)) = gio::spawn_blocking(snapshot_sources).await else {
            return; // the poller asks again in two seconds
        };
        apply_refresh(
            &dropdown,
            &dropdown_row,
            &vol_adj,
            new_sources,
            new_default,
            &sources_state,
            &active,
            volume_for.as_ref(),
            &suppress,
        );
    });
}

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
    // Skip a tick while a snapshot is still in flight so a slow `pw-cli` can't
    // stack overlapping refreshes (and clobber each other's dropdown writes).
    let polling = Rc::new(Cell::new(false));
    glib::timeout_add_local(REFRESH_INTERVAL, move || {
        let (Some(dropdown), Some(row), Some(vol_adj)) = (
            dropdown_weak.upgrade(),
            dropdown_row_weak.upgrade(),
            vol_adj_weak.upgrade(),
        ) else {
            return glib::ControlFlow::Break;
        };
        if polling.get() {
            return glib::ControlFlow::Continue;
        }
        polling.set(true);
        let sources_state = Rc::clone(&sources_state);
        let active = Rc::clone(&active);
        let volume_for = Rc::clone(&volume_for);
        let suppress = Rc::clone(&suppress);
        let polling_done = Rc::clone(&polling);
        // `snapshot_sources` shells out to `pw-cli`/`pw-metadata`; run it on a
        // worker so the 2 s poll never blocks the GTK main loop, then apply the
        // (Send) result back on the main thread.
        glib::spawn_future_local(async move {
            let snapshot = gio::spawn_blocking(snapshot_sources).await;
            polling_done.set(false);
            let Ok((new_sources, new_default)) = snapshot else {
                return; // worker panicked — retry on the next tick
            };
            apply_refresh(
                &dropdown,
                &row,
                &vol_adj,
                new_sources,
                new_default,
                &sources_state,
                &active,
                volume_for.as_ref(),
                &suppress,
            );
        });
        glib::ControlFlow::Continue
    });
}

#[allow(clippy::too_many_arguments)]
fn apply_refresh<F>(
    dropdown: &gtk::DropDown,
    dropdown_row: &GtkBox,
    vol_adj: &gtk::Adjustment,
    new_sources: Vec<Source>,
    new_default: Option<u32>,
    sources_state: &Rc<RefCell<Vec<Source>>>,
    active: &Rc<Cell<Option<u32>>>,
    volume_for: &F,
    suppress: &Rc<Cell<bool>>,
) where
    F: Fn(u32) -> Option<f32>,
{
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

    if let Some(id) = target_id
        && let Some(idx) = new_sources.iter().position(|s| s.node_id == id)
    {
        let idx_u32 = u32::try_from(idx).unwrap_or(0);
        if dropdown.selected() != idx_u32 {
            suppress.set(true);
            dropdown.set_selected(idx_u32);
            suppress.set(false);
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
