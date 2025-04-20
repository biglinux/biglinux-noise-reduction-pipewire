# BigLinux Noise Reduction for PipeWire

A modern, feature-rich microphone noise reduction application for Linux systems running PipeWire. This tool provides an elegant GTK4 user interface with real-time audio visualization and easy-to-use controls for reducing background noise during calls, recordings, and online meetings.

![image](https://github.com/user-attachments/assets/45771b61-8bc1-4c7b-8b2c-58543464f885)


## ✨ Features

- **Advanced Noise Reduction**: Remove background noise and sounds that interfere with recordings and online calls
- **Real-time Audio Visualization**: See your microphone input with multiple visualization styles:
  - Modern Waves - Smooth flowing waveform visualization
  - Retro Bars - Classic equalizer-style visualization
  - Spectrum - Radial spectrum analyzer
- **Bluetooth Support**: Automatically activate Bluetooth microphone when requested
- **User-friendly Interface**: Modern GTK4 interface with Adwaita styling
- **Persistent Settings**: Your configuration is automatically saved and restored
- **Resource Efficient**: Minimal CPU and memory usage when running in the background
- **Wayland Compatible**: Works seamlessly on both Wayland and X11

## 📋 Requirements

- Linux system with PipeWire audio
- Python 3.7+
- GTK4 and libadwaita
- GStreamer with appropriate plugins
- NumPy

## 🚀 Installation

### From Package Manager (BigLinux)

```bash
sudo pacman -S biglinux-noise-reduction-pipewire
```

### From Source

1. Clone the repository:
```bash
git clone https://github.com/biglinux/biglinux-noise-reduction-pipewire.git
cd biglinux-noise-reduction-pipewire
```

2. Install dependencies:

For Arch-based systems:
```bash
sudo pacman -S noise-suppression-for-voice-big pipewire swh-plugins python-numpy gettext python-gobject
```

For Debian/Ubuntu-based systems:
```bash
sudo apt install pipewire ladspa-sdk python3-numpy gettext python3-gi python3-gi-cairo
# Note: You may need to manually install noise-suppression-for-voice from source
```

3. Run the application:
```bash
./usr/share/biglinux/microphone/launcher.py
```

## 💻 Usage

1. **Start the application** from your application menu or run:
```bash
biglinux-noise-reduction-pipewire
```

2. **Enable noise reduction** by toggling the "Noise Reduction" switch.

3. **Select visualization style** using the buttons below the audio visualizer.

4. **Toggle Bluetooth auto-switching** with the "Bluetooth Autoswitch" option.

5. **Click on the center icon** in the visualizer to quickly toggle noise reduction.

## 🔧 Technical Details

The application integrates with systemd user services to manage the noise reduction pipeline. The core components include:

- **NoiseReducerService**: Manages the systemd service and backend operations
- **AudioVisualizer**: Provides real-time audio visualization using GStreamer
- **NoiseReducerApp**: GTK4/Adwaita-based user interface

The noise reduction is implemented using the [noise-suppression-for-voice](https://github.com/werman/noise-suppression-for-voice) library integrated with PipeWire filters that are activated/deactivated through the systemd user service.

## 🧩 Architecture

```
biglinux-noise-reduction-pipewire/
├── noise_reducer.py       # Main application GUI
├── noise_reducer_service.py # Service manager
├── audio_visualizer.py    # Audio visualization component
├── launcher.py            # Dependency checking and application launcher
└── actions.sh             # System integration script
```

## 🔄 Integration

The application integrates seamlessly with:

- **PipeWire**: Modern audio server for Linux
- **Systemd**: Service management for noise reduction
- **noise-suppression-for-voice**: High-quality noise reduction library
- **GTK4/libadwaita**: For a native GNOME look and feel 
- **GStreamer**: Audio capture and visualization

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📜 License

This project is licensed under the GNU General Public License v3.0 - see the LICENSE file for details.

## 👏 Credits

Developed by the BigLinux Team.

This application utilizes the [noise-suppression-for-voice](https://github.com/werman/noise-suppression-for-voice) project by werman for its noise reduction capabilities.

## 📸 Screenshots

![image](https://github.com/user-attachments/assets/030fc674-52b2-47e1-aefe-ecc35f16ae70)

![image](https://github.com/user-attachments/assets/a8ca1637-9d31-4688-a79b-d341a8a4e1ec)

![image](https://github.com/user-attachments/assets/84c4f3a5-3682-45e8-97ad-1ac07eb23ff2)

