# SPDX-FileCopyrightText: 2026 BigLinux contributors
# SPDX-License-Identifier: MIT
"""Windows whose surface is released after a popover or tooltip showed on them;
see big-gtk-kit's ``src/surface.rs``.

GDK's Wayland popup takes its parent surface with ``g_value_dup_object`` and
nothing ever drops that reference, so a window that showed one popover, menu or
tooltip kept its ``GdkWaylandToplevel`` after it closed (GTK 4.22.4, still so on
``main``). Limited to those versions, so a GTK that fixes it is left alone.
"""

import ctypes

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, GLib, GObject, Gtk

_WATCHED = "_big_gtk_kit_surface_watched"
_GUARDED = "_big_gtk_kit_surface_guarded"
_gobject = ctypes.CDLL("libgobject-2.0.so.0")
_gobject.g_object_unref.argtypes = [ctypes.c_void_p]
_capsule = ctypes.pythonapi.PyCapsule_GetPointer
_capsule.argtypes = [ctypes.py_object, ctypes.c_char_p]
_capsule.restype = ctypes.c_void_p
_installed = False


def install():
    """Release the parent surface every Wayland popup keeps."""
    global _installed
    if _installed or Gtk.check_version(4, 22, 5) is None:
        return
    _installed = True
    GObject.TypeClass.get(Gtk.Widget.__gtype__)
    GObject.add_emission_hook(Gtk.Widget, "realize", _on_realize)


def _on_realize(widget):
    if isinstance(widget, Gtk.Native) and not getattr(widget, _WATCHED, False):
        setattr(widget, _WATCHED, True)
        # Each realize makes a new popup surface; the map after it has it.
        widget.connect("map", _watch)
    return True


def _watch(native):
    surface = native.get_surface()
    if surface is None or surface.__gtype__.name != "GdkWaylandPopup":
        return
    if getattr(surface, _GUARDED, False) or not isinstance(surface, Gdk.Popup):
        return
    parent = surface.get_parent()
    if parent is None:
        return
    setattr(surface, _GUARDED, True)

    def release():
        # The popup has finalized and unlinked itself; `parent` here keeps the
        # surface alive while the reference the popup lost is dropped.
        _gobject.g_object_unref(_capsule(parent.__gpointer__, None))
        return GLib.SOURCE_REMOVE

    # Weak notifications come at dispose, before GDK unlinks the popup from
    # the parent, so the release waits for the main loop.
    surface.weak_ref(lambda *_args: GLib.idle_add(release))
