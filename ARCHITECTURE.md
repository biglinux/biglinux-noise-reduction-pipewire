# ARCHITECTURE — biglinux-microphone (noise-reduction-pipewire)

Human overview + features: `README.md`. Agent edit-map: `AGENTS.md`. This file is
the cross-cutting architecture; `INVARIANTS.md` is the enforced contract.

## 1. Purpose

AI microphone (and system-sound) noise reduction for PipeWire. A GTK4/libadwaita
config window plus a Plasma 6 tray applet drive a LADSPA-hosted neural denoiser
(GTCRN; optional DeepFilterNet3) and a swh-plugins processing chain (EQ, gate,
compressor, pitch). One Rust library crate `biglinux_microphone` + three binaries.

## 2. Single source of truth

All three surfaces read/write **one** file:
`~/.config/biglinux-microphone/settings.json`. The GTK window, the CLI, the
plasmoid, and the running PipeWire chain stay in sync because each watches that
file (`gio::FileMonitor` / `inotifywait`) and re-derives state on change. There is
no second config or IPC channel — the file *is* the bus.

## 3. Binaries

| Binary | Source | Role |
|---|---|---|
| `biglinux-microphone` | `src/bin/gui.rs` | GTK4/libadwaita config window. |
| `biglinux-microphone-cli` | `src/bin/cli.rs` | Headless control + `doctor` diagnostics. |
| `biglinux-microphone-pwloader` | `src/bin/pwloader.rs` | PipeWire module / RT host; the libc RT-setup `unsafe` lives here. |

## 4. Library layers (`src/`)

| Layer | Path | Responsibility |
|---|---|---|
| `config` | `config/` | Settings model + atomic JSON persistence (`mod.rs::save`: temp → `sync_all` → rename). |
| `pipeline` | `pipeline/` | Filter-chain `.conf` generation + systemd user-unit orchestration. Param-only edits push live; topology edits reload only the affected chain. |
| `services` | `services/` | PipeWire/subprocess integration: `pw-cli`/`wpctl`/`journalctl` (argv arrays, no shell), live param push (`pipewire/live.rs`), audio monitor/analyzer, and `pipewire/user_tweaks.rs` (parse/merge PipeWire + WirePlumber config tweaks). |
| `ui` | `ui/` | libadwaita views/widgets, state machine (`ui/state.rs`), spectrum/source-picker, gettext i18n. |
| `diagnostics` | `diagnostics.rs` | `doctor` environment checks. |

## 5. Apply flow

Setting change (UI/CLI/applet) → `config` model update + atomic save → `pipeline`
regenerates the chain `.conf` → `services` either pushes a live `pw-cli` param
(param-only delta) or reloads the affected systemd-managed chain (topology delta).
Smart-filter routing (`filter.smart = true`) avoids virtual-device juggling.

## 6. Native + runtime boundaries

- PipeWire ≥ 1.4 + WirePlumber ≥ 0.5; GTK4 ≥ 4.20 / libadwaita ≥ 1.8.
- LADSPA backends are system packages (`gtcrn-ladspa`, `swh-plugins`,
  optional `deepfilternet-ladspa`) — never bundled; presence is detected at
  runtime and gates the model dropdown.
- `unsafe` is confined to FFI/RT setup (`pwloader.rs`) and two GTK/worker FFI
  sites; each carries a `SAFETY` note.
