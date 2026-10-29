// Rusty Solitaire - card.rs
// Copyright (C) 1998, 2003 Jonathan Blandford <jrb@mit.edu> (original Scheme API)
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

// Cards as the table and the card renderer see them. The game rules work
// with Scheme card lists; rules.rs converts between the two.
#![allow(dead_code)]

pub const JOKER: u8 = 0;
pub const ACE: u8 = 1;
pub const JACK: u8 = 11;
pub const QUEEN: u8 = 12;
pub const KING: u8 = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suit {
    Club = 0,
    Diamond = 1,
    Heart = 2,
    Spade = 3,
}

pub const SUITS: [Suit; 4] = [Suit::Club, Suit::Diamond, Suit::Heart, Suit::Spade];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
}

impl Suit {
    pub fn color(self) -> Color {
        match self {
            Suit::Club | Suit::Spade => Color::Black,
            Suit::Diamond | Suit::Heart => Color::Red,
        }
    }

}

/// A playing card. `rank` is 1 (ace) to 13 (king), or 0 for a joker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Card {
    pub rank: u8,
    pub suit: Suit,
    pub face_up: bool,
}

impl Card {
    pub fn new(rank: u8, suit: Suit) -> Self {
        Card { rank, suit, face_up: false }
    }

    pub fn color(&self) -> Color {
        self.suit.color()
    }

    pub fn is_red(&self) -> bool {
        self.color() == Color::Red
    }

    pub fn flipped(self) -> Self {
        Card { face_up: !self.face_up, ..self }
    }

    pub fn visible(self) -> Self {
        Card { face_up: true, ..self }
    }

    /// Same rank and suit, ignoring which way up the card is.
    pub fn same_card(&self, other: &Card) -> bool {
        self.rank == other.rank && self.suit == other.suit
    }

}


/// Small deterministic PRNG (SplitMix64), so a seed always gives the same
/// deal on every platform. Used for "Restart" and for reproducible tests.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}
