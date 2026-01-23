#!/usr/bin/env python3
"""
Main view for BigLinux Microphone Settings.

Unified view consolidating all audio settings in a single page with:
- Spectrum analyzer at the top
- Noise reduction toggle
- Quick settings (always visible)
- Advanced settings in collapsible expanders
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from typing import TYPE_CHECKING

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Gtk

from biglinux_microphone.config import (
    EQ_BANDS,
    EQ_PRESETS,
    STEREO_WIDTH_DEFAULT,
    STEREO_WIDTH_MAX,
    STEREO_WIDTH_MIN,
    STRENGTH_DEFAULT,
    STRENGTH_MAX,
    STRENGTH_MIN,
    AppSettings,
    StereoMode,
)
from biglinux_microphone.ui.components import (
    create_action_row_with_scale,
    create_action_row_with_switch,
    create_combo_row,
    create_compact_eq_slider,
    create_expander_row_with_switch,
    create_preferences_group,
)
from biglinux_microphone.ui.spectrum_widget import SpectrumAnalyzerWidget
from biglinux_microphone.utils.i18n import _
from biglinux_microphone.utils.tooltip_helper import TooltipHelper

if TYPE_CHECKING:
    from biglinux_microphone.services import PipeWireService, SettingsService

logger = logging.getLogger(__name__)


class MainView(Adw.NavigationPage):
    """
    Main view - unified page of the application.

    Contains all settings in an organized layout:
    - Spectrum analyzer (top)
    - Noise reduction toggle (simple switch)
    - Quick settings (always visible)
    - Gate filter (expander)
    - Voice effects (expander)
    - Equalizer (expander)
    - Advanced settings (expander)
    """

    def __init__(
        self,
        pipewire_service: PipeWireService,
        settings_service: SettingsService,
        monitor_service: object,
        on_toast: Callable[[str, int], None] | None = None,
        audio_monitor: object | None = None,
        tooltip_helper: TooltipHelper | None = None,
    ) -> None:
        """
        Initialize the main view.

        Args:
            pipewire_service: PipeWire backend service
            settings_service: Settings persistence service
            monitor_service: Headphone monitoring service
            on_toast: Callback to show toast notifications
            audio_monitor: Optional AudioMonitor to switch sources on filter toggle
            tooltip_helper: Optional TooltipHelper for custom tooltips
        """
        super().__init__(title="Microphone Settings", tag="main")

        self._pipewire = pipewire_service
        self._settings_service = settings_service
        self._settings = settings_service.get()
        self._monitor_service = monitor_service
        self._on_toast = on_toast
        self._audio_monitor = audio_monitor
        self._tooltip_helper = tooltip_helper

        # Widget references
        self._spectrum: SpectrumAnalyzerWidget | None = None
        self._eq_sliders: list[Gtk.Scale] = []

        # Loading state flag - prevents callbacks during state load
        self._loading = False

        # Timer for debounced EQ config updates
        self._eq_config_timer: int = 0

        # Timer for debounced settings save
        self._save_settings_timer: int = 0

        # Timer for debounced monitor delay
        self._monitor_delay_timer: int = 0

        # Timer for polling external state changes (e.g., plasmoid)
        self._state_poll_timer: int = 0
        self._last_known_enabled_state: bool | None = None

        self._setup_ui()
        self._load_state()
        self._start_state_polling()

    def _setup_ui(self) -> None:
        """Set up the UI layout."""
        # Main scrolled content
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)

        # Clamp for content width
        clamp = Adw.Clamp()
        clamp.set_maximum_size(600)
        clamp.set_margin_top(12)
        clamp.set_margin_bottom(24)
        clamp.set_margin_start(12)
        clamp.set_margin_end(12)

        # Main box
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)

        # === Spectrum Analyzer ===
        spectrum_frame = self._create_spectrum_section()
        main_box.append(spectrum_frame)

        # === Noise Reduction Toggle ===
        toggle_group = self._create_noise_reduction_section()
        main_box.append(toggle_group)

        # === Bluetooth (always visible - independent of noise reduction) ===
        bt_group = self._create_bluetooth_section()
        main_box.append(bt_group)

        # === Headphone Monitor ===
        monitor_group = self._create_monitor_section()
        main_box.append(monitor_group)

        # === Gate Filter (Expander) ===
        gate_group = self._create_gate_section()
        main_box.append(gate_group)

        # === Voice Effects (Expander) ===
        stereo_group = self._create_stereo_section()
        main_box.append(stereo_group)

        # === Equalizer (Expander) ===
        eq_group = self._create_equalizer_section()
        main_box.append(eq_group)

        clamp.set_child(main_box)
        scrolled.set_child(clamp)
        self.set_child(scrolled)

        # Setup tooltips after UI is created
        self._setup_tooltips()

    def _add_tooltip(self, widget, tooltip_key: str) -> None:
        """Add tooltip to a widget if tooltip helper is available."""
        if self._tooltip_helper is not None:
            self._tooltip_helper.add_tooltip(widget, tooltip_key)

    def _setup_tooltips(self) -> None:
        """Setup tooltips for all UI elements."""
        if self._tooltip_helper is None:
            return

        # Gate section
        self._add_tooltip(self._gate_expander, "gate_toggle")
        self._add_tooltip(self._threshold_row, "gate_threshold")
        self._add_tooltip(self._range_row, "gate_range")
        self._add_tooltip(self._attack_row, "gate_attack")
        self._add_tooltip(self._hold_row, "gate_hold")
        self._add_tooltip(self._release_row, "gate_release")

        # Noise Reduction section
        self._add_tooltip(self._nr_expander, "noise_reduction_toggle")
        self._add_tooltip(self._strength_row, "noise_reduction_strength")

        # Voice Effects section
        self._add_tooltip(self._stereo_expander, "stereo_toggle")
        self._add_tooltip(self._mode_combo, "stereo_mode")
        self._add_tooltip(self._width_row, "stereo_width")

        # Monitor section
        self._add_tooltip(self._monitor_expander, "monitor_toggle")
        self._add_tooltip(self._monitor_delay_row, "monitor_delay")

        # Bluetooth section
        self._add_tooltip(self._bt_row, "bluetooth_toggle")

        # Equalizer section
        self._add_tooltip(self._eq_expander, "equalizer_toggle")
        self._add_tooltip(self._eq_preset_combo, "equalizer_preset")

        # Add tooltips to EQ bands row (entire region)
        self._add_tooltip(self._eq_bands_row, "equalizer_bands")

    # =========================================================================
    # Section Builders
    # =========================================================================

    def _create_spectrum_section(self) -> Gtk.Widget:
        """Create the spectrum analyzer section."""
        # Spectrum analyzer widget without frame or title
        self._spectrum = SpectrumAnalyzerWidget()
        self._spectrum.set_margin_top(4)
        self._spectrum.set_margin_bottom(12)

        # We wrap it in a box just to control margins if needed
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        box.append(self._spectrum)

        return box

    def _create_noise_reduction_section(self) -> Adw.PreferencesGroup:
        """Create the noise reduction toggle section (expander)."""
        group = create_preferences_group("", "")

        # Expander with switch (Noise Reduction)
        self._nr_expander, self._main_switch = create_expander_row_with_switch(
            _("Activate Noise Reduction"),
            subtitle=_("Removes background noise (keyboard, fan) using AI."),
            icon_name="audio-input-microphone-symbolic",
            active=False,  # Will be set by _load_state
            expanded=False,
            on_toggled=self._on_main_toggle,
        )
        group.add(self._nr_expander)

        # Strength slider
        self._strength_row, self._strength_scale = create_action_row_with_scale(
            _("Cleanup Intensity"),
            subtitle=_(
                "Aggressiveness of removal. High values clean more but may alter voice."
            ),
            min_value=STRENGTH_MIN,
            max_value=STRENGTH_MAX,
            value=STRENGTH_DEFAULT,
            step=0.05,
            digits=0,
            on_changed=self._on_strength_changed,
            marks=[
                (0.0, _("Smooth")),
                (0.5, _("Medium")),
                (1.0, _("Strong")),
            ],
        )
        self._strength_row.set_icon_name("audio-volume-high-symbolic")
        self._nr_expander.add_row(self._strength_row)

        return group

    # =========================================================================
    # Helpers
    # =========================================================================

    def _to_log_scale(self, value: float, min_val: float, max_val: float) -> float:
        """Convert linear slider position (0-100) to logarithmic value."""
        import math

        # Avoid log(0)
        if min_val <= 0:
            min_val = 0.1

        min_log = math.log(min_val)
        max_log = math.log(max_val)

        # Scale 0-100 to min_log-max_log
        log_val = min_log + (value / 100.0) * (max_log - min_log)

        return math.exp(log_val)

    def _from_log_scale(self, value: float, min_val: float, max_val: float) -> float:
        """Convert logarithmic value to linear slider position (0-100)."""
        import math

        if min_val <= 0:
            min_val = 0.1

        # Clamp value
        value = max(min_val, min(value, max_val))

        min_log = math.log(min_val)
        max_log = math.log(max_val)

        # Inverse log mapping
        return ((math.log(value) - min_log) / (max_log - min_log)) * 100.0

    def _create_gate_section(self) -> Adw.PreferencesGroup:
        """Create gate filter section as expander."""
        group = create_preferences_group("", "")

        # Expander with switch
        self._gate_expander, self._gate_switch = create_expander_row_with_switch(
            _("Silence Filter (Gate)"),
            subtitle=_(
                "Automatically silences the microphone when you are not talking."
            ),
            icon_name="microphone-sensitivity-high-symbolic",
            active=False,  # Will be set correctly by _load_state
            expanded=False,
            on_toggled=self._on_gate_toggled,
        )
        group.add(self._gate_expander)

        # Threshold (Linear, optimized for -30dB center)
        # Range: -60dB to 0dB. Center (-30dB) is exactly 50%.
        self._threshold_row, self._threshold_scale = create_action_row_with_scale(
            _("Sensitivity (Threshold)"),
            subtitle="Activates at -30 dB",
            min_value=0,
            max_value=100,
            value=50,  # Default -30dB map
            step=1,
            digits=0,
            on_changed=self._on_threshold_scale_changed,
            marks=[
                (0, _("Low")),
                (50, _("Medium")),
                (100, _("High")),
            ],
        )
        self._gate_expander.add_row(self._threshold_row)

        # Range/Reduction (Linear, optimized for -80dB max)
        # Range: -80dB to 0dB.
        self._range_row, self._range_scale = create_action_row_with_scale(
            _("Silence Reduction"),
            subtitle="Reduction amount (-60 dB)",
            min_value=0,
            max_value=100,
            value=75,  # -60dB is 75% of -80dB
            step=1,
            digits=0,
            on_changed=self._on_range_scale_changed,
            marks=[
                (0, "0%"),
                (50, "50%"),
                (100, "100%"),
            ],
        )
        self._gate_expander.add_row(self._range_row)

        # Attack (Logarithmic)
        # Range: 1ms to 400ms. Default 20ms.
        # Log center of 1-400 is sqrt(1*400) = 20ms. Matches default exactly.
        self._attack_row, self._attack_scale = create_action_row_with_scale(
            _("Attack (Opening)"),
            subtitle="How fast gate opens (20.0 ms)",
            min_value=0,
            max_value=100,
            value=50,  # Exactly 20ms now
            step=1,
            digits=0,
            on_changed=self._on_attack_changed,
            marks=[
                (0, "1ms"),
                (50, "20ms"),
                (100, "400ms"),
            ],
        )
        self._gate_expander.add_row(self._attack_row)

        # Hold (Logarithmic)
        # Range: 50ms to 1800ms. Default 300ms.
        # Log center is sqrt(50*1800) = 300ms. Matches default exactly.
        self._hold_row, self._hold_scale = create_action_row_with_scale(
            _("Hold"),
            subtitle="Min open time (300.0 ms)",
            min_value=0,
            max_value=100,
            value=50,  # Exactly 300ms now
            step=1,
            digits=0,
            on_changed=self._on_hold_changed,
            marks=[
                (0, "50ms"),
                (50, "300ms"),
                (100, "1.8s"),
            ],
        )
        self._gate_expander.add_row(self._hold_row)

        # Release (Logarithmic)
        # Range: 10ms to 2250ms. Default 150ms.
        # Log center is sqrt(10*2250) = 150ms. Matches default exactly.
        self._release_row, self._release_scale = create_action_row_with_scale(
            _("Release"),
            subtitle="Decay time (150.0 ms)",
            min_value=0,
            max_value=100,
            value=50,  # Exactly 150ms now
            step=1,
            digits=0,
            on_changed=self._on_release_changed,
            marks=[
                (0, "10ms"),
                (50, "150ms"),
                (100, "2.2s"),
            ],
        )
        self._gate_expander.add_row(self._release_row)

        return group

    def _create_stereo_section(self) -> Adw.PreferencesGroup:
        """Create voice effects section as expander."""
        group = create_preferences_group("", "")

        # Expander with switch
        self._stereo_expander, self._stereo_switch = create_expander_row_with_switch(
            _("Voice Effects"),
            subtitle=_("Voice enhancement, studio sound and pitch shifter."),
            icon_name="audio-speakers-symbolic",
            active=False,
            expanded=False,
            on_toggled=self._on_stereo_toggled,
        )
        group.add(self._stereo_expander)

        # Mode selection (Mono is handled by the switch, not a mode option)
        mode_names = [
            _("Dual Mono"),
            _("Studio"),
            _("Voice Changer (Pitch)"),
        ]

        self._mode_combo = create_combo_row(
            _("Voice Style"),
            subtitle=_("Choose how your voice sounds to others."),
            options=mode_names,
            selected_index=0,
            on_selected=self._on_mode_selected,
        )
        self._stereo_expander.add_row(self._mode_combo)

        # Width control (Effect Intensity)
        self._width_row, self._width_scale = create_action_row_with_scale(
            _("Stereo Expansion"),
            subtitle=_("Soundstage width. Higher = more immersive."),
            min_value=STEREO_WIDTH_MIN,
            max_value=STEREO_WIDTH_MAX,
            value=STEREO_WIDTH_DEFAULT,
            step=0.1,
            digits=1,
            on_changed=self._on_width_changed,
            marks=[
                (0.0, _("Focus")),
                (1.0, _("Wide")),
            ],
        )
        self._stereo_expander.add_row(self._width_row)

        # Delay control

        return group

    def _create_equalizer_section(self) -> Adw.PreferencesGroup:
        """Create equalizer section as expander with compact sliders."""
        group = create_preferences_group("", "")

        # Expander with switch
        self._eq_expander, self._eq_switch = create_expander_row_with_switch(
            _("Equalizer"),
            subtitle=_("Fine-tuning of bass, mids, and treble."),
            icon_name="view-media-equalizer",
            active=False,
            expanded=False,
            on_toggled=self._on_eq_toggled,
        )
        group.add(self._eq_expander)

        # Preset selection

        self._eq_preset_combo = create_combo_row(
            _("Presets"),
            subtitle=_("Ready-made adjustments for different situations."),
            options=list(EQ_PRESETS.keys()),
            selected_index=0,
            on_selected=self._on_eq_preset_selected,
        )
        self._eq_expander.add_row(self._eq_preset_combo)

        # Compact EQ sliders in a horizontal box
        self._eq_bands_row = Adw.ActionRow()
        self._eq_bands_row.set_title(_("Bands"))
        self._eq_bands_row.set_subtitle(_("Adjust each frequency"))

        eq_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        eq_box.set_halign(Gtk.Align.CENTER)
        eq_box.set_margin_top(8)
        eq_box.set_margin_bottom(8)

        self._eq_sliders = []
        for i, freq in enumerate(EQ_BANDS):
            slider = create_compact_eq_slider(
                index=i,
                freq=freq,
                value=0.0,
                on_changed=self._on_eq_band_changed,
            )
            eq_box.append(slider)
            # Store reference to the scale widget
            # The scale is the second child (after value label)
            scale = slider.get_first_child().get_next_sibling()
            if isinstance(scale, Gtk.Scale):
                self._eq_sliders.append(scale)

        self._eq_bands_row.add_suffix(eq_box)
        self._eq_expander.add_row(self._eq_bands_row)

        return group

    def _create_bluetooth_section(self) -> Adw.PreferencesGroup:
        """Create bluetooth section (directly visible)."""
        group = create_preferences_group("", "")

        # Bluetooth auto-switch
        self._bt_row, self._bt_switch = create_action_row_with_switch(
            _("Bluetooth Headset Mode"),
            subtitle=_("Automatically switch between high quality vs call profile."),
            active=False,  # Will be set correctly by _load_state
            on_toggled=self._on_bluetooth_toggled,
        )
        self._bt_row.set_icon_name("bluetooth-symbolic")
        group.add(self._bt_row)

        return group

    def _create_monitor_section(self) -> Adw.PreferencesGroup:
        """Create headphone monitor section."""
        group = create_preferences_group("", "")

        # Expander with switch
        self._monitor_expander, self._monitor_switch = create_expander_row_with_switch(
            _("Monitor Feedback"),
            subtitle=_("Hear how your voice sounds"),
            icon_name="audio-headphones-symbolic",
            active=False,
            expanded=False,
            on_toggled=self._on_monitor_toggled,
        )
        group.add(self._monitor_expander)

        # Delay slider
        self._monitor_delay_row, self._monitor_delay_scale = (
            create_action_row_with_scale(
                _("Monitor Delay"),
                subtitle=_("Monitor feedback delay. Useful for checking sync."),
                min_value=0,
                max_value=5000,
                value=0,
                step=100,
                digits=0,
                on_changed=self._on_monitor_delay_changed,
                marks=[(0, _("0s")), (1000, _("1s")), (3000, _("3s")), (5000, _("5s"))],
            )
        )
        self._monitor_expander.add_row(self._monitor_delay_row)
        return group

    # =========================================================================
    # State Management
    # =========================================================================

    def _load_state(self) -> None:
        """Load current state from settings and services."""
        self._loading = True
        try:
            # Noise reduction
            is_enabled = self._pipewire.is_enabled()
            self._main_switch.set_active(is_enabled)

            # Strength
            self._strength_scale.set_value(self._settings.noise_reduction.strength)

            # Bluetooth
            self._bt_switch.set_active(self._settings.bluetooth.auto_switch_headset)

            # Headphone Monitor
            self._monitor_switch.set_active(self._settings.monitor.enabled)
            self._monitor_switch.set_active(self._settings.monitor.enabled)
            self._monitor_delay_scale.set_value(self._settings.monitor.delay_ms)
            self._strength_row.set_sensitive(is_enabled)

            # Start monitor if enabled
            if self._settings.monitor.enabled:
                source = self._get_monitor_source()
                channels = self._get_monitor_channels()
                self._monitor_service.start_monitor(
                    source=source,
                    delay_ms=self._settings.monitor.delay_ms,
                    channels=channels,
                )

            # Gate
            self._gate_switch.set_active(self._settings.gate.enabled)

            # Map Gate values to slider positions

            # Threshold: -60 to 0. Linear.
            # val = map(db, -60, 0, 0, 100)
            t_min, t_max = -60, 0
            t_val = max(t_min, min(self._settings.gate.threshold_db, t_max))
            t_pos = ((t_val - t_min) / (t_max - t_min)) * 100
            self._threshold_scale.set_value(t_pos)
            self._threshold_row.set_subtitle(
                f"Activates at {self._settings.gate.threshold_db} dB"
            )

            # Range: -80 to 0. Linear.
            r_target = -80
            r_val = self._settings.gate.range_db  # e.g. -60
            r_pos = (r_val / r_target) * 100 if r_target != 0 else 0
            self._range_scale.set_value(r_pos)
            self._range_row.set_subtitle(
                _("Reduction amount ({reduction} dB)").format(reduction=r_val)
            )

            # Attack: Log 1..400
            a_pos = self._from_log_scale(self._settings.gate.attack_ms, 1.0, 400.0)
            self._attack_scale.set_value(a_pos)
            self._attack_row.set_subtitle(
                f"How fast gate opens ({self._settings.gate.attack_ms:.1f} ms)"
            )

            # Hold: Log 50..1800
            h_pos = self._from_log_scale(self._settings.gate.hold_ms, 50.0, 1800.0)
            self._hold_scale.set_value(h_pos)
            self._hold_row.set_subtitle(
                f"Min open time ({self._settings.gate.hold_ms:.1f} ms)"
            )

            # Release: Log 10..2250
            rel_pos = self._from_log_scale(self._settings.gate.release_ms, 10.0, 2250.0)
            self._release_scale.set_value(rel_pos)
            self._release_row.set_subtitle(
                f"Decay time ({self._settings.gate.release_ms:.1f} ms)"
            )

            self._update_gate_sensitivity()
            # Sync gate state to PipeWire service
            self._pipewire.set_gate_enabled(self._settings.gate.enabled)
            self._pipewire.set_gate_threshold(self._settings.gate.threshold_db)
            self._pipewire.set_gate_range(self._settings.gate.range_db)
            self._pipewire.set_gate_attack(self._settings.gate.attack_ms)
            self._pipewire.set_gate_hold(self._settings.gate.hold_ms)
            self._pipewire.set_gate_release(self._settings.gate.release_ms)

            # Update subtitles with loaded values
            self._threshold_row.set_subtitle(
                _("Activates at {threshold} dB").format(
                    threshold=self._settings.gate.threshold_db
                )
            )
            self._range_row.set_subtitle(
                _("Reduction amount ({reduction} dB)").format(
                    reduction=self._settings.gate.range_db
                )
            )
            self._attack_row.set_subtitle(
                _("How fast gate opens ({attack:.1f} ms)").format(
                    attack=self._settings.gate.attack_ms
                )
            )
            self._hold_row.set_subtitle(
                _("Min open time ({hold:.1f} ms)").format(
                    hold=self._settings.gate.hold_ms
                )
            )
            self._release_row.set_subtitle(
                _("Decay time ({release:.1f} ms)").format(
                    release=self._settings.gate.release_ms
                )
            )

            # Strength subtitle
            strength_percent = int(self._settings.noise_reduction.strength * 100)
            self._strength_row.set_subtitle(
                _(
                    "Removal aggressiveness. High values clean more but may alter voice. ({percent}%)"
                ).format(percent=strength_percent)
            )

            # Monitor Delay subtitle
            self._monitor_delay_row.set_subtitle(
                _(
                    "Monitor feedback delay. Useful for checking sync. ({delay} ms)"
                ).format(delay=self._settings.monitor.delay_ms)
            )

            # Stereo - sync UI and service state
            self._stereo_switch.set_active(self._settings.stereo.enabled)
            mode = self._settings.stereo.mode
            if isinstance(mode, str):
                mode = StereoMode(mode)
            # The combo only has non-MONO modes, so we need to map accordingly
            # If mode is MONO, default to DUAL_MONO in the combo
            stereo_modes = [
                StereoMode.DUAL_MONO,
                StereoMode.RADIO,
                StereoMode.VOICE_CHANGER,
            ]
            if mode in stereo_modes:
                mode_index = stereo_modes.index(mode)
            else:
                # Default to DUAL_MONO if mode is MONO or IMMERSIVE (removed)
                mode_index = 0
                mode = StereoMode.DUAL_MONO
            self._mode_combo.set_selected(mode_index)
            self._update_stereo_ui_labels(mode)
            self._width_scale.set_value(self._settings.stereo.width)
            self._update_stereo_sensitivity()
            # Sync stereo state to PipeWire service - use MONO if disabled
            actual_mode = mode if self._settings.stereo.enabled else StereoMode.MONO
            self._pipewire.set_stereo_enabled(self._settings.stereo.enabled)
            self._pipewire.set_stereo_mode(actual_mode)
            self._pipewire.set_stereo_width(self._settings.stereo.width)

            # Equalizer - sync UI and service state
            logger.info(
                "Loading EQ state: enabled=%s, preset=%s",
                self._settings.equalizer.enabled,
                self._settings.equalizer.preset,
            )
            self._eq_switch.set_active(self._settings.equalizer.enabled)
            bands = self._settings.equalizer.bands
            for i, scale in enumerate(self._eq_sliders):
                if i < len(bands):
                    scale.set_value(bands[i])

            # Restore preset selection
            preset_names = list(EQ_PRESETS.keys())
            if self._settings.equalizer.preset in preset_names:
                idx = preset_names.index(self._settings.equalizer.preset)
                self._eq_preset_combo.set_selected(idx)

            self._update_eq_sensitivity()
            # Sync EQ state to PipeWire service
            self._pipewire.set_eq_enabled(self._settings.equalizer.enabled)
            self._pipewire.set_eq_bands(list(bands))
        finally:
            self._loading = False
            self._update_all_controls_sensitivity()

    def _update_all_controls_sensitivity(self) -> None:
        """Update all controls sensitivity based on noise reduction state."""
        nr_enabled = self._main_switch.get_active()

        # Noise reduction intensity depends on main toggle
        self._strength_row.set_sensitive(nr_enabled)

        # All sections depend on noise reduction except Bluetooth
        self._gate_expander.set_sensitive(nr_enabled)
        self._stereo_expander.set_sensitive(nr_enabled)
        self._eq_expander.set_sensitive(nr_enabled)

        # Update internal sensitivity if enabled
        if nr_enabled:
            self._update_gate_sensitivity()
            self._update_stereo_sensitivity()
            self._update_eq_sensitivity()

    def _update_gate_sensitivity(self) -> None:
        """Update gate controls sensitivity."""
        enabled = self._gate_switch.get_active()
        self._threshold_row.set_sensitive(enabled)
        self._range_row.set_sensitive(enabled)
        self._attack_row.set_sensitive(enabled)
        self._hold_row.set_sensitive(enabled)
        self._release_row.set_sensitive(enabled)

    def _update_stereo_sensitivity(self) -> None:
        """Update stereo controls sensitivity."""
        enabled = self._stereo_switch.get_active()
        self._mode_combo.set_sensitive(enabled)
        # Dual Mono mode doesn't use Effect Intensity
        mode = self._get_selected_stereo_mode()
        width_enabled = enabled and mode != StereoMode.DUAL_MONO
        self._width_row.set_visible(width_enabled)

    def _update_eq_sensitivity(self) -> None:
        """Update EQ controls sensitivity."""
        enabled = self._eq_switch.get_active()
        self._eq_preset_combo.set_sensitive(enabled)
        for slider in self._eq_sliders:
            slider.set_sensitive(enabled)

    def _get_selected_stereo_mode(self) -> StereoMode:
        """Get currently selected stereo mode from combo (excludes MONO)."""
        index = self._mode_combo.get_selected()
        stereo_modes = [
            StereoMode.DUAL_MONO,
            StereoMode.RADIO,
            StereoMode.VOICE_CHANGER,
        ]
        return (
            stereo_modes[index]
            if 0 <= index < len(stereo_modes)
            else StereoMode.DUAL_MONO
        )

    # =========================================================================
    # Spectrum Analyzer Integration
    # =========================================================================

    def update_audio_level(self, level: float) -> None:
        """
        Update spectrum analyzer with audio level.

        Args:
            level: Audio level (0.0 to 1.0)
        """
        if self._spectrum:
            self._spectrum.set_level(level)

    def update_spectrum_bands(self, bands: list[float]) -> None:
        """
        Update spectrum analyzer with frequency band data.

        Args:
            bands: List of frequency band levels (0.0 to 1.0)
        """
        if self._spectrum:
            self._spectrum.set_bands(bands)

    # =========================================================================
    # Event Handlers
    # =========================================================================

    def _update_service_state(self) -> None:
        """
        Update service state based on noise reduction master toggle.

        Enable Noise Reduction is the master control - service only runs when it's enabled.
        Other features (Gate, Stereo, EQ) are sub-features that only work when NR is active.
        """
        # Master control: noise reduction must be enabled for service to run
        should_run = self._settings.noise_reduction.enabled

        if should_run:
            if self._pipewire.is_enabled():
                # Already running, just update config (restart service to flush changes)
                self._pipewire.apply_config(
                    self._settings, on_complete=self._schedule_monitor_restart
                )
            else:
                # Not running, need to start
                # Use idle_add to run async start logic
                GLib.idle_add(self._start_noise_reduction)
        else:
            # Noise reduction disabled, stop service
            # Use idle_add to run async stop logic
            GLib.idle_add(self._stop_noise_reduction)

    def _on_main_toggle(self, active: bool) -> None:
        """Handle main toggle state change."""
        # Skip if we're loading initial state
        if self._loading:
            return

        is_active = self._main_switch.get_active()
        logger.info("Noise reduction toggle: %s", is_active)

        self._settings.noise_reduction.enabled = is_active
        self._settings_service.save(self._settings)

        # Sync state to PipeWire service
        self._pipewire._noise_reduction_enabled = is_active

        self._update_service_state()
        self._update_all_controls_sensitivity()

    def _start_noise_reduction(self) -> bool:
        """Start noise reduction in background thread."""
        import asyncio
        import threading

        def _run_in_thread():
            async def _start() -> None:
                success = await self._pipewire.start(self._settings)
                if success:
                    logger.info("Noise reduction started successfully")

                    if self._on_toast:
                        GLib.idle_add(self._on_toast, "Noise reduction enabled", 2)

                    # Update monitor to use filtered source
                    GLib.idle_add(self._update_monitor_state)
                else:
                    GLib.idle_add(lambda: self._main_switch.set_active(False))
                    logger.error("Failed to start noise reduction")
                    if self._on_toast:
                        GLib.idle_add(
                            self._on_toast, "Failed to start noise reduction", 4
                        )

            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(_start())
            finally:
                loop.close()

        thread = threading.Thread(target=_run_in_thread, daemon=True)
        thread.start()

        return False

    def _stop_noise_reduction(self) -> bool:
        """Stop noise reduction in background thread."""
        import asyncio
        import threading

        def _run_in_thread():
            async def _stop() -> None:
                await self._pipewire.stop()
                logger.info("Noise reduction stopped")

                # Update monitor to use raw source
                GLib.idle_add(self._update_monitor_state)

                if self._on_toast:
                    GLib.idle_add(self._on_toast, "Noise reduction disabled", 2)

            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(_stop())
            finally:
                loop.close()

        thread = threading.Thread(target=_run_in_thread, daemon=True)
        thread.start()

        return False

    def _schedule_monitor_restart(self) -> None:
        """
        Restart audio monitor safely.

        This is now a helper that gets passed as a callback to PipeWireService,
        ensuring we only restart AFTER the service is fully up.
        """
        if self._audio_monitor and hasattr(self._audio_monitor, "restart"):
            logger.info("Restarting audio monitor after service update")
            self._audio_monitor.restart()

    def _schedule_eq_config_update(self) -> None:
        """Schedule EQ config file update with debouncing."""
        # Cancel previous timer if exists
        if self._eq_config_timer:
            GLib.source_remove(self._eq_config_timer)

        # Schedule config update after 500ms of inactivity
        self._eq_config_timer = GLib.timeout_add(500, self._apply_eq_config_update)

    def _apply_eq_config_update(self) -> bool:
        """Apply EQ config update to PipeWire config file.

        Note: This is no longer needed since EQ band changes use live updates.
        Keeping the method for compatibility but it does nothing.
        """
        self._eq_config_timer = 0
        # Removed apply_config() - EQ bands use live pw-cli updates, no restart needed
        return False  # Don't repeat

    def _schedule_settings_save(self) -> None:
        """Schedule a debounced settings save."""
        if self._save_settings_timer:
            GLib.source_remove(self._save_settings_timer)
        self._save_settings_timer = GLib.timeout_add(500, self._save_settings_debounced)

    def _save_settings_debounced(self) -> bool:
        """Save settings to disk (called by timer)."""
        self._save_settings_timer = 0
        self._settings_service.save(self._settings)
        return False

    def _start_state_polling(self) -> None:
        """Start polling for external state changes (e.g., from plasmoid)."""
        # Poll every 2 seconds
        self._state_poll_timer = GLib.timeout_add(2000, self._check_external_state)
        # Initialize with current state
        self._last_known_enabled_state = self._pipewire.is_enabled()

    def _stop_state_polling(self) -> None:
        """Stop the state polling timer."""
        if self._state_poll_timer:
            GLib.source_remove(self._state_poll_timer)
            self._state_poll_timer = 0

    def _check_external_state(self) -> bool:
        """Check if the noise reduction state changed externally."""
        try:
            current_state = self._pipewire.is_enabled()
            if (
                self._last_known_enabled_state is not None
                and current_state != self._last_known_enabled_state
            ):
                logger.info(
                    "External state change detected: %s -> %s",
                    self._last_known_enabled_state,
                    current_state,
                )
                # Update UI to match external change
                self._loading = True
                try:
                    self._main_switch.set_active(current_state)
                    self._settings.noise_reduction.enabled = current_state
                    self._update_all_controls_sensitivity()
                finally:
                    self._loading = False
            self._last_known_enabled_state = current_state
        except Exception:
            logger.exception("Error checking external state")
        return True  # Continue polling

    def _on_strength_changed(self, value: float) -> None:
        """Handle strength slider change."""
        if self._loading:
            return

        logger.debug("Strength changed: %.2f", value)

        strength_percent = int(value * 100)
        self._strength_row.set_subtitle(
            _(
                "Removal aggressiveness. High values clean more but may alter voice. ({percent}%)"
            ).format(percent=strength_percent)
        )
        self._settings.noise_reduction.strength = value
        # Debounce save to avoid blocking UI with disk I/O
        self._schedule_settings_save()
        self._pipewire.set_strength(value)

    def _on_gate_toggled(self, active: bool) -> None:
        """Handle gate toggle."""
        if self._loading:
            self._update_gate_sensitivity()
            return

        logger.info("Gate toggled: %s", active)

        self._settings.gate.enabled = active
        self._settings_service.save(self._settings)
        self._update_gate_sensitivity()

        # Sync state to PipeWire service
        self._pipewire.set_gate_enabled(active)

        if self._pipewire.is_enabled():
            self._pipewire.apply_config(
                self._settings, on_complete=self._schedule_monitor_restart
            )

    def _on_threshold_scale_changed(self, value: float) -> None:
        """Handle gate threshold scale change."""
        if self._loading:
            return

        # Map 0-100% -> -60dB to 0dB
        t_min = -60
        t_max = 0
        threshold_db = int(t_min + (value / 100.0 * (t_max - t_min)))

        logger.debug("Threshold changed: %.1f%% -> %d dB", value, threshold_db)

        self._settings.gate.threshold_db = threshold_db
        self._threshold_row.set_subtitle(f"Activates at {threshold_db} dB")
        self._schedule_settings_save()
        self._pipewire.set_gate_threshold(threshold_db)

    def _on_range_scale_changed(self, value: float) -> None:
        """Handle gate range scale change."""
        if self._loading:
            return

        # Map 0-100% -> 0dB to -80dB
        r_max_red = -80
        range_db = int((value / 100.0) * r_max_red)

        logger.debug("Range changed: %.1f%% -> %d dB", value, range_db)

        self._settings.gate.range_db = range_db
        self._range_row.set_subtitle(f"Reduction amount ({range_db} dB)")
        self._schedule_settings_save()
        self._pipewire.set_gate_range(range_db)

    def _on_attack_changed(self, value: float) -> None:
        """Handle gate attack change."""
        if self._loading:
            return

        # Log Scale 1..400
        ms = self._to_log_scale(value, 1.0, 400.0)
        ms = round(ms, 1)

        self._attack_row.set_subtitle(f"How fast gate opens ({ms:.1f} ms)")
        self._settings.gate.attack_ms = ms
        self._schedule_settings_save()
        self._pipewire.set_gate_attack(ms)

    def _on_hold_changed(self, value: float) -> None:
        """Handle gate hold change."""
        if self._loading:
            return

        # Log Scale 50..1800
        ms = self._to_log_scale(value, 50.0, 1800.0)
        ms = round(ms, 1)

        self._hold_row.set_subtitle(f"Min open time ({ms:.1f} ms)")
        self._settings.gate.hold_ms = ms
        self._schedule_settings_save()
        self._pipewire.set_gate_hold(ms)

    def _on_release_changed(self, value: float) -> None:
        """Handle gate release change."""
        if self._loading:
            return

        # Log Scale 10..2250
        ms = self._to_log_scale(value, 10.0, 2250.0)
        ms = round(ms, 1)

        self._release_row.set_subtitle(f"Decay time ({ms:.1f} ms)")
        self._settings.gate.release_ms = ms
        self._schedule_settings_save()
        self._pipewire.set_gate_release(ms)

    def _on_stereo_toggled(self, active: bool) -> None:
        """Handle stereo toggle."""
        if self._loading:
            self._update_stereo_sensitivity()
            return

        logger.info("Stereo toggled: %s", active)

        self._settings.stereo.enabled = active
        self._settings_service.save(self._settings)
        self._update_stereo_sensitivity()

        # Sync state to PipeWire service
        self._pipewire.set_stereo_enabled(active)

        # Stereo enable/disable requires restart (structural change)
        if self._pipewire.is_enabled():
            self._pipewire.apply_config(
                self._settings, on_complete=self._schedule_monitor_restart
            )
        else:
            # If disabled, we still need to restart monitor to update channel mixing logic
            self._schedule_monitor_restart()

    def _update_stereo_ui_labels(self, mode: StereoMode) -> None:
        """Update stereo UI labels/marks based on mode."""
        width_percent = int(self._settings.stereo.width * 100)

        if mode == StereoMode.RADIO:
            self._width_row.set_title(_("Radio Effect Intensity"))
            self._width_scale.clear_marks()
            self._width_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, _("Smooth"))
            self._width_scale.add_mark(1.0, Gtk.PositionType.BOTTOM, _("Intense"))
            self._width_row.set_subtitle(
                _("Adjust compression and presence level. ({percent}%)").format(
                    percent=width_percent
                )
            )
        elif mode == StereoMode.VOICE_CHANGER:
            self._width_row.set_title(_("Voice Tonality (Pitch)"))
            self._width_scale.clear_marks()
            self._width_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, _("Deep"))
            self._width_scale.add_mark(0.5, Gtk.PositionType.BOTTOM, _("Normal"))
            self._width_scale.add_mark(1.0, Gtk.PositionType.BOTTOM, _("Sharp"))
            self._width_row.set_subtitle(
                _(
                    "Adjust voice pitch. 0%=Deep, 50%=Normal, 100%=Sharp. ({percent}%)"
                ).format(percent=width_percent)
            )
        else:
            self._width_row.set_title(_("Stereo Expansion"))
            self._width_scale.clear_marks()
            self._width_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, _("Focus"))
            self._width_scale.add_mark(1.0, Gtk.PositionType.BOTTOM, _("Wide"))
            self._width_row.set_subtitle(
                _("Soundstage width. Higher = more immersive. ({percent}%)").format(
                    percent=width_percent
                )
            )

    def _on_mode_selected(self, index: int) -> None:
        """Handle stereo mode selection."""
        if self._loading:
            return

        stereo_modes = [
            StereoMode.DUAL_MONO,
            StereoMode.RADIO,
            StereoMode.VOICE_CHANGER,
        ]
        if 0 <= index < len(stereo_modes):
            mode = stereo_modes[index]
            logger.info("Stereo mode: %s", mode.value)

            self._settings.stereo.mode = mode
            self._settings_service.save(self._settings)

            self._update_stereo_ui_labels(mode)
            self._update_stereo_sensitivity()

            self._pipewire.set_stereo_mode(mode)
            if self._pipewire.is_enabled():
                self._pipewire.apply_config(
                    self._settings, on_complete=self._schedule_monitor_restart
                )
            else:
                # If disabled, we still need to restart monitor to update channel mixing logic
                self._schedule_monitor_restart()

    def _on_width_changed(self, value: float) -> None:
        """Handle stereo width change."""
        if self._loading:
            return

        logger.debug("Stereo width: %.2f", value)

        width_percent = int(value * 100)
        mode = self._settings.stereo.mode

        if mode == StereoMode.RADIO:
            self._width_row.set_subtitle(
                _("Adjust compression and presence level. ({percent}%)").format(
                    percent=width_percent
                )
            )
        else:
            self._width_row.set_subtitle(
                _("Soundstage width. Higher = more immersive. ({percent}%)").format(
                    percent=width_percent
                )
            )

        self._settings.stereo.width = value
        self._schedule_settings_save()
        # Live update via pw-cli (no restart needed)
        self._pipewire.set_stereo_width(value)

    def _on_eq_toggled(self, active: bool) -> None:
        """Handle EQ toggle."""
        if self._loading:
            self._update_eq_sensitivity()
            return

        logger.info("EQ toggled: %s", active)

        self._settings.equalizer.enabled = active
        # Schedule settings save
        self._schedule_settings_save()
        logger.info("EQ settings saved: enabled=%s", active)
        self._update_eq_sensitivity()

        # Sync state to PipeWire service
        self._pipewire.set_eq_enabled(active)

        self._update_service_state()

    def _on_eq_preset_selected(self, index: int) -> None:
        """Handle EQ preset selection."""
        if self._loading:
            return

        preset_names = list(EQ_PRESETS.keys())
        if 0 <= index < len(preset_names):
            preset_name = preset_names[index]
            logger.info("EQ preset: %s", preset_name)

            # If selecting custom - just update settings (don't overwrite bands)
            if preset_name == "custom":
                self._settings.equalizer.preset = preset_name
                self._schedule_settings_save()
                return

            # For other presets, update bands
            preset_data = EQ_PRESETS[preset_name]
            bands = preset_data["bands"]

            # Use loading flag to prevent _on_eq_band_changed callbacks
            # from firing during preset application
            self._loading = True
            try:
                for scale, value in zip(self._eq_sliders, bands, strict=False):
                    scale.set_value(value)
            finally:
                self._loading = False

            # Now save the preset
            self._settings.equalizer.preset = preset_name
            self._settings.equalizer.bands = list(bands)
            self._schedule_settings_save()

            # Live update all bands immediately (no restart needed)
            self._pipewire.set_eq_bands(list(bands))

    def _on_eq_band_changed(self, index: int, value: float) -> None:
        """Handle EQ band value change."""
        if self._loading:
            return

        logger.debug("EQ band %d changed: %.1f dB", index, value)

        # Debounce EQ updates to avoid spamming the service
        if self._eq_config_timer:
            GLib.source_remove(self._eq_config_timer)

        # Update settings immediately
        if index < len(self._settings.equalizer.bands):
            self._settings.equalizer.bands[index] = value

        # If we just moved a slider and preset isn't 'custom', change to 'custom'
        # But we must avoid recursion or resetting sliders
        if self._settings.equalizer.preset != "custom":
            self._settings.equalizer.preset = "custom"
            # Update combo box without triggering 'selected' signal logic that resets bands
            preset_names = list(EQ_PRESETS.keys())
            if "custom" in preset_names:
                idx = preset_names.index("custom")
                # We need to temporarily block signal to avoid re-triggering _on_eq_preset_selected
                self._eq_preset_combo.set_selected(idx)

        # Schedule service update
        self._eq_config_timer = GLib.timeout_add(100, self._apply_eq_config)
        self._schedule_settings_save()

    def _apply_eq_config(self) -> bool:
        """Apply accumulated EQ changes to service."""
        logger.debug("Applying accumulated EQ changes")
        self._pipewire.set_eq_bands(list(self._settings.equalizer.bands))
        self._eq_config_timer = 0
        return False

    def restore_defaults(self) -> None:
        """Restore all settings to default values."""
        logger.info("Restoring default settings")

        # Create new settings with defaults
        self._settings = AppSettings()

        # Save to disk
        self._settings_service.save(self._settings)

        # Reload UI to reflect defaults
        self._load_state()

        # Apply all settings to service
        # We must perform a clean restart to ensure all defaults (EQ, Stereo, etc) are applied
        # We use a dedicated thread to ensure stop() completes before start() begins
        import asyncio
        import threading

        def _restart_sequence():
            async def _async_restart():
                # Force stop first
                await self._pipewire.stop()

                # If enabled by default, start it up
                if self._settings.noise_reduction.enabled:
                    success = await self._pipewire.start(self._settings)
                    if success:
                        GLib.idle_add(
                            self._on_toast, _("Settings successfully restored"), 3
                        )
                        GLib.idle_add(self._update_monitor_state)

            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(_async_restart())
            finally:
                loop.close()

        thread = threading.Thread(target=_restart_sequence, daemon=True)
        thread.start()

        # Show confirmation (immediate UI feedback)
        if self._on_toast:
            self._on_toast(_("All settings have been restored!"), 3)

    def _on_bluetooth_toggled(self, active: bool) -> None:
        """Handle Bluetooth auto-switch toggle."""
        if self._loading:
            return
        logger.info("Bluetooth autoswitch: %s", active)

        self._settings.bluetooth.auto_switch_headset = active
        self._settings_service.save(self._settings)
        self._pipewire.set_bluetooth_autoswitch(active)

    def _on_monitor_toggled(self, active: bool) -> None:
        """Handle headphone monitor toggle."""
        if self._loading:
            return

        logger.info("Headphone monitor toggled: %s", active)

        self._settings.monitor.enabled = active
        self._settings_service.save(self._settings)

        # Update rows sensitivity
        self._monitor_delay_row.set_sensitive(active)

        if active:
            # Start monitor with current delay
            source = self._get_monitor_source()
            channels = self._get_monitor_channels()
            self._monitor_service.start_monitor(
                source=source,
                delay_ms=self._settings.monitor.delay_ms,
                channels=channels,
            )
        else:
            # Stop monitor
            self._monitor_service.stop_monitor()

    def _on_monitor_delay_changed(self, value: float) -> None:
        """Handle monitor delay change."""
        if self._loading:
            return

        delay_ms = int(value)
        logger.debug("Monitor delay: %d ms", delay_ms)

        self._monitor_delay_row.set_subtitle(
            _("Monitor feedback delay. Useful for checking sync. ({delay} ms)").format(
                delay=delay_ms
            )
        )
        self._settings.monitor.delay_ms = delay_ms
        self._settings_service.save(self._settings)

        # Debounce monitor update to avoid spamming process restarts
        if self._monitor_delay_timer:
            GLib.source_remove(self._monitor_delay_timer)

        self._monitor_delay_timer = GLib.timeout_add(
            300, self._apply_monitor_delay, delay_ms
        )

    def _apply_monitor_delay(self, delay_ms: int) -> bool:
        """Apply monitor delay after debounce."""
        self._monitor_delay_timer = 0
        if self._settings.monitor.enabled:
            self._monitor_service.set_delay(delay_ms)
        return False

    def _get_monitor_channels(self) -> int:
        """Get appropriate channel count for monitor based on stereo settings."""
        if self._settings.noise_reduction.enabled:
            # Filtered source is already processed.
            # We should monitor it as stereo (2) to hear the result correctly.
            return 2

        # Raw source monitoring
        # If stereo is disabled, or mode is MONO/DUAL_MONO, force mono downmix (1)
        # to ensure signal on both ears if source is 1ch-silent-stereo.
        mode = self._settings.stereo.mode
        if not self._settings.stereo.enabled or mode in (
            StereoMode.MONO,
            StereoMode.DUAL_MONO,
        ):
            return 1

        return 2

    def _get_monitor_source(self) -> str:
        """Get the appropriate audio source for monitoring."""
        # Prefer filtered source if noise reduction is active
        if self._settings.noise_reduction.enabled:
            # Try to detect the filter source
            import json
            import subprocess

            try:
                result = subprocess.run(
                    ["pw-dump"],
                    capture_output=True,
                    text=True,
                    timeout=2,
                )

                if result.returncode == 0:
                    data = json.loads(result.stdout)
                    for obj in data:
                        if obj.get("type") != "PipeWire:Interface:Node":
                            continue
                        props = obj.get("info", {}).get("props", {})
                        if props.get("filter.smart.name") == "big.filter-microphone":
                            name = props.get("node.name")
                            if name:
                                return name
            except Exception:
                pass

        # Fallback to default source
        try:
            result = subprocess.run(
                ["pactl", "get-default-source"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                return result.stdout.strip()
        except Exception:
            pass

        # Last resort
        return "@DEFAULT_SOURCE@"

    def _update_monitor_state(self) -> None:
        """Update monitor state when noise reduction status changes."""
        if not self._settings.monitor.enabled:
            return

        def _do_update() -> bool:
            # Restart monitor to pick up new source (filtered or raw)
            source = self._get_monitor_source()
            channels = self._get_monitor_channels()
            self._monitor_service.start_monitor(
                source=source,
                delay_ms=self._settings.monitor.delay_ms,
                channels=channels,
            )
            return False

        # Small delay to ensure PipeWire graph is updated
        GLib.timeout_add(500, _do_update)
