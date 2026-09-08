# big-relm4-components

Reusable Relm4/libadwaita components for BigLinux applications.

This crate is for BigLinux visual patterns and app workflows that are not
already covered by upstream `relm4-components`.

This crate wraps libadwaita through Relm4/gtk-rs. It does not copy or vendor
libadwaita implementation code, CSS, screenshots, icons, or demo assets.

## Rule

Before adding a component, check upstream first:

- `relm4`
- `relm4-components`
- `relm4-css` through `relm4::css`
- `relm4-icons` / `relm4-icons-build`
- `relm4-macros` through `relm4` re-exports
- GTK/libadwaita widgets

If upstream already solves the generic problem, use it directly or wrap it with
BigLinux styling. Do not fork or duplicate generic behavior.

If two screens look or behave alike, make the shared component/widget first.
Local one-off copies are only acceptable while proving a pattern.

See `USE_UPSTREAM_RELM4_COMPONENTS.md` for the components that must be used
directly from upstream.

See `docs/upstream-relm4-ecosystem.md` for the wider Relm4 crate inventory and
the adoption rules for icons, CSS constants, and macros.

See `docs/component-story-harness.md` for the one-command story harness agents
should use to render shared menus/dialogs at 1280x720 before opening a full app.

See `ADWAITA_GALLERY_ROADMAP.md` for the libadwaita widget-gallery based
implementation order.

See `docs/multimedia-component-candidates.md` for candidates found in
BigLinux multimedia apps.

See `docs/multimedia-simplification-plan.md` for the thin-app-code extraction
strategy.

## Current Scope

Good candidates:

- BigLinux app shell: header/sidebar/content/footer layout.
- Media queue rows and factories.
- Conversion/progress task rows.
- Settings groups with BigLinux spacing/a11y conventions.
- Action/navigation rows with explicit AT-SPI-reachable buttons.
- Empty states.
- Dialog/page shells with shared spacing and header behavior.
- Illustrated settings cards.
- Slider rows with reset buttons.
- Illustrated slider rows with value labels and reset buttons.
- Media transport controls.
- Custom tooltip and icon-button a11y helpers.
- A11y contract helpers.
- Screenshot/AT-SPI test helpers.

Out of scope:

- Generic alert/open/save dialogs already provided by `relm4-components`.
- Generic combo wrappers already provided by `relm4-components`.
- App domain logic: ffmpeg, mpv policy, profiles, settings keys, filesystem
  mutations.

## Promotion Flow

1. Build component inside a real BigLinux app.
2. Stabilize API through app use.
3. Check upstream duplication again.
4. Copy/promote the generic part here.
5. Add model tests, a11y contract tests, and example usage.
