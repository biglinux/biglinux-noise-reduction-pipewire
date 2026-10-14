# Release checklist

A green unit-test run is necessary, not sufficient, for a stable microphone-audio release.

## Source and documentation

- [ ] `README.md`, `AGENTS.md` and maintained docs describe the current Python UI + three Rust backend binaries.
- [ ] No obsolete plan/review dump is presented as current architecture.
- [ ] No `CLAUDE.md`/duplicated agent-instruction file has drifted from `AGENTS.md`.
- [ ] Dead Python modules/imports and generated caches are absent.

## Python/UI

- [ ] `tools/check-release.sh --ui-only` on the delivered tree, with no missing-display skips.
- [ ] Launcher tests pass.
- [ ] UI rendered and visually reviewed at default width, 1024×600 and narrow width.
- [ ] Light/dark and accessibility states reviewed.
- [ ] Cancel/close cleans up calibration, monitoring and temporary audio.

## Rust/backend

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] Full `tools/check-release.sh` with all locked dependencies available.
- [ ] Effective model/fallback behavior tested on machines representing supported CPU classes.

## Packaging

- [ ] Arch `PKGBUILD` syntax/metadata validation passes.
- [ ] Staged package uses `/usr/lib/biglinux-microphone`, not a hard-coded Python minor directory for application code.
- [ ] `/usr/bin/biglinux-microphone` relative symlink resolves correctly.
- [ ] No `__pycache__`, `.pyc`, `.pyo`, build directories or stale wheels ship.
- [ ] Full-system Python upgrade scenario tested in a disposable rolling-release VM/session.
- [ ] Desktop and AppStream metadata validate.

## Audio/hardware

- [ ] USB microphone.
- [ ] Analog/internal microphone where available.
- [ ] Webcam microphone.
- [ ] Bluetooth path if supported.
- [ ] Headphones and speakers.
- [ ] CPU load / battery scenario.
- [ ] PipeWire/WirePlumber restart and device hotplug.
- [ ] Latency/XRUN observations recorded.

## Translations

- [ ] `tools/validate-translations.py` passes; `po/LINGUAS` contains only approved complete locales.
- [ ] New UI strings are extracted.
- [ ] PT-BR and other release-critical locales reviewed in the real UI.

Only checks actually performed should be marked complete.

## Handoff integrity

- [ ] Final sources are committed; the working tree is clean.
- [ ] Source TAR, cumulative patch, Git bundle and SHA-256 manifest refer to that commit.
- [ ] TAR extracted and bundle cloned in independent directories; source hashes agree.
- [ ] Patch applied from its stated base and the resulting Git tree matches.
- [ ] At least one real regression command passed from the restored tree.
- [ ] Logs belong to this revision; earlier lost/uncommitted changes are not implied to be recovered.

Do not check boxes from memory. Record the commit, runtime versions, command,
result and hardware limitations in the external release report.
