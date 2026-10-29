// Rusty Solitaire - board.rs
//
// The playing area: card layout, hit testing, click / drag & drop handling
// and painting. A port of board-noclutter.c. All coordinates are physical
// pixels.
//
// Copyright (C) 1998, 2003 Jonathan Blandford <jrb@mit.edu>
// Copyright (C) 2007, 2008 Christian Persch
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use resvg::tiny_skia::{Color, FilterQuality, Paint, Pattern, Pixmap, PixmapPaint, Rect, SpreadMode, Transform};

use crate::card::Card;
use crate::game::{Expansion, Game, Slot};
use crate::theme::CardImages;

// Style defaults from ar-style-private.h.
const CARD_SLOT_RATIO: f64 = 0.95;
const CARD_OVERHANG: f64 = 0.0;
const CARD_STEP: f64 = 0.2;
const MIN_DELTA: f64 = 0.05;
/// rgb(0%, 31%, 6%)
const BACKGROUND: (u8, u8, u8) = (0, 79, 15);
/// GTK's defaults for gtk-double-click-time and gtk-dnd-drag-threshold.
/// The threshold is in logical pixels; see `Board::scale`.
const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(400);
const DRAG_THRESHOLD: f32 = 8.0;
/// Fingers are slower and less precise than a mouse: Windows' own double-tap
/// time, and a wider threshold before a touch becomes a drag.
const DOUBLE_TAP_TIME: Duration = Duration::from_millis(500);
const TOUCH_DRAG_THRESHOLD: f32 = 12.0;

static BAIZE_PNG: &[u8] = include_bytes!("../assets/art/baize.png");

#[derive(Clone, Copy, Debug, Default)]
struct SlotGeom {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pixeldx: i32,
    pixeldy: i32,
    exposed: usize,
}

impl SlotGeom {
    fn contains(&self, x: i32, y: i32) -> bool {
        self.x <= x && x <= self.x + self.w && self.y <= y && y <= self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClickStatus {
    None,
    Show,
    IsDrag,
    MaybeDrag,
    NotDrag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Open,
    Closed,
}

/// Cards being dragged, already lifted off their slot.
struct Drag {
    origin: usize,
    cards: Vec<Card>,
    expansion: Expansion,
    dx: Option<f64>,
    dy: Option<f64>,
    x: i32,
    y: i32,
}

struct LastClick {
    x: i32,
    y: i32,
    slot: Option<usize>,
    card: Option<usize>,
    time: Instant,
    left: bool,
    touch: bool,
}

/// What the window should do after an input event.
#[derive(Default, Debug)]
pub struct Outcome {
    pub sound: Option<&'static str>,
    /// The game wants a delayed call (autoplay).
    pub autoplay: bool,
    pub bell: bool,
}

pub struct Board {
    pub width: i32,
    pub height: i32,
    pub card_w: i32,
    pub card_h: i32,
    xslotstep: f64,
    yslotstep: f64,
    xoffset: i32,
    yoffset: i32,
    xbaseoffset: i32,

    status: ClickStatus,
    selection: Option<(usize, usize)>,
    show_selection: bool,
    highlight: Option<usize>,
    revealed: Option<(usize, usize)>,
    drag: Option<Drag>,
    grab_x: i32,
    grab_y: i32,
    last_click: LastClick,

    pub click_to_move: bool,
    pub cursor: Cursor,
    /// Physical pixels per logical pixel (200% display scaling = 2.0).
    pub scale: f32,
    background: Option<Pixmap>,
}

impl Board {
    pub fn new() -> Self {
        Board {
            width: 0,
            height: 0,
            card_w: 0,
            card_h: 0,
            xslotstep: 0.0,
            yslotstep: 0.0,
            xoffset: 0,
            yoffset: 0,
            xbaseoffset: 0,
            status: ClickStatus::None,
            selection: None,
            show_selection: false,
            highlight: None,
            revealed: None,
            drag: None,
            grab_x: 0,
            grab_y: 0,
            last_click: LastClick {
                x: 0,
                y: 0,
                slot: None,
                card: None,
                time: Instant::now(),
                left: false,
                touch: false,
            },
            click_to_move: false,
            cursor: Cursor::Default,
            scale: 1.0,
            background: None,
        }
    }

    /// Work out card size and slot spacing for a board of `width` x `height`
    /// pixels (aisleriot_board_setup_geometry). `card_size` maps the slot
    /// size to the theme's card size. Returns the card size.
    pub fn setup_geometry(
        &mut self,
        game: &Game,
        width: i32,
        height: i32,
        card_size: impl Fn(i32, i32, f64) -> (i32, i32),
    ) -> (i32, i32) {
        if width != self.width || height != self.height {
            self.background = None;
        }
        self.width = width;
        self.height = height;
        let table = &game.table;
        self.xslotstep = width as f64 / table.width;
        self.yslotstep = height as f64 / table.height;

        let (cw, ch) = card_size(self.xslotstep as i32, self.yslotstep as i32, CARD_SLOT_RATIO);
        self.card_w = cw;
        self.card_h = ch;

        // If the cards are too far apart, bunch them in the middle.
        self.xbaseoffset = 0;
        if self.xslotstep > ((cw * 3) / 2) as f64 {
            self.xslotstep = ((cw * 3) / 2) as f64;
            self.xbaseoffset = ((width as f64 - self.xslotstep * table.width) / 2.0) as i32;
        }
        if self.yslotstep > ((ch * 3) / 2) as f64 {
            self.yslotstep = ((ch * 3) / 2) as f64;
        }
        self.xoffset = ((self.xslotstep - cw as f64) / 2.0) as i32;
        self.yoffset = ((self.yslotstep - ch as f64) / 2.0) as i32;
        (cw, ch)
    }

    /// slot_update_geometry: where the slot is and how far apart its cards are.
    fn geometry_at(
        &self,
        x: i32,
        y: i32,
        expansion: Expansion,
        (max_dx, max_dy): (Option<f64>, Option<f64>),
        n_cards: usize,
    ) -> SlotGeom {
        let (cw, ch) = (self.card_w as f64, self.card_h as f64);
        let mut g = SlotGeom { x, y, ..Default::default() };
        let mut pixeldx = 0;
        if n_cards > 1 {
            let n = (n_cards - 1) as f64;
            let mut dy = 0.0;
            if expansion.is_down() {
                let y_from_bottom = (self.height - y) as f64 / ch;
                dy = ((y_from_bottom - (1.0 - CARD_OVERHANG)) / n).clamp(MIN_DELTA, max_dy.unwrap_or(CARD_STEP));
            } else if expansion.is_right() {
                let x_from_right = (self.width - x) as f64 / cw;
                let dx = ((x_from_right - (1.0 - CARD_OVERHANG)) / n).clamp(MIN_DELTA, max_dx.unwrap_or(CARD_STEP));
                pixeldx = (dx * cw).ceil() as i32;
                g.pixeldx = pixeldx;
            }
            g.pixeldy = (dy * ch).ceil() as i32;
        }

        g.exposed = n_cards;
        let depth = expansion.depth();
        if depth > 0 && depth < g.exposed {
            g.exposed = depth;
        }
        if (g.pixeldx == 0 && g.pixeldy == 0 && g.exposed > 1) || (n_cards > 0 && g.exposed < 1) {
            g.exposed = 1;
        }
        let delta = g.exposed.saturating_sub(1) as i32;
        g.w = self.card_w + delta * pixeldx;
        g.h = self.card_h + delta * g.pixeldy;
        g
    }

    fn slot_geometry(&self, slot: &Slot) -> SlotGeom {
        let x = (self.xslotstep * slot.x) as i32 + self.xoffset + self.xbaseoffset;
        let y = (self.yslotstep * slot.y) as i32 + self.yoffset;
        self.geometry_at(x, y, slot.expansion, (slot.dx, slot.dy), slot.cards.len())
    }

    fn drag_geometry(&self, drag: &Drag) -> SlotGeom {
        // The dragged pile shows all its cards, laid out as if it sat at the
        // top-left slot position (drag_begin's temporary slot).
        let expansion = match drag.expansion {
            Expansion::PartialDown(_) => Expansion::Down,
            Expansion::PartialRight(_) => Expansion::Right,
            e => e,
        };
        let overrides = (drag.dx, drag.dy);
        let mut g = self.geometry_at(self.xoffset + self.xbaseoffset, self.yoffset, expansion, overrides, drag.cards.len());
        g.x = drag.x;
        g.y = drag.y;
        g
    }

    fn ready(&self) -> bool {
        self.card_w > 0 && self.card_h > 0
    }

    /// get_slot_and_card_from_point: the topmost slot under the point, and
    /// the index of the card under it (from the bottom of the slot).
    fn hit(&self, game: &Game, x: i32, y: i32) -> (Option<usize>, Option<usize>) {
        let mut found: Option<usize> = None;
        let mut card_id: i32 = -1;
        for (i, slot) in game.table.slots.iter().enumerate().rev() {
            let g = self.slot_geometry(slot);
            if !g.contains(x, y) {
                continue;
            }
            let num_cards = slot.cards.len();
            if found.is_none() || num_cards > 0 {
                let mut depth = 1;
                if g.pixeldx > 0 {
                    depth += ((x - g.x) / g.pixeldx) as usize;
                } else if g.pixeldy > 0 {
                    depth += ((y - g.y) / g.pixeldy) as usize;
                }
                depth = depth.min(g.exposed);
                found = Some(i);
                card_id = num_cards as i32 + depth as i32 - g.exposed as i32;
                if num_cards > 0 {
                    break;
                }
            }
        }
        let card = if card_id > 0 { Some((card_id - 1) as usize) } else { None };
        (found, card)
    }

    fn droppable(&self, game: &mut Game, slot: Option<usize>) -> bool {
        match (slot, &self.drag) {
            (Some(slot), Some(drag)) => game.drop_valid(drag.origin, slot, &drag.cards),
            _ => false,
        }
    }

    /// find_drop_target: where the dragged cards (top-left at x, y) would land.
    fn find_drop_target(&self, game: &mut Game, x: i32, y: i32) -> Option<usize> {
        let (cw, ch) = (self.card_w, self.card_h);
        let (slot, _) = self.hit(game, x + cw / 2, y + ch / 2);
        if self.droppable(game, slot) {
            return slot;
        }

        // Otherwise try the four corners and take the nearest droppable slot.
        let mut best = None;
        let mut min_distance = i64::MAX;
        for i in 0..4 {
            let (slot, card) = self.hit(game, x + cw * (i / 2), y + ch * (i % 2));
            let Some(slot_id) = slot else { continue };
            // Games without droppable? still land on the nearest corner.
            if game.droppable_supported() && !self.droppable(game, slot) {
                continue;
            }
            let g = self.slot_geometry(&game.table.slots[slot_id]);
            // As in the original, card ids here are 1-based minus one.
            let card_id = card.map(|c| c as i32 + 1).unwrap_or(-1);
            let dx = (g.x + (card_id - 1) * g.pixeldx - x) as i64;
            let dy = (g.y + (card_id - 1) * g.pixeldy - y) as i64;
            let distance = dx * dx + dy * dy;
            if distance <= min_distance {
                best = slot;
                min_distance = distance;
            }
        }
        best
    }

    // ---- Input ----

    /// A mouse button or finger went down. `button` is 1 (left), 2 (middle)
    /// or 3 (right); a finger counts as the left button.
    pub fn press(&mut self, game: &mut Game, x: i32, y: i32, button: u8, touch: bool) -> Outcome {
        let mut out = Outcome::default();
        if !self.ready() {
            return out;
        }
        if self.status != ClickStatus::None {
            // With a mouse, a second button during a click is ignored, as in
            // the original. A finger can't overlap like that, so a press
            // still in progress means its release got lost: start afresh.
            if !touch {
                return out;
            }
            self.abort(game);
        }

        game.start();
        let (slot, card) = self.hit(game, x, y);
        let now = Instant::now();
        let double_time = if touch { DOUBLE_TAP_TIME } else { DOUBLE_CLICK_TIME };
        let is_double_click = button == 2
            || (self.last_click.left
                && now.duration_since(self.last_click.time) <= double_time
                && self.last_click.slot == slot
                && self.last_click.card == card);
        self.last_click = LastClick { x, y, slot, card, time: now, left: button == 1, touch };

        let Some(slot) = slot else {
            self.selection = None;
            self.status = ClickStatus::None;
            return out;
        };
        self.cursor = Cursor::Closed;

        if button == 3 {
            self.reveal(game, Some(slot), card);
            return out;
        }
        self.reveal(game, None, None);

        if is_double_click {
            self.status = ClickStatus::None;
            // Otherwise three clicks would count as two double clicks.
            self.last_click.left = false;
            game.double_click(slot);
            out.autoplay = game.has_delayed_call();
            self.cursor = Cursor::Open;
            return out;
        }

        if let Some((sel_slot, sel_card)) = self.selection {
            if self.click_to_move && (slot != sel_slot || card.map(|c| c + 1) == Some(sel_card)) {
                if self.move_selected_to(game, slot, &mut out) {
                    return out;
                }
                if slot != sel_slot {
                    out.bell = true;
                }
            }
            if slot == sel_slot && card == Some(sel_card) && self.click_to_move {
                // Clicking the selected card again deselects it.
                self.selection = None;
                self.show_selection = false;
                self.reveal(game, Some(slot), card);
                return out;
            }
        }

        let drag_valid = card.is_some_and(|c| game.drag_valid(slot, c));
        if drag_valid {
            self.selection = Some((slot, card.unwrap()));
            self.show_selection = self.click_to_move;
            self.status = if self.click_to_move { ClickStatus::NotDrag } else { ClickStatus::MaybeDrag };
        } else {
            self.selection = None;
            self.show_selection = false;
            self.status = ClickStatus::NotDrag;
        }
        if self.click_to_move {
            self.reveal(game, Some(slot), card);
        }
        out
    }

    pub fn release(&mut self, game: &mut Game, x: i32, y: i32) -> Outcome {
        let mut out = Outcome::default();
        match self.status {
            ClickStatus::Show => self.reveal(game, None, None),
            ClickStatus::IsDrag => {
                self.highlight = None;
                self.drop_moving_cards(game, x, y, &mut out);
            }
            ClickStatus::MaybeDrag | ClickStatus::NotDrag => {
                // Don't act if the pointer moved off the clicked slot.
                let (slot, _) = self.hit(game, x, y);
                if let Some(slot) = slot.filter(|s| Some(*s) == self.last_click.slot) {
                    if game.click(slot) {
                        out.sound = Some("click");
                    }
                    out.autoplay = game.has_delayed_call();
                }
            }
            ClickStatus::None => {}
        }
        self.status = ClickStatus::None;
        self.update_cursor(game, x, y);
        out
    }

    pub fn motion(&mut self, game: &mut Game, x: i32, y: i32) {
        match self.status {
            ClickStatus::IsDrag => {
                let (nx, ny) = (x - self.grab_x, y - self.grab_y);
                let target = self.find_drop_target(game, nx, ny);
                self.highlight = target.filter(|_| self.droppable(game, target));
                if let Some(drag) = &mut self.drag {
                    drag.x = nx;
                    drag.y = ny;
                }
                self.cursor = Cursor::Closed;
            }
            ClickStatus::MaybeDrag if self.beyond_drag_threshold(x, y) => {
                self.drag_begin(game);
            }
            _ => self.update_cursor(game, x, y),
        }
    }

    fn beyond_drag_threshold(&self, x: i32, y: i32) -> bool {
        let logical = if self.last_click.touch { TOUCH_DRAG_THRESHOLD } else { DRAG_THRESHOLD };
        let threshold = (logical * self.scale).round() as i32;
        (x - self.last_click.x).abs() > threshold || (y - self.last_click.y).abs() > threshold
    }

    /// The press was cancelled by the system (e.g. a touch interrupted by a
    /// palm or a second finger): put any dragged cards back, change nothing.
    pub fn cancel(&mut self, game: &mut Game) {
        self.abort(game);
        self.cursor = Cursor::Default;
    }

    /// A finger has rested on the table without moving: show the covered
    /// card under it, as a right-click does with a mouse.
    pub fn long_press(&mut self, game: &mut Game) -> bool {
        if !matches!(self.status, ClickStatus::MaybeDrag | ClickStatus::NotDrag) || !self.last_click.touch {
            return false;
        }
        let (slot, card) = (self.last_click.slot, self.last_click.card);
        self.selection = None;
        self.show_selection = false;
        self.status = ClickStatus::None;
        self.reveal(game, slot, card);
        if self.status != ClickStatus::Show {
            // Nothing to reveal (top card, face down): just ignore the press.
            self.status = ClickStatus::Show;
        }
        true
    }

    fn update_cursor(&mut self, game: &Game, x: i32, y: i32) {
        let (slot, card) = self.hit(game, x, y);
        let face_up = match (slot, card) {
            (Some(s), Some(c)) => game.table.cards(s)[c].face_up,
            _ => false,
        };
        self.cursor = if !face_up {
            Cursor::Default
        } else if self.status == ClickStatus::None {
            Cursor::Open
        } else {
            Cursor::Closed
        };
    }

    fn reveal(&mut self, game: &Game, slot: Option<usize>, card: Option<usize>) {
        if self.revealed.map(|(s, _)| s) == slot && slot.is_some() {
            return;
        }
        if self.revealed.take().is_some() {
            self.status = ClickStatus::None;
        }
        let (Some(slot), Some(card)) = (slot, card) else { return };
        let cards = game.table.cards(slot);
        if card + 1 >= cards.len() || !cards[card].face_up {
            return;
        }
        self.revealed = Some((slot, card));
        self.status = ClickStatus::Show;
    }

    fn drag_begin(&mut self, game: &mut Game) {
        let Some((slot, card)) = self.selection.take() else {
            self.status = ClickStatus::None;
            return;
        };
        self.status = ClickStatus::IsDrag;
        game.record_move_from(game.table.clone());

        let g = self.slot_geometry(&game.table.slots[slot]);
        let num_moving = game.table.cards(slot).len() - card;
        let delta = g.exposed as i32 - num_moving as i32;
        let x = g.x + delta * g.pixeldx;
        let y = g.y + delta * g.pixeldy;
        self.grab_x = self.last_click.x - x;
        self.grab_y = self.last_click.y - y;

        let origin = &mut game.table.slots[slot];
        let cards = origin.cards.split_off(card);
        let (expansion, dx, dy) = (origin.expansion, origin.dx, origin.dy);
        self.drag = Some(Drag { origin: slot, cards, expansion, dx, dy, x, y });
        self.cursor = Cursor::Closed;
    }

    fn drop_moving_cards(&mut self, game: &mut Game, x: i32, y: i32, out: &mut Outcome) {
        let target = self.find_drop_target(game, x - self.grab_x, y - self.grab_y);
        let Some(drag) = self.drag.take() else { return };
        let moved = target.is_some_and(|t| game.drop_cards(drag.origin, t, &drag.cards));
        if moved {
            game.end_move();
            out.sound = Some("click");
            game.test_end_of_game();
            out.autoplay = game.has_delayed_call();
        } else {
            game.discard_move();
            game.table.add_cards(drag.origin, &drag.cards);
            out.sound = Some("slide");
        }
    }

    /// Click-to-move: move the selected cards onto `target`.
    fn move_selected_to(&mut self, game: &mut Game, target: usize, out: &mut Outcome) -> bool {
        let Some((slot, card)) = self.selection.take() else { return false };
        self.show_selection = false;
        self.status = ClickStatus::None;
        game.record_move();
        let cards = game.table.slots[slot].cards.split_off(card);
        let moved = game.drop_cards(slot, target, &cards);
        if moved {
            game.end_move();
            out.sound = Some("click");
            game.test_end_of_game();
            out.autoplay = game.has_delayed_call();
        } else {
            game.discard_move();
            game.table.add_cards(slot, &cards);
        }
        moved
    }

    /// Drop any selection or drag, e.g. before undo or a new game.
    pub fn abort(&mut self, game: &mut Game) {
        if let Some(drag) = self.drag.take() {
            game.discard_move();
            game.table.add_cards(drag.origin, &drag.cards);
        }
        self.selection = None;
        self.show_selection = false;
        self.highlight = None;
        self.revealed = None;
        self.status = ClickStatus::None;
    }

    // ---- Painting ----

    fn background(&mut self) -> &Pixmap {
        if self.background.is_none() {
            let mut bg = Pixmap::new(self.width.max(1) as u32, self.height.max(1) as u32).unwrap();
            let (r, g, b) = BACKGROUND;
            bg.fill(Color::from_rgba8(r, g, b, 255));
            if let Ok(baize) = Pixmap::decode_png(BAIZE_PNG) {
                let paint = Paint {
                    shader: Pattern::new(
                        baize.as_ref(),
                        SpreadMode::Repeat,
                        FilterQuality::Nearest,
                        1.0,
                        Transform::identity(),
                    ),
                    ..Default::default()
                };
                let rect = Rect::from_xywh(0.0, 0.0, self.width as f32, self.height as f32).unwrap();
                bg.fill_rect(rect, &paint, Transform::identity(), None);
            }
            self.background = Some(bg);
        }
        self.background.as_ref().unwrap()
    }

    /// aisleriot_board_draw, minus the cards being dragged (see
    /// `drag_layer`). Draws into `out`, an RGBA buffer of the board's size,
    /// so the result can go to the screen without another copy.
    pub fn paint_into(&mut self, game: &Game, images: &CardImages, out: &mut [u8]) {
        let (w, h) = (self.width.max(1), self.height.max(1));
        out.copy_from_slice(self.background().data());
        if !self.ready() || images.width as i32 != self.card_w || images.height as i32 != self.card_h {
            return;
        }
        let mut draw = |image: &Pixmap, x: i32, y: i32| blit(out, w, h, image, x, y);

        let slots = &game.table.slots;
        let geoms: Vec<SlotGeom> = slots.iter().map(|s| self.slot_geometry(s)).collect();

        // Empty slots first, so they never cover cards.
        for (i, slot) in slots.iter().enumerate() {
            if slot.cards.is_empty() {
                draw(images.slot(self.highlight == Some(i)), geoms[i].x, geoms[i].y);
            }
        }

        for (i, slot) in slots.iter().enumerate() {
            let n = slot.cards.len();
            if n == 0 {
                continue;
            }
            let g = geoms[i];
            let highlight_from = if self.highlight == Some(i) {
                n - 1
            } else if self.show_selection && self.selection.map(|(s, _)| s) == Some(i) {
                self.selection.unwrap().1
            } else {
                usize::MAX
            };
            let first = n - g.exposed;
            for (k, card) in slot.cards[first..].iter().enumerate() {
                let image = images.card(card, first + k >= highlight_from);
                draw(image, g.x + k as i32 * g.pixeldx, g.y + k as i32 * g.pixeldy);
            }
        }

        if let Some((slot, card)) = self.revealed {
            let g = geoms[slot];
            let n = slots[slot].cards.len();
            let first = n - g.exposed;
            if card >= first && card < n {
                let k = (card - first) as i32;
                draw(images.card(&slots[slot].cards[card], false), g.x + k * g.pixeldx, g.y + k * g.pixeldy);
            }
        }
    }

    /// The whole board as a new pixmap (tests and benchmarks).
    #[cfg(test)]
    pub fn paint(&mut self, game: &Game, images: &CardImages) -> Pixmap {
        let mut pixmap = Pixmap::new(self.width.max(1) as u32, self.height.max(1) as u32).unwrap();
        self.paint_into(game, images, pixmap.data_mut());
        pixmap
    }

    /// The cards being dragged, drawn once into their own small image that
    /// the window moves around (so the board needn't be redrawn per move).
    pub fn drag_layer(&self, images: &CardImages) -> Option<Pixmap> {
        let drag = self.drag.as_ref()?;
        let g = self.drag_geometry(drag);
        let mut pixmap = Pixmap::new(g.w.max(1) as u32, g.h.max(1) as u32)?;
        for (k, card) in drag.cards.iter().enumerate() {
            let (x, y) = (k as i32 * g.pixeldx, k as i32 * g.pixeldy);
            pixmap.draw_pixmap(x, y, images.card(card, false).as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        Some(pixmap)
    }

    /// Top-left corner of the dragged cards, in physical pixels.
    pub fn drag_position(&self) -> Option<(i32, i32)> {
        self.drag.as_ref().map(|d| (d.x, d.y))
    }

    /// Everything that changes how the board (without the dragged cards)
    /// looks apart from the table itself. If a move leaves this unchanged,
    /// only the drag layer needs moving.
    pub fn scene_key(&self) -> impl PartialEq + use<> {
        (
            self.highlight,
            self.revealed,
            self.selection.filter(|_| self.show_selection),
            self.drag.as_ref().map(|d| (d.origin, d.cards.len())),
        )
    }
}

/// Draw `src` at (x, y) onto an RGBA buffer of `w` x `h` pixels (both
/// premultiplied), clipped to the buffer. A plain "source over" copy: card
/// images are opaque except for their rounded corners, so this is far
/// quicker than a general-purpose drawing call.
fn blit(dst: &mut [u8], w: i32, h: i32, src: &Pixmap, x: i32, y: i32) {
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + sw).min(w), (y + sh).min(h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let data = src.data();
    let row_len = ((x1 - x0) * 4) as usize;
    for dy in y0..y1 {
        let s_off = (((dy - y) * sw + (x0 - x)) * 4) as usize;
        let d_off = ((dy * w + x0) * 4) as usize;
        let s_row = &data[s_off..s_off + row_len];
        let d_row = &mut dst[d_off..d_off + row_len];
        for (d, s) in d_row.chunks_exact_mut(4).zip(s_row.chunks_exact(4)) {
            match s[3] {
                255 => d.copy_from_slice(s),
                0 => {}
                a => {
                    let inv = 255 - a as u32;
                    for i in 0..4 {
                        d[i] = (s[i] as u32 + (d[i] as u32 * inv + 127) / 255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Suit;

    const WASTE: usize = 1;
    const FIRST_FOUNDATION: usize = 2;
    const FIRST_TABLEAU: usize = 6;

    /// A Klondike board at 700x500 with an 80x120 card, and a known position:
    /// king of spades alone on the first tableau pile, queen of hearts on the waste.
    fn setup() -> (Game, Board) {
        let mut game = Game::new("klondike", 42).unwrap();
        for slot in &mut game.table.slots[1..] {
            slot.cards.clear();
        }
        game.table.slots[FIRST_TABLEAU].cards.push(Card::new(13, Suit::Spade).visible());
        game.table.slots[WASTE].cards.push(Card::new(12, Suit::Heart).visible());
        let mut board = Board::new();
        board.setup_geometry(&game, 700, 500, |_, _, _| (80, 120));
        (game, board)
    }

    fn centre(board: &Board, game: &Game, slot: usize) -> (i32, i32) {
        let g = board.slot_geometry(&game.table.slots[slot]);
        (g.x + g.w / 2, g.y + g.h / 2)
    }

    #[test]
    fn drag_and_drop_moves_card() {
        let (mut game, mut board) = setup();
        let (fx, fy) = centre(&board, &game, WASTE);
        let (tx, ty) = centre(&board, &game, FIRST_TABLEAU);
        board.press(&mut game, fx, fy, 1, false);
        board.motion(&mut game, fx + 20, fy + 20);
        board.motion(&mut game, tx, ty);
        assert_eq!(board.highlight, Some(FIRST_TABLEAU));
        let out = board.release(&mut game, tx, ty);
        assert_eq!(out.sound, Some("click"));
        assert!(game.table.cards(WASTE).is_empty());
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 2);
        assert!(game.can_undo());
        game.undo();
        assert_eq!(game.table.cards(WASTE).len(), 1);
    }

    #[test]
    fn invalid_drop_returns_cards() {
        let (mut game, mut board) = setup();
        let (fx, fy) = centre(&board, &game, WASTE);
        let (tx, ty) = centre(&board, &game, FIRST_FOUNDATION);
        board.press(&mut game, fx, fy, 1, false);
        board.motion(&mut game, fx + 20, fy + 20);
        board.motion(&mut game, tx, ty);
        let out = board.release(&mut game, tx, ty);
        assert_eq!(out.sound, Some("slide"));
        assert_eq!(game.table.cards(WASTE).len(), 1);
        assert!(!game.can_undo());
    }

    #[test]
    fn click_to_move() {
        let (mut game, mut board) = setup();
        board.click_to_move = true;
        let (fx, fy) = centre(&board, &game, WASTE);
        let (tx, ty) = centre(&board, &game, FIRST_TABLEAU);
        board.press(&mut game, fx, fy, 1, false);
        board.release(&mut game, fx, fy);
        board.press(&mut game, tx, ty, 1, false);
        board.release(&mut game, tx, ty);
        assert!(game.table.cards(WASTE).is_empty());
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 2);
    }

    /// Let the double-click / double-tap window pass, like a pause between
    /// two separate presses.
    fn pause(board: &mut Board) {
        board.last_click.time = board.last_click.time.checked_sub(Duration::from_secs(1)).unwrap();
    }

    /// A 200%-scaled screen, like a Surface Pro.
    fn touch_setup() -> (Game, Board) {
        let (game, mut board) = setup();
        board.scale = 2.0;
        (game, board)
    }

    #[test]
    fn wobbly_finger_tap_is_not_a_drag() {
        let (mut game, mut board) = touch_setup();
        let (x, y) = centre(&board, &game, FIRST_TABLEAU);
        board.press(&mut game, x, y, 1, true);
        // A finger wobbles: 20 physical pixels is past the mouse threshold at
        // this scale (8 * 2 = 16) but inside the finger one (12 * 2 = 24).
        board.motion(&mut game, x + 9, y + 9);
        board.motion(&mut game, x + 20, y - 15);
        assert!(board.drag.is_none());
        board.release(&mut game, x + 20, y - 15);
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 1);
    }

    #[test]
    fn double_tap_sends_card_to_foundation() {
        let (mut game, mut board) = touch_setup();
        game.table.slots[WASTE].cards = vec![Card::new(1, Suit::Club).visible()];
        let (x, y) = centre(&board, &game, WASTE);
        board.press(&mut game, x, y, 1, true);
        board.motion(&mut game, x + 6, y + 4);
        board.release(&mut game, x + 6, y + 4);
        board.press(&mut game, x + 3, y + 2, 1, true);
        board.release(&mut game, x + 3, y + 2);
        assert!(game.table.cards(WASTE).is_empty());
        assert_eq!(game.table.cards(FIRST_FOUNDATION).len(), 1);
    }

    #[test]
    fn cancelled_drag_puts_cards_back_and_next_tap_works() {
        let (mut game, mut board) = touch_setup();
        let (fx, fy) = centre(&board, &game, WASTE);
        board.press(&mut game, fx, fy, 1, true);
        board.motion(&mut game, fx + 60, fy + 60);
        assert!(board.drag.is_some());
        board.cancel(&mut game);
        assert_eq!(game.table.cards(WASTE).len(), 1);
        assert!(!game.can_undo());
        // The board accepts input again.
        let (tx, ty) = centre(&board, &game, FIRST_TABLEAU);
        pause(&mut board);
        board.press(&mut game, fx, fy, 1, true);
        board.motion(&mut game, fx + 60, fy + 60);
        board.motion(&mut game, tx, ty);
        board.release(&mut game, tx, ty);
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 2);
    }

    #[test]
    fn lost_release_does_not_freeze_touch_input() {
        let (mut game, mut board) = touch_setup();
        let (fx, fy) = centre(&board, &game, WASTE);
        board.press(&mut game, fx, fy, 1, true);
        board.motion(&mut game, fx + 60, fy + 60);
        // No release or cancel arrives; the next finger press must still work.
        let (tx, ty) = centre(&board, &game, FIRST_TABLEAU);
        pause(&mut board);
        board.press(&mut game, fx, fy, 1, true);
        assert_eq!(game.table.cards(WASTE).len(), 1, "dragged cards were put back");
        board.motion(&mut game, fx + 60, fy + 60);
        board.motion(&mut game, tx, ty);
        board.release(&mut game, tx, ty);
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 2);
    }

    #[test]
    fn long_press_reveals_covered_card() {
        let (mut game, mut board) = touch_setup();
        let pile = &mut game.table.slots[FIRST_TABLEAU].cards;
        pile.push(Card::new(12, Suit::Heart).visible());
        pile.push(Card::new(11, Suit::Club).visible());
        let g = board.slot_geometry(&game.table.slots[FIRST_TABLEAU]);
        // On the king, which is covered by the queen and jack.
        let (x, y) = (g.x + g.w / 2, g.y + 2);
        board.press(&mut game, x, y, 1, true);
        assert!(board.long_press(&mut game));
        assert_eq!(board.revealed, Some((FIRST_TABLEAU, 0)));
        board.release(&mut game, x, y);
        assert_eq!(board.revealed, None);
        assert_eq!(game.table.cards(FIRST_TABLEAU).len(), 3, "a long press moves nothing");
    }

    #[test]
    fn blit_matches_tiny_skia() {
        use crate::theme::{CardTheme, DEFAULT_THEME};
        let images = CardTheme::load(DEFAULT_THEME).unwrap().render(90, 130);
        let card = images.card(&Card::new(12, Suit::Heart).visible(), true);
        let mut reference = Pixmap::new(200, 200).unwrap();
        reference.fill(Color::from_rgba8(0, 79, 15, 255));
        let mut ours = reference.clone();
        // Partly off the edge, to exercise clipping.
        for (x, y) in [(10, 20), (150, 120), (-30, -40)] {
            reference.draw_pixmap(x, y, card.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            blit(ours.data_mut(), 200, 200, card, x, y);
        }
        let worst = reference.data().iter().zip(ours.data()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "pixels differ by up to {worst}");
    }

    #[test]
    fn layout_matches_original_geometry() {
        let (game, board) = setup();
        // 700 / 7 = 100 px per slot column, card 80 wide -> 10 px offset.
        let stock = board.slot_geometry(&game.table.slots[0]);
        assert_eq!((stock.x, stock.w, stock.h), (10, 80, 120));
        let tableau = board.slot_geometry(&game.table.slots[FIRST_TABLEAU]);
        assert_eq!(tableau.x, 10);
        assert!(tableau.y > stock.y + stock.h);
    }
}

/// Timing of a full table redraw at Surface Pro 11 resolution.
/// Run with: cargo test --release bench_paint -- --ignored --nocapture
#[cfg(test)]
mod bench {
    use super::*;
    use crate::theme::{CardTheme, DEFAULT_THEME};

    #[test]
    #[ignore]
    fn bench_paint() {
        let game = Game::new("klondike", 1).unwrap();
        let theme = CardTheme::load(DEFAULT_THEME).unwrap();
        let mut board = Board::new();
        let (w, h) = (2880, 1700);
        let (cw, ch) = board.setup_geometry(&game, w, h, |a, b, p| theme.card_size(a, b, p));
        let images = theme.render(cw as u32, ch as u32);
        let n = 20;
        let t = std::time::Instant::now();
        for _ in 0..n {
            let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w as u32, h as u32);
            board.paint_into(&game, &images, buffer.make_mut_bytes());
            std::hint::black_box(buffer);
        }
        eprintln!("full redraw at {w}x{h}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}

#[cfg(test)]
mod bench_parts {
    use super::*;

    #[test]
    #[ignore]
    fn bench_paint_parts() {
        let (w, h) = (2880u32, 1700u32);
        let n = 20;
        let t = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h));
        }
        eprintln!("allocate: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
        let src = vec![7u8; (w * h * 4) as usize];
        let mut dst = vec![0u8; (w * h * 4) as usize];
        let t = std::time::Instant::now();
        for _ in 0..n {
            dst.copy_from_slice(&src);
            std::hint::black_box(&dst);
        }
        eprintln!("copy background: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}
