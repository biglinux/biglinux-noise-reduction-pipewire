# Native regression testing

Record the tested Git commit and dependency versions with every result. A
workflow's own revision can differ from the application revision it checks out.
A passing compilation is not evidence that ignored tests were executed.

## One owner for each automatic check

`ci.yml` owns change-driven application tests and security scans. Its small
planner uses the complete Git diff (including removed and renamed paths), not
the GitHub path filter's truncated file list. Documentation-only and
translation-only change sets do not start Rust builds. Unknown build inputs,
new branches or unavailable history conservatively select every check.

A push to a testing/stable branch with an open PR into main at the same SHA is
covered by that PR. If discovery fails, push checks run rather than losing
coverage. A push to main is never suppressed. PRs validate the merge checkout;
post-merge builds validate the actual main revision. These are intentionally
different integration points, not interchangeable success evidence.

Concurrency cancels obsolete runs within an event and branch/PR. Push and PR
concurrency groups are separate, so a duplicate push that does no work cannot
cancel the real PR validation. Manual CI dispatch selects all checks.

`Security` only performs scheduled/manual RustSec scans for newly published
advisories. It does not repeat CI on every push or PR and no longer rebuilds
Miri daily. Change-driven RustSec has one owner in CI: cargo-audit checks both
lockfiles; cargo-deny checks licenses, bans and sources without repeating the
advisory scan. Miri and the secret scan each run once under CI. CodeQL remains
a distinct code-analysis tool; a dependency audit is not a replacement for it.

The `CI result` check fails when planning or any selected job fails/cancels.
Non-applicable jobs are visibly skipped, not represented as tests that ran.
There is no workflow-wide documentation path exclusion leaving CI pending.
Repositories using branch protection should require this aggregate check.
The planner itself is covered by inexpensive Python unit tests.

## Application checks, without an allocator matrix

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

The maintainers validate their own jemalloc fork separately. This application
CI does not rebuild upstream jemalloc, apply compatibility patches, run an
allocator probe, or repeat the suites across allocator variants. Runtime tests
use the runner's system allocator once. The application's default features,
packaging dependency and fork are unchanged; MSRV/release compile the production
configuration. A system-allocator test is not claimed as validation of the fork.

The normal Rust suite leaves GTK and native PipeWire cases ignored. The two
scripts above execute only their explicit ignored contracts in private
sessions. Their `--no-run` compilation does not execute a second test suite;
compiled artifacts are reused with the same features. Each GTK case needs its
own process because GTK must stay on its initializing thread.

## GTK and libadwaita contracts

Install Xvfb, xauth, a DBus session daemon, Python 3 and a usable font.
`test-ui.sh` builds once and discovers `ui::tests::gtk_*` cases. It fails if the
suite is empty. Each case gets a private process, display, DBus session and XDG
directories. All cases are attempted and failures are aggregated. Warnings are
fatal in the test process, not injected into unrelated DBus-activated services.
PipeWire and PulseAudio paths never point to the developer's devices.

## Real PipeWire module and lifecycle contracts

Install PipeWire audio modules and the SWH LADSPA plugins. `test-pipewire.sh`
creates a private runtime directory and DBus session. Its fixture refuses an
existing daemon socket and loads generated microphone, stereo playback and
mono playback graphs using the production module loader. It observes node
publication, recreates the microphone while playback remains alive, and stops
playback without removing the microphone. Children are reaped and a deadline
bounds native hangs.

No systemd user units, WirePlumber, physical devices or neural runtimes are
started by this fixture. Passing proves the tested graph loading/lifecycle,
not routing policy, denoising, audio quality or hardware latency.

## Release acceptance

Use a disposable desktop session for routing, selected models, device changes,
preview restoration and recovery under load. Measure latency, XRUNs, completed
denoising work, channel separation and distortion. Validate keyboard navigation,
large text, contrast, AT-SPI/Orca and the real Plasma applet. A QML reducer test
is not a Plasma interaction test. Installation/upgrades, NixOS and experimental
Flatpak need independent acceptance. The CI policy is not release certification.

## Primary references

- Workflow concurrency and filtering: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
- Cargo test selection: https://doc.rust-lang.org/cargo/commands/cargo-test.html
- Libadwaita initialization: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/initialization.html
- GTK threading: https://docs.gtk.org/gtk4/section-threading.html
