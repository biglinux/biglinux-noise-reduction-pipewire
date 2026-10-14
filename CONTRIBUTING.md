# Contributing to BigLinux Microphone

Thanks for helping improve microphone audio on Linux. Contributions do not need to be large: hardware reports, translations, accessibility fixes and reproducible bug reports are all useful.

## Before changing code

1. Read [docs/architecture.md](docs/architecture.md) for process ownership.
2. For UI work, read [docs/ui-ux.md](docs/ui-ux.md).
3. For audio/backend work, check the dated evidence in [docs/research/](docs/research/) before changing a policy based on performance assumptions.

## Code map

- `app/biglinux_microphone/`: Python GTK4/libadwaita UI and thin platform helpers; `app/tests/` holds its tests.
- `src/`: Rust domain model, audio pipeline and runtime services.
- `src/bin/cli.rs`: the backend command surface used by the UI and systemd.
- `src/bin/pwloader.rs`: PipeWire module host. `src/bin/probe.rs`: diagnostics probe.
- `usr/`: package payload: desktop and AppStream metadata, systemd units, WirePlumber policy, icons and the Plasma applet.
- `packaging/launcher.py`: the GUI launcher; its isolation and version-neutral path are release invariants.
- `pkgbuild/`: the BigLinux/Arch package recipe.
- `po/`: translation source of truth.
- `docs/research/`: dated measurements; check the code before citing them as current behavior.

## Invariants

1. The GUI lives in `/usr/lib/biglinux-microphone`, never in a `pythonX.Y/site-packages` path, and `/usr/bin/biglinux-microphone` links to the launcher.
2. Nothing scans older `site-packages` directories at runtime: an old package loaded by a newer interpreter mixes incompatible dependencies and hides packaging errors.
3. `packaging/launcher.py` keeps `#!/usr/bin/python3 -I`. Isolated mode blocks `PYTHONPATH`, user site-packages and current-directory shadowing; the launcher adds only the package's private root.
4. Rendering does not write state. Building pages, syncing snapshots, navigating and closing the window never change audio settings.
5. No blocking backend or PipeWire work on the GTK main thread. Use the backend runner and its generation guards.
6. Calibration, playback, monitoring and other subprocess work stop and clean up when their page or window closes.
7. Temporary audio stays private (mode `0600`) and is deleted on completion, cancel or close.
8. The person chooses the model and the app never swaps it. The saved choice and the model the running graph reports are different readings.
9. Controls that only one model uses are not shown as effective under the others.
10. Unit tests do not prove intelligibility, echo cancellation, XRUN behavior or hardware latency.

## Checks

```bash
PYTHONPATH=app python3 -m pytest -q app/tests
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 tools/validate-translations.py
desktop-file-validate usr/share/applications/br.com.biglinux.microphone.desktop
appstreamcli validate --no-net usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml
```

`tools/check-release.sh` runs them sequentially, locked and offline; on a
memory-limited machine start with `CARGO_BUILD_JOBS=1`.

## UI rules

- One obvious primary action per state; technical names stay out of the first-run path unless they explain an error.
- libadwaita patterns before custom drawing. Respect the system color scheme.
- Keyboard, touch and screen-reader semantics; icon-only controls carry accessible labels.
- 1024x600 is a required desktop size; narrower windows reflow instead of clipping actions. Check large text.
- A hidden page stops high-rate drawing.
- `Gtk.Stack`, not `Adw.ViewStack`, and `Gtk.AboutDialog`, not `Adw.AboutDialog`: libadwaita 1.9.3 retains their contents after disposal. Ruff bans both in `app/pyproject.toml`.
- GTK callbacks borrow their owner through `ui.callbacks.weak_callback`. `app/tests/test_window_resources.py` checks that closed windows are finalized.

## Audio rules

- PipeWire graph ownership stays in the Rust backend, not in resident Python workers.
- Reconcile on events instead of polling. A paused `AudioMonitor` parks its worker until activation or shutdown wakes it.
- Real-time callbacks do no allocation, locking, filesystem access or subprocess work.
- A started process is not proof that a PipeWire node carries audio; diagnostics and hardware tests are separate.

## Development setup

The project has two main parts:

- Rust backend (`src/`), pinned to Rust 1.98.1 by `rust-toolchain.toml`.
- Python GTK4/libadwaita UI (`app/`) using system PyGObject.

Run the Python suite with:

```bash
tools/check-release.sh --ui-only
```

Run Rust checks with:

```bash
CARGO_BUILD_JOBS=1 tools/check-release.sh
```

Do not weaken tests to make an environment limitation disappear. If a test needs a display, PipeWire hardware or an unavailable dependency, document that boundary.

## Pull requests

Keep changes focused. A good pull request explains:

- the user-visible problem;
- why the chosen layer owns the fix;
- compatibility/migration impact;
- tests run;
- hardware checks still required.

UI changes should include a real application screenshot when practical, not only a design mockup.

## Translations

English strings in source are the gettext source language. Translation catalogs live in `po/`; see [docs/translations.md](docs/translations.md).

Do not copy `pt_BR` into `pt`, or simplified Chinese into traditional Chinese if both are added in the future. Treat each locale as its own language variant.

## Packaging changes

Read [docs/packaging.md](docs/packaging.md). The Python GUI lives outside versioned `site-packages` in the BigLinux/Arch package. Do not replace that with a hard-coded Python minor path or a runtime scan of old paths.

## Commit hygiene

Do not commit:

- `target/`, Python caches, PEP 517 build output, makepkg work directories;
- temporary recordings or screenshots unrelated to maintained documentation;
- copied dependency caches;
- one-off review/evidence bundles that belong in release artifacts.

## Reporting a bug

See [SUPPORT.md](SUPPORT.md). Include enough system/audio context to reproduce the issue and remove private information from logs before attaching them.
