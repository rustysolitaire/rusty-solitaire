// Rusty Solitaire - theme.rs
//
// SVG card themes. Each theme is one SVG laid out as a 13 x 5 grid: four rows
// of suits (clubs, diamonds, hearts, spades; ace to king) and a fifth row with
// the black joker, red joker and the card back. Port of ar-card-theme-svg.c.
//
// Copyright (C) 2004 Richard Hoelscher, 2007, 2008 Christian Persch
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, PremultipliedColorU8, Transform};
use resvg::usvg;

use crate::card::Card;

include!(concat!(env!("OUT_DIR"), "/card_themes.rs"));

pub const DEFAULT_THEME: &str = "anglo.svgz";

const N_COLS: f64 = 13.0;
const N_ROWS: f64 = 5.0;

/// Selection/highlight colour: rgba(0%, 0%, 66%, 0.5)
const SELECTION: (f32, f32, f32, f32) = (0.0, 0.0, 168.0, 0.5);

static SLOT_SVG: &[u8] = include_bytes!("../assets/art/slot.svg");

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/fonts/Cantarell-Regular.otf").to_vec());
        db.load_font_data(include_bytes!("../assets/fonts/Cantarell-Bold.otf").to_vec());
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

fn parse(data: &[u8]) -> Option<usvg::Tree> {
    let options = usvg::Options { fontdb: fontdb(), ..Default::default() };
    usvg::Tree::from_data(data, &options).ok()
}

/// "anglo_poker.svgz" -> "Anglo Poker", as ar_filename_to_display_name does.
pub fn display_name(file_name: &str) -> String {
    let base = file_name.split('.').next().unwrap_or(file_name);
    let spaced: String = base
        .chars()
        .map(|c| if "_-|> <".contains(c) { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn theme_files() -> impl Iterator<Item = &'static str> {
    CARD_THEMES.iter().map(|(name, _)| *name)
}

/// Images of every card in one theme at one size.
pub struct CardImages {
    pub width: u32,
    pub height: u32,
    cards: Vec<Pixmap>,
    highlighted: Vec<Pixmap>,
    slot: Pixmap,
    slot_highlighted: Pixmap,
}

/// Index of a card image in the 13 x 5 grid.
fn image_index(card: &Card) -> usize {
    const BACK: usize = 4 * 13 + 2;
    if !card.face_up {
        BACK
    } else if card.rank == 0 {
        4 * 13 + if card.is_red() { 1 } else { 0 }
    } else {
        // Ace-high games use rank 14 for the ace.
        card.suit as usize * 13 + (card.rank as usize - 1) % 13
    }
}

impl CardImages {
    pub fn card(&self, card: &Card, highlighted: bool) -> &Pixmap {
        let i = image_index(card);
        if highlighted { &self.highlighted[i] } else { &self.cards[i] }
    }

    pub fn slot(&self, highlighted: bool) -> &Pixmap {
        if highlighted { &self.slot_highlighted } else { &self.slot }
    }
}

pub struct CardTheme {
    pub file_name: String,
    tree: usvg::Tree,
    slot_tree: usvg::Tree,
}

impl CardTheme {
    pub fn load(file_name: &str) -> Option<CardTheme> {
        let (_, data) = CARD_THEMES.iter().find(|(name, _)| *name == file_name)?;
        Some(CardTheme {
            file_name: file_name.to_string(),
            tree: parse(data)?,
            slot_tree: parse(SLOT_SVG)?,
        })
    }

    /// Width / height of a single card.
    pub fn aspect(&self) -> f64 {
        let size = self.tree.size();
        (size.width() as f64 * N_ROWS) / (size.height() as f64 * N_COLS)
    }

    /// The largest card that fits in `proportion` of a slot, keeping the
    /// theme's aspect ratio (ar_card_theme_preimage_set_card_size).
    pub fn card_size(&self, slot_width: i32, slot_height: i32, proportion: f64) -> (i32, i32) {
        let aspect = self.aspect();
        let mut w = proportion * slot_width as f64;
        let mut h = proportion * slot_height as f64;
        if w / h < aspect {
            h = w / aspect;
        } else {
            w = h * aspect;
        }
        ((w as i32).max(1), (h as i32).max(1))
    }

    /// Render every card at the given size.
    pub fn render(&self, width: u32, height: u32) -> CardImages {
        let size = self.tree.size();
        let sheet_w = width * 13;
        let sheet_h = height * 5;
        let mut sheet = Pixmap::new(sheet_w, sheet_h).expect("card sheet size");
        let transform = Transform::from_scale(
            sheet_w as f32 / size.width(),
            sheet_h as f32 / size.height(),
        );
        resvg::render(&self.tree, transform, &mut sheet.as_mut());

        let mut cards = Vec::with_capacity(65);
        for row in 0..5 {
            for col in 0..13 {
                cards.push(crop(&sheet, col * width, row * height, width, height));
            }
        }
        let highlighted = cards.iter().map(tint).collect();

        let slot_size = self.slot_tree.size();
        let mut slot = Pixmap::new(width, height).expect("slot size");
        resvg::render(
            &self.slot_tree,
            Transform::from_scale(width as f32 / slot_size.width(), height as f32 / slot_size.height()),
            &mut slot.as_mut(),
        );
        let slot_highlighted = tint(&slot);

        CardImages { width, height, cards, highlighted, slot, slot_highlighted }
    }
}

fn crop(sheet: &Pixmap, x: u32, y: u32, w: u32, h: u32) -> Pixmap {
    let mut out = Pixmap::new(w, h).unwrap();
    let src = sheet.pixels();
    let stride = sheet.width() as usize;
    let dst = out.pixels_mut();
    for row in 0..h as usize {
        let start = (y as usize + row) * stride + x as usize;
        dst[row * w as usize..(row + 1) * w as usize].copy_from_slice(&src[start..start + w as usize]);
    }
    out
}

/// Paint the selection colour over the image, masked by the image's own
/// alpha, like cairo_mask() does in the original board.
fn tint(image: &Pixmap) -> Pixmap {
    let mut out = image.clone();
    let (sr, sg, sb, sa) = SELECTION;
    for px in out.pixels_mut() {
        let coverage = px.alpha() as f32 / 255.0;
        let a = sa * coverage;
        let blend = |dst: u8, src: f32| (src * a + dst as f32 * (1.0 - a)).round() as u8;
        let alpha = (255.0 * a + px.alpha() as f32 * (1.0 - a)).round() as u8;
        let (r, g, b) = (blend(px.red(), sr), blend(px.green(), sg), blend(px.blue(), sb));
        *px = PremultipliedColorU8::from_rgba(r.min(alpha), g.min(alpha), b.min(alpha), alpha).unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(display_name("anglo_poker.svgz"), "Anglo Poker");
        assert_eq!(display_name("guyenne-classic.svgz"), "Guyenne Classic");
        assert_eq!(display_name("XSkat.svgz"), "XSkat");
    }

    #[test]
    fn every_theme_parses() {
        for name in theme_files() {
            let theme = CardTheme::load(name).unwrap_or_else(|| panic!("{name} failed to load"));
            assert!(theme.aspect() > 0.4 && theme.aspect() < 1.0, "{name} aspect");
        }
    }
}
