# big-app-kit

Typed app-building contracts for BigLinux Rust desktop applications.

The crate keeps app code small by exposing typed, display-free specs for common
desktop workflows. Relm4/GTK widgets can wrap these specs without moving app
domain logic into shared UI crates, which keeps the specs testable in isolation.

## Module groups

- **Data + persistence:** `storage`, `recent_files`, `profile_registry`,
  `keyring`, `scheme_loader`.
- **Desktop integration:** `desktop`, `shell`, `subprocess`, `url`, `services`.
- **UI contracts:** `dialogs`, `file_dialogs`, `forms`, `previews`, `widgets`,
  `catalog`, `collections`.
- **Media + remote:** `mpris`, `remote_control` (feature `remote-control`),
  `http_client` (feature `http-client`).
- **AI workflow scaffolding:** `ai_assistant`, `ai_history`, `ai_provider_config`.
- **Misc:** `files`, `tasks`, `keybindings`.

## Features

- `http-client` (default) — blocking HTTP policy wrapper backed by system `curl`.
- `remote-control` (default) — local HTTP remote-control endpoint (`tiny_http`).

Disable defaults for a smaller dependency graph when those modules are unused.

## License

MIT
