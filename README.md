# BigLinux Microphone - AI Noise Reduction for PipeWire


![BigLinux Microphone Interface](https://github.com/user-attachments/assets/45771b61-8bc1-4c7b-8b2c-58543464f885)

## Features

### AI Noise Reduction
- **GTCRN Neural Network** - Superior voice quality with deep learning
- **Adjustable Strength** - Fine-tune noise reduction intensity (0-100%)

### Audio Processing
- **Equalizer**
- **EQ Presets** - Voice Boost, Podcast, Warm, Bright, De-esser, and more
- **Noise Gate** - Eliminate background noise during silence

### Voice Enhancement
- **Dual Mono** - Simple stereo duplication
- **Radio Voice** - Professional broadcast compression
- **Voice Changer** - Pitch adjustment

### Visualization & Monitoring
- **Real-time Spectrum Analyzer** - Three visualization styles
- **Headphone Monitor** - Listen to processed audio with adjustable delay
- **Live Parameter Updates** - Instant feedback without restarting

### User Experience
- **Smart Filter Integration** - Uses PipeWire's `filter.smart` (no virtual device needed)
- **Persistent Settings** - Automatic save/restore on startup

## Requirements

- Linux with PipeWire audio server
- Python 3.10 or later
- GTK4 and Libadwaita 1.0+
- GStreamer with base/good plugins
- GTCRN LADSPA plugin

## License for our configuration interface

GNU General Public License v3.0 - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [GTCRN Project](https://github.com/Xiaobin-Rong/gtcrn) - Neural network for noise reduction
- [PipeWire](https://pipewire.org/) - Modern audio server
- [GTK4](https://gtk.org/) / [Libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - UI framework
