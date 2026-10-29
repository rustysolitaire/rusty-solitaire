// Rusty Solitaire - scheme/builtins.rs
//
// The standard procedures available to game rule files: the parts of R5RS,
// Guile and SRFI-1 that Aisleriot's games use.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::value::{copy_tree, equal, eqv, to_string, Value};
use super::{err, Error, Interp, Result};

fn int(v: &Value, who: &str) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
        other => err(format!("{who}: expected integer, got {other:?}")),
    }
}

fn num(v: &Value, who: &str) -> Result<f64> {
    v.as_f64().ok_or_else(|| Error::new(format!("{who}: expected number, got {v:?}")))
}

fn list_arg(v: &Value, who: &str) -> Result<Vec<Value>> {
    v.to_vec().ok_or_else(|| Error::new(format!("{who}: expected list, got {v:?}")))
}

fn string_arg(v: &Value, who: &str) -> Result<Rc<str>> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => err(format!("{who}: expected string, got {other:?}")),
    }
}

fn pair_arg<'a>(v: &'a Value, who: &str) -> Result<&'a Rc<super::value::Pair>> {
    match v {
        Value::Pair(p) => Ok(p),
        other => err(format!("{who}: expected pair, got {other:?}")),
    }
}

fn float_or_int(f: f64) -> Value {
    Value::Float(f)
}

// ---- arithmetic ----

fn add(_: &mut Interp, args: Vec<Value>) -> Result<Value> {
    let mut acc = Value::Int(0);
    for a in &args {
        acc = match (&acc, a) {
            (Value::Int(x), Value::Int(y)) => match x.checked_add(*y) {
                Some(r) => Value::Int(r),
                None => float_or_int(*x as f64 + *y as f64),
            },
            _ => float_or_int(num(&acc, "+")? + num(a, "+")?),
        };
    }
    Ok(acc)
}

fn mul(_: &mut Interp, args: Vec<Value>) -> Result<Value> {
    let mut acc = Value::Int(1);
    for a in &args {
        acc = match (&acc, a) {
            (Value::Int(x), Value::Int(y)) => match x.checked_mul(*y) {
                Some(r) => Value::Int(r),
                None => float_or_int(*x as f64 * *y as f64),
            },
            _ => float_or_int(num(&acc, "*")? * num(a, "*")?),
        };
    }
    Ok(acc)
}

fn sub(_: &mut Interp, args: Vec<Value>) -> Result<Value> {
    if args.len() == 1 {
        return Ok(match &args[0] {
            Value::Int(i) => Value::Int(-i),
            other => Value::Float(-num(other, "-")?),
        });
    }
    let mut acc = args[0].clone();
    for a in &args[1..] {
        acc = match (&acc, a) {
            (Value::Int(x), Value::Int(y)) => match x.checked_sub(*y) {
                Some(r) => Value::Int(r),
                None => Value::Float(*x as f64 - *y as f64),
            },
            _ => Value::Float(num(&acc, "-")? - num(a, "-")?),
        };
    }
    Ok(acc)
}

fn div(_: &mut Interp, args: Vec<Value>) -> Result<Value> {
    let (mut acc, rest) = if args.len() == 1 { (Value::Int(1), &args[..]) } else { (args[0].clone(), &args[1..]) };
    for a in rest {
        acc = match (&acc, a) {
            (Value::Int(_), Value::Int(0)) => return err("Numerical overflow: division by zero"),
            (Value::Int(x), Value::Int(y)) if x % y == 0 => Value::Int(x / y),
            _ => Value::Float(num(&acc, "/")? / num(a, "/")?),
        };
    }
    Ok(acc)
}

fn compare(args: &[Value], who: &str, ok: fn(f64, f64) -> bool) -> Result<Value> {
    for w in args.windows(2) {
        let (a, b) = (num(&w[0], who)?, num(&w[1], who)?);
        if !ok(a, b) {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn num_eq(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    compare(&a, "=", |x, y| x == y)
}
fn num_lt(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    compare(&a, "<", |x, y| x < y)
}
fn num_gt(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    compare(&a, ">", |x, y| x > y)
}
fn num_le(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    compare(&a, "<=", |x, y| x <= y)
}
fn num_ge(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    compare(&a, ">=", |x, y| x >= y)
}

fn quotient(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let (x, y) = (int(&a[0], "quotient")?, int(&a[1], "quotient")?);
    if y == 0 {
        return err("quotient: division by zero");
    }
    Ok(Value::Int(x / y))
}

fn remainder(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let (x, y) = (int(&a[0], "remainder")?, int(&a[1], "remainder")?);
    if y == 0 {
        return err("remainder: division by zero");
    }
    Ok(Value::Int(x % y))
}

fn modulo(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let (x, y) = (int(&a[0], "modulo")?, int(&a[1], "modulo")?);
    if y == 0 {
        return err("modulo: division by zero");
    }
    Ok(Value::Int(((x % y) + y) % y))
}

fn min_max(args: &[Value], who: &str, pick_first: fn(f64, f64) -> bool) -> Result<Value> {
    let mut best = args[0].clone();
    let mut inexact = matches!(best, Value::Float(_));
    for a in &args[1..] {
        inexact |= matches!(a, Value::Float(_));
        if pick_first(num(a, who)?, num(&best, who)?) {
            best = a.clone();
        }
    }
    Ok(if inexact { Value::Float(num(&best, who)?) } else { best })
}

fn min(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    min_max(&a, "min", |x, y| x < y)
}
fn max(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    min_max(&a, "max", |x, y| x > y)
}

fn abs(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(match &a[0] {
        Value::Int(i) => Value::Int(i.abs()),
        other => Value::Float(num(other, "abs")?.abs()),
    })
}

fn expt(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    match (&a[0], &a[1]) {
        (Value::Int(b), Value::Int(e)) if *e >= 0 => match b.checked_pow(*e as u32) {
            Some(r) => Ok(Value::Int(r)),
            None => Ok(Value::Float((*b as f64).powf(*e as f64))),
        },
        (b, e) => Ok(Value::Float(num(b, "expt")?.powf(num(e, "expt")?))),
    }
}

fn logand(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut acc = -1i64;
    for v in &a {
        acc &= int(v, "logand")?;
    }
    Ok(Value::Int(acc))
}

fn round_like(a: &[Value], who: &str, f: fn(f64) -> f64) -> Result<Value> {
    Ok(match &a[0] {
        Value::Int(i) => Value::Int(*i),
        other => Value::Float(f(num(other, who)?)),
    })
}

fn round(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    // Scheme rounds half to even.
    round_like(&a, "round", |x| {
        let r = x.round();
        if (x - x.trunc()).abs() == 0.5 { 2.0 * (x / 2.0).round() } else { r }
    })
}
fn floor(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    round_like(&a, "floor", f64::floor)
}
fn ceiling(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    round_like(&a, "ceiling", f64::ceil)
}
fn truncate(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    round_like(&a, "truncate", f64::trunc)
}

fn inexact_to_exact(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(match &a[0] {
        Value::Float(f) if f.fract() == 0.0 => Value::Int(*f as i64),
        other => other.clone(),
    })
}

fn exact_to_inexact(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Float(num(&a[0], "exact->inexact")?))
}

fn predicate(a: &[Value], f: impl Fn(&Value) -> bool) -> Result<Value> {
    Ok(Value::Bool(f(&a[0])))
}

fn zero_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(num(&a[0], "zero?")? == 0.0))
}
fn positive_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(num(&a[0], "positive?")? > 0.0))
}
fn negative_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(num(&a[0], "negative?")? < 0.0))
}
fn even_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(int(&a[0], "even?")? % 2 == 0))
}
fn odd_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(int(&a[0], "odd?")? % 2 != 0))
}
fn number_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Int(_) | Value::Float(_)))
}
fn integer_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| match v {
        Value::Int(_) => true,
        Value::Float(f) => f.fract() == 0.0,
        _ => false,
    })
}
fn one_plus(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    add(i, vec![a[0].clone(), Value::Int(1)])
}
fn one_minus(i: &mut Interp, a: Vec<Value>) -> Result<Value> {
    sub(i, vec![a[0].clone(), Value::Int(1)])
}

fn number_to_string(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    num(&a[0], "number->string")?;
    Ok(Value::str(&to_string(&a[0], false)))
}

fn string_to_number(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let s = string_arg(&a[0], "string->number")?;
    Ok(super::reader::parse_number(s.trim()).unwrap_or(Value::Bool(false)))
}

// ---- equivalence and types ----

fn not(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(!a[0].is_true()))
}
fn eq_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(eqv(&a[0], &a[1])))
}
fn equal_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(equal(&a[0], &a[1])))
}
fn null_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, Value::is_null)
}
fn pair_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Pair(_)))
}
fn list_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, Value::is_list)
}
fn boolean_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Bool(_)))
}
fn string_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Str(_)))
}
fn symbol_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Sym(_)))
}
fn vector_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, |v| matches!(v, Value::Vector(_)))
}
fn procedure_p(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    predicate(&a, Value::is_procedure)
}

// ---- pairs and lists ----

fn cons(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::cons(a[0].clone(), a[1].clone()))
}

fn car(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    a[0].car().ok_or_else(|| Error::new(format!("car: wrong type argument {:?}", a[0])))
}

fn cdr(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    a[0].cdr().ok_or_else(|| Error::new(format!("cdr: wrong type argument {:?}", a[0])))
}

/// c[ad]+r: apply the letters from right to left.
fn cxr(path: &str, v: &Value) -> Result<Value> {
    let mut cur = v.clone();
    for c in path.chars().rev() {
        cur = match c {
            'a' => cur.car(),
            _ => cur.cdr(),
        }
        .ok_or_else(|| Error::new(format!("c{path}r: wrong type argument {v:?}")))?;
    }
    Ok(cur)
}

macro_rules! cxr_fns {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(fn $name(_: &mut Interp, a: Vec<Value>) -> Result<Value> { cxr($path, &a[0]) })*
        const CXR: &[(&str, super::value::BuiltinFn)] = &[$((concat!("c", $path, "r"), $name)),*];
    };
}

cxr_fns! {
    caar => "aa", cadr => "ad", cdar => "da", cddr => "dd",
    caaar => "aaa", caadr => "aad", cadar => "ada", caddr => "add",
    cdaar => "daa", cdadr => "dad", cddar => "dda", cdddr => "ddd",
    caaaar => "aaaa", caaadr => "aaad", caadar => "aada", caaddr => "aadd",
    cadaar => "adaa", cadadr => "adad", caddar => "adda", cadddr => "addd",
    cdaaar => "daaa", cdaadr => "daad", cdadar => "dada", cdaddr => "dadd",
    cddaar => "ddaa", cddadr => "ddad", cdddar => "ddda", cddddr => "dddd",
    caddddr => "adddd",
}

fn set_car(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    *pair_arg(&a[0], "set-car!")?.car.borrow_mut() = a[1].clone();
    Ok(Value::Unspecified)
}

fn set_cdr(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    *pair_arg(&a[0], "set-cdr!")?.cdr.borrow_mut() = a[1].clone();
    Ok(Value::Unspecified)
}

fn list(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::list(a))
}

fn length(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Int(list_arg(&a[0], "length")?.len() as i64))
}

fn append(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let Some((last, init)) = a.split_last() else { return Ok(Value::Nil) };
    let mut result = last.clone();
    for l in init.iter().rev() {
        for item in list_arg(l, "append")?.into_iter().rev() {
            result = Value::cons(item, result);
        }
    }
    Ok(result)
}

fn reverse(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(list_arg(&a[0], "reverse")?.into_iter().fold(Value::Nil, |acc, v| Value::cons(v, acc)))
}

fn list_tail(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut cur = a[0].clone();
    for _ in 0..int(&a[1], "list-tail")? {
        cur = cur.cdr().ok_or_else(|| Error::new("list-tail: index out of range"))?;
    }
    Ok(cur)
}

fn list_head(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let n = int(&a[1], "list-head")? as usize;
    let items: Vec<Value> = a[0].iter().take(n).collect();
    if items.len() < n {
        return err("list-head: index out of range");
    }
    Ok(Value::list(items))
}

fn list_ref(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let n = int(&a[1], "list-ref")?;
    a[0].iter().nth(n as usize).ok_or_else(|| Error::new(format!("list-ref: index {n} out of range")))
}

fn list_set(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut cur = a[0].clone();
    for _ in 0..int(&a[1], "list-set!")? {
        cur = cur.cdr().ok_or_else(|| Error::new("list-set!: index out of range"))?;
    }
    *pair_arg(&cur, "list-set!")?.car.borrow_mut() = a[2].clone();
    Ok(Value::Unspecified)
}

fn last_pair(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut cur = a[0].clone();
    while let Some(next @ Value::Pair(_)) = cur.cdr() {
        cur = next;
    }
    Ok(cur)
}

fn last(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    list_arg(&a[0], "last")?.pop().ok_or_else(|| Error::new("last: empty list"))
}

fn drop_right(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut items = list_arg(&a[0], "drop-right")?;
    let n = int(&a[1], "drop-right")? as usize;
    items.truncate(items.len().saturating_sub(n));
    Ok(Value::list(items))
}

fn mem_generic(a: &[Value], same: fn(&Value, &Value) -> bool) -> Result<Value> {
    let mut cur = a[1].clone();
    while let Value::Pair(p) = &cur {
        if same(&a[0], &p.car.borrow()) {
            return Ok(cur.clone());
        }
        let next = p.cdr.borrow().clone();
        cur = next;
    }
    Ok(Value::Bool(false))
}

fn member(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    mem_generic(&a, equal)
}
fn memv(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    mem_generic(&a, eqv)
}

fn ass_generic(a: &[Value], same: fn(&Value, &Value) -> bool) -> Result<Value> {
    for entry in a[1].iter() {
        if entry.car().is_some_and(|k| same(&a[0], &k)) {
            return Ok(entry);
        }
    }
    Ok(Value::Bool(false))
}

fn assoc(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    ass_generic(&a, equal)
}
fn assv(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    ass_generic(&a, eqv)
}

fn take_right(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let items = list_arg(&a[0], "take-right")?;
    let n = (int(&a[1], "take-right")? as usize).min(items.len());
    Ok(Value::list(items[items.len() - n..].to_vec()))
}

fn make_list(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let n = int(&a[0], "make-list")? as usize;
    let fill = a.get(1).cloned().unwrap_or(Value::Unspecified);
    Ok(Value::list(vec![fill; n]))
}

/// SRFI-1 find: the first element satisfying the predicate, or #f.
fn find(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    for item in list_arg(&a[1], "find")? {
        if interp.apply(&a[0], vec![item.clone()])?.is_true() {
            return Ok(item);
        }
    }
    Ok(Value::Bool(false))
}

fn delete(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::list(list_arg(&a[1], "delete")?.into_iter().filter(|v| !equal(&a[0], v)).collect::<Vec<_>>()))
}

fn list_copy(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::list(list_arg(&a[0], "list-copy")?))
}

fn copy_tree_fn(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(copy_tree(&a[0]))
}

fn iota(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let n = int(&a[0], "iota")?;
    let start = a.get(1).map(|v| int(v, "iota")).transpose()?.unwrap_or(0);
    let step = a.get(2).map(|v| int(v, "iota")).transpose()?.unwrap_or(1);
    Ok(Value::list((0..n).map(|i| Value::Int(start + i * step)).collect::<Vec<_>>()))
}

// ---- higher order ----

/// The argument lists of a multi-list map, cut to the shortest list.
fn zip_lists(lists: &[Value], who: &str) -> Result<Vec<Vec<Value>>> {
    let lists: Vec<Vec<Value>> = lists.iter().map(|l| list_arg(l, who)).collect::<Result<_>>()?;
    let n = lists.iter().map(Vec::len).min().unwrap_or(0);
    Ok((0..n).map(|i| lists.iter().map(|l| l[i].clone()).collect()).collect())
}

fn map(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut out = Vec::new();
    for args in zip_lists(&a[1..], "map")? {
        out.push(interp.apply(&a[0], args)?);
    }
    Ok(Value::list(out))
}

fn for_each(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    for args in zip_lists(&a[1..], "for-each")? {
        interp.apply(&a[0], args)?;
    }
    Ok(Value::Unspecified)
}

fn append_map(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut lists = Vec::new();
    for args in zip_lists(&a[1..], "append-map")? {
        lists.push(interp.apply(&a[0], args)?);
    }
    append(interp, lists)
}

/// or-map / any: the first true result.
fn or_map(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    for args in zip_lists(&a[1..], "or-map")? {
        let v = interp.apply(&a[0], args)?;
        if v.is_true() {
            return Ok(v);
        }
    }
    Ok(Value::Bool(false))
}

/// and-map / every: false at the first false result, else the last result.
fn and_map(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut last = Value::Bool(true);
    for args in zip_lists(&a[1..], "and-map")? {
        last = interp.apply(&a[0], args)?;
        if !last.is_true() {
            return Ok(Value::Bool(false));
        }
    }
    Ok(last)
}

fn filter(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut out = Vec::new();
    for item in list_arg(&a[1], "filter")? {
        if interp.apply(&a[0], vec![item.clone()])?.is_true() {
            out.push(item);
        }
    }
    Ok(Value::list(out))
}

fn apply(interp: &mut Interp, mut a: Vec<Value>) -> Result<Value> {
    let f = a.remove(0);
    let last = a.pop().unwrap_or(Value::Nil);
    a.extend(list_arg(&last, "apply")?);
    interp.apply(&f, a)
}

fn sort(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let (items, as_vector) = match &a[0] {
        Value::Vector(v) => (v.borrow().clone(), true),
        other => (list_arg(other, "sort")?, false),
    };
    // Stable merge sort calling the Scheme comparison.
    let sorted = merge_sort(interp, items, &a[1])?;
    Ok(if as_vector { Value::Vector(Rc::new(RefCell::new(sorted))) } else { Value::list(sorted) })
}

fn merge_sort(interp: &mut Interp, items: Vec<Value>, less: &Value) -> Result<Vec<Value>> {
    if items.len() <= 1 {
        return Ok(items);
    }
    let mut left = items;
    let right = left.split_off(left.len() / 2);
    let left = merge_sort(interp, left, less)?;
    let right = merge_sort(interp, right, less)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if interp.apply(less, vec![right[j].clone(), left[i].clone()])?.is_true() {
            out.push(right[j].clone());
            j += 1;
        } else {
            out.push(left[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    Ok(out)
}

// ---- vectors ----

fn vector_arg<'a>(v: &'a Value, who: &str) -> Result<&'a Rc<RefCell<Vec<Value>>>> {
    match v {
        Value::Vector(items) => Ok(items),
        other => err(format!("{who}: expected vector, got {other:?}")),
    }
}

fn vector(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Vector(Rc::new(RefCell::new(a))))
}

fn make_vector(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let n = int(&a[0], "make-vector")? as usize;
    let fill = a.get(1).cloned().unwrap_or(Value::Unspecified);
    Ok(Value::Vector(Rc::new(RefCell::new(vec![fill; n]))))
}

fn vector_ref(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let i = int(&a[1], "vector-ref")?;
    vector_arg(&a[0], "vector-ref")?
        .borrow()
        .get(i as usize)
        .cloned()
        .ok_or_else(|| Error::new(format!("vector-ref: index {i} out of range")))
}

fn vector_set(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let i = int(&a[1], "vector-set!")? as usize;
    let items = vector_arg(&a[0], "vector-set!")?;
    let mut items = items.borrow_mut();
    let slot = items.get_mut(i).ok_or_else(|| Error::new(format!("vector-set!: index {i} out of range")))?;
    *slot = a[2].clone();
    Ok(Value::Unspecified)
}

fn vector_length(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Int(vector_arg(&a[0], "vector-length")?.borrow().len() as i64))
}

fn vector_fill(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    for item in vector_arg(&a[0], "vector-fill!")?.borrow_mut().iter_mut() {
        *item = a[1].clone();
    }
    Ok(Value::Unspecified)
}

fn list_to_vector(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Vector(Rc::new(RefCell::new(list_arg(&a[0], "list->vector")?))))
}

fn vector_to_list(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::list(vector_arg(&a[0], "vector->list")?.borrow().clone()))
}

// ---- strings and symbols ----

fn string_append(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let mut out = String::new();
    for v in &a {
        out.push_str(&string_arg(v, "string-append")?);
    }
    Ok(Value::str(&out))
}

fn string_eq(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let first = string_arg(&a[0], "string=?")?;
    for v in &a[1..] {
        if *string_arg(v, "string=?")? != *first {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn string_lt(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Bool(string_arg(&a[0], "string<?")? < string_arg(&a[1], "string<?")?))
}

fn string_length(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::Int(string_arg(&a[0], "string-length")?.chars().count() as i64))
}

fn substring(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let s: Vec<char> = string_arg(&a[0], "substring")?.chars().collect();
    let start = int(&a[1], "substring")? as usize;
    let end = a.get(2).map(|v| int(v, "substring")).transpose()?.map(|e| e as usize).unwrap_or(s.len());
    if start > end || end > s.len() {
        return err("substring: index out of range");
    }
    Ok(Value::str(&s[start..end].iter().collect::<String>()))
}

fn symbol_to_string(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    match &a[0] {
        Value::Sym(s) => Ok(Value::str(&s.name())),
        other => err(format!("symbol->string: expected symbol, got {other:?}")),
    }
}

fn string_to_symbol(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::sym(&string_arg(&a[0], "string->symbol")?))
}

/// The part of (ice-9 format) the games use: ~a ~s ~d ~% ~~.
fn format(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let template = string_arg(&a[1], "format")?;
    let mut args = a[2..].iter();
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != '~' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('a' | 'A' | 'd' | 'D') => {
                out.push_str(&to_string(args.next().ok_or_else(|| Error::new("format: too few arguments"))?, false))
            }
            Some('s' | 'S') => {
                out.push_str(&to_string(args.next().ok_or_else(|| Error::new("format: too few arguments"))?, true))
            }
            Some('%') => out.push('\n'),
            Some('~') => out.push('~'),
            other => return err(format!("format: unsupported directive ~{}", other.unwrap_or(' '))),
        }
    }
    if a[0].is_true() {
        eprint!("{out}");
        Ok(Value::Unspecified)
    } else {
        Ok(Value::str(&out))
    }
}

fn display(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    if std::env::var_os("RUSTY_SCHEME_DEBUG").is_some() {
        eprint!("{}", to_string(&a[0], false));
    }
    Ok(Value::Unspecified)
}

fn newline(_: &mut Interp, _: Vec<Value>) -> Result<Value> {
    if std::env::var_os("RUSTY_SCHEME_DEBUG").is_some() {
        eprintln!();
    }
    Ok(Value::Unspecified)
}

fn identity(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(a[0].clone())
}

fn number_to_locale_string(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    Ok(Value::str(&to_string(&a[0], false)))
}

fn unspecified(_: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Unspecified)
}

fn error(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let parts: Vec<String> = a.iter().map(|v| to_string(v, false)).collect();
    err(parts.join(" "))
}

fn values(_: &mut Interp, mut a: Vec<Value>) -> Result<Value> {
    Ok(if a.len() == 1 { a.remove(0) } else { Value::Values(Rc::new(a)) })
}

fn call_with_values(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let produced = interp.apply(&a[0], Vec::new())?;
    let args = match produced {
        Value::Values(v) => v.as_ref().clone(),
        single => vec![single],
    };
    interp.apply(&a[1], args)
}

/// (primitive-load-path "osmosis"), (load "api.scm"): run another rule file.
fn primitive_load(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let name = string_arg(&a[0], "load")?;
    interp.load_file(&name)?;
    Ok(Value::Unspecified)
}

/// (eval expr module): the module is ignored; everything is global.
fn eval(interp: &mut Interp, a: Vec<Value>) -> Result<Value> {
    interp.eval(&a[0], &None)
}

// ---- hash tables (FreeCell's solver) ----

fn make_hash_table(_: &mut Interp, _: Vec<Value>) -> Result<Value> {
    Ok(Value::Hash(Rc::new(RefCell::new(HashMap::new()))))
}

fn hash_arg<'a>(v: &'a Value) -> Result<&'a Rc<RefCell<HashMap<String, Value>>>> {
    match v {
        Value::Hash(h) => Ok(h),
        other => err(format!("expected hash table, got {other:?}")),
    }
}

fn hash_ref(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    let key = to_string(&a[1], true);
    Ok(hash_arg(&a[0])?.borrow().get(&key).cloned().unwrap_or_else(|| a.get(2).cloned().unwrap_or(Value::Bool(false))))
}

fn hash_set(_: &mut Interp, a: Vec<Value>) -> Result<Value> {
    hash_arg(&a[0])?.borrow_mut().insert(to_string(&a[1], true), a[2].clone());
    Ok(Value::Unspecified)
}

pub fn install(i: &mut Interp) {
    let any = None;
    i.define_builtin("+", 0, any, add);
    i.define_builtin("*", 0, any, mul);
    i.define_builtin("-", 1, any, sub);
    i.define_builtin("/", 1, any, div);
    i.define_builtin("=", 1, any, num_eq);
    i.define_builtin("<", 1, any, num_lt);
    i.define_builtin(">", 1, any, num_gt);
    i.define_builtin("<=", 1, any, num_le);
    i.define_builtin(">=", 1, any, num_ge);
    i.define_builtin("quotient", 2, Some(2), quotient);
    i.define_builtin("remainder", 2, Some(2), remainder);
    i.define_builtin("modulo", 2, Some(2), modulo);
    i.define_builtin("min", 1, any, min);
    i.define_builtin("max", 1, any, max);
    i.define_builtin("abs", 1, Some(1), abs);
    i.define_builtin("expt", 2, Some(2), expt);
    i.define_builtin("integer-expt", 2, Some(2), expt);
    i.define_builtin("logand", 0, any, logand);
    i.define_builtin("round", 1, Some(1), round);
    i.define_builtin("floor", 1, Some(1), floor);
    i.define_builtin("ceiling", 1, Some(1), ceiling);
    i.define_builtin("truncate", 1, Some(1), truncate);
    i.define_builtin("inexact->exact", 1, Some(1), inexact_to_exact);
    i.define_builtin("exact->inexact", 1, Some(1), exact_to_inexact);
    i.define_builtin("zero?", 1, Some(1), zero_p);
    i.define_builtin("positive?", 1, Some(1), positive_p);
    i.define_builtin("negative?", 1, Some(1), negative_p);
    i.define_builtin("even?", 1, Some(1), even_p);
    i.define_builtin("odd?", 1, Some(1), odd_p);
    i.define_builtin("number?", 1, Some(1), number_p);
    i.define_builtin("integer?", 1, Some(1), integer_p);
    i.define_builtin("1+", 1, Some(1), one_plus);
    i.define_builtin("1-", 1, Some(1), one_minus);
    i.define_builtin("number->string", 1, Some(2), number_to_string);
    i.define_builtin("string->number", 1, Some(2), string_to_number);
    i.define_builtin("number->locale-string", 1, Some(2), number_to_locale_string);

    i.define_builtin("not", 1, Some(1), not);
    i.define_builtin("eq?", 2, Some(2), eq_p);
    i.define_builtin("eqv?", 2, Some(2), eq_p);
    i.define_builtin("equal?", 2, Some(2), equal_p);
    i.define_builtin("null?", 1, Some(1), null_p);
    i.define_builtin("nil?", 1, Some(1), null_p);
    i.define_builtin("pair?", 1, Some(1), pair_p);
    i.define_builtin("list?", 1, Some(1), list_p);
    i.define_builtin("boolean?", 1, Some(1), boolean_p);
    i.define_builtin("string?", 1, Some(1), string_p);
    i.define_builtin("symbol?", 1, Some(1), symbol_p);
    i.define_builtin("vector?", 1, Some(1), vector_p);
    i.define_builtin("procedure?", 1, Some(1), procedure_p);

    i.define_builtin("cons", 2, Some(2), cons);
    i.define_builtin("car", 1, Some(1), car);
    i.define_builtin("cdr", 1, Some(1), cdr);
    for (name, f) in CXR {
        i.define_builtin(name, 1, Some(1), *f);
    }
    i.define_builtin("set-car!", 2, Some(2), set_car);
    i.define_builtin("set-cdr!", 2, Some(2), set_cdr);
    i.define_builtin("list", 0, any, list);
    i.define_builtin("length", 1, Some(1), length);
    i.define_builtin("append", 0, any, append);
    i.define_builtin("append!", 0, any, append);
    i.define_builtin("reverse", 1, Some(1), reverse);
    i.define_builtin("reverse!", 1, Some(1), reverse);
    i.define_builtin("list-tail", 2, Some(2), list_tail);
    i.define_builtin("list-head", 2, Some(2), list_head);
    i.define_builtin("take", 2, Some(2), list_head);
    i.define_builtin("drop", 2, Some(2), list_tail);
    i.define_builtin("list-ref", 2, Some(2), list_ref);
    i.define_builtin("list-set!", 3, Some(3), list_set);
    i.define_builtin("last-pair", 1, Some(1), last_pair);
    i.define_builtin("last", 1, Some(1), last);
    i.define_builtin("drop-right", 2, Some(2), drop_right);
    i.define_builtin("take-right", 2, Some(2), take_right);
    i.define_builtin("make-list", 1, Some(2), make_list);
    i.define_builtin("find", 2, Some(2), find);
    i.define_builtin("member", 2, Some(2), member);
    i.define_builtin("memv", 2, Some(2), memv);
    i.define_builtin("memq", 2, Some(2), memv);
    i.define_builtin("assoc", 2, Some(2), assoc);
    i.define_builtin("assv", 2, Some(2), assv);
    i.define_builtin("assq", 2, Some(2), assv);
    i.define_builtin("delete", 2, Some(2), delete);
    i.define_builtin("delete!", 2, Some(2), delete);
    i.define_builtin("list-copy", 1, Some(1), list_copy);
    i.define_builtin("copy-tree", 1, Some(1), copy_tree_fn);
    i.define_builtin("iota", 1, Some(3), iota);

    i.define_builtin("map", 2, any, map);
    i.define_builtin("for-each", 2, any, for_each);
    i.define_builtin("append-map", 2, any, append_map);
    i.define_builtin("or-map", 2, any, or_map);
    i.define_builtin("any", 2, any, or_map);
    i.define_builtin("and-map", 2, any, and_map);
    i.define_builtin("every", 2, any, and_map);
    i.define_builtin("filter", 2, Some(2), filter);
    i.define_builtin("apply", 2, any, apply);
    i.define_builtin("sort", 2, Some(2), sort);
    i.define_builtin("sort!", 2, Some(2), sort);
    i.define_builtin("stable-sort", 2, Some(2), sort);

    i.define_builtin("vector", 0, any, vector);
    i.define_builtin("make-vector", 1, Some(2), make_vector);
    i.define_builtin("vector-ref", 2, Some(2), vector_ref);
    i.define_builtin("vector-set!", 3, Some(3), vector_set);
    i.define_builtin("vector-length", 1, Some(1), vector_length);
    i.define_builtin("vector-fill!", 2, Some(2), vector_fill);
    i.define_builtin("list->vector", 1, Some(1), list_to_vector);
    i.define_builtin("vector->list", 1, Some(1), vector_to_list);

    i.define_builtin("string-append", 0, any, string_append);
    i.define_builtin("string=?", 1, any, string_eq);
    i.define_builtin("string<?", 2, Some(2), string_lt);
    i.define_builtin("string-length", 1, Some(1), string_length);
    i.define_builtin("substring", 2, Some(3), substring);
    i.define_builtin("symbol->string", 1, Some(1), symbol_to_string);
    i.define_builtin("string->symbol", 1, Some(1), string_to_symbol);
    i.define_builtin("format", 2, any, format);
    i.define_builtin("display", 1, Some(2), display);
    i.define_builtin("write", 1, Some(2), display);
    i.define_builtin("newline", 0, Some(1), newline);
    i.define_builtin("gettext", 1, Some(2), identity);
    i.define_builtin("error", 0, any, error);
    i.define_builtin("values", 0, any, values);
    i.define_builtin("call-with-values", 2, Some(2), call_with_values);
    i.define_builtin("eval", 1, Some(2), eval);
    i.define_builtin("primitive-load-path", 1, Some(2), primitive_load);
    i.define_builtin("load", 1, Some(1), primitive_load);
    i.define_builtin("current-module", 0, Some(0), unspecified);
    i.define_builtin("fluid-set!", 2, Some(2), unspecified);

    i.define_builtin("make-hash-table", 0, Some(1), make_hash_table);
    i.define_builtin("hash-ref", 2, Some(3), hash_ref);
    i.define_builtin("hash-set!", 3, Some(3), hash_set);

    i.set_global(super::Sym::new("%default-port-encoding"), Value::Unspecified);
}
