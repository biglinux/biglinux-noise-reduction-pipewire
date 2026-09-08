// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Compositor background-blur integration for BigLinux app windows.
//!
//! One shared mechanism for every suite app: KWin Wayland blur
//! (`org_kde_kwin_blur_manager`), the staging Wayland background-effect
//! protocol (`ext_background_effect_manager_v1`), and the KWin X11 property
//! (`_KDE_NET_WM_BLUR_BEHIND_REGION`). Apps decide WHEN blur is on (their
//! settings semantics); this module owns HOW it is applied.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use gdk4_wayland::{WaylandDisplay, WaylandSurface, prelude::WaylandSurfaceExtManual};
use gdk4_x11::X11Surface;
use gtk::prelude::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_region, wl_registry, wl_surface};
use wayland_client::{Connection as WaylandConnection, Dispatch, Proxy, delegate_noop};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

const KWIN_BLUR_ATOM: &str = "_KDE_NET_WM_BLUR_BEHIND_REGION";
const KDE_BLUR_GLOBAL_NAME: &str = "org_kde_kwin_blur_manager";
const WAYLAND_BACKGROUND_EFFECT_GLOBAL_NAME: &str = "ext_background_effect_manager_v1";

static LAST_X11_ATTEMPTED: LazyLock<Mutex<HashMap<u64, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static WAYLAND_EFFECTS: RefCell<HashMap<u32, WaylandBlurEntry>> = RefCell::new(HashMap::new());
}

/// Re-apply blur every time the window maps, resolving the desired state
/// from the app-supplied callback (settings read, transparency check, …).
pub fn install_window_blur<W, F>(window: &W, resolve_should_blur: F)
where
    W: IsA<gtk::Window> + IsA<gtk::Widget>,
    F: Fn() -> bool + 'static,
{
    window.connect_map(move |window| {
        apply_window_blur(window, resolve_should_blur());
    });
}

/// Apply or remove compositor background blur on a mapped window.
///
/// No-op when the window has no surface yet or no supported compositor
/// protocol is available; failures are logged at debug level only.
pub fn apply_window_blur(window: &impl IsA<gtk::Window>, should_blur: bool) {
    let window = window.upcast_ref::<gtk::Window>();
    let Some(surface) = window.surface() else {
        return;
    };
    let Some(x11_surface) = surface.downcast_ref::<X11Surface>() else {
        apply_wayland_to_surface(&surface, should_blur);
        return;
    };
    let xid = x11_surface.xid();
    if xid == 0 {
        return;
    }
    let xid = xid_to_u64(xid);
    if already_attempted_x11(xid, should_blur) {
        return;
    }

    if let Err(err) = apply_kwin_blur_property(xid, should_blur) {
        log::debug!("failed to update KWin blur property: {err}");
    }
}

fn already_attempted_x11(xid: u64, should_blur: bool) -> bool {
    let Ok(mut states) = LAST_X11_ATTEMPTED.lock() else {
        return false;
    };
    if states.get(&xid).copied() == Some(should_blur) {
        return true;
    }
    states.insert(xid, should_blur);
    false
}

fn apply_wayland_to_surface(surface: &gtk::gdk::Surface, should_blur: bool) {
    let Some(wayland_surface) = surface.downcast_ref::<WaylandSurface>() else {
        return;
    };
    let display = surface.display();
    let Some(wayland_display) = display.downcast_ref::<WaylandDisplay>() else {
        return;
    };
    if !wayland_display.query_registry(KDE_BLUR_GLOBAL_NAME)
        && !wayland_display.query_registry(WAYLAND_BACKGROUND_EFFECT_GLOBAL_NAME)
    {
        return;
    }
    let Some(wl_surface) = wayland_surface.wl_surface() else {
        return;
    };
    let surface_id = wl_surface.id().protocol_id();

    WAYLAND_EFFECTS.with_borrow_mut(|entries| {
        if entries
            .get(&surface_id)
            .is_some_and(|entry| !entry.wl_surface.is_alive())
        {
            entries.remove(&surface_id);
        }
        if let Some(entry) = entries.get_mut(&surface_id) {
            if let Err(err) = entry.set_blur(should_blur) {
                log::debug!("failed to update Wayland blur region: {err}");
            }
            return;
        }
        if !should_blur {
            return;
        }
        match WaylandBlurEntry::new(wl_surface) {
            Ok(mut entry) => {
                if let Err(err) = entry.set_blur(true) {
                    log::debug!("failed to enable Wayland blur: {err}");
                    return;
                }
                entries.insert(surface_id, entry);
            }
            Err(err) => log::debug!("Wayland blur is unavailable: {err}"),
        }
    });
}

struct WaylandBlurEntry {
    connection: WaylandConnection,
    event_queue: wayland_client::EventQueue<WaylandBlurDispatch>,
    wl_surface: wl_surface::WlSurface,
    compositor: wl_compositor::WlCompositor,
    backend: WaylandBlurBackend,
    enabled: bool,
}

impl WaylandBlurEntry {
    fn new(wl_surface: wl_surface::WlSurface) -> Result<Self, String> {
        let backend = wl_surface
            .backend()
            .upgrade()
            .ok_or_else(|| "surface Wayland backend is gone".to_owned())?;
        let connection = WaylandConnection::from_backend(backend);
        let (globals, event_queue) =
            registry_queue_init::<WaylandBlurDispatch>(&connection).map_err(|e| e.to_string())?;
        let qh = event_queue.handle();
        let compositor: wl_compositor::WlCompositor = globals
            .bind(&qh, 1..=6, ())
            .map_err(|err| err.to_string())?;
        let backend = if has_global(globals.contents(), KDE_BLUR_GLOBAL_NAME) {
            let kde_blur_protocol: OrgKdeKwinBlurManager = globals
                .bind(&qh, 1..=1, ())
                .map_err(|err| err.to_string())?;
            WaylandBlurBackend::Kde {
                manager: kde_blur_protocol,
                blur: None,
            }
        } else if has_global(globals.contents(), WAYLAND_BACKGROUND_EFFECT_GLOBAL_NAME) {
            let background_effect_protocol: ExtBackgroundEffectManagerV1 = globals
                .bind(&qh, 1..=1, ())
                .map_err(|err| err.to_string())?;
            let effect = background_effect_protocol.get_background_effect(&wl_surface, &qh, ());
            WaylandBlurBackend::BackgroundEffect { effect }
        } else {
            return Err("no supported Wayland blur protocol is advertised".to_owned());
        };
        Ok(Self {
            connection,
            event_queue,
            wl_surface,
            compositor,
            backend,
            enabled: false,
        })
    }

    fn set_blur(&mut self, should_blur: bool) -> Result<(), String> {
        if self.enabled == should_blur {
            return Ok(());
        }
        if should_blur {
            let qh = self.event_queue.handle();
            self.backend.enable(&self.wl_surface, &self.compositor, &qh);
        } else {
            self.backend.disable(&self.wl_surface);
        }
        if matches!(self.backend, WaylandBlurBackend::BackgroundEffect { .. }) {
            self.wl_surface.commit();
        }
        self.connection.flush().map_err(|err| err.to_string())?;
        self.enabled = should_blur;
        Ok(())
    }
}

enum WaylandBlurBackend {
    Kde {
        manager: OrgKdeKwinBlurManager,
        blur: Option<OrgKdeKwinBlur>,
    },
    BackgroundEffect {
        effect: ExtBackgroundEffectSurfaceV1,
    },
}

impl WaylandBlurBackend {
    fn enable(
        &mut self,
        wl_surface: &wl_surface::WlSurface,
        compositor: &wl_compositor::WlCompositor,
        qh: &wayland_client::QueueHandle<WaylandBlurDispatch>,
    ) {
        match self {
            Self::Kde { manager, blur } => {
                let blur = blur.get_or_insert_with(|| manager.create(wl_surface, qh, ()));
                blur.set_region(None);
                blur.commit();
            }
            Self::BackgroundEffect { effect } => {
                let region = compositor.create_region(qh, ());
                region.add(0, 0, i32::MAX, i32::MAX);
                effect.set_blur_region(Some(&region));
                region.destroy();
            }
        }
    }

    fn disable(&mut self, wl_surface: &wl_surface::WlSurface) {
        match self {
            Self::Kde { manager, blur } => {
                manager.unset(wl_surface);
                if let Some(blur) = blur.take() {
                    blur.release();
                }
            }
            Self::BackgroundEffect { effect } => {
                effect.set_blur_region(None);
            }
        }
    }
}

fn has_global(globals: &GlobalListContents, name: &str) -> bool {
    globals.with_list(|items| items.iter().any(|global| global.interface == name))
}

struct WaylandBlurDispatch;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandBlurDispatch {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &WaylandConnection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(WaylandBlurDispatch: ignore ExtBackgroundEffectManagerV1);
delegate_noop!(WaylandBlurDispatch: ExtBackgroundEffectSurfaceV1);
delegate_noop!(WaylandBlurDispatch: OrgKdeKwinBlurManager);
delegate_noop!(WaylandBlurDispatch: OrgKdeKwinBlur);
delegate_noop!(WaylandBlurDispatch: wl_compositor::WlCompositor);
delegate_noop!(WaylandBlurDispatch: wl_region::WlRegion);

fn apply_kwin_blur_property(xid: u64, should_blur: bool) -> Result<(), String> {
    let window = u32::try_from(xid).map_err(|_| format!("invalid X11 window id {xid}"))?;
    let (connection, _) = x11rb::connect(None).map_err(|err| err.to_string())?;
    let atom = connection
        .intern_atom(!should_blur, KWIN_BLUR_ATOM.as_bytes())
        .map_err(|err| err.to_string())?
        .reply()
        .map_err(|err| err.to_string())?
        .atom;
    if atom == u32::from(AtomEnum::NONE) {
        return Ok(());
    }

    if should_blur {
        connection
            .change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[0])
            .map_err(|err| err.to_string())?;
    } else {
        connection
            .delete_property(window, atom)
            .map_err(|err| err.to_string())?;
    }
    connection.flush().map_err(|err| err.to_string())
}

// `X11Surface::xid()` returns the `gdk4_x11::XWindow` alias without the
// `xlib` feature and `x11::xlib::Window` with it — the alias is NOT
// re-exported then, and feature unification in a multicall host (ashpd
// enables `xlib`) flips it under us. Both are `c_ulong`; the generic
// bound compiles under either configuration and pointer width.
fn xid_to_u64(xid: impl Into<u64>) -> u64 {
    xid.into()
}
