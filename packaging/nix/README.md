# Nix integration

The derivation uses the system allocator, the same feature set for build and
tests, a declared PipeWire/WirePlumber/systemd runtime closure, and the gettext
domain `biglinux-microphone`. It installs all four binaries, user units and
WirePlumber policy alongside the UI assets. Native source paths are replaced
with store paths during the package build, not at application startup.

Supply the neural plugins explicitly with `denoiserPackages`. They must provide
the expected shared objects in `lib/ladspa` and their inference-runtime closure.
The default package without those plugins can build, but the application reports
that its processing engine is unavailable; it is not a working denoiser.

For NixOS, import `packaging/nix/module.nix`, enable
`programs.biglinux-microphone.enable` and supply `denoiserPackages`. The module
registers user services and WirePlumber policy through their declared package
options. Merely putting the GUI on PATH is not equivalent to that integration.

Requires Nixpkgs with Rust >= 1.97.1, GTK >= 4.22 and libadwaita >= 1.9.
The native BigLinux/Arch validation does not certify a NixOS deployment: run
`nix build`, the package tests and a disposable NixOS audio-session test before
publishing a Nix release. Hardware and neural-runtime checks remain separate.

References: Nixpkgs Rust packaging manual (Cargo features and bindgenHook),
and the NixOS PipeWire/WirePlumber module's `configPackages` and
`requiredLadspaPackages` contracts.
