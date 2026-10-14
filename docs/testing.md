# Testing

## Sequential release gate

Prepare the runtime/test dependencies and exact Cargo.lock dependency cache or
vendor before going offline. With the configured runtime active, run:

```bash
CARGO_BUILD_JOBS=1 tools/check-release.sh --ui-only  # Python, gettext, shell, metadata
CARGO_BUILD_JOBS=1 tools/check-release.sh            # includes full Rust checks
```

`--ui-only` is deliberately narrower, not a stable-release certification. The
full gate resolves dependencies before starting expensive Rust checks; missing
crates leave dependent stages `NOT_RUN`. It never downloads or changes the lock.
`CARGO_TARGET_DIR` and an already prepared Cargo home/cache are reusable.

`BIGMIC_CHECK_OUTPUT` selects the evidence parent directory (default
`.release-check`). Each invocation creates a fresh `run-...` directory, so old
Rust successes cannot be mistaken for a later UI-only or failed run.
Each stage has a disk log and a JSON result with exit status, deadline and
cgroup memory/event observations; `status.tsv` summarizes results. Do not infer
acoustic validity from a process exit or a successful settings read.

`BIGMIC_PYTHON` selects the test interpreter. `BIGMIC_PYTHON_NO_SITE=1` is only
for a deliberately prepared private test sysroot with its module paths already
set. The production launcher remains `/usr/bin/python3 -I`, never `-S`.

The guard refuses overlapping validation, reserves disk space and stops at 80%
of a finite cgroup memory limit. Defaults are one Cargo job, no incremental state
and no debug symbols in the test profile. It terminates the process group after
a timeout; do not start another build merely because a tool transport timed out.
`UNKNOWN` means a failed command still needs diagnosis; it is not automatically
a product defect. `ENV` and `DEPENDENCY` must not be disguised as source fixes.

## Focused Python checks

```bash
timeout --signal=TERM --kill-after=10s 60s \
  dbus-run-session -- xvfb-run -a \
  env PYTHONPATH="app${PYTHONPATH:+:$PYTHONPATH}" GSK_RENDERER=cairo \
  python3 -m pytest -q app/tests/test_view_contracts.py
```

`app/tests/test_launcher.py` executes private-launcher fixtures in isolated
mode. `test_monitor_launcher.py` executes the monitor entrypoint. Packaging
fixtures verify the actual `package()` function using marked fake backend
binaries; they are layout tests, not a production Rust build or Arch install.

The full Python suite uses controlled backend fixtures, real GTK widgets and
bounded subprocesses. It must run with a working display; missing-display
skips do not certify GUI behavior. The gettext gate checks extraction, contexts,
plural forms, approved-locale completeness and format strings, not native-speaker
quality. Release locales are explicitly listed in `po/LINGUAS`.

## Visual and accessibility acceptance

Inspect actual windows at desktop size, 1024×600 and narrow widths. Cover no
microphone, loading/error, calibration/cancel/result, microphone test, model
changes, expanded DFN3 controls and the custom equalizer. Inspect light/dark,
large-text and high-contrast states. Record whether the backend/audio was real
or supplied by a fixture. Screenshots and focus traversal do not certify Orca,
AT-SPI, touchscreen hardware or acoustic behavior.

## Full release acceptance

Run the full gate on the exact delivered commit with a complete locked vendor.
Then build/install with makepkg in a clean BigLinux/Arch chroot or VM and test
system-managed Python/PyGObject, plugins and user services together. Validate
suspension, reconnect, source switching, PipeWire/WirePlumber restart, USB,
internal and webcam microphones, headphones/speakers, CPU load, clipping,
XRUNs, latency and echo. Preserve logs and listen to the processed result.

A previous revision's green logs are historical evidence, not a pass for a
reconstructed or subsequently changed tree. See [release.md](release.md).
