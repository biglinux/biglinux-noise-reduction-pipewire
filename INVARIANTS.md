# INVARIANTS — biglinux-microphone

Living contract. Each invariant: rule → reason → validation. Validation points at
the check that enforces it today; where only a code shape or crate default guards
it, the line says so.

Scope: library crate `biglinux_microphone` + binaries `gui`, `cli`, `pwloader`.

## H1 — Crash-safe disk writes

- Rule: `settings.json` and generated `.conf` bodies are written temp → `sync_all`
  → `rename` (`src/config/mod.rs:127`–`129`).
- Reason: a crash mid-write must never leave a torn/empty settings or chain file —
  the file is the single source of truth for every surface.
- Validation: `config/mod.rs::save`; pipeline round-trip test
  `round_trip_through_disk_produces_identical_conf` (pipeline_apply suite).

## H2 — Subprocess via argv, never a shell

- Rule: `pw-cli`/`wpctl`/`journalctl` and friends are invoked through
  `std::process::Command` argv arrays; no `sh -c`, no string interpolation. Node
  IDs are numeric.
- Reason: live param push and topology reload run on user-influenced state; a
  shell path would be an injection surface.
- Validation: `services/pipewire/live.rs` + `services/` build sites; cross-check
  `grep -RnE 'Command::new\("(sh|bash)"\)' src/` → 0.

## H3 — `unsafe` is FFI/RT-only, each with `SAFETY`

- Rule: `unsafe` appears only in `src/bin/pwloader.rs` (libc RT setup),
  `src/services/pipewire/worker.rs`, and `src/ui/window.rs` (GTK/FFI); every block
  carries a `// SAFETY:` note.
- Reason: bound the UB surface to the FFI/RT boundary, not application logic.
- Validation: 3 files contain `unsafe`; SAFETY-comment count ≥ unsafe-block count
  in each (rechecked 2026-06-19); clippy `-D warnings`.

## H4 — Settings file is the only sync channel

- Rule: GUI, CLI, plasmoid, and the running chain coordinate exclusively through
  `~/.config/biglinux-microphone/settings.json`, watched via `gio::FileMonitor` /
  `inotifywait`. No second config, no custom IPC.
- Reason: one bus = no split-brain between window and tray; bidirectional sync is
  automatic.
- Validation: `config/` is the only persisted state; UI/CLI/applet all load it.

## H5 — LADSPA/native backends are system-owned, runtime-detected

- Rule: `gtcrn-ladspa`, `swh-plugins`, optional `deepfilternet-ladspa` are
  distro packages; presence is detected at runtime and gates the model dropdown.
  Nothing is bundled.
- Reason: codec/DSP licensing + security updates stay distro-owned; the app
  degrades gracefully when an optional backend is absent.
- Validation: model-availability logic in `pipeline`/`config`; `doctor` reports
  missing backends.

## H6 — i18n: English source, `po/` is the only translation truth

- Rule: user-facing strings flow through the gettext helper (`ui/i18n.rs`); the
  dev `.mo` tree under `locale/` is generated, gitignored, never committed.
- Reason: single translation source; no drift between hand-edited binaries and
  catalogs.
- Validation: `./scripts/refresh-pot.sh`; `msgfmt -c` on `po/*.po`.

## H7 — Live vs topology change discipline

- Rule: param-only edits push a single `pw-cli` param; topology edits reload only
  the affected chain, not the whole graph.
- Reason: avoid audio dropouts / full-graph churn on a slider drag.
- Validation: pipeline_apply tests
  (`switching_models_only_changes_the_model_control`,
  `enabling_master_then_disabling_keeps_conf_present`).
