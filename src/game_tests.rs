// Rusty Solitaire - game_tests.rs
//
// Plays every game with random moves through the same calls the board
// makes, checking that the rule code never errors and never loses or
// duplicates a card.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use std::collections::HashMap;

use crate::card::Rng;
use crate::game::{Game, GameState, Table};
use crate::rules;

/// How many of each card are on the table.
type Deck = HashMap<(u8, u8), usize>;

fn total_cards(t: &Table) -> Deck {
    let mut deck = Deck::new();
    for card in t.slots.iter().flat_map(|s| &s.cards) {
        // Ace-high games use 14 for the ace.
        let rank = if card.rank == 14 { 1 } else { card.rank };
        *deck.entry((rank, card.suit as u8)).or_default() += 1;
    }
    deck
}

/// No rule errors, and no card ever duplicated or conjured up. (Several
/// games discard matched cards, so cards may disappear.)
fn check(game: &mut Game, dealt: &Deck, what: &str) -> Result<(), String> {
    let errors = game.rules.take_errors();
    if !errors.is_empty() {
        return Err(format!("{} after {what}: {}", game.file(), errors.join(" | ")));
    }
    for (card, n) in total_cards(&game.table) {
        let allowed = dealt.get(&card).copied().unwrap_or(0);
        if n > allowed {
            return Err(format!("{} after {what}: {n} copies of {card:?}, only {allowed} dealt", game.file()));
        }
    }
    Ok(())
}

fn run_autoplay(game: &mut Game) {
    for _ in 0..500 {
        if !game.has_delayed_call() {
            return;
        }
        game.run_delayed_call();
    }
}

/// A card the player could pick up: only fanned-out cards are reachable,
/// and on an unfanned pile only the top card (Board::hit).
fn grabbable_card(game: &Game, slot: usize, rng: &mut Rng) -> usize {
    let s = &game.table.slots[slot];
    let len = s.cards.len();
    let exposed = match s.expansion {
        crate::game::Expansion::None => 1,
        e if e.depth() > 0 => e.depth().min(len),
        _ => len,
    }
    .max(1);
    len.saturating_sub(exposed) + rng.below(exposed.min(len.max(1)))
}

/// Drag the cards from `card` up off `slot` and drop them, the way
/// Board::drag_begin / drop_moving_cards do.
fn try_drag(game: &mut Game, rng: &mut Rng, slot: usize, card: usize) -> bool {
    if !game.drag_valid(slot, card) {
        return false;
    }
    let before = game.table.clone();
    let cards = game.table.slots[slot].cards.split_off(card);
    game.record_move_from(before);
    let n = game.table.slots.len();
    let mut targets: Vec<usize> = (0..n).collect();
    rng.shuffle(&mut targets);
    // Mostly aim for a legal target, sometimes anywhere.
    let target = if rng.below(4) > 0 {
        targets.iter().copied().find(|&t| t != slot && game.drop_valid(slot, t, &cards)).unwrap_or(targets[0])
    } else {
        targets[0]
    };
    if game.drop_cards(slot, target, &cards) {
        game.end_move();
        game.test_end_of_game();
        true
    } else {
        game.discard_move();
        game.table.add_cards(slot, &cards);
        false
    }
}

struct Stats {
    moves: usize,
    slowest_hint: Duration,
}

fn play(file: &str, seed: u64, steps: usize) -> Result<Stats, String> {
    let mut game = Game::new(file, seed).map_err(|e| format!("{file}: load failed: {e}"))?;
    let mut expected = total_cards(&game.table);
    check(&mut game, &expected, "deal")?;
    if game.table.slots.is_empty() || expected.is_empty() {
        return Err(format!("{file}: empty table after deal"));
    }

    // Each option combination must deal too.
    let options = game.options();
    for i in 0..options.len() {
        game.set_option(i, !options[i].value);
        game.new_game(seed).map_err(|e| format!("{file}: deal with option {i} failed: {e}"))?;
        let n = total_cards(&game.table);
        check(&mut game, &n, &format!("option {i}"))?;
        game.set_options(&options.iter().map(|o| o.value).collect::<Vec<_>>());
    }
    game.new_game(seed).map_err(|e| e.to_string())?;
    expected = total_cards(&game.table);

    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9));
    let mut stats = Stats { moves: 0, slowest_hint: Duration::ZERO };
    for step in 0..steps {
        game.start();
        let n_slots = game.table.slots.len();
        let slot = rng.below(n_slots);
        let action = rng.below(12);
        let what = match action {
            0..=4 => {
                let len = game.table.cards(slot).len();
                let card = grabbable_card(&game, slot, &mut rng);
                if len > 0 && try_drag(&mut game, &mut rng, slot, card) {
                    stats.moves += 1;
                }
                "drag"
            }
            5 | 6 => {
                if game.click(slot) {
                    stats.moves += 1;
                }
                "click"
            }
            7 => {
                game.double_click(slot);
                "double-click"
            }
            8 => {
                if game.can_deal() {
                    game.deal();
                }
                "deal"
            }
            9 => {
                game.undo();
                "undo"
            }
            10 => {
                game.redo();
                "redo"
            }
            _ => {
                let t = Instant::now();
                let hint = game.hint();
                stats.slowest_hint = stats.slowest_hint.max(t.elapsed());
                if hint.is_empty() {
                    return Err(format!("{file}: empty hint"));
                }
                "hint"
            }
        };
        run_autoplay(&mut game);
        check(&mut game, &expected, &format!("{what} (step {step}, seed {seed})"))?;
        if game.state >= GameState::Over && rng.below(3) == 0 {
            game.new_game(seed + step as u64).map_err(|e| e.to_string())?;
            expected = total_cards(&game.table);
            check(&mut game, &expected, "new game")?;
        }
    }
    Ok(stats)
}

#[test]
fn every_game_plays_without_errors() {
    let games = rules::game_files();
    assert!(games.len() >= 90, "only {} games found", games.len());
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for file in &games {
        for seed in [1u64, 2, 3] {
            match play(file, seed, 250) {
                Ok(stats) => {
                    if seed == 1 {
                        report.push(format!(
                            "{file}: {} moves, slowest hint {:?}",
                            stats.moves, stats.slowest_hint
                        ));
                    }
                }
                Err(e) => {
                    failures.push(e);
                    break;
                }
            }
        }
    }
    if std::env::var_os("RUSTY_TEST_REPORT").is_some() {
        eprintln!("{}", report.join("\n"));
    }
    assert!(failures.is_empty(), "{} games failed:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn klondike_matches_original_layout() {
    let game = Game::new("klondike", 1).unwrap();
    let t = &game.table;
    assert_eq!(t.slots.len(), 13);
    assert_eq!((t.width, t.height), (7.0, 3.1));
    assert_eq!(t.cards(0).len(), 24);
    for i in 0..7 {
        let pile = t.cards(6 + i);
        assert_eq!(pile.len(), i + 1);
        assert!(pile.last().unwrap().face_up);
        assert!(pile[..i].iter().all(|c| !c.face_up));
    }
    assert_eq!(t.status_message, "Stock left: 24   Redeals left: 2");
}

#[test]
fn undo_restores_scheme_state() {
    let mut game = Game::new("klondike", 7).unwrap();
    // Run through the stock and recycle it, which bumps FLIP-COUNTER.
    for _ in 0..25 {
        game.click(0);
    }
    assert_eq!(game.table.status_message, "Stock left: 24   Redeals left: 1");
    game.undo();
    assert_eq!(game.table.status_message, "Stock left: 0   Redeals left: 2");
    game.redo();
    assert_eq!(game.table.status_message, "Stock left: 24   Redeals left: 1");
}

/// Debug helper: replay a game's random run and print the table when the
/// rule code first fails. Run with `cargo test -- --ignored replay`.
#[test]
#[ignore]
fn replay_failure() {
    let file = std::env::var("REPLAY_GAME").unwrap_or_else(|_| "cruel".into());
    let seed: u64 = std::env::var("REPLAY_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut game = Game::new(&file, seed).unwrap();
    let options = game.options();
    for i in 0..options.len() {
        game.set_option(i, !options[i].value);
        game.new_game(seed).unwrap();
        game.set_options(&options.iter().map(|o| o.value).collect::<Vec<_>>());
    }
    game.new_game(seed).unwrap();
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9));
    let mut log = Vec::new();
    for step in 0..250 {
        let before = format!("{:?}", game.table.slots.iter().map(|s| s.cards.len()).collect::<Vec<_>>());
        let score = game.table.score.clone();
        game.start();
        let n_slots = game.table.slots.len();
        let slot = rng.below(n_slots);
        let action = rng.below(12);
        match action {
            0..=4 => {
                let len = game.table.cards(slot).len();
                let card = grabbable_card(&game, slot, &mut rng);
                if len > 0 {
                    try_drag(&mut game, &mut rng, slot, card);
                }
            }
            5 | 6 => {
                game.click(slot);
            }
            7 => game.double_click(slot),
            8 => {
                if game.can_deal() {
                    game.deal();
                }
            }
            9 => game.undo(),
            10 => game.redo(),
            _ => {
                game.hint();
            }
        }
        run_autoplay(&mut game);
        log.push(format!("step {step}: action {action} slot {slot}; before {before} score {score}"));
        let errors = game.rules.take_errors();
        if !errors.is_empty() {
            for line in &log[log.len().saturating_sub(8)..] {
                eprintln!("{line}");
            }
            eprintln!("after: {:?} score {}", game.table.slots.iter().map(|s| s.cards.len()).collect::<Vec<_>>(), game.table.score);
            eprintln!("errors: {errors:?}");
            let mut t = game.table.clone();
            eprintln!("just-redealt = {:?}", game.rules.eval(&mut t, "just-redealt"));
            eprintln!("variable-list = {:?}", game.rules.eval(&mut t, "variable-list"));
            return;
        }
        if game.state >= GameState::Over && rng.below(3) == 0 {
            game.new_game(seed + step as u64).unwrap();
        }
    }
    eprintln!("no failure");
}
