# Native Dependency Exceptions

## pipewire-rs 0.10 vendored bindings

- owner: BigLinux audio stack maintainers.
- reason: `vendor/pipewire-0.10.0` and `vendor/libspa-0.10.0` are temporary Rust binding snapshots used while this app is aligned with the shared BigLinux media runtime and the workspace-wide PipeWire crate version policy.
- pacman: `pacman -Si pipewire pipewire-audio pipewire-pulse` confirms distro PipeWire packages are available (`pipewire` 1:1.6.6-1 on 2026-06-18). The exception is for the Rust bindings snapshot, not for bundling the PipeWire runtime library.
- license: local `vendor/pipewire-0.10.0/LICENSE` and `vendor/libspa-0.10.0/LICENSE` are MIT.
- update: review during each PipeWire crate/runtime sweep; prefer normal crates.io dependencies or `big-media-runtime` reexports once all sibling repos can use the same PipeWire binding version without `links` conflicts.
- removal: remove this exception and the `vendor/` snapshots when the repo builds against the canonical workspace PipeWire bindings or a shared runtime wrapper with no vendored source.
