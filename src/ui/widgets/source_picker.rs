//! Microphone controls with one serialized, coalescing worker lane.
//!
//! Every PipeWire query and volume read runs off the GTK main thread. A slow
//! command cannot reorder a newer selection, and polling cannot overwrite a
//! drag or an in-flight user action. External defaults and volume changes are
//! adopted on the next refresh, even while the previous device remains live.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use super::super::i18n::i18n;
use super::didactic::{labelled_row, slider_row};
use crate::services::pipewire::{Source, set_default_source, set_source_volume, snapshot_sources};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const VOLUME_DEBOUNCE: Duration = Duration::from_millis(150);
type VolumeReader = Arc<dyn Fn(u32) -> Option<f32> + Send + Sync>;

pub struct PickerRows {
    pub dropdown_row: gtk::Box,
    pub volume_row: gtk::Box,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Operation {
    Refresh,
    Select(u32),
    Volume(u32, f32),
}

#[derive(Default)]
struct Pending {
    source: Option<u32>,
    volume: Option<(u32, f32)>,
    refresh: bool,
}

impl Pending {
    fn select(&mut self, id: u32) {
        self.source = Some(id);
        self.volume = None;
    }

    fn has_write(&self) -> bool {
        self.source.is_some() || self.volume.is_some()
    }

    fn take(&mut self) -> Option<Operation> {
        if let Some(id) = self.source.take() {
            return Some(Operation::Select(id));
        }
        if let Some((id, value)) = self.volume.take() {
            return Some(Operation::Volume(id, value));
        }
        std::mem::take(&mut self.refresh).then_some(Operation::Refresh)
    }
}

struct Snapshot {
    sources: Vec<Source>,
    default: Option<u32>,
    volume: Option<f32>,
}

struct Picker {
    owner: glib::WeakRef<gtk::Box>,
    dropdown: glib::WeakRef<gtk::DropDown>,
    microphone_row: glib::WeakRef<gtk::Box>,
    volume_row: glib::WeakRef<gtk::Box>,
    adjustment: glib::WeakRef<gtk::Adjustment>,
    banner: glib::WeakRef<adw::Banner>,
    sources: RefCell<Vec<Source>>,
    active: Cell<Option<u32>>,
    suppress: Cell<bool>,
    busy: Cell<bool>,
    pending: RefCell<Pending>,
    poll: Cell<Option<glib::SourceId>>,
    debounce: Cell<Option<glib::SourceId>>,
    volume_for: VolumeReader,
}

pub fn build_rows(volume_for: impl Fn(u32) -> Option<f32> + Send + Sync + 'static) -> PickerRows {
    let dropdown = gtk::DropDown::from_strings(&["—"]);
    dropdown.set_sensitive(false);
    let microphone_row = labelled_row(&i18n("Microphone"), &dropdown);
    let adjustment = gtk::Adjustment::new(100.0, 0.0, 150.0, 1.0, 10.0, 0.0);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    scale.add_mark(100.0, gtk::PositionType::Bottom, None);
    let spin = gtk::SpinButton::new(Some(&adjustment), 1.0, 0);
    let volume_row = slider_row(&i18n("Volume"), &scale, &spin);
    volume_row.set_sensitive(false);
    volume_row.set_tooltip_text(Some(&i18n(
        "Above 100% may distort your voice. Lower the volume if the level meter reaches the top.",
    )));
    let banner = adw::Banner::builder().revealed(false).build();
    let owner = gtk::Box::new(gtk::Orientation::Vertical, 0);
    owner.append(&banner);
    owner.append(&microphone_row);
    let picker = Rc::new(Picker {
        owner: owner.downgrade(),
        dropdown: dropdown.downgrade(),
        microphone_row: microphone_row.downgrade(),
        volume_row: volume_row.downgrade(),
        adjustment: adjustment.downgrade(),
        banner: banner.downgrade(),
        sources: RefCell::new(Vec::new()),
        active: Cell::new(None),
        suppress: Cell::new(false),
        busy: Cell::new(false),
        pending: RefCell::new(Pending::default()),
        poll: Cell::new(None),
        debounce: Cell::new(None),
        volume_for: Arc::new(volume_for),
    });
    let weak = Rc::downgrade(&picker);
    dropdown.connect_selected_notify(move |dropdown| {
        let Some(picker) = weak.upgrade() else {
            return;
        };
        if picker.suppress.get() {
            return;
        }
        let id = picker
            .sources
            .borrow()
            .get(dropdown.selected() as usize)
            .map(|source| source.node_id);
        let Some(id) = id else {
            return;
        };
        if picker.active.get() == Some(id) {
            return;
        }
        if let Some(timer) = picker.debounce.take() {
            timer.remove();
        }
        picker.active.set(Some(id));
        picker.pending.borrow_mut().select(id);
        if let Some(row) = picker.volume_row.upgrade() {
            row.set_sensitive(false);
        }
        picker.pump();
    });
    let weak = Rc::downgrade(&picker);
    adjustment.connect_value_changed(move |adjustment| {
        let Some(picker) = weak.upgrade() else {
            return;
        };
        if picker.suppress.get() {
            return;
        }
        let Some(id) = picker.active.get() else {
            return;
        };
        let value = (adjustment.value() / 100.0).clamp(0.0, 1.5) as f32;
        if let Some(timer) = picker.debounce.take() {
            timer.remove();
        }
        let weak = Rc::downgrade(&picker);
        picker.debounce.set(Some(glib::timeout_add_local_once(
            VOLUME_DEBOUNCE,
            move || {
                let Some(picker) = weak.upgrade() else {
                    return;
                };
                picker.debounce.set(None);
                if picker.active.get() == Some(id) {
                    picker.pending.borrow_mut().volume = Some((id, value));
                    picker.pump();
                }
            },
        )));
    });
    // The row owns the controller; the controller holds only weak widgets.
    // Removing a page destroys its timers without leaving a reference cycle.
    let on_map = Rc::clone(&picker);
    owner.connect_map(move |_| on_map.refresh());
    let weak = Rc::downgrade(&picker);
    picker
        .poll
        .set(Some(glib::timeout_add_local(REFRESH_INTERVAL, move || {
            let Some(picker) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if picker
                .owner
                .upgrade()
                .is_some_and(|owner| owner.is_mapped())
            {
                picker.refresh();
            }
            glib::ControlFlow::Continue
        })));
    picker.refresh();
    PickerRows {
        dropdown_row: owner,
        volume_row,
    }
}

impl Picker {
    fn refresh(self: &Rc<Self>) {
        self.pending.borrow_mut().refresh = true;
        self.pump();
    }

    fn pump(self: &Rc<Self>) {
        // While a drag is settling, neither a poll nor an older completion
        // may replace the number beneath the pointer.
        if self.busy.get()
            || self.debounce.take().is_some_and(|id| {
                self.debounce.set(Some(id));
                true
            })
        {
            return;
        }
        let operation = self.pending.borrow_mut().take();
        let Some(operation) = operation else {
            return;
        };
        self.busy.set(true);
        let volume_for = Arc::clone(&self.volume_for);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || run(operation, &volume_for))
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("microphone worker failed")));
            let Some(picker) = weak.upgrade() else {
                return;
            };
            picker.busy.set(false);
            match result {
                Ok(snapshot) => {
                    if !picker.pending.borrow().has_write() {
                        // A drag can have started after dispatch as well.
                        let pending_drag = picker.debounce.take();
                        let dragging = pending_drag.is_some();
                        picker.debounce.set(pending_drag);
                        if !dragging {
                            picker.show(snapshot);
                        }
                    }
                    if operation != Operation::Refresh
                        && let Some(banner) = picker.banner.upgrade()
                    {
                        banner.set_revealed(false);
                    }
                }
                Err(error) => {
                    log::warn!("microphone controls: {error}");
                    if let Some(banner) = picker.banner.upgrade() {
                        banner.set_title(&i18n("Could not update the microphone. Check the audio connection and try again."));
                        banner.set_revealed(true);
                    }
                    if operation != Operation::Refresh {
                        picker.pending.borrow_mut().refresh = true;
                    }
                }
            }
            picker.pump();
        });
    }

    fn show(&self, snapshot: Snapshot) {
        let (Some(dropdown), Some(row), Some(volume_row), Some(adjustment)) = (
            self.dropdown.upgrade(),
            self.microphone_row.upgrade(),
            self.volume_row.upgrade(),
            self.adjustment.upgrade(),
        ) else {
            return;
        };
        let target = snapshot.default.and_then(|id| {
            snapshot
                .sources
                .iter()
                .position(|source| source.node_id == id)
        });
        let changed = *self.sources.borrow() != snapshot.sources;
        self.suppress.set(true);
        if changed {
            let labels: Vec<&str> = snapshot
                .sources
                .iter()
                .map(|source| source.description.as_str())
                .collect();
            dropdown.set_model(Some(&gtk::StringList::new(&labels)));
        }
        dropdown.set_selected(
            target
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(gtk::INVALID_LIST_POSITION),
        );
        dropdown.set_sensitive(!snapshot.sources.is_empty());
        row.set_visible(snapshot.sources.len() != 1 || target.is_none());
        self.active
            .set(target.map(|index| snapshot.sources[index].node_id));
        volume_row.set_sensitive(self.active.get().is_some() && snapshot.volume.is_some());
        if let Some(volume) = snapshot.volume.filter(|volume| volume.is_finite()) {
            adjustment.set_value((f64::from(volume) * 100.0).clamp(0.0, 150.0));
        }
        *self.sources.borrow_mut() = snapshot.sources;
        self.suppress.set(false);
    }
}

fn run(operation: Operation, volume_for: &VolumeReader) -> std::io::Result<Snapshot> {
    match operation {
        Operation::Select(id) => set_default_source(id)?,
        Operation::Volume(id, volume) => set_source_volume(id, volume)?,
        Operation::Refresh => {}
    }
    let (sources, default) = snapshot_sources();
    let volume = default.and_then(|id| volume_for(id));
    Ok(Snapshot {
        sources,
        default,
        volume,
    })
}

impl Drop for Picker {
    fn drop(&mut self) {
        if let Some(timer) = self.poll.take() {
            timer.remove();
        }
        if let Some(timer) = self.debounce.take() {
            timer.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_microphone_cancels_a_pending_volume_for_the_old_one() {
        let mut pending = Pending {
            source: None,
            volume: Some((12, 0.8)),
            refresh: true,
        };
        pending.select(20);
        pending.select(30);
        assert_eq!(pending.take(), Some(Operation::Select(30)));
        assert_eq!(pending.take(), Some(Operation::Refresh));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn coalesced_writes_precede_refreshes() {
        let mut pending = Pending::default();
        pending.refresh = true;
        pending.volume = Some((12, 0.2));
        pending.volume = Some((12, 0.9));
        assert_eq!(pending.take(), Some(Operation::Volume(12, 0.9)));
        assert_eq!(pending.take(), Some(Operation::Refresh));
    }
}
