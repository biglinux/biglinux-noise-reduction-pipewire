# Native release acceptance

The `release-candidate` PR label requests production-feature MSRV and release
builds on the next ordinary CI event, even while the PR remains a draft. It
is an opt-in request for evidence, not an announcement of a stable release.
Adding the label alone does not start another workflow. Remove it to return
to the inexpensive draft cadence. Ready PRs and manual CI dispatch already
request these builds. There is still one owner for every application suite.

`CI result` checks both the selection plan and the result of each required
job. A selected job that was skipped is a failure of acceptance. Out-of-scope
checks and deliberately deferred draft builds remain explicit skips.

The release job calls the Arch recipe's `build()` and `package()` functions.
It does not execute the recipe's `check()` a second time, install the package,
run install hooks, start a user service or validate the maintained allocator
fork. It checks nested local-source preparation, all four binaries, the full
runtime resource tree, translated catalogs, desktop/AppStream metadata,
permissions and library resolution. The Portuguese guide is part of the
installed payload. The report records file hashes and the tested Git SHA.

Run it in a disposable environment with the documented build dependencies:

```sh
bash scripts/test-package.sh .ci-results/release
```

Native and release evidence use workspace-relative directories to avoid host
versus container temporary-path differences. The reports are separate from a
signed pacman package; successful staging is not evidence of an installation
or upgrade on the distribution.

## Decision boundary

A clean CI run including release and MSRV is necessary for native acceptance.
Record the PR head and the tested merge SHA. Do not substitute a prior green
run, a successfully compiled executable or skipped tests for that evidence.
The maintained production allocator and its native package dependency remain
unchanged, as requested by the maintainers.

Before broad stable distribution, validate on a disposable BigLinux desktop:

- Upgrade from the shipped package without losing settings or user PipeWire
  configuration; login starts the expected services and removal leaves audio
  usable. Keep the original package and configuration backup for rollback.
- Check a real microphone and a selected neural backend: pause/resume, a call,
  output stereo separation, device changes and recovery after a daemon restart.
  Confirm that no test changes the physical input unexpectedly.
- Check buffer-preview restoration and other audio applications, then exercise
  keyboard navigation, larger text and the screen reader used by the project.

Record the machine, audio devices, package versions and result. Synthetic
PipeWire lifecycle tests do not measure intelligibility, acoustic echo,
latency, dropouts or true-peak behavior. A QML reducer test does not instantiate
Plasma. Flatpak remains experimental; NixOS has a separate acceptance path.

References: the Arch `PKGBUILD(5)` build/package contracts and GitHub Actions
workflow/event documentation. This checklist describes requirements, not an
assertion that unperformed manual checks passed.
