# Rusty Solitaire on the web (prototype)

The same game, compiled to WebAssembly, running in a web page: no install.

```bash
web/build.sh            # build into web/dist/
web/build.sh --serve    # build, then serve on http://localhost:8090/
```

Needs the `wasm32-unknown-unknown` Rust target and `wasm-bindgen-cli` at the version of
the `wasm-bindgen` crate in `Cargo.lock` (the build fails with a clear message if they
differ):

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

If binaryen's `wasm-opt` is installed, the build also runs it to make the file smaller.

`web/dist/` holds `rusty-solitaire.js` and `rusty-solitaire_bg.wasm` (about 19 MB, 11 MB
compressed: every card theme is built in), and `index.html`, a page with just the game and
its own menus for trying it out. The website's play page (`website/src/play/`, written by
`website/tools/gen-games.py`, with `play.js`) puts the website's controls around the
game; `website/build.sh` copies the two game files there. Any web server can host them;
`.wasm` must be served as `application/wasm`, and should be compressed.

## How it fits together

| Part | Desktop | Web |
|---|---|---|
| Window | A window | `<canvas id="canvas">` in the page, sized to fill the element around it (`src/web.rs`) |
| Settings, statistics | `settings.ini` in the config folder | The same text in the browser's local storage (`rusty-solitaire-settings`) |
| Sound | ALSA / cpal on a thread | Web Audio, from the first click or tap (browsers allow no sound before) |
| Rules | A page opened in the web browser | `window.rustySolitaireShowHelp(html)` in the page (`index.html` shows it over the game); without it, a new tab |
| Scheme stack | Grown as needed (`stacker`) | A fixed 32 MB (`.cargo/config.toml`); every game plays in 8 MB |
| `--log-input` | A log file | Not available |

The page may put the canvas in any element; the game follows that element's size. So
it can fill the page, or sit in a box or an `<iframe>` on the website.

## The page's own controls

The website's play page styles the game like the rest of the site: its own drop-down for
the game, buttons for New game, Restart, Undo, Redo, Deal, Hint and Rules, an Options
menu (the game's options, card style, sound, click to move, statistics, keyboard
shortcuts) and a status line. The game then shows only the card table. A page gets this by defining, before the
game starts:

- `window.rustySolitaireState(state)`: called with the game's state whenever it changes,
  sometimes only the parts that changed (the clock sends just `time`). Once at the start:
  `games` and `themes`, lists of `[file, name]`. Then: `game`, `name`, `status`, `score`,
  `showScore`, `time`, `canUndo`, `canRedo`, `canDeal`, `canRestart`, `canHint`, `theme`,
  `sound`, `clickToMove`, and `options` (`{label, value, group}`; options with the same
  `group` number are radio buttons, `null` is a checkbox).

The game then defines `window.rustySolitaireAction(id)` for the page to call, with the ids
of the desktop menu items (`main.rs`): `new-game`, `restart`, `undo`, `redo`, `deal`,
`hint`, `help-game`, `statistics`, `shortcuts`, `toggle-sound`, `toggle-click-to-move`,
`game:<file>`, `theme:<file>`, `option:<index>`.

A page without `rustySolitaireState` gets the game with its own menu bar, toolbar and
status bar, as on the desktop. Dialogs (hints, end of game, statistics, keyboard
shortcuts) are the game's own in both cases.

## Known gaps

- Without the page's controls, on screens narrower than 400 px the window's minimum
  width cuts off the right edge (with them, the minimum is 240 px).
- Leaving the page doesn't count the game in progress in the statistics (the desktop
  counts it when the window closes).
- Without the page's controls, View → Fullscreen hasn't been tested in a page. Game →
  Close does nothing in a page, and Close on the end-of-game dialog only closes the
  dialog.
