# Filter noise — AI Noise Reduction for PipeWire


<img width="851" height="828" alt="image" src="https://github.com/user-attachments/assets/73dcdd89-4e9c-4766-aadb-76608402c95e" />


GTK4/libadwaita configuration window plus a Plasma 6 system-tray
applet, both backed by a single `~/.config/biglinux-microphone/settings.json`
watched via `gio::FileMonitor` / `inotifywait` for instant bidirectional
sync.

## Features

### AI noise reduction
- **GTCRN neural network** — voice-grade denoising via a LADSPA host
- **DNS3 & VCTK models** (16 kHz) — aggressive or gentle profiles
- **DeepFilterNet3** (full-band 48 kHz) — optional backend, exposed in
  the model dropdown when the `deepfilternet-ladspa` package is
  installed; same intensity slider drives its attenuation cap

### Audio processing
- **Equalizer** with ten bands and voice presets
- **Noise gate** — silences the chain during silence
- **Acoustic echo cancellation** — Automatic, Always or Never. Automatic mode
  follows the active output and avoids cancellation for headphones. Each filter
  runs in a module-loader client connected to the existing PipeWire daemon.

### Voice enhancement
- **Dual mono** — stereo duplication of the cleaned voice
- **Radio voice** — broadcast-style compression
- **Voice changer** — pitch slider with Deep / Lower / Natural / Higher / Chipmunk marks

### System sound filter
- Cleans every sound the system plays before it reaches the speakers,
  using the same GTCRN-based chain on the playback side.

### Visualization & monitoring
- **Spectrum analyzer** — 30 bands at 60 fps
- **Headphone monitor** — hear the processed signal with adjustable delay
- **Live parameter updates** — param-only changes are pushed via
  `pw-cli`; topology changes reload only the affected chain

### User experience
- **Smart-filter routing** — `filter.smart = true`, no virtual device juggling
- **Plasma 6 applet** — toggle both filters from the system tray with
  bidirectional sync against the GTK window
- **Persistent settings** — `serde`-backed JSON, atomic writes
- **Processing quality** — automatic or explicit model cost selection; native
  plugin measurements run in the CLI process, isolated from the GUI allocator

## Requirements

### Runtime
- Linux with PipeWire **>= 1.4**, WirePlumber **>= 0.5**, and systemd user services
- `jemalloc-gtk-fixed` for the default native GUI build
- GTK4 **>= 4.22** and libadwaita **>= 1.9**
- `gtcrn-ladspa` (neural denoiser plugin — GTCRN backend)
- `swh-plugins` (gate, compressor, pitch shifter)
- `deepfilternet-ladspa` *(optional)* — enables the DeepFilterNet3
  full-band 48 kHz backend in the model dropdown

### Build (from source)
- Rust **>= 1.97.1** (`rustup` recommended)
- `pkg-config`, `clang`, `pipewire-devel`, `gtk4-devel`, `libadwaita-devel`

## Build & Run

```bash
cargo build --release        # builds all four binaries into target/release/
```

Resulting binaries:

| Binary | Role |
|---|---|
| `biglinux-microphone`          | GTK4/libadwaita configuration window (`src/bin/gui.rs`) |
| `biglinux-microphone-cli`      | Headless control + diagnostics (`src/bin/cli.rs`) |
| `biglinux-microphone-pwloader` | PipeWire module loader / RT host (`src/bin/pwloader.rs`) |
| `biglinux-microphone-probe` | PipeWire diagnostics probe (`src/bin/probe.rs`) |

```bash
cargo run --release --bin biglinux-microphone          # launch the GUI
cargo run --release --bin biglinux-microphone-cli doctor   # environment diagnostics
```

Packaging (Arch/BigLinux): `packaging/arch/PKGBUILD` builds with
`--locked`, compiles `po/*.po` into `build-locale/`, and installs the
systemd user units, plasmoid, and PipeWire/WirePlumber drop-ins.

The main user service runs `biglinux-microphone-cli watch`, which applies saved
settings and follows PipeWire changes. Package hooks enable units without editing
user homes or files owned by WirePlumber.

The experimental Flatpak recipe under `packaging/flatpak/` builds with
`--no-default-features` to use the SDK allocator. It requires a compatible GNOME
SDK and separate host audio integration; it is not an installed-unit validation.

## Source ownership

This repository builds without another local checkout. `vendor/big-rust-components`
contains the six runtime support crates and their test harness from commit
`47da8738fc9ca0ba1fb7820e417582883f81d16e`; their MIT license is retained there.
They are maintained with this application. The application remains GPL-3.0-or-later;
`LICENSE-MIT` covers the ported MIT model contracts.

## Contributing

- Run the quality gate before sending a change: `./scripts/quality-check.sh`
  (`--ci` mirrors the exact CI gate; `--fix` applies `cargo fmt`).
- Source language is English. Translatable UI strings flow through
  gettext: `po/` is the only translation source of truth. Refresh the
  catalog with `./scripts/refresh-pot.sh` after touching user-facing text.
- Tuning research and PipeWire/WirePlumber config rationale live in
  [TIPS.md](TIPS.md). The offline calibration harness lives in
  `scripts/calibrate/` (see its README).

## Architecture

`src/` is split by concern (each module carries a top-level doc-comment):

| Module | Responsibility |
|---|---|
| `config/`   | Settings model + atomic JSON persistence (`~/.config/biglinux-microphone/settings.json`) |
| `pipeline/` | Filter-chain `.conf` generation + systemd unit orchestration |
| `services/` | PipeWire / subprocess integration (`pw-cli`, `wpctl`), live param updates, audio monitor |
| `ui/`       | GTK4/libadwaita views, widgets, and gettext i18n |
| `bin/`      | The four entrypoints (`gui`, `cli`, `pwloader`, `probe`) |

## License for our configuration interface

GNU General Public License v3.0 - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [GTCRN Project](https://github.com/Xiaobin-Rong/gtcrn) - Neural network for noise reduction
- [DeepFilterNet](https://github.com/Rikorose/DeepFilterNet) - Full-band neural denoiser (DFN3 backend)
- [PipeWire](https://pipewire.org/) - Modern audio server
- [GTK4](https://gtk.org/) / [Libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - UI framework
