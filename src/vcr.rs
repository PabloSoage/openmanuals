//! Vehicle configuration restrictions: the `domain:value` boolean expressions
//! GlobalTIS attaches to tree nodes, documents and paragraphs.
//!
//! `2:1 and 3:2001 and (7:31 or 7:40)`, `[8:1 and 7:166] or [8:3 and 7:165]`.
//! A term about a domain the vehicle does not pin down (no engine chosen yet,
//! say) does not exclude anything, so a half-specified vehicle still sees
//! everything that may apply to it.

use std::collections::{HashMap, HashSet};

/// A parsed restriction.
#[derive(Debug, Clone)]
pub enum Expr {
    /// `domain:value`
    Atom(i64, i64),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

/// What is known about the vehicle: the values it has, per domain.
#[derive(Debug, Clone, Default)]
pub struct Context {
    values: HashMap<i64, HashSet<i64>>,
}

impl Context {
    /// `2:1,6:36,3:2011` (order free; a domain may repeat).
    pub fn parse(s: &str) -> Self {
        let mut c = Self::default();
        for part in s.split([',', ' ']).filter(|p| !p.is_empty()) {
            if let Some((d, v)) = part.split_once(':')
                && let (Ok(d), Ok(v)) = (d.trim().parse(), v.trim().parse())
            {
                c.add(d, v);
            }
        }
        c
    }

    pub fn add(&mut self, domain: i64, value: i64) {
        self.values.entry(domain).or_default().insert(value);
    }

    pub fn get(&self, domain: i64) -> Option<&HashSet<i64>> {
        self.values.get(&domain)
    }

    pub fn with(&self, domain: i64, value: i64) -> Self {
        let mut c = self.clone();
        c.values.insert(domain, HashSet::from([value]));
        c
    }
}

impl Expr {
    pub fn parse(s: &str) -> Option<Expr> {
        let toks = tokenize(s);
        let mut p = Parser { toks: &toks, i: 0 };
        let e = p.or()?;
        (p.i == toks.len()).then_some(e)
    }

    /// True unless the vehicle contradicts it.
    pub fn allows(&self, c: &Context) -> bool {
        match self {
            Expr::Atom(d, v) => c.get(*d).is_none_or(|set| set.contains(v)),
            Expr::And(xs) => xs.iter().all(|x| x.allows(c)),
            Expr::Or(xs) => xs.iter().any(|x| x.allows(c)),
        }
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    And,
    Or,
    Atom(i64, i64),
}

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' => {
                out.push(Tok::Open);
                i += 1;
            }
            b')' | b']' => {
                out.push(Tok::Close);
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            c if c.is_ascii_digit() || c == b'-' => {
                let st = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b':' || b[i] == b'-') {
                    i += 1;
                }
                if let Some((d, v)) = s[st..i].split_once(':')
                    && let (Ok(d), Ok(v)) = (d.parse(), v.parse())
                {
                    out.push(Tok::Atom(d, v));
                }
            }
            _ => {
                let st = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                match s[st..i].to_ascii_lowercase().as_str() {
                    "and" => out.push(Tok::And),
                    "or" => out.push(Tok::Or),
                    _ => i = i.max(st + 1),
                }
            }
        }
    }
    out
}

struct Parser<'a> {
    toks: &'a [Tok],
    i: usize,
}

impl Parser<'_> {
    fn or(&mut self) -> Option<Expr> {
        let mut xs = vec![self.and()?];
        while self.toks.get(self.i) == Some(&Tok::Or) {
            self.i += 1;
            xs.push(self.and()?);
        }
        Some(if xs.len() == 1 { xs.pop()? } else { Expr::Or(xs) })
    }

    fn and(&mut self) -> Option<Expr> {
        let mut xs = vec![self.atom()?];
        while self.toks.get(self.i) == Some(&Tok::And) {
            self.i += 1;
            xs.push(self.atom()?);
        }
        Some(if xs.len() == 1 { xs.pop()? } else { Expr::And(xs) })
    }

    fn atom(&mut self) -> Option<Expr> {
        match self.toks.get(self.i)? {
            Tok::Open => {
                self.i += 1;
                let e = self.or()?;
                if self.toks.get(self.i) == Some(&Tok::Close) {
                    self.i += 1;
                }
                Some(e)
            }
            Tok::Atom(d, v) => {
                self.i += 1;
                Some(Expr::Atom(*d, *v))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnf_with_groups() {
        let e = Expr::parse("[8:1 and 7:166] or [8:3 and (7:165 or 7:166)]").unwrap();
        assert!(e.allows(&Context::parse("8:3,7:165")));
        assert!(!e.allows(&Context::parse("8:1,7:165")));
        // engine unknown: still possible
        assert!(e.allows(&Context::parse("8:1")));
    }

    #[test]
    fn plain_and() {
        let e = Expr::parse("2:1 and 3:2001 and 6:14").unwrap();
        assert!(e.allows(&Context::parse("2:1,3:2001,6:14,7:5")));
        assert!(!e.allows(&Context::parse("6:15")));
    }
}
