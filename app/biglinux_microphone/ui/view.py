"""Persistent sidebar navigation and cancellable microphone tasks."""

from __future__ import annotations

import logging
import subprocess
import threading
import weakref
from collections.abc import Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gio, GLib, GObject, Gtk

from biglinux_microphone import cli
from biglinux_microphone.ui import style
from biglinux_microphone.ui.audio_capture import SpectrumTap
from biglinux_microphone.ui.backend import BackendRunner
from biglinux_microphone.ui.callbacks import weak_callback
from biglinux_microphone.ui.pages.base import PageContext
from biglinux_microphone.ui.pages.calibrate import CalibratePage
from biglinux_microphone.ui.pages.compare import ComparePage
from biglinux_microphone.ui.pages.diagnostics import DiagnosticsPage
from biglinux_microphone.ui.pages.echo_routing import EchoRoutingPage
from biglinux_microphone.ui.pages.home import HomePage
from biglinux_microphone.ui.pages.noise_reduction import NoiseReductionPage
from biglinux_microphone.ui.pages.source_picker import SourcePickerPage
from biglinux_microphone.ui.pages.voice_sound import VoiceSoundPage
from biglinux_microphone.ui.pages.voice_volume import VoiceVolumePage
from biglinux_microphone.ui.state import AudioState
from biglinux_microphone.utils.i18n import _

logger = logging.getLogger(__name__)

_PAGES = (
    HomePage,
    NoiseReductionPage,
    VoiceVolumePage,
    VoiceSoundPage,
    EchoRoutingPage,
    ComparePage,
    DiagnosticsPage,
)
_TASKS = (SourcePickerPage, CalibratePage)


class MicrophoneView(Gtk.Box):
    """Whole application content excluding the window's toast overlay."""

    def __init__(self, on_toast: Callable[[str], None]) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self._toast = on_toast
        self._closed = False
        self._process_lock = threading.Lock()
        self._monitor_generation = 0
        self._monitor_proc: subprocess.Popen | None = None
        self.monitor_active = False
        self._breakpoint = None
        self._breakpoint_handlers: list[int] = []
        self._runner = BackendRunner(
            on_error=weak_callback(self, type(self)._backend_failed)
        )
        # Kept for tests and for callers that need to drain queued backend work.
        self._cli_worker = self._runner.executor
        self._state = AudioState(self._runner)
        self._spectrum_tap = SpectrumTap(
            weak_callback(self, type(self)._on_spectrum_frame)
        )
        self._pages = []
        self._by_id = {}
        self._task_pages: dict[str, Adw.NavigationPage] = {}
        self._sidebar_rows: dict[str, Gtk.ListBoxRow] = {}
        self._current_id = "home"
        self._shown_id: str | None = None

        style.install()
        self._build()
        self._state.subscribe(self._on_snapshot)
        self._state.reload()
        self._start_spectrum()

    # ── shell ──────────────────────────────────────────────────────────────

    def _build(self) -> None:
        self._context = PageContext(
            state=self._state,
            meter=None,
            toast=self._toast,
            navigate=weak_callback(self, type(self).navigate),
            shell=weakref.proxy(self),
        )
        self._split = Adw.OverlaySplitView(
            sidebar_width_fraction=0.25,
            min_sidebar_width=220,
            max_sidebar_width=270,
        )
        self.append(self._split)
        self._split.set_sidebar(self._build_sidebar())

        toolbar = Adw.ToolbarView()
        header = Adw.HeaderBar(show_start_title_buttons=False)
        self._page_title = Adw.WindowTitle(title=_("Home"), subtitle="")
        header.set_title_widget(self._page_title)
        self._back = Gtk.Button(icon_name="go-previous-symbolic", visible=False)
        self._back.set_tooltip_text(_("Back"))
        self._back.update_property([Gtk.AccessibleProperty.LABEL], [_("Back")])
        self._back.connect(
            "clicked", weak_callback(self, lambda self, *_: self._nav.pop())
        )
        header.pack_start(self._back)
        self._sidebar_button = Gtk.ToggleButton(
            icon_name="sidebar-show-symbolic", visible=False
        )
        self._sidebar_button.set_tooltip_text(_("Pages"))
        self._sidebar_button.update_property(
            [Gtk.AccessibleProperty.LABEL], [_("Pages")]
        )
        header.pack_start(self._sidebar_button)
        self._sidebar_binding = self._split.bind_property(
            "show-sidebar",
            self._sidebar_button,
            "active",
            GObject.BindingFlags.BIDIRECTIONAL | GObject.BindingFlags.SYNC_CREATE,
        )
        toolbar.add_top_bar(header)
        self._stack = Gtk.Stack(vexpand=True)
        self._nav = Adw.NavigationView()
        self._nav.add(
            Adw.NavigationPage(child=self._stack, title=_("Microphone"), tag="pages")
        )
        self._nav.connect(
            "popped", weak_callback(self, lambda self, *_: self._page_changed())
        )
        toolbar.set_content(self._nav)
        self._split.set_content(toolbar)
        self._home = self._ensure_page("home")
        self._stack.connect(
            "notify::visible-child-name",
            weak_callback(self, lambda self, *_: self._page_changed()),
        )
        self._install_shortcuts()
        self.connect(
            "notify::root",
            weak_callback(self, lambda self, *_: self._attach_breakpoint()),
        )
        self._page_changed()

    def _build_sidebar(self) -> Gtk.Widget:
        toolbar = Adw.ToolbarView()
        header = Adw.HeaderBar(show_end_title_buttons=False)
        header.set_title_widget(Adw.WindowTitle(title=_("Microphone"), subtitle=""))
        menu = Gio.Menu()
        menu.append(_("About"), "app.about")
        menu.append(_("Quit"), "app.quit")
        button = Gtk.MenuButton(icon_name="open-menu-symbolic", menu_model=menu)
        button.set_tooltip_text(_("Main menu"))
        button.update_property([Gtk.AccessibleProperty.LABEL], [_("Main menu")])
        header.pack_end(button)
        toolbar.add_top_bar(header)
        self._sidebar_list = Gtk.ListBox(css_classes=["navigation-sidebar"])
        self._sidebar_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self._sidebar_list.connect(
            "row-activated",
            weak_callback(self, lambda self, _list, row: self.navigate(row.page_id)),
        )
        for factory in _PAGES:
            row = Adw.ActionRow(title=factory.title, activatable=True)
            row.add_prefix(Gtk.Image.new_from_icon_name(factory.icon))
            row.page_id = factory.id
            self._sidebar_list.append(row)
            self._sidebar_rows[factory.id] = row
        toolbar.set_content(Gtk.ScrolledWindow(child=self._sidebar_list, vexpand=True))
        # An action, not a place: kept out of the page list, below it.
        self._listen_back = Adw.SwitchRow(
            title=_("Listen back"),
            subtitle=_("2 s later — use headphones"),
        )
        self._listen_back.add_prefix(
            Gtk.Image.new_from_icon_name("audio-headphones-symbolic")
        )
        self._listen_back.connect(
            "notify::active", weak_callback(self, type(self)._listen_back_toggled)
        )
        actions = Gtk.ListBox(
            css_classes=["navigation-sidebar"], selection_mode=Gtk.SelectionMode.NONE
        )
        actions.append(self._listen_back)
        toolbar.add_bottom_bar(actions)
        return toolbar

    def _listen_back_toggled(self, row: Adw.SwitchRow, _param) -> None:
        if row.get_active() != self.monitor_active:
            self.set_monitor(row.get_active())

    def _install_shortcuts(self) -> None:
        controller = Gtk.ShortcutController(scope=Gtk.ShortcutScope.GLOBAL)
        for index, page in enumerate(_PAGES, start=1):
            controller.add_shortcut(
                Gtk.Shortcut(
                    trigger=Gtk.ShortcutTrigger.parse_string(f"<Control>{index}"),
                    action=Gtk.CallbackAction.new(
                        weak_callback(
                            self,
                            lambda self, *_a, page_id=page.id: (
                                self.navigate(page_id),
                                True,
                            )[1],
                        )
                    ),
                )
            )
        controller.add_shortcut(
            Gtk.Shortcut(
                trigger=Gtk.ShortcutTrigger.parse_string("<Control>r"),
                action=Gtk.CallbackAction.new(
                    weak_callback(
                        self, lambda self, *_: (self._state.reload(), True)[1]
                    )
                ),
            )
        )
        self.add_controller(controller)

    def _attach_breakpoint(self) -> None:
        window = self.get_root()
        if (
            not isinstance(window, Adw.ApplicationWindow)
            or self._breakpoint is not None
        ):
            return
        self._breakpoint = Adw.Breakpoint.new(
            Adw.BreakpointCondition.parse("max-width: 820sp")
        )
        self._breakpoint_handlers.append(
            self._breakpoint.connect(
                "apply", weak_callback(self, lambda self, *_: self._set_narrow(True))
            )
        )
        self._breakpoint_handlers.append(
            self._breakpoint.connect(
                "unapply", weak_callback(self, lambda self, *_: self._set_narrow(False))
            )
        )
        window.add_breakpoint(self._breakpoint)
        window.connect(
            "notify::suspended",
            weak_callback(self, lambda self, *_: self._sync_capture()),
        )
        self._set_narrow(False)

    def _set_narrow(self, narrow: bool) -> None:
        was_collapsed = self._split.get_collapsed()
        self._split.set_collapsed(narrow)
        self._sidebar_button.set_visible(narrow)
        if not narrow:
            self._split.set_show_sidebar(True)
        elif not was_collapsed:
            self._split.set_show_sidebar(False)

    def _ensure_page(self, page_id: str):
        if page_id in self._by_id:
            return self._by_id[page_id]
        factory = next(
            (page for page in (*_PAGES, *_TASKS) if page.id == page_id), None
        )
        if factory is None or self._closed:
            return None
        page = factory(self._context)
        self._pages.append(page)
        self._by_id[page.id] = page
        if factory in _PAGES:
            self._stack.add_titled(page.widget, page.id, page.title)
        else:
            self._task_pages[page.id] = Adw.NavigationPage(
                child=page.widget, title=page.title, tag=page.id
            )
        page.sync(self._state.snapshot)
        page.on_monitor(self.monitor_active)
        return page

    def navigate(self, page_id: str) -> None:
        if self._closed or self._ensure_page(page_id) is None:
            return
        if page_id in self._task_pages:
            nav_page = self._task_pages[page_id]
            if nav_page.get_parent() is None:
                self._nav.push(nav_page)
        else:
            self._nav.pop_to_tag("pages")
            self._stack.set_visible_child_name(page_id)
        if self._split.get_collapsed():
            self._split.set_show_sidebar(False)
        self._page_changed()

    def _visible_id(self) -> str:
        visible = self._nav.get_visible_page()
        tag = visible.get_tag() if visible is not None else "pages"
        if tag == "pages":
            return self._stack.get_visible_child_name() or "home"
        return tag

    def _page_changed(self) -> None:
        current = self._visible_id()
        self._current_id = current
        page = self._by_id.get(current)
        if page is not None:
            self._page_title.set_title(page.title)
        self._sidebar_list.select_row(self._sidebar_rows.get(current))
        self._back.set_visible(current in self._task_pages)
        previous = self._shown_id
        if self._closed or previous == current:
            return
        self._shown_id = current
        for page_id, method in ((previous, "on_hidden"), (current, "on_shown")):
            page = self._by_id.get(page_id)
            if page is not None:
                try:
                    getattr(page, method)()
                except Exception:
                    logger.exception("page lifecycle failed: %s", page.id)
        self._sync_capture()

    # ── state and meter ────────────────────────────────────────────────────

    def _on_snapshot(self, snapshot) -> None:
        for page in self._pages:
            try:
                page.sync(snapshot)
            except Exception:
                logger.exception("page %s failed to render", page.id)
        # A return that is playing can always be stopped.
        self._listen_back.set_sensitive(snapshot.ready or self.monitor_active)
        self._sync_capture()

    def _backend_failed(self, message: str) -> None:
        self._toast(str(message)[:300])

    def _start_spectrum(self) -> None:
        self._state.watch()
        self.connect("map", weak_callback(self, lambda self, *_a: self._sync_capture()))
        self.connect(
            "unmap", weak_callback(self, lambda self, *_a: self._sync_capture())
        )
        self._spectrum_tap.start()

    def _sync_capture(self) -> None:
        snapshot = self._state.snapshot
        page_id = self._visible_id()
        noise = self._by_id.get("noise_reduction")
        window = self.get_root()
        suspended = isinstance(window, Gtk.Window) and window.get_property("suspended")
        detailed = page_id == "home" or bool(
            page_id == "noise_reduction"
            and noise is not None
            and noise._editor_reveal.get_reveal_child()
        )
        if (
            self.get_mapped()
            and not suspended
            and snapshot.ready
            and not snapshot.muted
            and (page_id == "home" or detailed)
        ):
            self._spectrum_tap.configure(
                snapshot.capture_target, snapshot.capture_identity, detailed=detailed
            )
        else:
            self._spectrum_tap.configure()
            self._home._meter.clear()

    def _on_spectrum_frame(
        self,
        bands: list[float],
        peak: float,
    ) -> None:
        if self._closed:
            return
        page = self._by_id.get(self._visible_id())
        if page is not None:
            try:
                page.on_bands(bands)
                page.on_peak(peak)
            except Exception:
                logger.exception("meter update failed: %s", page.id)

    # ── delayed live monitor (the Listen back switch) ────────────────────

    def set_monitor(self, on: bool) -> None:
        if self._closed:
            return
        on = bool(on)
        if on == self.monitor_active:
            return
        self.monitor_active = on
        self._sync_monitor_surfaces()
        if on:
            self._start_monitor()
        else:
            self._stop_monitor()

    def _sync_monitor_surfaces(self) -> None:
        self._listen_back.set_active(self.monitor_active)
        for page in self._pages:
            try:
                page.on_monitor(self.monitor_active)
            except Exception:
                logger.exception("monitor state render failed: %s", page.id)

    def _start_monitor(self) -> None:
        with self._process_lock:
            self._monitor_generation += 1
            generation = self._monitor_generation

        def start() -> None:
            try:
                proc = subprocess.Popen(cli.monitor_argv())
            except OSError as exc:
                GLib.idle_add(self._monitor_failed, generation, str(exc))
                return
            with self._process_lock:
                keep = not self._closed and generation == self._monitor_generation
                if keep:
                    self._monitor_proc = proc
            if not keep:
                cli.terminate_process(proc)
                return

            def wait_for_exit() -> None:
                returncode = proc.wait()
                GLib.idle_add(self._monitor_exited, generation, proc, returncode)

            threading.Thread(
                target=wait_for_exit, name="bigmic-monitor-watch", daemon=True
            ).start()

        threading.Thread(target=start, name="bigmic-monitor-start", daemon=True).start()

    def _monitor_failed(self, generation: int, message: str) -> bool:
        if not self._closed and generation == self._monitor_generation:
            self.monitor_active = False
            self._sync_monitor_surfaces()
            self._toast(message[:200])
        return False

    def _monitor_exited(
        self, generation: int, proc: subprocess.Popen, returncode: int
    ) -> bool:
        with self._process_lock:
            if generation != self._monitor_generation or self._monitor_proc is not proc:
                return False
            self._monitor_proc = None
        self.monitor_active = False
        self._sync_monitor_surfaces()
        if returncode != 0 and not self._closed:
            self._toast(_("Live monitoring stopped unexpectedly"))
        return False

    def _stop_monitor(self) -> None:
        with self._process_lock:
            self._monitor_generation += 1
            proc, self._monitor_proc = self._monitor_proc, None
        if proc is not None:
            threading.Thread(
                target=cli.terminate_process,
                args=(proc,),
                name="bigmic-monitor-stop",
                daemon=True,
            ).start()

    def shutdown(self) -> None:
        with self._process_lock:
            if self._closed:
                return
            self._closed = True
        for handler in self._breakpoint_handlers:
            self._breakpoint.disconnect(handler)
        self._breakpoint_handlers.clear()
        for page in self._pages:
            try:
                page.shutdown()
            except Exception:
                logger.exception("page shutdown failed: %s", page.id)
        self._stop_monitor()
        self._spectrum_tap.stop()
        self._state.shutdown()
        self._runner.shutdown()
