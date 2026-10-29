// Rusty Solitaire - scheme/value.rs
//
// Values of the small Scheme dialect that runs Aisleriot's game rules.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::{Env, Interp, Result};

/// An interned symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sym(pub u32);

thread_local! {
    static SYMBOLS: RefCell<(HashMap<String, u32>, Vec<Rc<str>>)> = RefCell::new((HashMap::new(), Vec::new()));
}

impl Sym {
    pub fn new(name: &str) -> Sym {
        SYMBOLS.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(&id) = s.0.get(name) {
                return Sym(id);
            }
            let id = s.1.len() as u32;
            s.1.push(Rc::from(name));
            s.0.insert(name.to_string(), id);
            Sym(id)
        })
    }

    pub fn name(self) -> Rc<str> {
        SYMBOLS.with(|s| s.borrow().1[self.0 as usize].clone())
    }
}

pub struct Pair {
    pub car: RefCell<Value>,
    pub cdr: RefCell<Value>,
}

pub struct Lambda {
    pub name: Option<Sym>,
    pub params: Vec<Sym>,
    pub rest: Option<Sym>,
    pub body: Rc<Vec<Value>>,
    pub env: Env,
}

pub type BuiltinFn = fn(&mut Interp, Vec<Value>) -> Result<Value>;

pub struct Builtin {
    pub name: &'static str,
    pub func: BuiltinFn,
    pub min_args: usize,
    /// None means any number.
    pub max_args: Option<usize>,
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Sym(Sym),
    Pair(Rc<Pair>),
    Vector(Rc<RefCell<Vec<Value>>>),
    Lambda(Rc<Lambda>),
    Builtin(Rc<Builtin>),
    /// A hash table keyed by the written form of the key (equal? semantics).
    Hash(Rc<RefCell<HashMap<String, Value>>>),
    /// The result of (values ...) with other than one value.
    Values(Rc<Vec<Value>>),
    Unspecified,
}

impl Value {
    pub fn cons(car: Value, cdr: Value) -> Value {
        Value::Pair(Rc::new(Pair { car: RefCell::new(car), cdr: RefCell::new(cdr) }))
    }

    pub fn sym(name: &str) -> Value {
        Value::Sym(Sym::new(name))
    }

    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn list(items: impl IntoIterator<Item = Value, IntoIter: DoubleEndedIterator>) -> Value {
        items.into_iter().rev().fold(Value::Nil, |acc, v| Value::cons(v, acc))
    }

    pub fn is_true(&self) -> bool {
        !matches!(self, Value::Bool(false))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Nil)
    }

    pub fn is_procedure(&self) -> bool {
        matches!(self, Value::Lambda(_) | Value::Builtin(_))
    }

    pub fn car(&self) -> Option<Value> {
        match self {
            Value::Pair(p) => Some(p.car.borrow().clone()),
            _ => None,
        }
    }

    pub fn cdr(&self) -> Option<Value> {
        match self {
            Value::Pair(p) => Some(p.cdr.borrow().clone()),
            _ => None,
        }
    }

    pub fn as_sym(&self) -> Option<Sym> {
        match self {
            Value::Sym(s) => Some(*s),
            _ => None,
        }
    }

    /// Iterate a proper (or improper, stopping at the tail) list.
    pub fn iter(&self) -> ListIter {
        ListIter { cur: self.clone() }
    }

    /// Collect a list into a Vec; None if it is not a proper list.
    pub fn to_vec(&self) -> Option<Vec<Value>> {
        let mut out = Vec::new();
        let mut cur = self.clone();
        loop {
            match cur {
                Value::Nil => return Some(out),
                Value::Pair(p) => {
                    out.push(p.car.borrow().clone());
                    let next = p.cdr.borrow().clone();
                    cur = next;
                }
                _ => return None,
            }
        }
    }

    pub fn is_list(&self) -> bool {
        let mut cur = self.clone();
        loop {
            match cur {
                Value::Nil => return true,
                Value::Pair(p) => {
                    let next = p.cdr.borrow().clone();
                    cur = next;
                }
                _ => return false,
            }
        }
    }


    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
}

pub struct ListIter {
    cur: Value,
}

impl Iterator for ListIter {
    type Item = Value;
    fn next(&mut self) -> Option<Value> {
        match self.cur.clone() {
            Value::Pair(p) => {
                let item = p.car.borrow().clone();
                self.cur = p.cdr.borrow().clone();
                Some(item)
            }
            _ => None,
        }
    }
}

/// eqv? (and eq?, since small numbers are immediate in Guile too).
pub fn eqv(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) | (Value::Unspecified, Value::Unspecified) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Sym(x), Value::Sym(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y) || (x.is_empty() && y.is_empty()),
        (Value::Pair(x), Value::Pair(y)) => Rc::ptr_eq(x, y),
        (Value::Vector(x), Value::Vector(y)) => Rc::ptr_eq(x, y),
        (Value::Lambda(x), Value::Lambda(y)) => Rc::ptr_eq(x, y),
        (Value::Builtin(x), Value::Builtin(y)) => Rc::ptr_eq(x, y),
        (Value::Hash(x), Value::Hash(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Pair(x), Value::Pair(y)) => {
            Rc::ptr_eq(x, y)
                || (equal(&x.car.borrow(), &y.car.borrow()) && equal(&x.cdr.borrow(), &y.cdr.borrow()))
        }
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Vector(x), Value::Vector(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| equal(a, b))
        }
        _ => eqv(a, b),
    }
}

/// Deep copy of pairs and vectors (Guile's copy-tree).
pub fn copy_tree(v: &Value) -> Value {
    match v {
        Value::Pair(p) => Value::cons(copy_tree(&p.car.borrow()), copy_tree(&p.cdr.borrow())),
        Value::Vector(items) => {
            Value::Vector(Rc::new(RefCell::new(items.borrow().iter().map(copy_tree).collect())))
        }
        other => other.clone(),
    }
}

fn format_float(f: f64) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// `display` style when `write` is false, `write` style otherwise.
pub fn to_string(v: &Value, write: bool) -> String {
    let mut out = String::new();
    write_value(&mut out, v, write);
    out
}

fn write_value(out: &mut String, v: &Value, write: bool) {
    match v {
        Value::Nil => out.push_str("()"),
        Value::Bool(true) => out.push_str("#t"),
        Value::Bool(false) => out.push_str("#f"),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => out.push_str(&format_float(*f)),
        Value::Str(s) => {
            if write {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            } else {
                out.push_str(s);
            }
        }
        Value::Sym(s) => out.push_str(&s.name()),
        Value::Pair(_) => {
            out.push('(');
            let mut cur = v.clone();
            let mut first = true;
            loop {
                match cur {
                    Value::Pair(p) => {
                        if !first {
                            out.push(' ');
                        }
                        first = false;
                        write_value(out, &p.car.borrow(), write);
                        let next = p.cdr.borrow().clone();
                        cur = next;
                    }
                    Value::Nil => break,
                    other => {
                        out.push_str(" . ");
                        write_value(out, &other, write);
                        break;
                    }
                }
            }
            out.push(')');
        }
        Value::Vector(items) => {
            out.push_str("#(");
            for (i, item) in items.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(out, item, write);
            }
            out.push(')');
        }
        Value::Lambda(l) => {
            out.push_str("#<procedure ");
            out.push_str(&l.name.map(|n| n.name().to_string()).unwrap_or_default());
            out.push('>');
        }
        Value::Builtin(b) => {
            out.push_str("#<primitive ");
            out.push_str(b.name);
            out.push('>');
        }
        Value::Hash(_) => out.push_str("#<hash-table>"),
        Value::Values(vals) => {
            for (i, item) in vals.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(out, item, write);
            }
        }
        Value::Unspecified => {}
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_string(self, true))
    }
}
