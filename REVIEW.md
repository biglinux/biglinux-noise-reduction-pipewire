# Review corrections — 2026-09-08

Reference audit: main at `f674c2006b97f91cfded5a5463f68f3714b10c87`.
Implementation: pull request #30. Read the PR's latest validation summary and
Actions runs for the exact tested commit; this file is not a test certificate.
The June report is preserved under `docs/reviews/2026-06-19.md`, not carried
forward as a current 10/10 score.

## Traceability

| Finding | Implementation / regression surface |
| --- | --- |
| Preserve user configuration | `pipeline/migration.rs`, generic filter-chain preservation and no-overwrite backup tests |
| Concurrent settings and corrupt files | `config/storage.rs`, stable lock, three-way merge, conflict and malformed-file tests |
| Backend reduction versus gate | `pipeline/mic.rs`, `tests/review_audio.rs` |
| Shared CLI/GUI graph reconciliation and honest failures | `services/reconcile.rs`, observed-state planner tests, CLI integration |
| Close without a working audio server | `ui/mic_shell.rs`, monitor lifecycle contracts |
| Device selection and volume off the UI thread | `ui/widgets/source_picker.rs` |
| Applet process retries and accessibility | Plasma `main.qml`, explicit controls, bounded retry policy |
| Reversible buffer preview | `services/preview.rs`, timed preview lease, cancellation and restoration |
| Applied versus edited tuning, reset and context | advanced view/apply, retained tuning page, navigation restoration |
| Preserve stereo or explicitly choose mono | `pipeline/output.rs`, `tests/review_stereo.rs` |
| Preserve preferences while bypassing | runtime settings projection, `tests/review_master.rs` |
| FFT scratch reuse and bounded monitoring | analyzer/capture/monitor modules and tests |
| Structured journal diagnostics and measured quality | `pwloader/denoise_watch.rs`, `config/plugin_cost.rs` |
| Optional resource tuning | `config/runtime.rs`, loader affinity/memory settings, collapsed advanced rows |
| Meter values and reduced motion | native LevelBar/text value, accessible spectrum container |
| Rust and QML translation coverage | extraction markers, `refresh-pot.sh --check`, merged gettext catalogs |
| Subprocess allow-list, deadlines and output bounds | vendored `big-os-kit/subprocess`, `tests/review_subprocess.rs` |
| XDG integration | loader arguments and `tests/review_xdg_units.rs` |
| Dependency advisories | updated Cargo locks and synchronized Flatpak registry sources |
| Real test coverage instead of silent skips | `scripts/test-ui.sh`, `scripts/test-miri.sh`, strict portable quality gate |
| Nix closure and gettext | `default.nix`, `packaging/nix/` |

## Validation boundaries

Compilation and pure regression tests do not measure sound quality, actual
hardware latency, CPU improvement, assistive-technology usability or visual
polish. Required before a production release: a disposable real PipeWire and
WirePlumber session; GTK and Plasma keyboard/AT-SPI tests; long translations,
large fonts and contrast themes; USB/Bluetooth hotplug and loaded-CPU audio;
packaged installation and upgrade tests. NixOS and Flatpak require their own
integration tests and are not certified by an Arch build.

Do not dismiss a secret scanner's unverified finding as a false positive without
locating and reviewing it. Unsupported Miri FFI is not evidence of undefined
behavior, and an unavailable required tool is not a passing check.
