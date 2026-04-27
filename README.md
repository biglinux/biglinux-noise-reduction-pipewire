# Filter noise — AI Noise Reduction for PipeWire


<img width="851" height="828" alt="image" src="https://github.com/user-attachments/assets/73dcdd89-4e9c-4766-aadb-76608402c95e" />


GTK4/libadwaita configuration window plus a Plasma 6 system-tray
applet, both backed by a single `~/.config/biglinux-microphone/settings.json`
watched via `gio::FileMonitor` / `inotifywait` for instant bidirectional
sync.

## Features

### AI noise reduction
- **GTCRN neural network** — voice-grade denoising via a LADSPA host
- **DNS3 & VCTK models** — aggressive or gentle profiles

### Audio processing
- **Equalizer** with presets (Voice Boost, Podcast, Warm, Bright, De-esser, Low-cut)
- **Noise gate** — silences the chain during silence
- **Acoustic echo cancellation** — WebRTC AEC, on by default in a
  standalone `pipewire -c` instance; optional toggle in the Advanced
  view

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

## Requirements

### Runtime
- Linux with PipeWire **>= 1.4** + WirePlumber **>= 0.5**
- GTK4 **>= 4.20** and libadwaita **>= 1.8**
- `gtcrn-ladspa` (neural denoiser plugin)
- `swh-plugins` (gate, compressor, pitch shifter)

### Build (from source)
- Rust **>= 1.82** (`rustup` recommended)
- `pkg-config`, `clang`, `pipewire-devel`, `gtk4-devel`, `libadwaita-devel`

## License for our configuration interface

GNU General Public License v3.0 - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [GTCRN Project](https://github.com/Xiaobin-Rong/gtcrn) - Neural network for noise reduction
- [PipeWire](https://pipewire.org/) - Modern audio server
- [GTK4](https://gtk.org/) / [Libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - UI framework
