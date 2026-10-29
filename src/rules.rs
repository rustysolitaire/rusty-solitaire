// Rusty Solitaire - rules.rs
//
// Runs one of Aisleriot's original game rule files (games/*.scm) and
// connects it to the card table. This is the Rust counterpart of the
// "interface" half of game.c: the procedures the Scheme code calls
// (add-slot, get-slot, set-cards-c!, ...) and the calls into the game's
// lambdas (new-game, button-pressed, ...).
//
// Copyright (C) 1998, 2003 Jonathan Blandford <jrb@mit.edu>
// Copyright (C) 2007 Christian Persch
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::card::{Card, Rng, SUITS};
use crate::game::{Expansion, Slot, SlotType, Table};
use crate::scheme::{err, Error, Interp, Result, Value};

include!(concat!(env!("OUT_DIR"), "/game_files.rs"));

// Indices into Host::lambdas, in the order set-lambda takes them.
const NEW_GAME: usize = 0;
const BUTTON_PRESSED: usize = 1;
const BUTTON_RELEASED: usize = 2;
const BUTTON_CLICKED: usize = 3;
const BUTTON_DOUBLE_CLICKED: usize = 4;
const GAME_OVER: usize = 5;
const WINNING_GAME: usize = 6;
const HINT: usize = 7;
const GET_OPTIONS: usize = 8;
const APPLY_OPTIONS: usize = 9;
const TIMEOUT: usize = 10;
const DROPPABLE: usize = 11;
const DEALABLE: usize = 12;
const N_LAMBDAS: usize = 13;
const LAST_MANDATORY_LAMBDA: usize = TIMEOUT;

const LAMBDA_NAMES: [&str; N_LAMBDAS] = [
    "new-game",
    "button-pressed",
    "button-released",
    "button-clicked",
    "button-double-clicked",
    "game-over",
    "winning-game",
    "hint",
    "get-options",
    "apply-options",
    "timeout",
    "droppable",
    "dealable",
];

pub const FEATURE_DROPPABLE: u32 = 1;
pub const FEATURE_SCORES_DISABLED: u32 = 2;
pub const FEATURE_DEALABLE: u32 = 4;

/// One entry in the game's own options menu.
#[derive(Clone, Debug, PartialEq)]
pub struct GameOption {
    pub label: String,
    pub value: bool,
    /// Options in the same group behave like radio buttons.
    pub group: Option<u32>,
}

/// State the Scheme side reads and writes through the bridge procedures.
struct Host {
    table: Table,
    features: u32,
    lambdas: Vec<Value>,
    rng: Rng,
    delayed_call: Option<Value>,
    timeout: i64,
    /// Errors raised by the rule code, kept for tests.
    errors: Vec<String>,
}

fn host(interp: &mut Interp) -> &mut Host {
    interp.host.downcast_mut::<Host>().expect("game host")
}

/// The file names (without .scm) of every game, sorted by display name.
pub fn game_files() -> Vec<&'static str> {
    let mut games: Vec<&str> = GAME_FILES
        .iter()
        .map(|(file, _)| *file)
        .filter(|f| *f != "api.scm")
        .map(|f| f.trim_end_matches(".scm"))
        .collect();
    games.sort_by_key(|g| display_name(g).to_lowercase());
    games
}

/// "forty-thieves" -> "Forty Thieves", like ar_filename_to_display_name.
pub fn display_name(file: &str) -> String {
    crate::theme::display_name(file)
}

fn source(name: &str) -> Option<String> {
    let name = name.rsplit('/').next().unwrap_or(name);
    let with_ext = if name.ends_with(".scm") { name.to_string() } else { format!("{name}.scm") };
    GAME_FILES.iter().find(|(file, _)| *file == with_ext).map(|(_, src)| src.to_string())
}

// ---- conversions between table cards and Scheme card lists ----

fn card_to_value(card: &Card) -> Value {
    Value::list([Value::Int(card.rank as i64), Value::Int(card.suit as i64), Value::Bool(card.face_up)])
}

/// Cards (bottom first) to a Scheme list (top first).
pub fn cards_to_value(cards: &[Card]) -> Value {
    cards.iter().fold(Value::Nil, |acc, c| Value::cons(card_to_value(c), acc))
}

fn value_to_card(v: &Value) -> Result<Card> {
    let items = v.to_vec().ok_or_else(|| Error::new(format!("not a card: {v:?}")))?;
    let (Some(Value::Int(rank)), Some(Value::Int(suit))) = (items.first(), items.get(1)) else {
        return err(format!("not a card: {v:?}"));
    };
    let face_up = items.get(2).is_some_and(Value::is_true);
    Ok(Card { rank: *rank as u8, suit: SUITS[(*suit as usize).min(3)], face_up })
}

/// A Scheme list (top first) to cards (bottom first).
fn value_to_cards(v: &Value) -> Result<Vec<Card>> {
    let mut cards = v.iter().map(|c| value_to_card(&c)).collect::<Result<Vec<_>>>()?;
    cards.reverse();
    Ok(cards)
}

fn int_arg(v: &Value) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) => Ok(*f as i64),
        other => err(format!("expected integer, got {other:?}")),
    }
}

fn slot_index(interp: &mut Interp, v: &Value) -> Result<usize> {
    let id = int_arg(v)?;
    if id < 0 || id as usize >= host(interp).table.slots.len() {
        return err(format!("no slot with id {id}"));
    }
    Ok(id as usize)
}

// ---- the procedures game files call (cscm_init in game.c) ----

fn set_feature_word(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    host(i).features = int_arg(&a[0])? as u32;
    Ok(Value::Nil)
}

fn get_feature_word(i: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Int(host(i).features as i64))
}

fn set_statusbar_message_c(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    if let Value::Str(s) = &a[0] {
        host(i).table.status_message = s.to_string();
    }
    Ok(Value::Nil)
}

fn reset_surface(i: &mut Interp, _: Vec<Value>) -> Result<Value> {
    host(i).table.slots.clear();
    Ok(Value::Nil)
}

fn add_slot(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let data = &a[0];
    let items = data.to_vec().ok_or_else(|| Error::new("add-slot: bad slot"))?;
    let [id, cards, placement, rest @ ..] = items.as_slice() else { return err("add-slot: bad slot") };
    let placement = placement.to_vec().ok_or_else(|| Error::new("add-slot: bad placement"))?;
    let kind = placement.first().and_then(Value::as_sym).map(|s| s.name().to_string()).unwrap_or_default();
    let pos = placement.get(1).and_then(Value::to_vec).unwrap_or_default();
    let depth = placement.get(2).map(int_arg).transpose()?.unwrap_or(0) as usize;
    let expansion = match kind.as_str() {
        "expanded" => Expansion::Down,
        "expanded-right" => Expansion::Right,
        "partially-expanded" => Expansion::PartialDown(depth),
        "partially-expanded-right" => Expansion::PartialRight(depth),
        _ => Expansion::None,
    };
    let slot_type = match rest.first().and_then(Value::as_sym).map(|s| s.name().to_string()).as_deref() {
        Some("chooser") => SlotType::Chooser,
        Some("foundation") => SlotType::Foundation,
        Some("reserve") => SlotType::Reserve,
        Some("stock") => SlotType::Stock,
        Some("tableau") => SlotType::Tableau,
        Some("waste") => SlotType::Waste,
        _ => SlotType::Unknown,
    };
    let coord = |n: usize| pos.get(n).and_then(Value::as_f64).unwrap_or(0.0);
    let slot = Slot {
        cards: value_to_cards(cards)?,
        x: coord(0),
        y: coord(1),
        expansion,
        slot_type,
        dx: None,
        dy: None,
    };
    let id = int_arg(id)? as usize;
    let slots = &mut host(i).table.slots;
    if id != slots.len() {
        return err(format!("add-slot: slot ids must be consecutive, got {id}"));
    }
    slots.push(slot);
    Ok(Value::Nil)
}

fn get_slot(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let Ok(id) = slot_index(i, &a[0]) else { return Ok(Value::Nil) };
    let cards = cards_to_value(&host(i).table.slots[id].cards);
    Ok(Value::list([a[0].clone(), cards]))
}

fn set_cards_c(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let id = slot_index(i, &a[0])?;
    let cards = if a[1].is_list() { value_to_cards(&a[1])? } else { Vec::new() };
    host(i).table.slots[id].cards = cards;
    Ok(Value::Bool(true))
}

fn set_slot_y_expansion(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let id = slot_index(i, &a[0])?;
    let dy = a[1].as_f64().ok_or_else(|| Error::new("set-slot-y-expansion!: not a number"))?;
    host(i).table.slots[id].dy = Some(dy);
    Ok(Value::Nil)
}

fn set_slot_x_expansion(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let id = slot_index(i, &a[0])?;
    let dx = a[1].as_f64().ok_or_else(|| Error::new("set-slot-x-expansion!: not a number"))?;
    host(i).table.slots[id].dx = Some(dx);
    Ok(Value::Nil)
}

fn set_lambda(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let h = host(i);
    let mut args = a.into_iter();
    for idx in 0..=LAST_MANDATORY_LAMBDA {
        h.lambdas[idx] = args.next().unwrap_or(Value::Unspecified);
    }
    if h.features & FEATURE_DROPPABLE != 0 {
        h.lambdas[DROPPABLE] = args.next().unwrap_or(Value::Unspecified);
    }
    if h.features & FEATURE_DEALABLE != 0 {
        h.lambdas[DEALABLE] = args.next().unwrap_or(Value::Unspecified);
    }
    Ok(Value::Nil)
}

fn set_lambda_x(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let name = a[0].as_sym().map(|s| s.name().to_string()).unwrap_or_default();
    let Some(idx) = LAMBDA_NAMES.iter().position(|n| *n == name) else {
        return err(format!("Unknown lambda name in set-lambda!: {name}"));
    };
    host(i).lambdas[idx] = a[1].clone();
    Ok(Value::Nil)
}

fn aisleriot_random(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let range = int_arg(&a[0])?.max(1) as usize;
    Ok(Value::Int(host(i).rng.below(range) as i64))
}

fn click_to_move_p(_: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(false))
}

fn update_score(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    host(i).table.score = crate::scheme::value::to_string(&a[0], false);
    Ok(a[0].clone())
}

fn get_timeout(i: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Int(host(i).timeout))
}

fn set_timeout(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    host(i).timeout = int_arg(&a[0])?;
    Ok(a[0].clone())
}

fn delayed_call(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let h = host(i);
    if h.delayed_call.is_some() {
        return err("Already have a delayed callback pending.");
    }
    h.delayed_call = Some(a[0].clone());
    Ok(Value::Bool(true))
}

/// Rule errors are logged, like the original's exception dialog, and
/// otherwise treated as "nothing happened".
fn report(interp: &mut Interp, message: String) {
    eprintln!("{message}");
    host(interp).errors.push(message);
}

fn ignore(_: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Nil)
}

fn install_bridge(i: &mut Interp) {
    i.define_builtin("set-feature-word!", 1, Some(1), set_feature_word);
    i.define_builtin("get-feature-word", 0, Some(0), get_feature_word);
    i.define_builtin("set-statusbar-message-c", 1, Some(1), set_statusbar_message_c);
    i.define_builtin("reset-surface", 0, Some(0), reset_surface);
    i.define_builtin("add-slot", 1, Some(1), add_slot);
    i.define_builtin("get-slot", 1, Some(1), get_slot);
    i.define_builtin("set-cards-c!", 2, Some(2), set_cards_c);
    i.define_builtin("set-slot-y-expansion!", 2, Some(2), set_slot_y_expansion);
    i.define_builtin("set-slot-x-expansion!", 2, Some(2), set_slot_x_expansion);
    i.define_builtin("set-lambda", 8, None, set_lambda);
    i.define_builtin("set-lambda!", 2, Some(2), set_lambda_x);
    i.define_builtin("aisleriot-random", 1, Some(1), aisleriot_random);
    i.define_builtin("click-to-move?", 0, Some(0), click_to_move_p);
    i.define_builtin("update-score", 1, Some(1), update_score);
    i.define_builtin("get-timeout", 0, Some(0), get_timeout);
    i.define_builtin("set-timeout!", 1, Some(1), set_timeout);
    i.define_builtin("delayed-call", 1, Some(1), delayed_call);
    i.define_builtin("undo-set-sensitive", 1, Some(1), ignore);
    i.define_builtin("redo-set-sensitive", 1, Some(1), ignore);
    i.define_builtin("dealable-set-sensitive", 1, Some(1), ignore);
}

/// A loaded game: an interpreter that has run api.scm and the game's file.
pub struct SchemeRules {
    interp: Interp,
    pub file: String,
    pub name: String,
}

impl SchemeRules {
    pub fn load(file: &str) -> Result<SchemeRules> {
        let state = Host {
            table: Table::default(),
            features: 0,
            lambdas: vec![Value::Unspecified; N_LAMBDAS],
            rng: Rng::new(0),
            delayed_call: None,
            timeout: 0,
            errors: Vec::new(),
        };
        let mut interp = Interp::new(Box::new(source), Box::new(state));
        install_bridge(&mut interp);
        interp.load_file("api.scm")?;
        interp.load_file(file)?;

        let h = host(&mut interp);
        for idx in 0..=LAST_MANDATORY_LAMBDA {
            if !h.lambdas[idx].is_procedure() {
                return err(format!("{file}: {} is not a procedure", LAMBDA_NAMES[idx]));
            }
        }
        if h.features & FEATURE_DROPPABLE != 0 && !h.lambdas[DROPPABLE].is_procedure() {
            return err(format!("{file}: droppable is not a procedure"));
        }
        if h.features & FEATURE_DEALABLE != 0 && !h.lambdas[DEALABLE].is_procedure() {
            return err(format!("{file}: dealable is not a procedure"));
        }
        Ok(SchemeRules { interp, file: file.to_string(), name: display_name(file) })
    }

    fn features(&mut self) -> u32 {
        host(&mut self.interp).features
    }

    pub fn droppable_supported(&mut self) -> bool {
        self.features() & FEATURE_DROPPABLE != 0
    }

    pub fn dealable_supported(&mut self) -> bool {
        self.features() & FEATURE_DEALABLE != 0
    }

    pub fn scores_disabled(&mut self) -> bool {
        self.features() & FEATURE_SCORES_DISABLED != 0
    }

    /// Run `f` with the table lent to the Scheme side.
    fn with_table<R>(&mut self, table: &mut Table, f: impl FnOnce(&mut Interp) -> R) -> R {
        std::mem::swap(&mut host(&mut self.interp).table, table);
        let result = f(&mut self.interp);
        std::mem::swap(&mut host(&mut self.interp).table, table);
        result
    }

    /// Call one of the game's lambdas. Errors are reported and count as #f,
    /// like the original's exception handler.
    fn call(&mut self, table: &mut Table, idx: usize, args: Vec<Value>) -> Option<Value> {
        let lambda = host(&mut self.interp).lambdas[idx].clone();
        if !lambda.is_procedure() {
            return None;
        }
        let file = self.file.clone();
        self.with_table(table, |interp| match interp.apply(&lambda, args) {
            Ok(v) => Some(v),
            Err(e) => {
                report(interp, format!("{file}: error in {}: {e}", LAMBDA_NAMES[idx]));
                None
            }
        })
    }

    fn call_named(&mut self, table: &mut Table, name: &str, args: Vec<Value>) -> Option<Value> {
        let file = self.file.clone();
        self.with_table(table, |interp| {
            let result = match interp.global(name) {
                Some(f) => interp.apply(&f, args),
                None => err(format!("{name} is not defined")),
            };
            result.map_err(|e| report(interp, format!("{file}: error in {name}: {e}"))).ok()
        })
    }

    fn call_bool(&mut self, table: &mut Table, idx: usize, args: Vec<Value>) -> bool {
        self.call(table, idx, args).is_some_and(|v| v.is_true())
    }

    /// Deal a new game from `seed` (game_scm_new_game). Deals again until
    /// the game isn't over before it starts.
    pub fn new_game(&mut self, table: &mut Table, seed: u64) -> Result<()> {
        host(&mut self.interp).rng = Rng::new(seed);
        host(&mut self.interp).delayed_call = None;
        for _ in 0..100 {
            *table = Table::default();
            let size = self
                .call(table, NEW_GAME, Vec::new())
                .ok_or_else(|| Error::new(format!("{}: new-game failed", self.file)))?;
            let dims = size.to_vec().unwrap_or_default();
            table.width = dims.first().and_then(Value::as_f64).unwrap_or(1.0).max(0.1);
            table.height = dims.get(1).and_then(Value::as_f64).unwrap_or(1.0).max(0.1);
            self.call_named(table, "start-game", Vec::new());
            if self.call(table, GAME_OVER, Vec::new()).is_none_or(|v| v.is_true()) {
                return Ok(());
            }
        }
        Ok(())
    }

    pub fn button_pressed(&mut self, table: &mut Table, slot: usize, cards: &[Card]) -> bool {
        self.call_bool(table, BUTTON_PRESSED, vec![Value::Int(slot as i64), cards_to_value(cards)])
    }

    pub fn droppable(&mut self, table: &mut Table, start: usize, cards: &[Card], end: usize) -> bool {
        if !self.droppable_supported() {
            return false;
        }
        let args = vec![Value::Int(start as i64), cards_to_value(cards), Value::Int(end as i64)];
        self.call_bool(table, DROPPABLE, args)
    }

    pub fn button_released(&mut self, table: &mut Table, start: usize, cards: &[Card], end: usize) -> bool {
        let args = vec![Value::Int(start as i64), cards_to_value(cards), Value::Int(end as i64)];
        self.call_bool(table, BUTTON_RELEASED, args)
    }

    pub fn button_clicked(&mut self, table: &mut Table, slot: usize) -> bool {
        self.call_bool(table, BUTTON_CLICKED, vec![Value::Int(slot as i64)])
    }

    pub fn button_double_clicked(&mut self, table: &mut Table, slot: usize) -> bool {
        self.call_bool(table, BUTTON_DOUBLE_CLICKED, vec![Value::Int(slot as i64)])
    }

    /// True while the game can go on (errors count as "can go on").
    pub fn game_over(&mut self, table: &mut Table) -> bool {
        self.call(table, GAME_OVER, Vec::new()).is_none_or(|v| v.is_true())
    }

    pub fn game_won(&mut self, table: &mut Table) -> bool {
        self.call_bool(table, WINNING_GAME, Vec::new())
    }

    pub fn dealable(&mut self, table: &mut Table) -> bool {
        self.dealable_supported() && self.call_bool(table, DEALABLE, Vec::new())
    }

    pub fn deal(&mut self, table: &mut Table) {
        self.call_named(table, "do-deal-next-cards", Vec::new());
    }

    /// aisleriot_game_get_hint
    pub fn hint(&mut self, table: &mut Table) -> String {
        let Some(hint) = self.call(table, HINT, Vec::new()) else {
            return "This game is unable to provide a hint.".into();
        };
        if !hint.is_true() {
            return "This game does not have hint support yet.".into();
        }
        let items = hint.to_vec().unwrap_or_default();
        let text = |n: usize| match items.get(n) {
            Some(Value::Str(s)) => Some(s.to_string()),
            _ => None,
        };
        let message = match items.first() {
            Some(Value::Int(0)) => text(1),
            Some(Value::Int(1 | 2)) => match (text(1), text(2)) {
                (Some(a), Some(b)) => Some(format!("Move {a} onto {b}.")),
                _ => None,
            },
            _ => None,
        };
        message.unwrap_or_else(|| "This game is unable to provide a hint.".into())
    }

    /// aisleriot_game_get_options: list entries are options; any other
    /// entry ('begin-exclusive / 'end-exclusive) toggles radio grouping.
    pub fn options(&mut self, table: &mut Table) -> Vec<GameOption> {
        let Some(list) = self.call(table, GET_OPTIONS, Vec::new()) else { return Vec::new() };
        let mut options = Vec::new();
        let mut group: Option<u32> = None;
        let mut next_group = 0;
        for entry in list.to_vec().unwrap_or_default() {
            if entry.is_list() && !entry.is_null() {
                let label = match entry.car() {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => continue,
                };
                let value = entry.iter().nth(1).is_some_and(|v| v.is_true());
                options.push(GameOption { label, value, group });
            } else if group.is_none() {
                group = Some(next_group);
                next_group += 1;
            } else {
                group = None;
            }
        }
        options
    }

    /// aisleriot_game_change_options: write `values` into the options list
    /// and hand it to apply-options.
    pub fn set_options(&mut self, table: &mut Table, values: &[bool]) {
        let Some(list) = self.call(table, GET_OPTIONS, Vec::new()) else { return };
        let mut i = 0;
        for entry in list.to_vec().unwrap_or_default() {
            if !(entry.is_list() && !entry.is_null()) {
                continue;
            }
            if let (Some(&value), Some(Value::Pair(p))) = (values.get(i), entry.cdr()) {
                *p.car.borrow_mut() = Value::Bool(value);
            }
            i += 1;
        }
        self.call(table, APPLY_OPTIONS, vec![list]);
    }

    /// The Scheme-side state an undo step restores (score, FLIP-COUNTER,
    /// status message and the game's def-save-var variables), as api.scm's
    /// record-move collects it.
    pub fn save_state(&mut self, table: &mut Table) -> Value {
        self.with_table(table, |interp| {
            interp
                .eval_str("(list (get-score) FLIP-COUNTER STATUSBAR-MESSAGE (save-variables variable-list))")
                .unwrap_or(Value::Nil)
        })
    }

    pub fn restore_state(&mut self, table: &mut Table, state: &Value) {
        if state.is_null() {
            return;
        }
        self.call_named(table, "undo-func", vec![state.clone()]);
    }

    pub fn take_delayed_call(&mut self) -> Option<Value> {
        host(&mut self.interp).delayed_call.take()
    }

    pub fn has_delayed_call(&mut self) -> bool {
        host(&mut self.interp).delayed_call.is_some()
    }

    pub fn run(&mut self, table: &mut Table, proc_: &Value) {
        let file = self.file.clone();
        self.with_table(table, |interp| {
            if let Err(e) = interp.apply(proc_, Vec::new()) {
                report(interp, format!("{file}: error in delayed call: {e}"));
            }
        });
    }

    #[cfg(test)]
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut host(&mut self.interp).errors)
    }

    #[cfg(test)]
    pub fn eval(&mut self, table: &mut Table, src: &str) -> Result<Value> {
        self.with_table(table, |interp| interp.eval_str(src))
    }
}
