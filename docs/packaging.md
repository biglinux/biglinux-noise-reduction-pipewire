# Packaging and rolling-release resilience

The production target is BigLinux/Arch-family packaging.

## Why the GUI is not installed in versioned `site-packages`

Arch's normal Python-library packaging places modules under `/usr/lib/pythonX.Y/site-packages`, and a Python minor upgrade requires packages tied to that interpreter to be rebuilt. That is correct for reusable Python modules and especially for C extensions.

BigLinux Microphone's GUI, however, is application-owned pure Python source. Installing that source in a private application directory avoids coupling the application's own files to a Python minor directory:

```text
/usr/lib/biglinux-microphone/
├── launcher.py
└── biglinux_microphone/
```

The distribution still owns Python itself and binary Python dependencies such as PyGObject/numpy. Those must be updated together with the active interpreter during a full system upgrade.

References:

- Python isolated mode (`-I`): https://docs.python.org/3/using/cmdline.html#cmdoption-I
- Python path initialization: https://docs.python.org/3/library/sys_path_init.html
- Arch Python package guidelines: https://wiki.archlinux.org/title/Python_package_guidelines

## Launcher contract

`packaging/launcher.py` starts with:

```text
#!/usr/bin/python3 -I
```

The package exposes it through a relative symlink:

```text
/usr/bin/biglinux-microphone -> ../lib/biglinux-microphone/launcher.py
```

`Path(__file__).resolve()` finds the private application root even when invoked through the symlink. Isolated mode:

- ignores `PYTHONPATH`;
- excludes user site-packages;
- prevents current-directory/script-directory import injection;
- still allows normal system site-packages because `-S` is not used.

The launcher then inserts only the package-owned private directory.

## Why not scan old Python directories

Do not implement a loop such as:

```text
/usr/lib/python3.14/site-packages/...
/usr/lib/python3.15/site-packages/...
```

Selecting an old directory can combine application code or binary extensions built for one interpreter with another interpreter. It also hides packaging mistakes and makes ownership ambiguous.

A launcher failure after a Python update should identify the missing active dependency (for example PyGObject) and ask for a complete system/package update, not silently run stale files.

## Arch package layout

`pkgbuild/PKGBUILD` installs:

- Rust backend binaries in `/usr/bin`;
- Python GUI in `/usr/lib/biglinux-microphone`;
- launcher symlink in `/usr/bin`;
- systemd user units under `/usr/lib/systemd/user`;
- desktop/AppStream/icons/WirePlumber/Plasma resources from `usr/`;
- only approved `po/LINGUAS` catalogs under `/usr/share/locale`;
- maintained documentation under `/usr/share/doc/<package>/`, preserving relative links.

The package does **not** ship Python caches (`__pycache__`, `.pyc`, `.pyo`).

## Upgrade tests

Every packaging change should verify at least:

1. launcher through the relative `/usr/bin` symlink;
2. arguments are preserved;
3. `PYTHONPATH`/user-site shadow packages are ignored;
4. an old fake `pythonX.Y/site-packages/biglinux_microphone` is ignored;
5. incomplete installation fails clearly;
6. missing PyGObject reports an actionable error;
7. no versioned Python directory is hard-coded in the launcher;
8. the package contains no generated Python caches.

The automated launcher tests live in `app/tests/test_launcher.py`.

## Other package formats

Broken legacy Nix/Flatpak definitions were removed when the GUI moved from Rust to Python. Reintroduce another package format only with a maintainer, a current manifest for the Python front-end, and an end-to-end build + launch test. The native Arch/BigLinux package is the release reference.

## Build and test cost

`build()` produces optimized Rust binaries once. `check()` runs library, binary
and integration targets sequentially in the normal test profile rather than
linking every harness again with release LTO. `CARGO_BUILD_JOBS` defaults to two
for makepkg; set it to one on a 4 GiB worker. `CARGO_TARGET_DIR` is honored both
by the build and by `package()`. A failed build or missing binary stops packaging.
The graphical tests use a bounded D-Bus/Xvfb session and retain an explicitly
configured runtime PYTHONPATH. This development path is not installed in the
production launcher.
