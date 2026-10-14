# Translations

The source language is English. `po/biglinux-microphone.pot` is the current extraction template and `po/*.po` contains locale translations.

## Rules

- Wrap user-visible Python strings with the project gettext helper.
- Add new translatable Python source files to `po/POTFILES.in`.
- Keep locale variants genuinely localized; do not copy `pt_BR` to `pt` or one Chinese variant to another and call it complete.
- Preserve placeholders, markup and accelerator semantics.
- Prefer short, direct language that explains the user action rather than internal implementation terminology.


## Refresh the template

After changing user-visible strings or `po/POTFILES.in`, run:

```bash
tools/update-translations.sh
```

The helper regenerates the POT from the maintained Python/QML surfaces, merges exact/fuzzy matches into existing catalogs and removes obsolete entries. Fuzzy or untranslated entries still require human review; generating a catalog is not equivalent to translating it.

## Validate catalogs

```bash
python3 tools/validate-translations.py
```

`po/LINGUAS` is the explicit release allowlist. The current approved catalog is
`pt_BR`; English is the source fallback. Other catalogs remain in source for
completion/review and are not installed by the native package. Do not promote a
locale merely because msgfmt succeeds. The gate rejects missing/fuzzy/stale or
empty approved messages and verifies Python/QML extraction.

PT-BR wording in R3 was manually edited with AI assistance and checked in real
GTK fixtures. This is not an independent native-speaker certification. Backend
and external-tool diagnostic messages may remain in their original language.

The Arch package compiles only approved catalogs to:

```text
/usr/share/locale/<locale>/LC_MESSAGES/biglinux-microphone.mo
```

## UI review

Translations should be checked in the real UI, especially at narrow widths. Long strings must wrap/reflow instead of clipping controls.
