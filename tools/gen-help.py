#!/usr/bin/env python3
"""Builds assets/help/rules.html, the help that opens from the Help menu.

    python3 tools/gen-help.py [AISLERIOT_HELP_DIR]

AISLERIOT_HELP_DIR is the help/C folder of the GNOME AisleRiot source (one
DocBook file per game, plus definitions.xml for the glossary); by default
../reference/aisleriot-original/help/C next to this app folder.

The result is a single HTML file with the contents page, every game's rules,
the keyboard keys, the glossary and the documentation license. It has no
other files to load, because the program writes it to a cache folder and
hands it to the web browser, and a sandboxed (Flatpak) program can only
pass the browser that one file. Links between pages are #anchors; the page
shown first is set by replacing @START@. The rules text is AisleRiot's
manual, under the GNU FDL.
"""

import html
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
HELP = Path(sys.argv[1]) if len(sys.argv) > 1 else APP.parent / "reference/aisleriot-original/help/C"
GAMES = APP / "assets/games"
GFDL = APP / "assets/licenses/COPYING.GFDL"
OUT = APP / "assets/help/rules.html"


def help_file(game):
    for name in (game.replace("-", "_"), game):
        path = HELP / f"{name}.xml"
        if path.exists():
            return path
    raise SystemExit(f"no help page for {game}")


def esc(text):
    return html.escape(text, quote=True)


def squash(text):
    return re.sub(r"\s+", " ", text or "")


def anchor(title):
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")


class Renderer:
    """Turns the handful of DocBook elements the help uses into HTML."""

    def __init__(self, ids):
        self.ids = ids  # sect1 id -> page id, for cross-links
        self.page = ""  # prefix for section anchors on the current page

    def inline(self, el):
        out = esc(squash(el.text))
        for child in el:
            out += self.element(child) + esc(squash(child.tail))
        return out

    def children(self, el, skip=("title",)):
        out = []
        if squash(el.text).strip():
            out.append(f"<p>{esc(squash(el.text).strip())}</p>")
        for child in el:
            if child.tag not in skip:
                out.append(self.element(child))
            if squash(child.tail).strip():
                out.append(f"<p>{esc(squash(child.tail).strip())}</p>")
        return "\n".join(o for o in out if o)

    def element(self, el):
        tag = el.tag
        if not isinstance(tag, str) or tag in ("sect1info", "title", "screenshot"):
            return ""
        if tag == "para":
            text = self.inline(el).strip()
            return f"<p>{text}</p>" if text else ""
        if tag == "emphasis":
            return f"<em>{self.inline(el)}</em>"
        if tag == "ulink":
            url = el.get("url", "")
            target = url.split("#", 1)[1] if url.startswith("ghelp:aisleriot#") else None
            if target in self.ids:
                href = f"#{self.ids[target]}"
            elif url == "help:fdl":
                href = "#license"
            else:
                href = url
            return f'<a href="{esc(href)}">{self.inline(el)}</a>'
        if tag == "sect2":
            return self.section(el, "h2")
        if tag == "sect3":
            return self.section(el, "h3")
        if tag == "informaltable":
            return self.table(el)
        if tag == "variablelist":
            items = []
            for entry in el.findall("varlistentry"):
                term = entry.find("term")
                items.append(f"<dt>{self.inline(term).strip()}</dt>")
                items.append(f"<dd>{self.children(entry.find('listitem'))}</dd>")
            return '<dl class="options">\n' + "\n".join(items) + "\n</dl>"
        if tag in ("itemizedlist", "orderedlist"):
            name = "ul" if tag == "itemizedlist" else "ol"
            items = "\n".join(f"<li>{self.children(li)}</li>" for li in el.findall("listitem"))
            return f"<{name}>\n{items}\n</{name}>"
        return self.children(el)

    def section(self, el, heading):
        title = squash(el.findtext("title")).strip()
        return (f'<section id="{self.page}--{anchor(title)}">\n<{heading}>{esc(title)}</{heading}>\n'
                f"{self.children(el)}\n</section>")

    def table(self, el):
        rows = []
        for row in el.iter("row"):
            cells = row.findall("entry")
            parts = [f"<th scope=\"row\">{self.inline(cells[0]).strip()}</th>"]
            parts += [f"<td>{self.cell(c)}</td>" for c in cells[1:]]
            rows.append("<tr>" + "".join(parts) + "</tr>")
        return '<table class="setup">\n' + "\n".join(rows) + "\n</table>"

    def cell(self, el):
        if any(child.tag == "para" for child in el):
            return self.children(el)
        return self.inline(el).strip()


def author(sect1):
    """The "Written by …" line, removed from the body, or the sect1info author."""
    for para in sect1.findall("para"):
        text = squash("".join(para.itertext())).strip()
        if text.startswith("Written by"):
            sect1.remove(para)
            return text.removeprefix("Written by").strip(" ,")
    first = sect1.findtext("sect1info/author/firstname")
    last = sect1.findtext("sect1info/author/surname")
    return " ".join(p for p in (first, last) if p) or None


def setup_value(sect1, label):
    for row in sect1.iter("row"):
        cells = row.findall("entry")
        if len(cells) == 2 and squash("".join(cells[0].itertext())).strip() == label:
            return squash("".join(cells[1].itertext())).strip()
    return ""


def first_para(sect1, section):
    for sect in sect1.findall("sect2"):
        if squash(sect.findtext("title")).strip() == section:
            para = sect.find(".//para")
            if para is not None:
                return squash("".join(para.itertext())).strip()
    return ""


def keys(*names):
    return " ".join(f"<kbd>{esc(n)}</kbd>" for n in names)


def key_table(rows):
    body = "\n".join(f"<tr><th scope=\"row\">{' or '.join(keys(*k.split('+')) for k in ks.split(' | '))}</th><td>{text}</td></tr>"
                     for ks, text in rows)
    return f'<table class="setup keys">\n{body}\n</table>'


CONTENTS = """
<section id="index--playing">
<h2>Playing</h2>
<p>Drag cards with the mouse or a finger and let go where they should land.
Piles that can take the cards light up while you drag.</p>
<ul>
<li><strong>Click to move:</strong> with <em>Control → Click to Move</em> on, click a card to pick it up
and click where it should go. That is easier on the hand than holding the button down.</li>
<li><strong>Double-click</strong> (or double-tap) a card to send it to a foundation, if it may go there.</li>
<li><strong>See a covered card:</strong> hold the right mouse button on it, or rest a finger on it.</li>
<li><strong>Keyboard:</strong> every move can be made with the keys, see <a href="#keyboard">Keyboard</a>.</li>
<li><em>Control → Hint</em> suggests a move. <em>Undo Move</em> and <em>Redo Move</em> go back and forward
as far as you like.</li>
</ul>
<p>Each game has its own rules, and some have choices in the menu named after the game.
<em>Game → Select Game…</em> switches games; the last five you played are under <em>Recently Played</em>.</p>
</section>
<section id="index--statistics">
<h2>Statistics</h2>
<p><em>Game → Statistics</em> shows, for the game you are playing, how many games you have won out of how
many played, and your fastest and slowest win. A game counts once it has started, when you start a new
one, switch to another game or quit. <em>Restart</em> plays the same deal again and is not counted
separately. <em>Reset</em> in the Statistics window clears the numbers for that game.</p>
</section>
<section id="index--sound">
<h2>Sound</h2>
<p>Cards make a sound when they are put down or slide back, and there is a sound for winning and for
running out of moves. Turn them off or on with <em>Control → Sound</em>.</p>
</section>
"""

KEYBOARD_GENERAL = [
    ("Ctrl+N", "Start a new game"),
    ("Ctrl+O", "Select a game"),
    ("Ctrl+D", "Deal"),
    ("Ctrl+Z", "Undo"),
    ("Shift+Ctrl+Z", "Redo"),
    ("Ctrl+H", "Show a hint"),
    ("F11", "Fullscreen"),
    ("Ctrl+W", "Close the window"),
    ("F1", "Show Help"),
    ("Shift+F1", "Show the rules of this game"),
    ("Ctrl+? | Ctrl+F1", "Show the keyboard shortcuts"),
]

KEYBOARD_CARDS = [
    ("← | →", "Move to the next card in a pile spread sideways, or to the pile on the left or right"),
    ("↑ | ↓", "Move to the next card in a pile spread downwards, or to the pile above or below"),
    ("Ctrl+← | Ctrl+→ | Ctrl+↑ | Ctrl+↓ | Page Up | Page Down", "Move to the next pile, skipping the cards in this one"),
    ("Home | End", "Move to the first or last face-up card of the pile"),
    ("Ctrl+Home | Ctrl+End", "Move to the first or last pile"),
    ("Space", "Select the cards from the focused one up, or unselect them"),
    ("Shift+↑ | Shift+↓ | Shift+← | Shift+→", "Select one card more or less"),
    ("Shift+Home | Ctrl+A", "Select all the cards on top of the pile that can be moved together"),
    ("Shift+End | Shift+Ctrl+A", "Unselect the cards"),
    ("Return", "Move the selected cards onto the focused pile; with nothing selected, click the pile (deal from the stock, turn a card over)"),
    ("Ctrl+Return", "Double-click the focused card: send it to a foundation"),
]

KEYBOARD = f"""
<p>The first arrow key shows a dashed outline around a card: that card has the focus.</p>
<section id="keyboard--cards">
<h2>Moving cards</h2>
<p>To move cards, put the focus on the lowest card to move, press <kbd>Space</kbd> to select it and the
cards on top of it, move the focus to the pile they should go to and press <kbd>Return</kbd>.</p>
{key_table(KEYBOARD_CARDS)}
</section>
<section id="keyboard--general">
<h2>Menu keys</h2>
{key_table(KEYBOARD_GENERAL)}
</section>
"""

STYLE = """
:root {
  --felt: #0b4d1f; --felt-deep: #073716; --rust: #c2552b; --rust-dark: #9d3f1c;
  --ink: #1d1f1c; --ink-soft: #545a52; --paper: #fbfaf6; --paper-alt: #f1efe7;
  --line: #ddd9cc; --card: #ffffff;
  color-scheme: light dark;
}
@media (prefers-color-scheme: dark) {
  :root { --ink: #ecebe5; --ink-soft: #a9ada4; --paper: #141613; --paper-alt: #1b1e1a; --line: #30342e; --card: #20241f; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--paper); color: var(--ink);
  font: 16px/1.6 system-ui, -apple-system, "Segoe UI", Cantarell, Ubuntu, Roboto, sans-serif; }
a { color: var(--rust); }
a:hover { color: var(--rust-dark); }
.wrap { max-width: 1040px; margin: 0 auto; padding: 0 16px; }
@media (min-width: 720px) { .wrap { padding: 0 32px; } }
.topbar { position: sticky; top: 0; z-index: 1; background: var(--felt-deep); }
.topbar .wrap { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 20px; min-height: 52px; }
.topbar strong { color: #fff; margin-right: auto; }
.topbar a { color: #e8efe9; text-decoration: none; font-size: .95rem; }
.topbar a:hover { color: #fff; text-decoration: underline; }
.hero { background: linear-gradient(180deg, var(--felt) 0%, var(--felt-deep) 100%); color: #fff; padding: 28px 0 24px; }
.hero h1 { margin: 0 0 6px; font-size: clamp(1.8rem, 4vw, 2.4rem); line-height: 1.15; }
.hero p { margin: 0; color: #d7e4d9; }
.hero a { color: #f3c9a6; }
.tag { display: inline-block; font-size: .78rem; font-weight: 600; background: rgb(255 255 255 / .15);
  border-radius: 999px; padding: 2px 10px; margin-right: 6px; color: #fff; }
.layout { display: grid; gap: 28px; padding: 28px 16px 8px; }
@media (min-width: 960px) { .layout { grid-template-columns: minmax(0, 1fr) 220px; } }
.rules { max-width: 46em; }
.rules section { margin-bottom: 28px; }
.rules h2 { font-size: 1.35rem; margin: 0 0 10px; padding-bottom: 6px; border-bottom: 1px solid var(--line); }
.rules h3 { font-size: 1.08rem; margin: 18px 0 8px; }
.rules p { margin: 0 0 12px; }
.rules ul, .rules ol { padding-left: 1.3em; }
.rules li { margin-bottom: 4px; }
.rules li p { margin: 0 0 6px; }
.setup { width: 100%; border-collapse: collapse; font-size: .95rem; background: var(--card); border: 1px solid var(--line); }
.setup th, .setup td { text-align: left; vertical-align: top; padding: 9px 14px; border-bottom: 1px solid var(--line); }
.setup th { width: 9.5em; background: var(--paper-alt); font-weight: 600; }
.keys th { width: 15em; font-weight: 400; }
.setup td p:last-child { margin-bottom: 0; }
@media (max-width: 560px) { .setup th, .setup td { display: block; width: auto; } .setup th { border-bottom: 0; padding-bottom: 2px; } }
kbd { display: inline-block; font: .85em ui-monospace, "SF Mono", "Cascadia Mono", "DejaVu Sans Mono", monospace;
  background: var(--card); border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 5px; padding: 0 .4em; margin: 1px 0; }
.options dt { font-weight: 700; margin-top: 12px; }
.options dd { margin: 4px 0 0; padding-left: 14px; border-left: 3px solid var(--line); }
.options dd p { margin: 0 0 6px; }
.toc { display: flex; flex-direction: column; gap: 2px; border-left: 2px solid var(--line); }
.toc a { text-decoration: none; color: var(--ink-soft); padding: 3px 12px; font-size: .93rem; }
.toc a:hover { color: var(--rust); }
@media (max-width: 959px) { .toc { flex-direction: row; flex-wrap: wrap; border-left: 0; gap: 6px; }
  .toc a { border: 1px solid var(--line); border-radius: 999px; padding: 2px 12px; } }
.games { list-style: none; padding: 0; margin: 0; display: grid; gap: 10px; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
.games a { display: block; height: 100%; padding: 10px 14px; background: var(--card); border: 1px solid var(--line);
  border-radius: 10px; text-decoration: none; color: var(--ink); }
.games a:hover { border-color: var(--rust); }
.games strong { display: block; color: var(--rust); }
.games span { font-size: .88rem; color: var(--ink-soft); }
.find { width: 100%; max-width: 340px; margin: 0 0 14px; padding: 8px 12px; font: inherit; color: var(--ink);
  background: var(--card); border: 1px solid var(--line); border-radius: 8px; }
.pager { display: flex; justify-content: space-between; gap: 12px; padding: 0 16px 28px; }
.pager a { text-decoration: none; }
.pager span { display: block; font-size: .8rem; color: var(--ink-soft); }
.pager a:last-child { text-align: right; }
.license { white-space: pre-wrap; font: .85rem/1.5 ui-monospace, "DejaVu Sans Mono", monospace; }
footer { border-top: 1px solid var(--line); color: var(--ink-soft); font-size: .88rem; padding: 18px 0 28px; }
"""

SCRIPT = """
const pages = [...document.querySelectorAll(".page")];
function show() {
  const id = decodeURIComponent(location.hash.slice(1)) || document.body.dataset.start;
  let page = document.getElementById(id.split("--")[0]);
  if (!page || !page.classList.contains("page")) page = document.getElementById("index");
  for (const p of pages) p.hidden = p !== page;
  document.title = page.dataset.title + " — Rusty Solitaire Help";
  const target = id.includes("--") && document.getElementById(id);
  if (target) target.scrollIntoView(); else window.scrollTo(0, 0);
}
addEventListener("hashchange", show);
show();
const find = document.getElementById("find");
find.addEventListener("input", () => {
  const q = find.value.trim().toLowerCase();
  for (const li of document.querySelectorAll(".games li")) li.hidden = !li.dataset.name.includes(q);
});
"""


def page(pid, title, hero, body, side=None, pager=None):
    side_html = f'<aside>{side}</aside>' if side else ""
    return f"""<div class="page" id="{pid}" data-title="{esc(title)}">
<div class="hero"><div class="wrap">{hero}</div></div>
<div class="wrap layout"><article class="rules">
{body}
</article>{side_html}</div>
{pager or ""}
</div>"""


def main():
    games = sorted(p.stem for p in GAMES.glob("*.scm") if p.stem != "api")
    docs = {g: ET.parse(help_file(g)).getroot() for g in games}
    ids = {root.get("id"): g for g, root in docs.items()}
    render = Renderer(ids)

    info = []
    for g in games:
        root = docs[g]
        info.append({
            "slug": g,
            "title": squash(root.findtext("title")).strip(),
            "author": author(root),
            "deck": setup_value(root, "Type of Deck"),
            "goal": first_para(root, "Goal"),
            "root": root,
        })
    info.sort(key=lambda i: i["title"].lower())

    pages = []
    cards = "\n".join(
        f'<li data-name="{esc(i["title"].lower())}"><a href="#{i["slug"]}"><strong>{esc(i["title"])}</strong>'
        f'<span>{esc(i["goal"])}</span></a></li>'
        for i in info)
    pages.append(page(
        "index", "Contents",
        "<h1>Rusty Solitaire Help</h1><p>How to play, and the rules of all "
        f"{len(info)} games.</p>",
        CONTENTS + f"""<section id="index--games">
<h2>The games</h2>
<input class="find" id="find" type="search" placeholder="Find a game…" aria-label="Find a game">
<ul class="games">
{cards}
</ul>
</section>""",
        side='<nav class="toc"><a href="#index--playing">Playing</a><a href="#index--statistics">Statistics</a>'
             '<a href="#index--sound">Sound</a><a href="#index--games">The games</a>'
             '<a href="#keyboard">Keyboard</a><a href="#glossary">Glossary</a></nav>'))

    pages.append(page("keyboard", "Keyboard", "<h1>Keyboard</h1><p>Play without a mouse.</p>", KEYBOARD))

    for n, game in enumerate(info):
        prev_game, next_game = info[n - 1], info[(n + 1) % len(info)]
        render.page = game["slug"]
        sections = render.children(game["root"])
        toc = "".join(
            f'<a href="#{game["slug"]}--{anchor(t)}">{esc(t)}</a>'
            for t in (squash(s.findtext("title")).strip() for s in game["root"].findall("sect2")))
        byline = f"Rules written by {esc(game['author'])} for GNOME AisleRiot." if game["author"] else ""
        deck = f'<span class="tag">{esc(game["deck"])}</span>' if game["deck"] else ""
        pages.append(page(
            game["slug"], game["title"],
            f"<h1>{esc(game['title'])}</h1><p>{deck} {byline}</p>",
            sections,
            side=f'<nav class="toc">{toc}</nav><p class="muted">Words like <em>Tableau</em> and '
                 f'<em>Foundation</em> are explained in the <a href="#glossary">glossary</a>.</p>',
            pager=f"""<nav class="wrap pager">
<a href="#{prev_game['slug']}"><span>← Previous</span>{esc(prev_game['title'])}</a>
<a href="#{next_game['slug']}"><span>Next →</span>{esc(next_game['title'])}</a>
</nav>"""))

    render.page = "glossary"
    glossary = ET.parse(HELP / "definitions.xml").getroot()
    pages.append(page("glossary", "Glossary", "<h1>Glossary</h1><p>The words used in the rules.</p>",
                      render.children(glossary)))

    pages.append(page(
        "license", "Documentation license", "<h1>Documentation license</h1>",
        '<p>The game rules come from the GNOME AisleRiot manual, under the GNU Free Documentation License, '
        'version 1.1 or later:</p>\n'
        f'<div class="license">{esc(GFDL.read_text())}</div>'))

    doc = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Rusty Solitaire Help</title>
<style>{STYLE}</style>
</head>
<body data-start="@START@">
<header class="topbar"><div class="wrap">
<strong>Rusty Solitaire Help</strong>
<a href="#index">Contents</a><a href="#index--games">Games</a><a href="#keyboard">Keyboard</a><a href="#glossary">Glossary</a>
</div></header>
<main>
{chr(10).join(pages)}
</main>
<footer><div class="wrap">Rusty Solitaire © 2026 The Rusty Solitaire authors · GPL-3.0-or-later.
Game rules from the GNOME AisleRiot manual, under the <a href="#license">GNU Free Documentation License</a>.</div></footer>
<script>{SCRIPT}</script>
</body>
</html>
"""
    assert doc.count("@START@") == 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(doc)
    print(f"wrote {len(info)} games to {OUT.relative_to(APP)} ({len(doc) // 1024} KB)")


if __name__ == "__main__":
    main()
