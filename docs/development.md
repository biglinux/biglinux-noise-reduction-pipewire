# Development guide

## Runtime and tools

Use the project-pinned Rust 1.98.1 and distribution-managed Python/PyGObject. The
native package requires GTK 4.22+, libadwaita 1.9+, NumPy, Cairo, PipeWire,
WirePlumber and its declared plugin dependencies. Python application source is
kept independent of the interpreter's minor-version directory; C extensions
must still match the active system Python.

Reuse existing tools and offline artifacts. Do not replace system libraries,
download a second global toolchain or modify DSP dependencies merely to make
an incomplete offline environment appear green.

## Source execution

```bash
CARGO_BUILD_JOBS=1 cargo build --release --locked
PATH="${CARGO_TARGET_DIR:-$PWD/target}/release:$PATH" PYTHONPATH=app \
  python3 -m biglinux_microphone --debug
```

The three Rust binaries must be on PATH. Source execution is not a substitute
for installing the plugins, WirePlumber policy and user services. See
[packaging.md](packaging.md) for the production private launcher.

## Validation

Use [testing.md](testing.md) for the canonical sequential gate. Begin with the
small affected Python test or Rust target. Use one Cargo job on 4 GiB workers;
never run a second build while an earlier transport call is still executing.
The gate logs to disk, checks cgroup headroom and disk space, owns a process
group and stops it at its deadline. Never count an unavailable check as passed.

## Code and UI contracts

Settings reads and rendering must not write settings. Blocking work belongs in
the backend runner; callbacks carry session/generation identity. A page leaving
the screen invalidates obsolete completion handlers. Calibration restores the
captured original device/volume, not a later refreshed default device.

The spectrum is retained. Advanced controls are created only when needed and
removed from layout and accessibility exposure when hidden. Verify actual
windows at desktop/narrow sizes, 1024×600, large text, dark/light and high
contrast. Keyboard focus and screen-reader acceptance are distinct from a PNG.

Keep realtime callbacks free of filesystem access, subprocesses, blocking
locks and allocation-heavy work. Unit-policy tests do not prove DSP quality.

Maintained docs describe current behavior. Historical records live in
`docs/archive/`; environment evidence belongs in delivery artifacts, not the
source root. Preserve a Git checkpoint before expensive work and verify it by
cloning/extracting elsewhere and running at least one real test.
