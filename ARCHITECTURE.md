# Architecture — Filter noise

## Processes and ownership

One Rust library (`biglinux_microphone`) and four entry points:

| Binary | Responsibility |
| --- | --- |
| `biglinux-microphone` | Relm4 GTK4/libadwaita window and its monitoring resources |
| `biglinux-microphone-cli` | Settings mutations, diagnostics, reconciliation and session watch |
| `biglinux-microphone-pwloader` | Host one PipeWire module in a client process |
| `biglinux-microphone-probe` | Diagnostic measurements |

The mic, echo canceller and playback loaders connect to the existing PipeWire
server. They share the graph's scheduling/clock domain, not one daemon-owned
processing thread. `node.async` is an asynchronous scheduling choice, not an
adaptive-resampler switch; evaluate its added cycle latency separately.

## State contracts

Desired preferences, effective processing settings and observed graph state are
different. A master bypass changes the effective projection without erasing the
user's sub-effect choices. The persistent JSON lives under
`$XDG_CONFIG_HOME/biglinux-microphone`, with the usual home-directory fallback.
Loaders and the applet resolve that same location.

A stable sibling lock serializes participating writers. GUI edits are merged
against their original snapshot; unrelated external fields survive and a
conflicting change to the same field is reported. Invalid JSON is not silently
overwritten. Atomic replacement protects a single file; it is not a multi-file
transaction and does not by itself solve concurrent read-modify-write.

## Applying changes

The UI coalesces edits and executes blocking work outside GTK's main thread.
`services/reconcile.rs` is shared by CLI and GUI: render effective settings,
compare the previous applied snapshot, inspect actual nodes, update live
parameters where possible and reconcile only the affected loaders. AEC precedes
the microphone that consumes it. Missing/error nodes and incomplete updates
cannot be represented as fully applied settings. Failures propagate to callers.

Persistence, generated arguments, service state and plugin inference have
separate failure modes. Successful `systemctl` invocation alone is insufficient;
node presence is checked. Presence is still not proof of useful denoising, so
processing-health counters and listening/measurement tests remain necessary.

## Interface and lifecycle

Relm4 owns the main application state and apply/health generations. The device
picker performs blocking device operations on workers. The tuning page retains
its edit model and distinguishes pending choices from applied settings. A
buffer preview has a bounded lifetime and restores the prior override. Closing
the window must release its monitor without requiring a healthy audio server.

The microphone meter exposes a native GTK value and textual reading; the Cairo
spectrum is supplemental. Respect reduced motion, preserve navigation context,
and use visible labels plus accessible names for controls. Expert CPU affinity
and memory-reservation choices are opt-in, not universal performance guarantees.

## Monitoring and subprocesses

FFT plans, windows, normalization and scratch buffers are reused. Visualization
is bounded and favors recent frames; hidden capture must not intentionally block
a real-time writer. The subprocess boundary uses argv arrays, explicit policies,
nonblocking Unix I/O, bounded captured output, full-operation deadlines and
process-group cleanup. An explicitly empty allow-list denies every executable.

## Dependencies and translation

Native target: Rust >= 1.97.1, GTK >= 4.22, libadwaita >= 1.9, PipeWire >= 1.4,
WirePlumber >= 0.5, systemd user services, GTCRN and SWH LADSPA packages.
Optional neural backends require their matching inference runtimes. Nix uses
the system-allocator feature configuration rather than the BigLinux-specific
jemalloc patch. Portability of packaged integrations must be tested separately.

The gettext domain is `biglinux-microphone`. Rust `i18n` calls, deferred `mark`
literals and QML `i18nd` calls share the catalog. Extraction coverage is checked
independently of PO syntax and translation completeness. Empty UI text must not
be resolved as gettext's reserved metadata header.

## Tests and evidence

Run `scripts/quality-check.sh --ci` for required portable gates,
`scripts/test-ui.sh` for isolated graphical contracts, and
`scripts/test-miri.sh` for the selected pure data-model tests. See `REVIEW.md`
for finding-to-implementation mapping. No passing text-renderer test constitutes
an acoustic benchmark or an accessibility/visual-design certification.
