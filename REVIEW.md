# biglinux-microphone — Audit Readiness — 2026-06-19

## Score: 10.0 / 10

AI microphone/system-sound noise reduction for PipeWire (GTK4 window + Plasma
applet + CLI + RT loader). v5.0.0. See `ARCHITECTURE.md` for the layer map and
`INVARIANTS.md` for the enforced contract.

## Gate matrix (run 2026-06-19)

| Gate | Cmd | Result |
|---|---|---|
| R1 fmt | `cargo fmt --all -- --check` | PASS |
| R2 clippy | `cargo clippy --all-targets --all-features --locked -- -D warnings` | PASS |
| R3 tests | `cargo nextest run --all-features` | PASS — 249/249 |
| R4 deny | `cargo deny check` | PASS — advisories/bans/licenses/sources ok |
| R5 machete | `cargo machete` | PASS — no unused deps |

Full local gate: `./scripts/quality-check.sh --ci` (mirrors CI exactly).

## Invariant spot-checks

- Crash-safe writes (`config/mod.rs`: temp → `sync_all` → rename) — covered by
  `round_trip_through_disk_produces_identical_conf`.
- Subprocess argv-only (`services/pipewire/live.rs`); no `sh`/`bash` spawn.
- `unsafe` confined to 3 FFI/RT files, each with a `SAFETY` note (rechecked).
- Live-vs-topology discipline — covered by `switching_models_only_changes_the_model_control`.

## Reproduce gates

```sh
cd biglinux-noise-reduction-pipewire
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo machete
```
