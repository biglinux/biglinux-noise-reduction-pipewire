# Architecture

BigLinux Microphone separates the user interface from the audio engine. The Python process owns the window and short-lived UI helpers; the Rust backend owns persistent settings, policy and PipeWire graph reconciliation.

## Process model

| Process | Lifetime | Responsibility |
| --- | --- | --- |
| `biglinux-microphone` | While settings window is open | Python GTK4/libadwaita UI |
| `biglinux-microphone-cli` | Short-lived commands / systemd reconciliation | Read/write settings, calibration, diagnostics, apply desired state |
| `biglinux-microphone-pwloader` | While a hosted PipeWire module is required | Host one PipeWire module in its own client context |
| `biglinux-microphone-probe` | Short-lived | Diagnostic measurements |

The settings window is not a required resident daemon. Event-driven systemd/WirePlumber integration is preferred over keeping Python alive after the window closes.

## Python UI

`app/biglinux_microphone/` is one window: the settings in a sidebar, the live microphone beside them.

- `ui/view.py` — the window, task navigation, backend runner, meter and monitor ownership.
- `ui/controls.py` — the sidebar's setting rows and their cards.
- `ui/state.py` — immutable-ish UI snapshot + serialized backend reads/writes.
- `ui/backend.py` — worker execution and error delivery off the GTK main thread.
- `ui/pages/` — the live panel and the tasks: calibration, diagnostics.
- `ui/widgets/` — custom controls only where native libadwaita widgets are insufficient.
- `cli.py` — thin process/tool boundary used by the UI.

Every setting is a sidebar row that shows its value and opens a card, as in the BigLinux converters (`big_gtk_kit.hover_cards`), the microphone choice included; the sidebar becomes a drawer on narrow windows. The live panel shows the status, the meter and Listen back. Calibration and diagnostics are tasks pushed through `Adw.NavigationView` over the panel, with a Back action returning to it.

The panel and the sidebar are built at startup; a task is created on first navigation and receives the current snapshot before being shown, with its cleanup handled by its visibility lifecycle.

### UI state rules

- Rendering a snapshot must not write settings.
- Backend calls that can block run off the GTK main thread.
- Generation/cancellation guards prevent obsolete callbacks from updating a closed/replaced page.
- The application follows the system color scheme.
- The live panel shows a 30-band spectrum (50 Hz–20 kHz) and a separate sample-peak meter (−60–0 dBFS). Frequency analysis runs only while the panel is visible. Hidden/minimized windows and open tasks release the capture stream and stop animation; changing the source identity invalidates queued frames and clears the held peak.

The meter uses the OBS sample-peak convention, including the previous four samples at block boundaries, with immediate attack, the medium decay profile (11.76 dB/s), a 20-second peak hold and −20/−9 dBFS colour boundaries. Spectrum height retains the former −75..−20 dBFS band sensitivity; frequency energy never substitutes for waveform peak. This is not an inter-sample true-peak or LUFS measurement. The meter reads the processed endpoint applications record; comparison with OBS requires that same source, unity gain and no additional OBS filters. Animation is bounded to 30 updates/s; FFT stays on the capture worker.

`status` returns saved settings separately from observed PipeWire endpoints. The microphone graph exports its denoising flag and model identifier, so an absent filter cannot appear enabled just because its switch was saved. Idle/suspended endpoints are ready for clients, not proof of an acoustic result.

While the window exists, one `pactl subscribe` connection watches source, sink, server and card events. A GIO file monitor also observes atomic settings replacements. Refreshes coalesce; client/source-output events are ignored because the application's own queries and captures generate them. Recording/calibration waits use a separate worker so they cannot block device refreshes. Shutdown cancels the subscription and owned captures.

## Rust backend

`src/config/` contains the persistent settings model and policy. `src/pipeline/` builds desired PipeWire filter graphs. `src/services/` performs runtime I/O and reconciliation.

The persisted settings file is:

```text
$XDG_CONFIG_HOME/biglinux-microphone/settings.json
```

with the usual `~/.config` fallback.

Desired state and effective state differ: the saved model is the person's choice, and the running graph reports the model it actually loaded. The settings know three models (DFN3, DFN3-LL, DPDFNet); a file naming any other number loads as DFN3.

Both `settings` and `status` use strict loading: a malformed settings file reports failure and remains untouched.

Latency reports are planning estimates, not end-to-end measurements. Native DFN3, DFN3-LL and DPDFNet-2 have separate reference delays (1919, 959 and 2880 samples at 48 kHz, measured through the packaged LADSPA plugins with attenuation disabled; DPDFNet-2 also declares its delay on its latency port). With the voice gate on, every denoiser plugin holds its output to 3168 samples (66 ms) so the Silero detector decides on each sample's own input; the report adds that as a `voice-gate` stage. The output chain never runs the voice gate. Device, application and network buffering remain outside the estimate, and a different live plugin build still needs validation.

## Audio graph and routing

The backend integrates with the existing PipeWire daemon rather than launching a second audio server. WirePlumber policy and generated filter chains are responsible for routing. `biglinux-microphone-pwloader` hosts modules that must remain alive.

Model/plugin availability is runtime-dependent. A settings value naming a model is not proof that its plugin loaded successfully; diagnostics and graph observation are separate concerns.

## Calibration and monitoring

Volume can be adjusted manually or calibrated for a microphone. The Microphone volume card also restores the backend's existing bounded automatic gain control: a saved minimum/maximum range enables it, and disabling it clears that range. The backend starts its gain-control service only when a saved device has a range. The card shows this state in place of the manual slider, so the two controls cannot compete for the same capture volume. Rendering or opening the window never enables it.

Listen back, the delayed live return, is how a person hears the result; it must terminate when switched off or when the UI exits.

Calibration cancellation pins the original calibration device and rollback level. Window
close hands rollback to a bounded non-daemon worker: GTK does not wait for
PipeWire, but normal interpreter exit waits for that cleanup. A forced SIGKILL
or power loss cannot provide the same cleanup guarantee.

The continuous monitor runs the package-owned file in isolated Python mode; it
does not depend on importing the private package from a child process via -m.

## Packaging boundary

The BigLinux/Arch package installs the pure-Python UI under:

```text
/usr/lib/biglinux-microphone/
```

The public executable is a relative symlink to `launcher.py`. The launcher uses `/usr/bin/python3 -I` and adds only its resolved package directory to `sys.path`. This keeps the application sources independent of Python minor-version `site-packages` directories while still using distribution-managed PyGObject/numpy.

See [packaging.md](packaging.md).

## Internationalization

The gettext domain is `biglinux-microphone`. English source strings are extracted from the Python UI (and any other maintained translatable surface) into `po/`. The package compiles only locales approved in `po/LINGUAS` to `/usr/share/locale/<lang>/LC_MESSAGES/biglinux-microphone.mo`.

## Failure boundaries

Keep these separate when diagnosing a problem:

- configuration serialization;
- policy/model selection;
- plugin availability;
- PipeWire graph creation;
- routing visibility;
- hardware capture/playback;
- GTK rendering;
- package/runtime dependency compatibility.

A successful process exit does not prove acoustic quality. Unit tests do not prove hardware latency, intelligibility, echo performance or absence of XRUNs.
