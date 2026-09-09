"""Verify the staged Arch payload without installing it or running user units."""

from __future__ import annotations

import gettext
import hashlib
import json
from pathlib import Path
import stat
import sys
import tomllib


def verify(source: Path, package: Path, report: Path) -> None:
    manifest = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
    errors: list[str] = []
    for binary in manifest["bin"]:
        name = binary["name"]
        installed = package / "usr/bin" / name
        compiled = source / "target/release" / name
        if not installed.is_file() or not installed.stat().st_mode & stat.S_IXUSR:
            errors.append(f"Missing executable: {name}")
        elif installed.read_bytes() != compiled.read_bytes():
            errors.append(f"Installed executable differs from this build: {name}")

    # Every versioned runtime resource must reach the package, unchanged.
    for original in (source / "usr").rglob("*"):
        if original.is_file():
            relative = original.relative_to(source)
            installed = package / relative
            if not installed.is_file() or installed.read_bytes() != original.read_bytes():
                errors.append(f"Missing or changed runtime resource: {relative}")

    domain = "biglinux-microphone"
    expected = {path.stem for path in (source / "po").glob("*.po")}
    catalogs = package / "usr/share/locale"
    actual = {path.parent.parent.name for path in catalogs.glob(f"*/LC_MESSAGES/{domain}.mo")}
    if expected != actual:
        errors.append(f"Catalog languages differ: expected {sorted(expected)}, got {sorted(actual)}")
    for path in catalogs.glob(f"*/LC_MESSAGES/{domain}.mo"):
        with path.open("rb") as stream:
            gettext.GNUTranslations(stream)
    brazilian = catalogs / "pt_BR/LC_MESSAGES" / f"{domain}.mo"
    with brazilian.open("rb") as stream:
        translation = gettext.GNUTranslations(stream)
    for message in ("Main menu", "Restore default settings", "About Filter noise"):
        if translation.gettext(message) == message:
            errors.append(f"Brazilian catalog does not translate: {message}")

    documentation = package / "usr/share/doc/biglinux-noise-reduction-pipewire"
    if not (documentation / "guia-rapido.pt_BR.md").is_file():
        errors.append("The Portuguese quick-start guide was not installed")
    units = package / "usr/lib/systemd/user"
    for suffix in ("", "-mic", "-aec", "-output"):
        unit = units / f"biglinux-microphone{suffix}.service"
        if not unit.is_file():
            errors.append(f"Missing user unit: {unit.name}")
    for path in package.rglob("*"):
        if path.is_file() and path.stat().st_mode & (stat.S_ISUID | stat.S_ISGID | stat.S_IWOTH):
            errors.append(f"Unsafe installed permissions: {path.relative_to(package)}")

    files = {
        str(path.relative_to(package)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(package.rglob("*")) if path.is_file()
    }
    report.mkdir(parents=True, exist_ok=True)
    (report / "payload.json").write_text(json.dumps({
        "version": manifest["package"]["version"],
        "catalog_languages": sorted(actual),
        "files_sha256": files,
        "errors": errors,
        "scope": "Staged recipe payload; not a pacman installation or hardware audio test.",
    }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"Verified {len(files)} installed files and {len(actual)} gettext catalogs")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit("Usage: verify-package.py SOURCE STAGED_PACKAGE REPORT")
    verify(*(Path(argument).resolve() for argument in sys.argv[1:]))
