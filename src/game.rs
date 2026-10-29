// Rusty Solitaire - game.rs
//
// The game engine: slots, the table, undo/redo, the game state and clock.
// Rules come from the original Scheme files through `rules::SchemeRules`.
// This replaces Aisleriot's game.c.
//
// Copyright (C) 1998, 2003 Jonathan Blandford <jrb@mit.edu>
// Copyright (C) 2007 Christian Persch
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use crate::card::Card;
use crate::rules::{GameOption, SchemeRules};
use crate::scheme::{Result, Value};

/// How the cards in a slot are fanned out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Expansion {
    /// Only the top card is visible.
    None,
    /// Cards are fanned downwards (tableau).
    Down,
    /// Cards are fanned to the right.
    Right,
    /// Only the top `n` cards are fanned downwards.
    PartialDown(usize),
    /// Only the top `n` cards are fanned to the right (three-card waste).
    PartialRight(usize),
}

impl Expansion {
    pub fn is_down(self) -> bool {
        matches!(self, Expansion::Down | Expansion::PartialDown(_))
    }

    pub fn is_right(self) -> bool {
        matches!(self, Expansion::Right | Expansion::PartialRight(_))
    }

    /// Maximum number of fanned cards, 0 meaning unlimited.
    pub fn depth(self) -> usize {
        match self {
            Expansion::PartialDown(n) | Expansion::PartialRight(n) => n,
            _ => 0,
        }
    }
}

/// What a slot is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotType {
    Unknown,
    Chooser,
    Foundation,
    Reserve,
    Stock,
    Tableau,
    Waste,
}

#[derive(Clone, Debug)]
pub struct Slot {
    /// Cards from bottom (index 0) to top (last).
    pub cards: Vec<Card>,
    /// Position in card-sized units.
    pub x: f64,
    pub y: f64,
    pub expansion: Expansion,
    /// Not used yet; the original shows it in slot tooltips.
    #[allow(dead_code)]
    pub slot_type: SlotType,
    /// Fan spacing set by the game (set-slot-x/y-expansion!), in card units.
    pub dx: Option<f64>,
    pub dy: Option<f64>,
}

/// Everything about the cards on the table.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub slots: Vec<Slot>,
    pub score: String,
    pub status_message: String,
    /// Size of the playing area in card-sized units.
    pub width: f64,
    pub height: f64,
}

impl Table {
    pub fn cards(&self, id: usize) -> &[Card] {
        &self.slots[id].cards
    }

    pub fn add_cards(&mut self, id: usize, cards: &[Card]) {
        self.slots[id].cards.extend_from_slice(cards);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameState {
    /// Dealt, the clock has not started.
    Begin,
    Running,
    Over,
    Won,
}

/// One undo step: the table plus the game's own Scheme variables.
struct Snapshot {
    table: Table,
    state: Value,
}

/// A game in progress: rules + table + history + clock.
pub struct Game {
    pub rules: SchemeRules,
    pub table: Table,
    pub state: GameState,
    seed: u64,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    pending: Option<Snapshot>,
    started: Option<Instant>,
    elapsed: Duration,
    dealable: bool,
}

impl Game {
    /// Load a game by file name (e.g. "klondike") and deal.
    pub fn new(file: &str, seed: u64) -> Result<Self> {
        let rules = SchemeRules::load(file)?;
        let mut game = Game {
            rules,
            table: Table::default(),
            state: GameState::Begin,
            seed,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending: None,
            started: None,
            elapsed: Duration::ZERO,
            dealable: false,
        };
        game.deal_with_seed(seed)?;
        Ok(game)
    }

    pub fn name(&self) -> &str {
        &self.rules.name
    }

    pub fn file(&self) -> &str {
        &self.rules.file
    }

    fn deal_with_seed(&mut self, seed: u64) -> Result<()> {
        self.seed = seed;
        self.state = GameState::Begin;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.pending = None;
        self.started = None;
        self.elapsed = Duration::ZERO;
        self.rules.new_game(&mut self.table, seed)?;
        self.update_dealable();
        Ok(())
    }

    pub fn new_game(&mut self, seed: u64) -> Result<()> {
        self.deal_with_seed(seed)
    }

    pub fn restart(&mut self) -> Result<()> {
        self.deal_with_seed(self.seed)
    }

    /// Start the clock on the first interaction.
    pub fn start(&mut self) {
        if self.state == GameState::Begin {
            self.state = GameState::Running;
            self.started = Some(Instant::now());
        }
    }

    pub fn elapsed(&self) -> Duration {
        match (self.state, self.started) {
            (GameState::Running, Some(t)) => self.elapsed + t.elapsed(),
            _ => self.elapsed,
        }
    }

    fn set_state(&mut self, state: GameState) {
        if self.state == GameState::Running && state != GameState::Running {
            if let Some(t) = self.started.take() {
                self.elapsed += t.elapsed();
            }
        } else if self.state != GameState::Running && state == GameState::Running {
            self.started = Some(Instant::now());
        }
        self.state = state;
    }

    fn update_dealable(&mut self) {
        self.dealable = self.rules.dealable(&mut self.table);
    }

    fn snapshot(&mut self, table: Table) -> Snapshot {
        let state = self.rules.save_state(&mut self.table);
        Snapshot { table, state }
    }

    // ---- Undo history (record-move / end-move / discard-move) ----

    pub fn record_move(&mut self) {
        let snapshot = self.snapshot(self.table.clone());
        self.pending = Some(snapshot);
    }

    /// Like `record_move`, but from a table copied before cards were lifted
    /// off it for a drag.
    pub fn record_move_from(&mut self, table: Table) {
        let snapshot = self.snapshot(table);
        self.pending = Some(snapshot);
    }

    pub fn end_move(&mut self) {
        if let Some(snapshot) = self.pending.take() {
            self.undo_stack.push(snapshot);
            self.redo_stack.clear();
        }
    }

    pub fn discard_move(&mut self) {
        self.pending = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let current = self.snapshot(self.table.clone());
        self.table = snapshot.table;
        self.rules.restore_state(&mut self.table, &snapshot.state);
        current
    }

    pub fn undo(&mut self) {
        if self.state >= GameState::Over {
            self.set_state(GameState::Running);
        }
        if let Some(previous) = self.undo_stack.pop() {
            let current = self.restore(previous);
            self.redo_stack.push(current);
            self.update_dealable();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            let current = self.restore(next);
            self.undo_stack.push(current);
            self.test_end_of_game();
        }
    }

    pub fn test_end_of_game(&mut self) {
        self.end_move();
        self.update_dealable();
        if self.state < GameState::Over && !self.rules.game_over(&mut self.table) {
            let state = if self.rules.game_won(&mut self.table) { GameState::Won } else { GameState::Over };
            self.set_state(state);
        }
    }

    // ---- Player actions, as called by the board ----

    pub fn droppable_supported(&mut self) -> bool {
        self.rules.droppable_supported()
    }

    pub fn drag_valid(&mut self, slot: usize, card_index: usize) -> bool {
        let cards = self.table.cards(slot)[card_index..].to_vec();
        !cards.is_empty() && self.rules.button_pressed(&mut self.table, slot, &cards)
    }

    pub fn drop_valid(&mut self, start: usize, end: usize, cards: &[Card]) -> bool {
        self.rules.droppable(&mut self.table, start, cards, end)
    }

    /// Drop lifted cards onto `end`. The cards must already be off the table.
    pub fn drop_cards(&mut self, start: usize, end: usize, cards: &[Card]) -> bool {
        self.rules.button_released(&mut self.table, start, cards, end)
    }

    pub fn click(&mut self, slot: usize) -> bool {
        self.record_move();
        let moved = self.rules.button_clicked(&mut self.table, slot);
        if moved {
            self.end_move();
        } else {
            self.discard_move();
        }
        self.test_end_of_game();
        moved
    }

    pub fn double_click(&mut self, slot: usize) {
        self.record_move();
        if self.rules.button_double_clicked(&mut self.table, slot) {
            self.end_move();
        } else {
            self.discard_move();
        }
        self.test_end_of_game();
    }

    /// Whether the game asked to be called back shortly (autoplay).
    pub fn has_delayed_call(&mut self) -> bool {
        self.rules.has_delayed_call()
    }

    pub fn cancel_delayed_call(&mut self) {
        self.rules.take_delayed_call();
    }

    /// Run the pending delayed call (scm_execute_delayed_function).
    pub fn run_delayed_call(&mut self) {
        if let Some(callback) = self.rules.take_delayed_call() {
            self.rules.run(&mut self.table, &callback);
            self.test_end_of_game();
        }
    }

    pub fn can_deal(&self) -> bool {
        self.dealable
    }

    /// aisleriot_game_deal_cards
    pub fn deal(&mut self) {
        self.start();
        self.record_move();
        self.rules.deal(&mut self.table);
        self.end_move();
        self.test_end_of_game();
    }

    pub fn hint(&mut self) -> String {
        self.rules.hint(&mut self.table)
    }

    pub fn options(&mut self) -> Vec<GameOption> {
        self.rules.options(&mut self.table)
    }

    /// Set option `index`; radio groups switch off their other members.
    pub fn set_option(&mut self, index: usize, value: bool) {
        let options = self.options();
        let Some(changed) = options.get(index) else { return };
        let values: Vec<bool> = options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                if i == index {
                    value
                } else if changed.group.is_some() && o.group == changed.group && value {
                    false
                } else {
                    o.value
                }
            })
            .collect();
        self.set_options(&values);
    }

    pub fn set_options(&mut self, values: &[bool]) {
        self.rules.set_options(&mut self.table, values);
    }

    pub fn scores_disabled(&mut self) -> bool {
        self.rules.scores_disabled()
    }
}
