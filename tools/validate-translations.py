#!/usr/bin/env python3
"""Validate extraction, catalog syntax and explicitly approved release locales.

Uses GNU gettext and Python's standard library. This gate verifies completeness
and placeholder integrity; it cannot certify the linguistic quality of a locale.
"""
from __future__ import annotations

import argparse
import ast
import json
import re
import subprocess
import tempfile
from pathlib import Path


def read_po(path: Path) -> dict[tuple[str, str], dict]:
    entries = {}
    # GNU gettext is the syntax authority. This deliberately small reader only
    # collects keys, flags and strings for coverage comparisons after validation.
    for block in re.split(r"\n\s*\n", path.read_text(encoding="utf-8")):
        fields, flags, current = {}, set(), None
        obsolete = False
        for line in block.splitlines():
            line = line.strip()
            if line.startswith("#~"):
                obsolete = True
            if not line or line.startswith("#"):
                if line.startswith("#,"):
                    flags.update(x.strip() for x in line[2:].split(","))
                continue
            match = re.match(r'(msgctxt|msgid_plural|msgid|msgstr(?:\[\d+\])?)\s+(".*")$', line)
            if match:
                current = match[1]
                if current in fields:
                    raise ValueError(f"{path}: duplicate {current}")
                fields[current] = ast.literal_eval(match[2])
            elif line.startswith('"') and current is not None:
                fields[current] += ast.literal_eval(line)
            else:
                raise ValueError(f"{path}: unsupported PO line {line[:100]!r}")
        if obsolete or "msgid" not in fields:
            continue
        key = fields.get("msgctxt", ""), fields["msgid"]
        if key in entries:
            raise ValueError(f"{path}: duplicate message {key!r}")
        fields["flags"] = flags
        entries[key] = fields
    return entries


def release_languages(root: Path) -> list[str]:
    languages = []
    for line in (root / "po/LINGUAS").read_text().splitlines():
        languages.extend(line.split("#", 1)[0].split())
    if not languages or len(languages) != len(set(languages)):
        raise ValueError("po/LINGUAS must contain a nonempty unique release allowlist")
    if any(not re.fullmatch(r"[A-Za-z][A-Za-z0-9_@-]*", lang) for lang in languages):
        raise ValueError("Invalid locale identifier in po/LINGUAS")
    return languages


def run(argv: list[str], root: Path) -> None:
    subprocess.run(argv, cwd=root, check=True, timeout=30)


def extract(root: Path, destination: Path) -> dict:
    files = [line.strip() for line in (root / "po/POTFILES.in").read_text().splitlines()
             if line.strip() and not line.lstrip().startswith("#")]
    missing = []
    for path in (root / "app/biglinux_microphone").rglob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        if any(isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id == "_"
               for n in ast.walk(tree)) and path.relative_to(root).as_posix() not in files:
            missing.append(path.relative_to(root).as_posix())
    if missing:
        raise ValueError(f"Translatable files missing from POTFILES.in: {missing}")
    common = ["xgettext", "--from-code=UTF-8", "--output=" + str(destination)]
    python = [f for f in files if f.endswith(".py")]
    qml = [f for f in files if f.endswith(".qml")]
    if not python:
        raise ValueError("POTFILES.in contains no Python inputs")
    run(common + ["--language=Python", "--keyword=_", *python], root)
    if qml:
        run(common + ["--language=JavaScript", "--keyword=i18nd:2", "--join-existing", *qml], root)
    return read_po(destination)


def validate(root: Path) -> dict:
    approved = release_languages(root)
    expected = read_po(root / "po/biglinux-microphone.pot")
    source_keys = set(expected) - {("", "")}
    errors, locales = [], {}
    with tempfile.TemporaryDirectory(prefix="bigmic-gettext-") as directory:
        temp = Path(directory)
        actual = set(extract(root, temp / "current.pot")) - {("", "")}
        if actual != source_keys:
            errors.append("POT is stale; run tools/update-translations.sh")
        for po in sorted((root / "po").glob("*.po")):
            lang = po.stem
            run(["msgfmt", "--check", "--check-format", str(po), "-o", str(temp / (lang + ".mo"))], root)
            entries = read_po(po)
            header = entries.get(("", ""), {}).get("msgstr", "")
            plural = re.search(r"nplurals\s*=\s*(\d+)", header)
            missing, fuzzy, untranslated = [], [], []
            invalid_keys = set()
            for key in source_keys:
                entry = entries.get(key)
                if entry is None:
                    missing.append(key[1])
                    invalid_keys.add(key)
                    continue
                if "fuzzy" in entry["flags"]:
                    fuzzy.append(key[1])
                    invalid_keys.add(key)
                if "msgid_plural" in expected[key]:
                    count = int(plural[1]) if plural else 0
                    values = [entry.get(f"msgstr[{i}]", "") for i in range(count)]
                    if not values or not all(value.strip() for value in values):
                        untranslated.append(key[1])
                        invalid_keys.add(key)
                elif not entry.get("msgstr", "").strip():
                    untranslated.append(key[1])
                    invalid_keys.add(key)
            extra = set(entries) - source_keys - {("", "")}
            complete = len(source_keys - invalid_keys)
            locales[lang] = {"approved": lang in approved, "messages": len(source_keys),
                             "translated_nonfuzzy": complete, "missing": len(missing),
                             "fuzzy": len(fuzzy), "untranslated": len(untranslated), "extra": len(extra)}
            if lang in approved:
                if missing or fuzzy or untranslated or extra:
                    errors.append(f"{lang}: approved catalog is incomplete or stale")
                if not re.search(r"(?m)^Language:\s*" + re.escape(lang) + r"\s*$", header):
                    errors.append(f"{lang}: Language header mismatch")
        for lang in approved:
            if lang not in locales:
                errors.append(f"{lang}: approved catalog is missing")
    return {"ok": not errors, "source_messages": len(source_keys),
            "approved": approved, "locales": locales, "errors": errors}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    try:
        report = validate(args.root.resolve())
    except (OSError, ValueError, SyntaxError, subprocess.SubprocessError) as error:
        report = {"ok": False, "errors": [str(error)]}
    text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(text, encoding="utf-8")
    print(text, end="")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
