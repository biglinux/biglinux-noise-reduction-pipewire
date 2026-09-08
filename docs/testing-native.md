# Native regression testing

Record the tested Git commit and dependency versions with every result. A
workflow's own revision can differ from the application revision it checks out.
A passing compilation is not evidence that ignored tests were executed.

## Portable and headless checks

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked -- --test-threads=1
cargo test --no-default-features --locked -- --test-threads=1
node --test tests/plasmoid_status.test.cjs
bash scripts/refresh-pot.sh --check
```

The ignored GTK and native PipeWire tests have separate entry points below.
They deliberately do not start an audio daemon from the ordinary test suite.

## GTK and libadwaita contracts

```sh
bash scripts/test-ui.sh
bash scripts/test-ui.sh --no-default-features
```

Install Xvfb, xauth, a DBus session daemon, Python 3 and a usable font. The
script builds the test executable once and discovers the `ui::tests::gtk_*`
contracts. It fails if the suite is empty. Each contract runs in its own
process, display, DBus session and private XDG directories. All cases are
attempted and their failures are aggregated; one dialog failure cannot hide
the meter, equalizer, capture recovery or navigation contracts.

Tests initialize libadwaita before creating its widgets. GTK is confined to
its initializing test thread. `--test-threads=1` alone is insufficient to make
separate Rust tests share that thread, hence the process isolation.

Warnings are fatal in the test process. The script does not inject that debug
policy into unrelated DBus-activated services. PipeWire and PulseAudio runtime
paths point at the private session, not the developer's microphone or speakers.

### Native allocator requirement

The default `gtk-jemalloc` build requires the compatible allocator declared by
native packaging (`jemalloc-gtk-fixed`), not merely a library with the same
SONAME. GLib's C23 deallocation calls permit NULL, including with a nonzero
size. Incompatible sized-deallocation entry points can corrupt the allocator
before a later GTK allocation crashes.

`tests/jemalloc_compat.c` exercises the linked allocator's sized and
aligned-sized NULL frees and subsequent allocation/deallocation cycles. CI
runs it before the default-allocation graphical tests.

On disposable GitHub Actions runners only, `scripts/ci-jemalloc.sh` first tests
the installed allocator. If it fails, the script builds upstream jemalloc
commit `81034ce1f1373e37dc865038e1bc8eeecf559ce8` with explicit NULL guards at
both C23 entry points and tests that library. Its C++ operator replacements
are not enabled; the C allocation ABI is the contract needed here. The complete
source patch appears in the log. Library paths are changed only for that job;
no system library is overwritten and the application receives no allocator
shim or preload wrapper.

This CI fixture validates that compatibility condition. It does not certify
every patch in the distribution package or replace packaged-installation tests.
The system-allocator variant remains an independent control.

## Real PipeWire module and lifecycle contracts

```sh
bash scripts/test-pipewire.sh
```

Requires PipeWire's audio modules and the SWH LADSPA plugins in addition to the
build dependencies. The script uses a new private runtime directory and DBus
session. The Rust fixture refuses an existing daemon socket and starts its own
PipeWire daemon with the upstream configuration. It loads generated microphone,
stereo playback and mono playback graphs using the production
`biglinux-microphone-pwloader` executable.

The test observes real node publication, removes and recreates the microphone
chain while playback remains alive, and stops playback without removing the
microphone. Child processes are reaped on success and unwinding; an outer
process-group timeout bounds native hangs.

No systemd user units, WirePlumber, physical devices or neural runtimes are
started by this fixture. Consequently, passing it means that these generated
graphs load and have the tested lifecycle, not that routing policy, denoising,
audio samples, latency or sound quality have been validated.

## Release acceptance still required

Use a disposable desktop session for WirePlumber/systemd routing, selected
neural models, device changes, buffer-preview restoration and recovery under
load. Measure latency, XRUNs, actual completed denoising work, channel
separation and distortion. A sample ceiling is not a true-peak limiter.

Validate keyboard navigation, large text, contrast, AT-SPI/Orca and the real
Plasma applet. A QML reducer test is not a Plasma interaction test. Validate
native installation/upgrades, NixOS and experimental Flatpak independently.

## Primary API references

- Libadwaita initialization: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/initialization.html
- GTK threading: https://docs.gtk.org/gtk4/section-threading.html
- GLib sized deallocation: https://docs.gtk.org/glib/func.free_sized.html
- Pinned jemalloc source: https://github.com/jemalloc/jemalloc/blob/81034ce1f1373e37dc865038e1bc8eeecf559ce8/src/jemalloc.c
- jemalloc build options: https://github.com/jemalloc/jemalloc/blob/81034ce1f1373e37dc865038e1bc8eeecf559ce8/INSTALL.md
