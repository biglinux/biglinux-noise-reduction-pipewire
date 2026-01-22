# BigLinux Microphone - AI Noise Reduction for PipeWire

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Python 3.10+](https://img.shields.io/badge/Python-3.10%2B-blue.svg)](https://www.python.org/)
[![GTK4](https://img.shields.io/badge/GTK-4.0-green.svg)](https://gtk.org/)
[![Libadwaita](https://img.shields.io/badge/Libadwaita-1.0-purple.svg)](https://gnome.pages.gitlab.gnome.org/libadwaita/)

A professional microphone processing application for Linux systems running PipeWire. Features AI-powered noise reduction, 10-band equalizer, stereo enhancement, and real-time audio visualization in a modern GTK4/Libadwaita interface.

**Powered by GTCRN** - Gated Temporal Convolutional Recurrent Network for state-of-the-art voice enhancement.

![BigLinux Microphone Interface](https://github.com/user-attachments/assets/45771b61-8bc1-4c7b-8b2c-58543464f885)

## Features

### AI Noise Reduction
- **GTCRN Neural Network** - Superior voice quality with deep learning
- **Model Selection** - Full Quality (best results) or Low Latency (real-time)
- **Adjustable Strength** - Fine-tune noise reduction intensity (0-100%)

### Audio Processing
- **10-Band Parametric Equalizer** - Full control from 31Hz to 16kHz
- **EQ Presets** - Voice Boost, Podcast, Warm, Bright, De-esser, and more
- **Noise Gate** - Eliminate background noise during silence
- **Transient Suppressor** - Remove clicks and pops

### Stereo Enhancement
- **Dual Mono** - Simple stereo duplication
- **Radio Voice** - Professional broadcast compression
- **Voice Changer** - Pitch adjustment

### Visualization & Monitoring
- **Real-time Spectrum Analyzer** - Three visualization styles
- **Headphone Monitor** - Listen to processed audio with adjustable delay
- **Live Parameter Updates** - Instant feedback without restarting

### User Experience
- **Smart Filter Integration** - Uses PipeWire's `filter.smart` (no virtual device needed)
- **Profile Management** - Save and load custom configurations
- **Persistent Settings** - Automatic save/restore on startup
- **Wayland & X11** - Full compatibility with both display servers

## Requirements

- Linux with PipeWire audio server
- Python 3.10 or later
- GTK4 and Libadwaita 1.0+
- GStreamer with base/good plugins
- GTCRN LADSPA plugin

## Installation

### BigLinux / Arch Linux

```bash
sudo pacman -S biglinux-noise-reduction-pipewire
```

### From Source

1. **Clone the repository:**
```bash
git clone https://github.com/biglinux/biglinux-noise-reduction-pipewire.git
cd biglinux-noise-reduction-pipewire
```

2. **Install dependencies (Arch-based):**
```bash
sudo pacman -S --needed \
    gtcrn-ladspa \
    pipewire \
    swh-plugins \
    python-numpy \
    python-gobject \
    python-cairo \
    gtk4 \
    libadwaita \
    gstreamer \
    gst-plugins-base \
    gst-plugins-good
```

3. **Install the Python package:**
```bash
pip install --user .
```

4. **Run:**
```bash
python -m biglinux_microphone
```

## Usage

### Quick Start

1. Launch from your application menu or run `big-microphone-noise-reduction`
2. Toggle **Noise Reduction** to enable AI processing
3. Adjust the **Strength** slider to balance quality vs. noise removal
4. Expand sections to access Equalizer, Stereo, and advanced settings

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Q` | Quit application |
| `Ctrl+,` | Show preferences hint |

### Command Line

```bash
# Run with debug logging
python -m biglinux_microphone --debug

# Show version
python -m biglinux_microphone --version
```

## Architecture

```
src/biglinux_microphone/
├── application.py      # Adw.Application lifecycle
├── window.py           # Main window and UI composition
├── config.py           # Settings, constants, dataclasses
├── audio/
│   └── filter_chain.py # PipeWire filter-chain generation
├── services/
│   ├── pipewire_service.py    # PipeWire integration
│   ├── settings_service.py    # Settings management
│   ├── profile_service.py     # Profile save/load
│   ├── audio_monitor.py       # GStreamer spectrum analysis
│   ├── monitor_service.py     # Headphone monitoring
│   └── config_persistence.py  # State persistence
├── ui/
│   ├── main_view.py           # Primary UI components
│   ├── spectrum_widget.py     # Audio visualizer
│   ├── components.py          # Reusable UI widgets
│   └── base_view.py           # View base class
└── utils/
    ├── i18n.py                # Internationalization
    ├── async_utils.py         # Async helpers
    └── validators.py          # Input validation
```

## Technical Details

### PipeWire Integration

The application generates a PipeWire filter-chain configuration that processes audio in real-time. The `filter.smart` feature allows transparent integration with applications - no need to manually select a virtual microphone.

### GTCRN Neural Network

GTCRN (Gated Temporal Convolutional Recurrent Network) is a deep learning model optimized for real-time speech enhancement:

| Feature | GTCRN | Traditional (RNNoise) |
|---------|-------|-----------------------|
| Architecture | TCN + GRU | Simple RNN |
| Voice Quality | Excellent | Good |
| Model Options | 2 (Quality/Latency) | 1 |
| Strength Control | Yes (0-100%) | No |
| CPU Usage | Moderate | Low |

### Supported LADSPA Plugins

- `libgtcrn_ladspa.so` - AI noise reduction
- `sc4m_1916.so` - Radio voice compression
- `pitch_scale_1193.so` - Voice pitch shifting
- SWH plugins - Gate, EQ, stereo effects

## Localization

Translations available for 25+ languages. Translation files are in the `locale/` directory.

To contribute translations:
1. Copy `locale/biglinux-noise-reduction-pipewire.pot` to `locale/<lang>.po`
2. Translate the strings
3. Submit a pull request

## Contributing

Contributions are welcome! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Write tests for new functionality
4. Ensure code passes linting (`ruff check src/`)
5. Commit with descriptive messages
6. Push and open a Pull Request

### Development Setup

```bash
# Install development dependencies
pip install -e ".[dev]"

# Run tests
pytest tests/ -v

# Run linter
ruff check src/

# Type checking
mypy src/biglinux_microphone/
```

## License

GNU General Public License v3.0 - see [LICENSE](LICENSE) for details.

## Credits

**Developed by BigLinux Team**

- [GTCRN Project](https://github.com/Xiaobin-Rong/gtcrn) - Neural network for noise reduction
- [PipeWire](https://pipewire.org/) - Modern audio server
- [GTK4](https://gtk.org/) / [Libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - UI framework

## Screenshots

![Main Interface](https://github.com/user-attachments/assets/030fc674-52b2-47e1-aefe-ecc35f16ae70)

![Equalizer Settings](https://github.com/user-attachments/assets/a8ca1637-9d31-4688-a79b-d341a8a4e1ec)

![Advanced Options](https://github.com/user-attachments/assets/84c4f3a5-3682-45e8-97ad-1ac07eb23ff2)
