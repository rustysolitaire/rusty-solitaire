# Changelog

Every bug fix or improvement raises the minor version. The version is set in
`Cargo.toml`; add a matching `<release>` to
`packaging/linux/io.github.rustysolitaire.RustySolitaire.metainfo.xml`.

## 0.3.0 — 2026-10-29

- Game → Statistics: wins, games played, win percentage and the best and
  worst winning time for each game, with Reset. Counted as in AisleRiot: a
  started game counts when you start a new one, switch games or quit.
- Sounds: cards click when put down and slide back when a move fails, and
  there is a sound for a win and for running out of moves. Control → Sound
  turns them off.
- Play with the keyboard: arrow keys, Home, End, Page Up and Page Down move
  a focus outline between cards and piles, Space selects cards, Return moves
  them (or deals), Ctrl+Return sends a card to a foundation, Ctrl+A selects.
- Help → Keyboard Shortcuts (Ctrl+? or Ctrl+F1) lists all the keys.
- Help → Contents (F1) and Help on <game> (Shift+F1) open the rules of all 91
  games in the web browser. The pages are built into the program and work
  offline.
- Removed Help → Record Input Log; start the program with `--log-input` to
  record one.

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
