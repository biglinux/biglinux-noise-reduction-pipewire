# Contributing to BigLinux Microphone

Thanks for helping improve microphone audio on Linux. Contributions do not need to be large: hardware reports, translations, accessibility fixes and reproducible bug reports are all useful.

## Before changing code

1. Read [AGENTS.md](AGENTS.md) for repository invariants and validation commands.
2. Read [docs/architecture.md](docs/architecture.md) for process ownership.
3. For UI work, read [docs/ui-ux.md](docs/ui-ux.md).
4. For audio/backend work, check the dated evidence in [docs/research/](docs/research/) before changing a policy based on performance assumptions.

## Development setup

The project has two main parts:

- Rust backend (`src/`), pinned to Rust 1.98.1 by `rust-toolchain.toml`.
- Python GTK4/libadwaita UI (`app/`) using system PyGObject.

Run the Python suite with:

```bash
tools/check-release.sh --ui-only
```

Run Rust checks with:

```bash
CARGO_BUILD_JOBS=1 tools/check-release.sh
```

Do not weaken tests to make an environment limitation disappear. If a test needs a display, PipeWire hardware or an unavailable dependency, document that boundary.

## Pull requests

Keep changes focused. A good pull request explains:

- the user-visible problem;
- why the chosen layer owns the fix;
- compatibility/migration impact;
- tests run;
- hardware checks still required.

UI changes should include a real application screenshot when practical, not only a design mockup.

## Translations

English strings in source are the gettext source language. Translation catalogs live in `po/`; see [docs/translations.md](docs/translations.md).

Do not copy `pt_BR` into `pt`, or simplified Chinese into traditional Chinese if both are added in the future. Treat each locale as its own language variant.

## Packaging changes

Read [docs/packaging.md](docs/packaging.md). The Python GUI intentionally lives outside versioned `site-packages` in the BigLinux/Arch package. Do not replace that with a hard-coded Python minor path or a runtime scan of old paths.

## Commit hygiene

Do not commit:

- `target/`, Python caches, PEP 517 build output, makepkg work directories;
- temporary recordings or screenshots unrelated to maintained documentation;
- copied dependency caches;
- one-off review/evidence bundles that belong in release artifacts.

## Reporting a bug

See [SUPPORT.md](SUPPORT.md). Include enough system/audio context to reproduce the issue and remove private information from logs before attaching them.
