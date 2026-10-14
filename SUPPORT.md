# Support

## Start with diagnostics

Open **Advanced settings → Diagnostics** or run:

```bash
biglinux-microphone-cli doctor
```

When reporting a problem, include:

- BigLinux/Manjaro version and kernel;
- PipeWire and WirePlumber versions;
- CPU model and whether the problem appears only under load or on battery;
- microphone model/connection type (USB, analog, Bluetooth, webcam, etc.);
- selected noise-reduction mode and effective model shown by Diagnostics;
- exact error message;
- whether the problem reproduces after restarting PipeWire/WirePlumber.

Do not attach private recordings unless they are necessary and intentionally prepared for sharing. Review logs for usernames, paths and device identifiers before posting them publicly.

## Common packaging problem after a Python upgrade

The BigLinux/Arch package keeps the application sources in `/usr/lib/biglinux-microphone`, so the app itself is not tied to `/usr/lib/pythonX.Y/site-packages`.

If a Python upgrade leaves the launcher reporting that PyGObject is unavailable, update the whole system so `python`, `python-gobject` and the rest of the distribution Python stack are from the same repository state:

```bash
sudo pacman -Syu
```

Avoid copying an older `site-packages` directory into the new Python version.

## Audio problems

If the microphone cuts out, sounds robotic or causes XRUNs:

1. switch noise reduction to **Lighter**;
2. disable optional advanced processing temporarily;
3. check Diagnostics;
4. reproduce with one application recording the microphone;
5. include the result in the issue.

For echo problems, test with headphones first. That separates acoustic speaker-to-microphone leakage from processing/routing issues.
