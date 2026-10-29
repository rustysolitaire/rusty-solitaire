// Rusty Solitaire - scheme/reader.rs
//
// Turns Scheme source text into values.
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::rc::Rc;

use super::value::Value;
use super::{Error, Result};

pub struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
    file: &'a str,
}

impl<'a> Reader<'a> {
    pub fn new(src: &'a str, file: &'a str) -> Self {
        Reader { src: src.as_bytes(), pos: 0, file }
    }

    fn line(&self) -> usize {
        self.src[..self.pos].iter().filter(|&&c| c == b'\n').count() + 1
    }

    fn error(&self, msg: &str) -> Error {
        Error::new(format!("{}:{}: {msg}", self.file, self.line()))
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) -> Result<()> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some(b';') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some(b'#') if self.src.get(self.pos + 1) == Some(&b'|') => {
                    self.pos += 2;
                    let mut depth = 1;
                    while depth > 0 {
                        match (self.peek(), self.src.get(self.pos + 1)) {
                            (None, _) => return Err(self.error("unterminated block comment")),
                            (Some(b'|'), Some(b'#')) => {
                                depth -= 1;
                                self.pos += 2;
                            }
                            (Some(b'#'), Some(b'|')) => {
                                depth += 1;
                                self.pos += 2;
                            }
                            _ => self.pos += 1,
                        }
                    }
                }
                Some(b'#') if self.src.get(self.pos + 1) == Some(&b';') => {
                    self.pos += 2;
                    self.read()?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Read every datum in the source.
    pub fn read_all(&mut self) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        loop {
            self.skip_ws()?;
            if self.peek().is_none() {
                return Ok(out);
            }
            out.push(self.read()?);
        }
    }

    fn read(&mut self) -> Result<Value> {
        self.skip_ws()?;
        let Some(c) = self.peek() else { return Err(self.error("unexpected end of input")) };
        match c {
            b'(' | b'[' => {
                self.pos += 1;
                self.read_list(if c == b'(' { b')' } else { b']' })
            }
            b')' | b']' => Err(self.error("unexpected ')'")),
            b'\'' => self.read_quoted("quote", 1),
            b'`' => self.read_quoted("quasiquote", 1),
            b',' => {
                if self.src.get(self.pos + 1) == Some(&b'@') {
                    self.read_quoted("unquote-splicing", 2)
                } else {
                    self.read_quoted("unquote", 1)
                }
            }
            b'"' => self.read_string(),
            b'#' => self.read_hash(),
            _ => self.read_atom(),
        }
    }

    fn read_quoted(&mut self, name: &str, skip: usize) -> Result<Value> {
        self.pos += skip;
        let datum = self.read()?;
        Ok(Value::list([Value::sym(name), datum]))
    }

    fn read_list(&mut self, close: u8) -> Result<Value> {
        let mut items = Vec::new();
        let mut tail = Value::Nil;
        loop {
            self.skip_ws()?;
            match self.peek() {
                None => return Err(self.error("unterminated list")),
                Some(c) if c == close => {
                    self.pos += 1;
                    break;
                }
                Some(b'.')
                    if self.src.get(self.pos + 1).is_none_or(|c| c.is_ascii_whitespace() || *c == b'(') =>
                {
                    self.pos += 1;
                    tail = self.read()?;
                    self.skip_ws()?;
                    if self.peek() != Some(close) {
                        return Err(self.error("expected ')' after dotted tail"));
                    }
                    self.pos += 1;
                    break;
                }
                _ => items.push(self.read()?),
            }
        }
        Ok(items.into_iter().rev().fold(tail, |acc, v| Value::cons(v, acc)))
    }

    fn read_string(&mut self) -> Result<Value> {
        self.pos += 1;
        let mut bytes = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| self.error("bad escape"))?;
                    self.pos += 1;
                    match esc {
                        b'n' => bytes.push(b'\n'),
                        b't' => bytes.push(b'\t'),
                        b'\n' => {
                            // Line continuation: skip leading whitespace on the next line.
                            while matches!(self.peek(), Some(b' ' | b'\t')) {
                                self.pos += 1;
                            }
                        }
                        other => bytes.push(other),
                    }
                }
                Some(c) => {
                    bytes.push(c);
                    self.pos += 1;
                }
            }
        }
        let s = String::from_utf8(bytes).map_err(|_| self.error("invalid UTF-8 in string"))?;
        Ok(Value::str(&s))
    }

    fn read_hash(&mut self) -> Result<Value> {
        match self.src.get(self.pos + 1) {
            Some(b'(') => {
                self.pos += 2;
                let list = self.read_list(b')')?;
                let items = list.to_vec().ok_or_else(|| self.error("bad vector literal"))?;
                Ok(Value::Vector(Rc::new(RefCell::new(items))))
            }
            Some(b'\\') => {
                self.pos += 2;
                let start = self.pos;
                // A character: one char, or a name like #\space.
                let first = std::str::from_utf8(&self.src[self.pos..])
                    .ok()
                    .and_then(|s| s.chars().next())
                    .ok_or_else(|| self.error("bad character"))?;
                self.pos += first.len_utf8();
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric()) {
                    self.pos += 1;
                }
                let name = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                let ch = match name {
                    "space" => ' ',
                    "newline" => '\n',
                    "tab" => '\t',
                    _ => first,
                };
                // Characters are represented as one-character strings.
                Ok(Value::str(&ch.to_string()))
            }
            _ => {
                let atom = self.read_token();
                match atom.as_str() {
                    "#t" | "#true" => Ok(Value::Bool(true)),
                    "#f" | "#false" => Ok(Value::Bool(false)),
                    _ => Err(self.error(&format!("unknown syntax {atom}"))),
                }
            }
        }
    }

    fn read_token(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'[' | b']' | b'"' | b';' | b'\'') {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn read_atom(&mut self) -> Result<Value> {
        let token = self.read_token();
        if token.is_empty() {
            return Err(self.error("empty token"));
        }
        Ok(parse_number(&token).unwrap_or_else(|| Value::sym(&token)))
    }
}

pub fn parse_number(token: &str) -> Option<Value> {
    if let Ok(i) = token.parse::<i64>() {
        return Some(Value::Int(i));
    }
    if let Some((n, d)) = token.split_once('/') {
        let (n, d) = (n.parse::<i64>().ok()?, d.parse::<i64>().ok()?);
        if d == 0 {
            return None;
        }
        return Some(if n % d == 0 { Value::Int(n / d) } else { Value::Float(n as f64 / d as f64) });
    }
    let first = token.chars().next()?;
    if first.is_ascii_digit() || ((first == '-' || first == '+' || first == '.') && token.len() > 1) {
        if let Ok(f) = token.parse::<f64>() {
            return Some(Value::Float(f));
        }
    }
    None
}
