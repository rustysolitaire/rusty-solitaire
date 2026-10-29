// Rusty Solitaire - scheme/mod.rs
//
// A small Scheme interpreter that runs Aisleriot's original game rule files
// (games/*.scm and api.scm). It covers the subset of Guile those files use,
// with proper tail calls so loops written as recursion don't grow the stack.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

mod builtins;
mod reader;
pub mod value;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

pub use value::{Sym, Value};
use value::{Builtin, BuiltinFn, Lambda};

#[derive(Clone, Debug)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error { message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::new(message))
}

/// A local scope. `None` is the global scope.
pub type Env = Option<Rc<Frame>>;

pub struct Frame {
    vars: RefCell<Vec<(Sym, Value)>>,
    parent: Env,
}

fn new_frame(parent: &Env, vars: Vec<(Sym, Value)>) -> Env {
    Some(Rc::new(Frame { vars: RefCell::new(vars), parent: parent.clone() }))
}

/// Symbols of the special forms, interned once.
struct Forms {
    quote: Sym,
    quasiquote: Sym,
    unquote: Sym,
    unquote_splicing: Sym,
    if_: Sym,
    define: Sym,
    define_public: Sym,
    set: Sym,
    lambda: Sym,
    let_: Sym,
    let_star: Sym,
    letrec: Sym,
    letrec_star: Sym,
    begin: Sym,
    cond: Sym,
    case: Sym,
    and: Sym,
    or: Sym,
    when: Sym,
    unless: Sym,
    do_: Sym,
    while_: Sym,
    else_: Sym,
    arrow: Sym,
    defmacro: Sym,
    defmacro_public: Sym,
    define_macro: Sym,
    define_module: Sym,
    use_modules: Sym,
}

impl Forms {
    fn new() -> Self {
        Forms {
            quote: Sym::new("quote"),
            quasiquote: Sym::new("quasiquote"),
            unquote: Sym::new("unquote"),
            unquote_splicing: Sym::new("unquote-splicing"),
            if_: Sym::new("if"),
            define: Sym::new("define"),
            define_public: Sym::new("define-public"),
            set: Sym::new("set!"),
            lambda: Sym::new("lambda"),
            let_: Sym::new("let"),
            let_star: Sym::new("let*"),
            letrec: Sym::new("letrec"),
            letrec_star: Sym::new("letrec*"),
            begin: Sym::new("begin"),
            cond: Sym::new("cond"),
            case: Sym::new("case"),
            and: Sym::new("and"),
            or: Sym::new("or"),
            when: Sym::new("when"),
            unless: Sym::new("unless"),
            do_: Sym::new("do"),
            while_: Sym::new("while"),
            else_: Sym::new("else"),
            arrow: Sym::new("=>"),
            defmacro: Sym::new("defmacro"),
            defmacro_public: Sym::new("defmacro-public"),
            define_macro: Sym::new("define-macro"),
            define_module: Sym::new("define-module"),
            use_modules: Sym::new("use-modules"),
        }
    }
}

/// Finds the source of a file named by `load` / `primitive-load-path`.
pub type SourceLoader = Box<dyn Fn(&str) -> Option<String>>;

pub struct Interp {
    globals: Vec<Option<Value>>,
    macros: HashMap<Sym, Rc<Lambda>>,
    forms: Forms,
    loader: SourceLoader,
    /// State owned by whoever embeds the interpreter (the game bridge).
    pub host: Box<dyn Any>,
    /// Guards against runaway recursion in buggy rule code.
    depth: usize,
}

const MAX_DEPTH: usize = 200_000;

impl Interp {
    pub fn new(loader: SourceLoader, host: Box<dyn Any>) -> Self {
        let mut interp = Interp {
            globals: Vec::new(),
            macros: HashMap::new(),
            forms: Forms::new(),
            loader,
            host,
            depth: 0,
        };
        builtins::install(&mut interp);
        interp
    }

    pub fn define_builtin(&mut self, name: &'static str, min_args: usize, max_args: Option<usize>, func: BuiltinFn) {
        let b = Builtin { name, func, min_args, max_args };
        self.set_global(Sym::new(name), Value::Builtin(Rc::new(b)));
    }

    pub fn set_global(&mut self, sym: Sym, value: Value) {
        let i = sym.0 as usize;
        if i >= self.globals.len() {
            self.globals.resize(i + 1, None);
        }
        self.globals[i] = Some(value);
    }

    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.get(Sym::new(name).0 as usize).cloned().flatten()
    }

    fn lookup(&self, sym: Sym, env: &Env) -> Result<Value> {
        let mut frame = env.clone();
        while let Some(f) = frame {
            if let Some((_, v)) = f.vars.borrow().iter().rev().find(|(s, _)| *s == sym) {
                return Ok(v.clone());
            }
            frame = f.parent.clone();
        }
        match self.globals.get(sym.0 as usize) {
            Some(Some(v)) => Ok(v.clone()),
            _ => err(format!("Unbound variable: {}", sym.name())),
        }
    }

    fn define(&mut self, sym: Sym, value: Value, env: &Env) {
        match env {
            None => self.set_global(sym, value),
            Some(f) => {
                let mut vars = f.vars.borrow_mut();
                if let Some(slot) = vars.iter_mut().rev().find(|(s, _)| *s == sym) {
                    slot.1 = value;
                } else {
                    vars.push((sym, value));
                }
            }
        }
    }

    fn set(&mut self, sym: Sym, value: Value, env: &Env) -> Result<()> {
        let mut frame = env.clone();
        while let Some(f) = frame {
            if let Some(slot) = f.vars.borrow_mut().iter_mut().rev().find(|(s, _)| *s == sym) {
                slot.1 = value;
                return Ok(());
            }
            frame = f.parent.clone();
        }
        match self.globals.get(sym.0 as usize) {
            Some(Some(_)) => {
                self.set_global(sym, value);
                Ok(())
            }
            _ => err(format!("Unbound variable in set!: {}", sym.name())),
        }
    }

    /// Load and run a whole source file.
    pub fn load_source(&mut self, src: &str, file: &str) -> Result<()> {
        let forms = reader::Reader::new(src, file).read_all()?;
        for form in forms {
            self.eval(&form, &None).map_err(|e| Error::new(format!("{file}: {}", e.message)))?;
        }
        Ok(())
    }

    pub fn load_file(&mut self, name: &str) -> Result<()> {
        let src = (self.loader)(name).ok_or_else(|| Error::new(format!("cannot find {name}")))?;
        self.load_source(&src, name)
    }

    /// Evaluate a string in the global scope (used by tests and the bridge).
    pub fn eval_str(&mut self, src: &str) -> Result<Value> {
        let forms = reader::Reader::new(src, "<eval>").read_all()?;
        let mut result = Value::Unspecified;
        for form in forms {
            result = self.eval(&form, &None)?;
        }
        Ok(result)
    }

    /// Call a procedure value with arguments.
    pub fn apply(&mut self, f: &Value, args: Vec<Value>) -> Result<Value> {
        match f {
            Value::Builtin(b) => self.call_builtin(b, args),
            Value::Lambda(l) => {
                let env = self.bind(l, args)?;
                self.eval_body(&l.body, &env)
            }
            other => err(format!("Wrong type to apply: {other:?}")),
        }
    }

    fn call_builtin(&mut self, b: &Builtin, args: Vec<Value>) -> Result<Value> {
        if args.len() < b.min_args || b.max_args.is_some_and(|m| args.len() > m) {
            return err(format!("Wrong number of arguments to {}", b.name));
        }
        (b.func)(self, args)
    }

    fn bind(&self, l: &Lambda, mut args: Vec<Value>) -> Result<Env> {
        let n = l.params.len();
        if args.len() < n || (l.rest.is_none() && args.len() > n) {
            let name = l.name.map(|s| s.name().to_string()).unwrap_or_else(|| "#<lambda>".into());
            return err(format!("Wrong number of arguments to {name}: expected {n}, got {}", args.len()));
        }
        let rest_args = args.split_off(n);
        let mut vars: Vec<(Sym, Value)> = l.params.iter().copied().zip(args).collect();
        if let Some(rest) = l.rest {
            vars.push((rest, Value::list(rest_args)));
        }
        Ok(new_frame(&l.env, vars))
    }

    fn eval_body(&mut self, body: &[Value], env: &Env) -> Result<Value> {
        let mut result = Value::Unspecified;
        for expr in body {
            result = self.eval(expr, env)?;
        }
        Ok(result)
    }

    fn make_lambda(&self, name: Option<Sym>, params: &Value, body: Value, env: &Env) -> Result<Value> {
        let mut fixed = Vec::new();
        let mut cur = params.clone();
        let rest = loop {
            match cur {
                Value::Nil => break None,
                Value::Sym(s) => break Some(s),
                Value::Pair(p) => {
                    let param = p.car.borrow().as_sym().ok_or_else(|| Error::new("bad parameter list"))?;
                    fixed.push(param);
                    let next = p.cdr.borrow().clone();
                    cur = next;
                }
                _ => return err("bad parameter list"),
            }
        };
        let body = body.to_vec().ok_or_else(|| Error::new("bad lambda body"))?;
        Ok(Value::Lambda(Rc::new(Lambda { name, params: fixed, rest, body: Rc::new(body), env: env.clone() })))
    }

    pub fn eval(&mut self, expr: &Value, env: &Env) -> Result<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return err("Stack overflow (recursion too deep)");
        }
        // Grow the stack on demand: rule files recurse over card lists.
        let result = stacker::maybe_grow(128 * 1024, 4 * 1024 * 1024, || self.eval_inner(expr.clone(), env.clone()));
        self.depth -= 1;
        result
    }

    fn eval_inner(&mut self, mut expr: Value, mut env: Env) -> Result<Value> {
        loop {
            let (head, rest) = match &expr {
                Value::Sym(s) => return self.lookup(*s, &env),
                Value::Pair(p) => (p.car.borrow().clone(), p.cdr.borrow().clone()),
                Value::Nil => return err("Missing procedure in application ()"),
                other => return Ok(other.clone()),
            };

            if let Value::Sym(s) = head {
                let f = &self.forms;
                // ---- special forms; tail positions `continue` the loop ----
                if s == f.quote {
                    return Ok(arg(&rest, 0)?);
                } else if s == f.if_ {
                    let test = self.eval(&arg(&rest, 0)?, &env)?;
                    expr = if test.is_true() {
                        arg(&rest, 1)?
                    } else {
                        match nth(&rest, 2) {
                            Some(e) => e,
                            None => return Ok(Value::Unspecified),
                        }
                    };
                    continue;
                } else if s == f.define || s == f.define_public {
                    let target = arg(&rest, 0)?;
                    match target {
                        Value::Sym(name) => {
                            let value = match nth(&rest, 1) {
                                Some(e) => self.eval(&e, &env)?,
                                None => Value::Unspecified,
                            };
                            let value = name_lambda(value, name);
                            self.define(name, value, &env);
                        }
                        Value::Pair(p) => {
                            let name = p.car.borrow().as_sym().ok_or_else(|| Error::new("bad define"))?;
                            let params = p.cdr.borrow().clone();
                            let lambda = self.make_lambda(Some(name), &params, tail(&rest), &env)?;
                            self.define(name, lambda, &env);
                        }
                        _ => return err("bad define"),
                    }
                    return Ok(Value::Unspecified);
                } else if s == f.set {
                    let name = arg(&rest, 0)?.as_sym().ok_or_else(|| Error::new("bad set!"))?;
                    let value = self.eval(&arg(&rest, 1)?, &env)?;
                    self.set(name, value, &env)?;
                    return Ok(Value::Unspecified);
                } else if s == f.lambda {
                    return self.make_lambda(None, &arg(&rest, 0)?, tail(&rest), &env);
                } else if s == f.begin {
                    let body = rest.to_vec().ok_or_else(|| Error::new("bad begin"))?;
                    let Some((last, init)) = body.split_last() else { return Ok(Value::Unspecified) };
                    for e in init {
                        self.eval(e, &env)?;
                    }
                    expr = last.clone();
                    continue;
                } else if s == f.let_ {
                    let first = arg(&rest, 0)?;
                    if let Value::Sym(loop_name) = first {
                        // Named let.
                        let bindings = arg(&rest, 1)?;
                        let (names, inits) = self.split_bindings(&bindings)?;
                        let mut args = Vec::with_capacity(inits.len());
                        for init in &inits {
                            args.push(self.eval(init, &env)?);
                        }
                        let loop_env = new_frame(&env, Vec::new());
                        let params = Value::list(names.iter().map(|s| Value::Sym(*s)).collect::<Vec<_>>());
                        let body = tail(&tail(&rest));
                        let lambda = self.make_lambda(Some(loop_name), &params, body, &loop_env)?;
                        self.define(loop_name, lambda.clone(), &loop_env);
                        let Value::Lambda(l) = lambda else { unreachable!() };
                        env = self.bind(&l, args)?;
                        expr = self.body_tail(&l.body, &env)?;
                        continue;
                    }
                    let (names, inits) = self.split_bindings(&first)?;
                    let mut vars = Vec::with_capacity(names.len());
                    for (name, init) in names.into_iter().zip(inits) {
                        vars.push((name, self.eval(&init, &env)?));
                    }
                    env = new_frame(&env, vars);
                    expr = self.body_tail_list(&tail(&rest), &env)?;
                    continue;
                } else if s == f.let_star || s == f.letrec || s == f.letrec_star {
                    let (names, inits) = self.split_bindings(&arg(&rest, 0)?)?;
                    let frame = new_frame(&env, Vec::new());
                    if s != f.let_star {
                        for name in &names {
                            self.define(*name, Value::Unspecified, &frame);
                        }
                    }
                    for (name, init) in names.into_iter().zip(inits) {
                        let value = self.eval(&init, &frame)?;
                        let value = name_lambda(value, name);
                        self.define(name, value, &frame);
                    }
                    env = frame;
                    expr = self.body_tail_list(&tail(&rest), &env)?;
                    continue;
                } else if s == f.and {
                    let items = rest.to_vec().unwrap_or_default();
                    let Some((last, init)) = items.split_last() else { return Ok(Value::Bool(true)) };
                    for e in init {
                        if !self.eval(e, &env)?.is_true() {
                            return Ok(Value::Bool(false));
                        }
                    }
                    expr = last.clone();
                    continue;
                } else if s == f.or {
                    let items = rest.to_vec().unwrap_or_default();
                    let Some((last, init)) = items.split_last() else { return Ok(Value::Bool(false)) };
                    for e in init {
                        let v = self.eval(e, &env)?;
                        if v.is_true() {
                            return Ok(v);
                        }
                    }
                    expr = last.clone();
                    continue;
                } else if s == f.cond {
                    let mut next = None;
                    for clause in rest.iter() {
                        let test_expr = clause.car().ok_or_else(|| Error::new("bad cond clause"))?;
                        let body = tail(&clause);
                        let test = if test_expr.as_sym() == Some(self.forms.else_) {
                            Value::Bool(true)
                        } else {
                            self.eval(&test_expr, &env)?
                        };
                        if !test.is_true() {
                            continue;
                        }
                        if body.is_null() {
                            return Ok(test);
                        }
                        if body.car().and_then(|v| v.as_sym()) == Some(self.forms.arrow) {
                            let proc_ = self.eval(&arg(&body, 1)?, &env)?;
                            return self.apply(&proc_, vec![test]);
                        }
                        next = Some(self.body_tail_list(&body, &env)?);
                        break;
                    }
                    match next {
                        Some(e) => {
                            expr = e;
                            continue;
                        }
                        None => return Ok(Value::Unspecified),
                    }
                } else if s == f.case {
                    let key = self.eval(&arg(&rest, 0)?, &env)?;
                    let mut next = None;
                    for clause in tail(&rest).iter() {
                        let data = clause.car().ok_or_else(|| Error::new("bad case clause"))?;
                        let hit = data.as_sym() == Some(self.forms.else_)
                            || data.iter().any(|d| value::eqv(&d, &key));
                        if hit {
                            next = Some(self.body_tail_list(&tail(&clause), &env)?);
                            break;
                        }
                    }
                    match next {
                        Some(e) => {
                            expr = e;
                            continue;
                        }
                        None => return Ok(Value::Unspecified),
                    }
                } else if s == f.when || s == f.unless {
                    let is_when = s == f.when;
                    let test = self.eval(&arg(&rest, 0)?, &env)?.is_true();
                    if test != is_when {
                        return Ok(Value::Unspecified);
                    }
                    expr = self.body_tail_list(&tail(&rest), &env)?;
                    continue;
                } else if s == f.do_ {
                    return self.eval_do(&rest, &env);
                } else if s == f.while_ {
                    let test = arg(&rest, 0)?;
                    let body = tail(&rest).to_vec().unwrap_or_default();
                    while self.eval(&test, &env)?.is_true() {
                        self.eval_body(&body, &env)?;
                    }
                    return Ok(Value::Unspecified);
                } else if s == f.quasiquote {
                    return self.quasi(&arg(&rest, 0)?, &env, 1);
                } else if s == f.defmacro || s == f.defmacro_public {
                    let name = arg(&rest, 0)?.as_sym().ok_or_else(|| Error::new("bad defmacro"))?;
                    let lambda = self.make_lambda(Some(name), &arg(&rest, 1)?, tail(&tail(&rest)), &env)?;
                    if let Value::Lambda(l) = lambda {
                        self.macros.insert(name, l);
                    }
                    return Ok(Value::Unspecified);
                } else if s == f.define_macro {
                    let sig = arg(&rest, 0)?;
                    let name = sig.car().and_then(|v| v.as_sym()).ok_or_else(|| Error::new("bad define-macro"))?;
                    let lambda = self.make_lambda(Some(name), &tail(&sig), tail(&rest), &env)?;
                    if let Value::Lambda(l) = lambda {
                        self.macros.insert(name, l);
                    }
                    return Ok(Value::Unspecified);
                } else if s == f.define_module || s == f.use_modules {
                    return Ok(Value::Unspecified);
                } else if let Some(m) = self.macros.get(&s).cloned() {
                    let args = rest.to_vec().ok_or_else(|| Error::new("bad macro call"))?;
                    let menv = self.bind(&m, args)?;
                    expr = self.eval_body(&m.body, &menv)?;
                    continue;
                }
            }

            // ---- procedure application ----
            let func = self.eval(&head, &env)?;
            let mut args = Vec::new();
            for a in rest.iter() {
                args.push(self.eval(&a, &env)?);
            }
            match &func {
                Value::Lambda(l) => {
                    env = self.bind(l, args)?;
                    expr = self.body_tail(&l.body, &env)?;
                }
                Value::Builtin(b) => return self.call_builtin(b, args),
                other => return err(format!("Wrong type to apply: {other:?} in {expr:?}")),
            }
        }
    }

    /// Evaluate all but the last body expression; return the last for a tail call.
    fn body_tail(&mut self, body: &[Value], env: &Env) -> Result<Value> {
        let Some((last, init)) = body.split_last() else { return Ok(Value::Unspecified) };
        for e in init {
            self.eval(e, env)?;
        }
        Ok(last.clone())
    }

    fn body_tail_list(&mut self, body: &Value, env: &Env) -> Result<Value> {
        let body = body.to_vec().ok_or_else(|| Error::new("bad body"))?;
        // Internal defines in a body go into the body's own frame.
        let Some((last, init)) = body.split_last() else { return Ok(Value::Unspecified) };
        for e in init {
            self.eval(e, env)?;
        }
        // An unspecified-valued form as the tail is fine as an expression.
        Ok(last.clone())
    }

    fn split_bindings(&self, bindings: &Value) -> Result<(Vec<Sym>, Vec<Value>)> {
        let mut names = Vec::new();
        let mut inits = Vec::new();
        for b in bindings.iter() {
            match b {
                Value::Sym(s) => {
                    names.push(s);
                    inits.push(Value::Unspecified);
                }
                _ => {
                    names.push(b.car().and_then(|v| v.as_sym()).ok_or_else(|| Error::new("bad binding"))?);
                    inits.push(nth(&b, 1).unwrap_or(Value::Unspecified));
                }
            }
        }
        Ok((names, inits))
    }

    fn eval_do(&mut self, rest: &Value, env: &Env) -> Result<Value> {
        let specs = arg(rest, 0)?.to_vec().unwrap_or_default();
        let exit = arg(rest, 1)?;
        let body = tail(&tail(rest)).to_vec().unwrap_or_default();
        let mut vars = Vec::new();
        for spec in &specs {
            let name = spec.car().and_then(|v| v.as_sym()).ok_or_else(|| Error::new("bad do"))?;
            vars.push((name, self.eval(&arg(spec, 1)?, env)?));
        }
        let mut frame = new_frame(env, vars);
        let test = arg(&exit, 0)?;
        loop {
            if self.eval(&test, &frame)?.is_true() {
                return self.eval_body(&tail(&exit).to_vec().unwrap_or_default(), &frame);
            }
            self.eval_body(&body, &frame)?;
            let mut next = Vec::new();
            for spec in &specs {
                let name = spec.car().and_then(|v| v.as_sym()).unwrap();
                let value = match nth(spec, 2) {
                    Some(step) => self.eval(&step, &frame)?,
                    None => self.lookup(name, &frame)?,
                };
                next.push((name, value));
            }
            frame = new_frame(env, next);
        }
    }

    fn quasi(&mut self, template: &Value, env: &Env, depth: usize) -> Result<Value> {
        match template {
            Value::Pair(p) => {
                let head = p.car.borrow().clone();
                if head.as_sym() == Some(self.forms.unquote) {
                    let inner = arg(&p.cdr.borrow(), 0)?;
                    return if depth == 1 {
                        self.eval(&inner, env)
                    } else {
                        Ok(Value::list([head, self.quasi(&inner, env, depth - 1)?]))
                    };
                }
                if head.as_sym() == Some(self.forms.quasiquote) {
                    let inner = arg(&p.cdr.borrow(), 0)?;
                    return Ok(Value::list([head, self.quasi(&inner, env, depth + 1)?]));
                }
                // ,@x inside a list splices.
                if let Value::Pair(hp) = &head {
                    if hp.car.borrow().as_sym() == Some(self.forms.unquote_splicing) && depth == 1 {
                        let spliced = self.eval(&arg(&hp.cdr.borrow(), 0)?, env)?;
                        let rest = self.quasi(&p.cdr.borrow(), env, depth)?;
                        let items = spliced.to_vec().ok_or_else(|| Error::new("unquote-splicing: not a list"))?;
                        return Ok(items.into_iter().rev().fold(rest, |acc, v| Value::cons(v, acc)));
                    }
                }
                let car = self.quasi(&head, env, depth)?;
                let cdr = self.quasi(&p.cdr.borrow(), env, depth)?;
                Ok(Value::cons(car, cdr))
            }
            Value::Vector(items) => {
                let list = Value::list(items.borrow().clone());
                let expanded = self.quasi(&list, env, depth)?;
                Ok(Value::Vector(Rc::new(RefCell::new(expanded.to_vec().unwrap_or_default()))))
            }
            other => Ok(other.clone()),
        }
    }
}

fn name_lambda(value: Value, name: Sym) -> Value {
    match &value {
        Value::Lambda(l) if l.name.is_none() => Value::Lambda(Rc::new(Lambda {
            name: Some(name),
            params: l.params.clone(),
            rest: l.rest,
            body: l.body.clone(),
            env: l.env.clone(),
        })),
        _ => value,
    }
}

fn nth(list: &Value, n: usize) -> Option<Value> {
    list.iter().nth(n)
}

fn arg(list: &Value, n: usize) -> Result<Value> {
    nth(list, n).ok_or_else(|| Error::new(format!("missing form argument {n} in {list:?}")))
}

fn tail(list: &Value) -> Value {
    list.cdr().unwrap_or(Value::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interp() -> Interp {
        Interp::new(Box::new(|_| None), Box::new(()))
    }

    fn run(src: &str) -> String {
        let mut i = interp();
        match i.eval_str(src) {
            Ok(v) => format!("{v:?}"),
            Err(e) => format!("ERROR: {e}"),
        }
    }

    #[test]
    fn basics() {
        assert_eq!(run("(+ 1 2 3)"), "6");
        assert_eq!(run("(/ 7 2)"), "3.5");
        assert_eq!(run("(/ 6 2)"), "3");
        assert_eq!(run("(define (f x) (* x x)) (f 7)"), "49");
        assert_eq!(run("(let loop ((i 0) (acc '())) (if (= i 3) acc (loop (+ i 1) (cons i acc))))"), "(2 1 0)");
        assert_eq!(run("(cond ((assv 2 '((1 . a) (2 . b))) => cdr) (else 'no))"), "b");
        assert_eq!(run("(case 3 ((1 2) 'low) ((3 4) 'mid) (else 'high))"), "mid");
        assert_eq!(run("(define x 5) `(a ,x ,@(list 1 2) b)"), "(a 5 1 2 b)");
        assert_eq!(run("(and 1 '())"), "()");
        assert_eq!(run("(if '() 'yes 'no)"), "yes");
        assert_eq!(run("(do ((i 0 (+ i 1)) (s 0 (+ s i))) ((= i 5) s))"), "10");
        assert_eq!(run("(define v (vector 1 2 3)) (vector-set! v 0 9) (vector->list v)"), "(9 2 3)");
        assert_eq!(run("(format #f \"Stock left: ~a\" 12)"), "\"Stock left: 12\"");
        assert_eq!(run("(sort '(3 1 2) <)"), "(1 2 3)");
        assert_eq!(run("(defmacro twice (x) `(begin ,x ,x)) (define n 0) (twice (set! n (+ n 1))) n"), "2");
        assert_eq!(run("(letrec ((ev? (lambda (n) (if (= n 0) #t (od? (- n 1))))) (od? (lambda (n) (if (= n 0) #f (ev? (- n 1)))))) (ev? 10))"), "#t");
        assert_eq!(run("(define h (make-hash-table 31)) (hash-set! h (vector 1 2) 'x) (hash-ref h (vector 1 2))"), "x");
        assert_eq!(run("(equal? (list 1 (list 2)) '(1 (2)))"), "#t");
    }

    #[test]
    fn deep_tail_recursion_does_not_overflow() {
        assert_eq!(run("(define (count n) (if (= n 0) 'done (count (- n 1)))) (count 1000000)"), "done");
    }
}
