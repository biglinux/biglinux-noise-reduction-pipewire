# AGENTS.md

Repository instructions for humans using coding agents and for autonomous coding agents.

OpenAI Codex and other agents that support the `AGENTS.md` convention should use this file as the repository-wide source of standing instructions. Do not add parallel `CLAUDE.md`/`GEMINI.md` copies: duplicated instruction files drift. Add a nested `AGENTS.md` only if a subtree genuinely needs stricter or different rules.

## Project goal

BigLinux Microphone is a beginner-first Linux microphone noise-reduction application. The normal user should be able to enable noise reduction, select a microphone, calibrate input level once and test the result without understanding PipeWire graphs, model names, dBFS or frequency-domain controls. Advanced users may opt into model policy, residual-noise thresholds, EQ, echo/routing and diagnostics.

Primary supported deployment: BigLinux/Arch-family systems with PipeWire, WirePlumber, systemd user services, GTK4/libadwaita and distribution-managed Python/PyGObject.

## Read this map before editing

- `app/biglinux_microphone/` — Python GTK4/libadwaita UI and thin platform helpers.
- `app/tests/` — Python UI/state/launcher regression tests.
- `src/` — Rust domain model, audio pipeline and runtime services.
- `src/bin/cli.rs` — backend command surface used by the Python UI/systemd.
- `src/bin/pwloader.rs` — PipeWire module host.
- `src/bin/probe.rs` — diagnostics probe.
- `usr/` — package payload: desktop/AppStream metadata, systemd units, WirePlumber policy, icons and Plasma integration.
- `packaging/arch/` — production BigLinux/Arch package recipe.
- `packaging/launcher.py` — production Python GUI launcher; its isolation and version-neutral path are release invariants.
- `po/` — translation source of truth.
- `docs/` — maintained human documentation. Historical investigation plans live only in `docs/archive/`.
- `docs/research/` — measurements and design research; do not present these as current runtime contracts without checking the code.

## Non-negotiable invariants

1. **Do not put the production GUI in a hard-coded `pythonX.Y/site-packages` path.** The Arch package installs pure Python sources in `/usr/lib/biglinux-microphone` and symlinks `/usr/bin/biglinux-microphone` to the isolated launcher.
2. **Do not scan older Python `site-packages` directories at runtime.** Loading an old package with a newer interpreter can mix incompatible dependencies and hides packaging errors.
3. **Keep `#!/usr/bin/python3 -I` on `packaging/launcher.py`.** Isolated mode blocks `PYTHONPATH`, user site-packages and current-directory shadowing. The launcher adds only the package-owned private root.
4. **Rendering state must not write state.** Building pages, syncing snapshots, navigating pages and closing the window must not mutate audio settings.
5. **Never do blocking backend/PipeWire work on the GTK main thread.** Use the existing backend runner/worker patterns and generation guards.
6. **Cancellation must be real.** Calibration, test playback, monitoring and other subprocess work must be stoppable and cleaned up when the page/window closes.
7. **Temporary audio is private and short-lived.** Keep temporary samples local, mode `0600`, and delete them on completion/cancel/close.
8. **The person chooses the model; the app never swaps it.** The interface offers DFN3 (default), DFN3-LL and DPDFNet and writes `set model`. Nothing changes the saved model by itself, and rendering never writes it. The saved choice and the model the running graph reports are different readings.
9. **Model-specific controls are conditional.** DFN3-only residual thresholds must not be presented as effective under models that do not use them.
10. **The source language for translatable UI strings is English.** Update gettext extraction inputs whenever user-facing strings move or new files are added.
11. **Do not treat synthetic tests as acoustic validation.** Unit tests cannot prove intelligibility, echo cancellation quality, XRUN behavior or hardware latency.
12. **Do not turn environment failures into product patches.** Classify failures as CODE, TEST, ENV, DEPENDENCY or UNKNOWN before changing code.

## Work order

For non-trivial changes:

1. inspect the real files and current Git diff;
2. identify the smallest affected contracts;
3. make the change;
4. run targeted tests first;
5. run broader relevant tests;
6. execute/render the GTK app when UI behavior changed;
7. review the diff for unrelated changes and dead code;
8. update documentation/tests together with behavior;
9. preserve a clean checkpoint before expensive or risky work.

## Environment safety

Before expensive builds, inspect cgroup limits, not only host memory:

```bash
free -h
df -h /mnt/data
nproc
cat /sys/fs/cgroup/memory.max 2>/dev/null || true
cat /sys/fs/cgroup/memory.current 2>/dev/null || true
cat /sys/fs/cgroup/memory.events 2>/dev/null || true
```

For Rust in constrained environments:

```bash
export CARGO_BUILD_JOBS=1
```

Use `tools/check-release.sh` for a sequential, locked, offline gate and
`tools/run-bounded.py` for a single process group. A 4 GiB worker starts with
one job and no parallel build/linker. Keep logs on disk. Read the existing
process/log after a transport timeout instead of starting duplicate work.

Use timeouts around GUI/build commands. Do not download or replace system toolchains when an offline/project-provided toolchain is available.

## Validation commands

Python, always after Python changes:

```bash
python3 -m compileall -q app/biglinux_microphone app/tests
PYTHONPATH=app python3 -m pytest -q app/tests
```

Launcher-specific:

```bash
PYTHONPATH=app python3 -m pytest -q app/tests/test_launcher.py
```

Rust, when a compatible Rust 1.98.1 toolchain is available:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Metadata/package changes:

```bash
bash -n packaging/arch/PKGBUILD
desktop-file-validate usr/share/applications/br.com.biglinux.microphone.desktop
appstreamcli validate --no-net usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml
```

Translation changes:

```bash
python3 tools/validate-translations.py
```

If a required tool is missing, report `DEPENDENCY`/`ENV`; do not claim that check passed.

## UI expectations

- Home is beginner-first: one obvious primary action per state.
- Use libadwaita-native patterns before custom drawing.
- Keep technical names out of the first-run path unless they explain an error.
- Preserve keyboard, touch and screen-reader semantics; use accessible labels for icon-only controls.
- Verify narrow layouts and large text. 1024×600 is a required desktop target; smaller widths must reflow rather than clip important actions.
- Respect the system color scheme. Do not force dark or light mode.
- A hidden page should not continue high-rate visual work without a reason.
- Use `Gtk.Stack`, not `Adw.ViewStack`: libadwaita 1.9.3 retains the latter's animation and target after disposal. The Python API is banned by Ruff in `app/pyproject.toml`; Clippy does not inspect the Python frontend. Use `Gtk.AboutDialog` for About: libadwaita 1.9.3 retains the contents of `Adw.AboutDialog` after close. A leak sweep may allow one `AdwBreakpoint` per window on libadwaita 1.9.3, whose bin fails to release it, but never application widgets or captured handlers.
- GTK callbacks must borrow their owner through `ui.callbacks.weak_callback`; pass an unbound method or a lambda whose first argument is the owner. Page contexts and service callbacks must not retain their view. `app/tests/test_window_resources.py` proves finalization by qdata destructors while the process stays alive.

## Audio/runtime expectations

- Keep PipeWire graph ownership in the Rust backend/service layer, not in persistent Python workers.
- Prefer event-driven reconciliation over resident polling.
- A paused `AudioMonitor` parks its worker; activation, shutdown and drop must wake it. Capture-retry waits use the deadline and remain interruptible. Do not reintroduce a periodic paused poll.
- Do not perform allocations, locks, filesystem access or subprocess work in a real-time callback.
- Preserve rollback/cleanup paths before adding a new audio action.
- A successful process start is not proof a PipeWire node is useful; diagnostics and hardware tests remain separate.

## Packaging expectations

The production Arch layout is documented in `docs/packaging.md`. Any packaging change must preserve:

```text
/usr/bin/biglinux-microphone -> ../lib/biglinux-microphone/launcher.py
/usr/lib/biglinux-microphone/launcher.py
/usr/lib/biglinux-microphone/biglinux_microphone/
```

Do not reintroduce a Python-minor-version directory for the application itself. PyGObject/numpy remain distribution dependencies and must track the active system Python.

Other package formats must not be added as unverified templates. A Nix/Flatpak/etc. definition needs an end-to-end build + launch test before the README may advertise it.

## Documentation rules

- `README.md` is for users and prospective contributors: concise benefits, screenshots, quick start and accurate links.
- `AGENTS.md` is operational context, not marketing copy.
- `CONTRIBUTING.md` is the human contribution workflow.
- `docs/architecture.md` describes current architecture only.
- `docs/research/` may contain dated measurements and alternatives.
- `docs/archive/` is historical; never cite it as current behavior without verification.
- Remove obsolete plans, generated evidence, caches and one-off review bundles from the main source tree.
- Comments explain *why*, invariants and non-obvious hazards. Do not narrate obvious syntax.

## Before declaring completion

Report separately:

- what was inspected;
- what was changed;
- exact tests executed and their results;
- checks that were not run and why;
- generated artifacts/checkpoints;
- remaining hardware or release blockers.
