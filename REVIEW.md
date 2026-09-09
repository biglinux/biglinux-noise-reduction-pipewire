# PR #30 follow-up review — 2026-09-08

The R01–R20 identifiers below refer to the review of PR #30 at
`bf6cfca75bb43d94766d2925624067ae0673a4e2`. They are not the numbering of the
original 50-item repository audit. Implementation coverage is not a claim that
every original audit item or every platform has been certified.

## Finding-to-regression matrix

| ID | Implemented correction | Regression surface |
| --- | --- | --- |
| R01 | Normalize the native GTK meter to 0..1; publish physical dB in visible and accessible text. | `ui::tests::assert_meter_range_contract`, executed by the isolated GTK entry point with fatal warnings. |
| R02 | Separate recovering capture from terminal failure; keep the UI receiver alive and clear stale values. | `ui::tests::assert_monitor_binding_contract` injects frame, recovering and recovered frame events. |
| R03 | Track observed and successfully persisted/applied byte revisions independently; retry failed revisions with bounded backoff. | `services::settings_watch::tests`: failed application, concurrent newer edit, bounded retries. |
| R04 | Plan per-chain recovery after failed live controls; force repair bypasses the fast path and independent stops are still attempted. | `services::reconcile::tests`: failed push, forced repair, failed AEC with independent output repair. |
| R05 | Keep byte-exact persisted tuning revisions separate from applied state; recognize own partial writes and retry a failed restart. | `services::pipewire::user_tweaks::transaction::tests`: partial writes, external conflicts, idempotent restart retry. |
| R06 | Compare effective graph projections; unchanged suspended nodes do not require a push or a restart. | `unchanged_suspended_graphs_do_not_need_a_push_or_restart`. |
| R07 | Synchronize model, quality and bypass controls after worker normalization without treating slider drags as structural UI changes. | `mirrored_choices_track_worker_normalization_but_not_slider_drags`; GTK navigation contracts. |
| R08 | Expose an advanced master pause/resume control and explain preserved effect preferences; explicit enabling actions resume processing consistently. | `tests/review_master.rs`, settings intent and mirrored-choice tests. |
| R09 | Preserve the preview child's exit result; retry transient restoration failures with fresh ownership comparisons and show errors. | `services::preview::tests`: transient reads, failed writes, external override, bounded persistent failure. |
| R10 | Exclude Apply/Reset while preview startup is in flight; prevent a new preview during application and stop an existing preview before writes. | Shared `busy` / `preview_busy` transitions in the tuning controller; real event-order testing remains required. |
| R11 | Resume an interrupted migration only when the backup and original are regular files referring to the same device/inode. | `interrupted_migration_resumes_without_overwriting`, different-backup and backup-symlink tests. |
| R12 | Explicitly refresh runtime capabilities off the GTK thread; capability generations invalidate displayed availability. | Runtime capability refresh tests in `config/noise_model.rs`. |
| R13 | Check only the neural models and services required by the effective configuration, not an unconditional GTCRN dependency. | `ui::health::tests`: alternate model, equalizer-only and bypass. |
| R14 | Change the visible EQ preset to Custom on a manual band edit; reselecting a preset restores all bands without a signal loop. | `ui::widgets::eq_card::assert_interaction_contract` in the isolated GTK entry point. |
| R15 | Use shared frequency boundaries, allow empty unresolved low bands, test actual peak-band indices and stop constant-input meter decay. | Analyzer known-tone tests across FFT sizes/rates; spectrum steady-input regression. |
| R16 | Wrap EQ columns and use native font-scalable frequency labels plus theme-derived spectrum color. | EQ minimum-width GTK assertion. Large-text, contrast and screen-reader acceptance remain separate. |
| R17 | Include model identity and controls in the cost cache; reject asynchronous measurements without demonstrated completed work; use a conservative automatic fallback. | Completed-work, shared-library variants, logarithmic LADSPA defaults and automatic-quality tests. |
| R18 | Add default conservative headroom and a final sample ceiling, with an explicit advanced opt-out and matched live controls. | `pipeline::gain_safety::tests`, `tests/review_gain_safety.rs`; acoustic tests remain required. |
| R19 | Disable allocator background threads at boot, quiesce any explicit override before locale setup, avoid GTK repeating setlocale, then enable requested background purging. | Native compilation plus startup isolation guard. Test GUI startup and allocator behavior in a real packaged session. |
| R20 | Separate stale status-query errors from action failures; clear only an error superseded by a successful observation. | `node --test tests/plasmoid_status.test.cjs` runs the production QML status reducer. |

The original non-destructive migration, stable settings lock, three-way merge,
standalone gate independence, stereo default, bounded subprocess I/O, XDG loader
paths and gettext extraction corrections remain in this PR.

## Reproduction

One functional execution, the same sequence `ci.yml` runs and
`docs/testing-native.md` documents. Clippy with every feature is an analysis of
the build configuration, not a second functional run.

```sh
python3 -m unittest discover -s .github/ci -p 'test_*.py'
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --no-default-features --locked -- --test-threads=1
bash scripts/test-ui.sh --no-default-features
bash scripts/test-pipewire.sh --no-default-features
node --test tests/plasmoid_status.test.cjs
bash scripts/refresh-pot.sh --check
```

Use the PR validation comment and the artifact's `tested-commit.txt` for actual
results. A workbench workflow's own commit can differ from its pinned application
checkout; never infer the tested application revision from the workflow's SHA.
A failed test step does not prove subsequent tests were executed.

Passing this sequence is the integration criterion for this PR. The criteria for
publishing a stable package are separate and live in `docs/release-candidate.md`:
a real installation and upgrade, a disposable BigLinux session, hardware audio,
the Plasma applet and screen-reader acceptance.

## Remaining acceptance boundaries

GTK widget tests are not Orca/AT-SPI user acceptance. Node tests of a QML reducer
do not instantiate Plasma. Generated graph assertions are not hardware audio,
true-peak, latency, XRUN or intelligibility measurements. Nix syntax checks are
not `nix build` or NixOS integration. Flatpak remains experimental.

The gain guard is a sample ceiling, not an oversampled true-peak limiter. Its
conservative headroom may lower volume substantially with large combined boosts.
The clamp may distort an overloaded signal and cannot undo source distortion.
Compare music, calls and recorded speech before promoting a release.

Preview restoration retries are finite and preserve newer external overrides.
No implementation can promise restoration after SIGKILL or permanent loss of
the audio server. The UI must report an unconfirmed restoration rather than
claiming success.

The June readiness report remains historical under `docs/reviews/2026-06-19.md`.
Neither that score nor this implementation matrix certifies a later revision.
