# Changelog

Every bug fix or improvement raises the minor version. The version is set in
`Cargo.toml`; add a matching `<release>` to
`packaging/linux/io.github.rustysolitaire.RustySolitaire.metainfo.xml`.

## 0.2.0 — 2026-10-29

- Credits now name "The Rusty Solitaire authors" throughout: About box,
  installers, package details and the macOS and Windows file information.
- New application ID `io.github.rustysolitaire.RustySolitaire` (desktop entry,
  icons, Flatpak, macOS bundle). The Flatpak installs as a new app next to
  the 0.1.0 one, with fresh settings; uninstall the old one in your software
  centre.

## 0.1.0 — 2026-10-28

First release of Rusty Solitaire, a Rust port of GNOME AisleRiot.

- All 91 AisleRiot games, running the original rule files on a built-in
  Scheme engine, with options, hints, undo/redo and autoplay.
- GTK 3 / Adwaita look: menus, toolbar, status bar, Select Game dialog,
  Recently Played, 26 card designs.
- Touch screens: tap, double-tap, finger dragging and long-press to see a
  covered card; smooth dragging on high-resolution tablets.
- Help → Record Input Log for diagnosing input problems.
- Packages: Windows MSI (x64, ARM64) with a per-user / all-users choice,
  Linux Flatpak, .deb and .rpm (x86_64, ARM64), macOS universal .dmg.
