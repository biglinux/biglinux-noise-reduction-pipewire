# AGENTS.md

Orientation for AI coding agents and new contributors. Human-facing
overview, features, and architecture map: see [README.md](README.md).

## What this is

BigLinux AI microphone noise reduction for PipeWire. Rust workspace:
one library crate (`biglinux_microphone`) + three binaries. GTK4/
libadwaita config window, a headless CLI, and a PipeWire RT module
loader, all driven by a single `settings.json`.

## Commands (verified, run from repo root)

```bash
cargo build --release                              # all three binaries
cargo run --release --bin biglinux-microphone      # GUI
cargo run --release --bin biglinux-microphone-cli doctor   # diagnostics
./scripts/quality-check.sh         # full local gate (--ci, --fix, --full)
./scripts/refresh-pot.sh           # regenerate po/*.pot + msgmerge po/*.po
```

CI gate (`.github/workflows/ci.yml`), mirror locally with `--ci`:
`cargo fmt --all --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`cargo nextest run --all-features --locked`; `cargo build --release --locked`;
`cargo deny check`; `cargo machete`. `Cargo.lock` is committed; the gate
uses `--locked`.

## Module map (`src/`, from Cargo.toml + tree)

- `bin/gui.rs` → `biglinux-microphone` (GTK4 window)
- `bin/cli.rs` → `biglinux-microphone-cli` (control + `doctor`)
- `bin/pwloader.rs` → `biglinux-microphone-pwloader` (PipeWire module/RT host; the `unsafe` libc RT-setup blocks live here)
- `config/` — settings model + atomic JSON persistence (write-tmp + fsync + rename)
- `pipeline/` — filter-chain `.conf` generation + systemd unit orchestration
- `services/` — PipeWire/subprocess integration (`pw-cli`, `wpctl`, `journalctl`); `services/pipewire/live.rs` pushes live param updates (argv-based, no shell)
- `ui/` — libadwaita views/widgets + gettext i18n (`ui/i18n.rs`)
- `diagnostics.rs` — `doctor` implementation

## Where to add code

- New setting → `config/` (add to model + defaults; persistence is automatic).
- New filter-chain node/param → `pipeline/` (`mic.rs`); mirror the math in `scripts/calibrate/lib/chain.py` if calibrated.
- New UI control → `ui/views/` or `ui/widgets/`; wire live updates via `services/pipewire/live.rs`.
- New user-facing string → wrap with the i18n helper, then run `./scripts/refresh-pot.sh`.

## Invariants & conventions

- Source language English. UI strings via gettext; `po/` is the only
  translation source of truth. A compiled dev `.mo` tree under `locale/`
  is generated, gitignored, never hand-committed.
- Disk writes are crash-safe: settings.json and generated `.conf` bodies
  use write-`.tmp` + `sync_all()` + `rename`.
- `pw-cli`/`wpctl` invoked via `std::process::Command` argv arrays, never
  a shell. Node IDs are numeric; no injection surface.
- Every `unsafe` block carries a `SAFETY` comment. Keep that contract.
- Prefer shared BigLinux Rust components over local UI helpers where one
  exists; app-specific adapters only.
- Tuning rationale: [TIPS.md](TIPS.md). Offline calibration harness:
  `scripts/calibrate/` (dev-only; fetches models/datasets into
  `$XDG_CACHE_HOME`, nothing committed).
