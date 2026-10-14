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

`app/biglinux_microphone/` is organized around a beginner-first navigation shell:

- `ui/view.py` — navigation, page lifecycle, backend runner, meter and advanced monitor ownership.
- `ui/state.py` — immutable-ish UI snapshot + serialized backend reads/writes.
- `ui/backend.py` — worker execution and error delivery off the GTK main thread.
- `ui/pages/` — one user task/area per module.
- `ui/widgets/` — custom controls only where native libadwaita widgets are insufficient.
- `cli.py` — thin process/tool boundary used by the UI.

The same sidebar is available on Home and every settings or testing page; it becomes a drawer on narrow windows. Home exposes reduction mode, microphone selection, input volume, calibration and recording actions directly. Calibration and source selection are transient tasks pushed through `Adw.NavigationView`, with a Back action returning to the previous page.

Only Home is constructed at startup. Other pages are created on first navigation and receive the current snapshot before being shown. Returning to a page reuses its widgets, with task cleanup handled by its visibility lifecycle.

### UI state rules

- Rendering a snapshot must not write settings.
- Backend calls that can block run off the GTK main thread.
- Generation/cancellation guards prevent obsolete callbacks from updating a closed/replaced page.
- The application follows the system color scheme.
- Home shows a 30-band spectrum (50 Hz–20 kHz) and a separate sample-peak meter (−60–0 dBFS). Frequency analysis runs only while Home or the residual-noise editor is visible. Hidden/minimized windows and pages without meters release their capture stream and stop animation; changing the source identity invalidates queued frames and clears the held peak.

The Home meter uses the OBS sample-peak convention, including the previous four samples at block boundaries, with immediate attack, the medium decay profile (11.76 dB/s), a 20-second peak hold and −20/−9 dBFS colour boundaries. Spectrum height retains the former −75..−20 dBFS band sensitivity; frequency energy never substitutes for waveform peak. This is not an inter-sample true-peak or LUFS measurement. The meter reads the same processed endpoint as the microphone test; comparison with OBS requires that same source, unity gain and no additional OBS filters. Animation is bounded to 30 updates/s; FFT stays on the capture worker.

`status` returns saved settings separately from observed PipeWire endpoints. The microphone graph exports its denoising flag and model identifier, so an absent filter cannot appear enabled just because its switch was saved. Idle/suspended endpoints are ready for clients, not proof of an acoustic result. Microphone tests use the observed chain even when only EQ is enabled, and refuse to fall back to raw audio when the requested chain is missing.

While the window exists, one `pactl subscribe` connection watches source, sink, server and card events. A GIO file monitor also observes atomic settings replacements. Refreshes coalesce; client/source-output events are ignored because the application's own queries and captures generate them. Recording/calibration waits use a separate worker so they cannot block device refreshes. Shutdown cancels the subscription and owned captures.

## Rust backend

`src/config/` contains the persistent settings model and policy. `src/pipeline/` builds desired PipeWire filter graphs. `src/services/` performs runtime I/O and reconciliation.

The persisted settings file is:

```text
$XDG_CONFIG_HOME/biglinux-microphone/settings.json
```

with the usual `~/.config` fallback.

Desired state and effective state are intentionally different: the saved model is the person's choice, and the running graph reports the model it actually loaded. The settings know three models (DFN3, DFN3-LL, DPDFNet); a file naming any other number loads as DFN3.

Both `settings` and `status` use strict loading: a malformed settings file reports failure and remains untouched.

Latency reports are planning estimates, not end-to-end measurements. Native DFN3, DFN3-LL and DPDFNet-2 have separate reference delays (1919, 959 and 2880 samples at 48 kHz, measured through the packaged LADSPA plugins with attenuation disabled; DPDFNet-2 also declares its delay on its latency port). With the voice gate on, every denoiser plugin holds its output to 3168 samples (66 ms) so the Silero detector decides on each sample's own input; the report adds that as a `voice-gate` stage. The output chain never runs the voice gate. Device, application and network buffering remain outside the estimate, and a different live plugin build still needs validation.

## Audio graph and routing

The backend integrates with the existing PipeWire daemon rather than launching a second audio server. WirePlumber policy and generated filter chains are responsible for routing. `biglinux-microphone-pwloader` hosts modules that must remain alive.

Model/plugin availability is runtime-dependent. A settings value naming a model is not proof that its plugin loaded successfully; diagnostics and graph observation are separate concerns.

## Calibration and monitoring

Volume can be adjusted manually or calibrated for a microphone. The Voice volume page also restores the backend's existing bounded automatic gain control: a saved minimum/maximum range enables it, and disabling it clears that range. The backend starts its gain-control service only when a saved device has a range. Home shows this state in place of the manual slider, so the two controls cannot compete for the same capture volume. Rendering or opening the window never enables it.

The everyday microphone test records a short local sample and plays it back. Continuous delayed monitoring is an advanced feature and must terminate when disabled or when the UI exits.

When a supported native denoiser is running and `ffmpeg` is available, the comparison page records the selected input once and renders a second version with that denoiser. Both versions use matched RMS levels and shared peak headroom; the preview excludes EQ, echo cancellation and other downstream effects. Rendering and cancellation run off the GTK thread, and leaving the page removes both private recordings. The Home shortcut is hidden when this capability is unavailable; the sidebar page remains discoverable and explains why recording is unavailable.

Temporary samples are local, mode 0600, and removed on leaving the task or
quitting the application, including the application Quit action. An existing
sample remains replayable while its test page is open. Successful capture also
requires a valid, non-truncated mono 48 kHz PCM WAV, not merely a zero exit code.
PipeWire 1.6.8's sample-limit exit code is accepted only when stderr contains
just the sample path and the WAV contains exactly the requested number of
frames; real diagnostics and truncated recordings remain errors.
Cancellation pins the original calibration device and rollback level. Window
close hands rollback to a bounded non-daemon worker: GTK does not wait for
PipeWire, but normal interpreter exit waits for that cleanup. A forced SIGKILL
or power loss cannot provide the same cleanup guarantee. A stale
callback cannot replace the recording or playback of a newer session.

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
