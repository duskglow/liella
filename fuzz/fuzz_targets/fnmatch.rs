//! `fnmatch` (spec §14.8, D-14.3) on its own: every flag (`FNM_PATHNAME`, `FNM_NOESCAPE`,
//! `FNM_PERIOD`, `FNM_LEADING_DIR`, `FNM_CASEFOLD`, `FNM_EXTMATCH`), bracket expressions with
//! ranges, negation and the character classes, in `C` and `C.UTF-8`.
//!
//! Invariants, not only "no crash":
//! * `fnmatch` agrees with the compiled form (`Pattern::new` + `matches`);
//! * `Pattern::prefix_matches` / `suffix_matches`, when they answer, agree with `matches` on
//!   every prefix / suffix (C locale: every offset is a character boundary);
//! * where the pattern stays inside what POSIX defines exactly (no extended groups, no
//!   equivalence or collating symbols, no unterminated bracket, ASCII only, ...), the answer
//!   equals a slow backtracking reference model written from POSIX's description. Anything
//!   the model does not decide (it answers `None`) is not compared.
//!
//! Input: flags byte (bits 0-5 the six flags), a byte (bit 0: `C.UTF-8`), then
//! `pattern NUL string`.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::pattern::fnmatch::{self, Pattern};
use liella_fuzz::Input;

const ALL: [i32; 6] = [fnmatch::PATHNAME, fnmatch::NOESCAPE, fnmatch::PERIOD, fnmatch::LEADING_DIR, fnmatch::CASEFOLD, fnmatch::EXTMATCH];

/// The reference model: `None` where it does not decide.
struct Model<'a> {
    p: &'a [u8],
    s: &'a [u8],
    flags: i32,
    memo: Vec<Option<Option<bool>>>,
}

fn class(name: &[u8], c: u8) -> Option<bool> {
    Some(match name {
        b"alpha" => c.is_ascii_alphabetic(),
        b"digit" => c.is_ascii_digit(),
        b"alnum" => c.is_ascii_alphanumeric(),
        b"upper" => c.is_ascii_uppercase(),
        b"lower" => c.is_ascii_lowercase(),
        b"space" => matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'),
        b"blank" => matches!(c, b' ' | b'\t'),
        b"punct" => c.is_ascii_punctuation(),
        b"print" => (0x20..0x7f).contains(&c),
        b"graph" => (0x21..0x7f).contains(&c),
        b"cntrl" => c < 0x20 || c == 0x7f,
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => return None,
    })
}

impl Model<'_> {
    fn has(&self, f: i32) -> bool {
        self.flags & f != 0
    }

    fn leading(&self, si: usize) -> bool {
        self.has(fnmatch::PERIOD) && self.s.get(si) == Some(&b'.') && (si == 0 || (self.has(fnmatch::PATHNAME) && self.s[si - 1] == b'/'))
    }

    fn fold(&self, c: u8) -> u8 {
        if self.has(fnmatch::CASEFOLD) { c.to_ascii_lowercase() } else { c }
    }

    /// The bracket expression starting after `[` at `pi`: (whether `c` is in it, the index after
    /// the closing `]`), `None` when it is outside the model.
    fn bracket(&self, mut pi: usize, c: u8) -> Option<(bool, usize)> {
        let p = self.p;
        let negate = matches!(p.get(pi), Some(b'!' | b'^'));
        if negate {
            pi += 1;
        }
        let mut found = false;
        let mut first = true;
        loop {
            let &b = p.get(pi)?;
            if b == b']' && !first {
                return Some((found != negate, pi + 1));
            }
            first = false;
            if b == b'/' && self.has(fnmatch::PATHNAME) {
                return None;
            }
            if b == b'[' {
                match p.get(pi + 1) {
                    Some(b':') => {
                        let end = p[pi + 2..].windows(2).position(|w| w == b":]")? + pi + 2;
                        let name = &p[pi + 2..end];
                        if self.has(fnmatch::CASEFOLD) && matches!(name, b"upper" | b"lower") {
                            return None;
                        }
                        found |= class(name, c)?;
                        pi = end + 2;
                        continue;
                    }
                    Some(b'=' | b'.') => return None,
                    _ => {}
                }
            }
            let (lo, next) = if b == b'\\' && !self.has(fnmatch::NOESCAPE) { (*p.get(pi + 1)?, pi + 2) } else { (b, pi + 1) };
            if p.get(next) == Some(&b'-') && p.get(next + 1).is_some_and(|&e| e != b']') {
                let hi = p[next + 1];
                if hi == b'[' || hi == b'\\' || hi == b'/' || lo > hi || self.has(fnmatch::CASEFOLD) {
                    return None;
                }
                found |= (lo..=hi).contains(&c);
                pi = next + 2;
            } else {
                found |= self.fold(lo) == self.fold(c);
                pi = next;
            }
        }
    }

    fn at(&mut self, pi: usize, si: usize) -> Option<bool> {
        let k = pi * (self.s.len() + 1) + si;
        if let Some(r) = self.memo[k] {
            return r;
        }
        let r = self.step(pi, si);
        self.memo[k] = Some(r);
        r
    }

    fn step(&mut self, pi: usize, si: usize) -> Option<bool> {
        let (p, s) = (self.p, self.s);
        let Some(&c) = p.get(pi) else {
            return Some(si == s.len() || (self.has(fnmatch::LEADING_DIR) && s[si] == b'/'));
        };
        let pathname = self.has(fnmatch::PATHNAME);
        let slash = move |si: usize| pathname && s.get(si) == Some(&b'/');
        match c {
            b'?' => {
                if si == s.len() || slash(si) || self.leading(si) {
                    return Some(false);
                }
                self.at(pi + 1, si + 1)
            }
            b'*' => {
                if self.leading(si) {
                    return None;
                }
                let mut next = pi;
                while p.get(next) == Some(&b'*') {
                    next += 1;
                }
                let mut unknown = false;
                let mut end = si;
                loop {
                    match self.at(next, end) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        Some(false) => {}
                    }
                    if end == s.len() || slash(end) {
                        break;
                    }
                    end += 1;
                }
                if unknown { None } else { Some(false) }
            }
            b'[' => {
                let ch = s.get(si).copied();
                // The model needs the bracket well formed even when the string is at its end.
                let (inside, after) = self.bracket(pi + 1, ch.unwrap_or(0))?;
                let Some(_) = ch else { return Some(false) };
                if slash(si) {
                    return Some(false);
                }
                if self.leading(si) {
                    return None;
                }
                if !inside {
                    return Some(false);
                }
                self.at(after, si + 1)
            }
            _ => {
                let (lit, next) = if c == b'\\' && !self.has(fnmatch::NOESCAPE) { (*p.get(pi + 1)?, pi + 2) } else { (c, pi + 1) };
                match s.get(si) {
                    Some(&x) if self.fold(x) == self.fold(lit) => self.at(next, si + 1),
                    _ => Some(false),
                }
            }
        }
    }
}

fn reference(p: &[u8], s: &[u8], flags: i32, utf8: bool) -> Option<bool> {
    if flags & fnmatch::EXTMATCH != 0 || p.len() > 64 || s.len() > 64 {
        return None;
    }
    if utf8 && (p.iter().chain(s).any(|&b| b >= 0x80)) {
        return None;
    }
    let mut m = Model { p, s, flags, memo: vec![None; (p.len() + 1) * (s.len() + 1)] };
    m.at(0, 0)
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let bits = input.byte();
    let flags = ALL.iter().enumerate().filter(|(i, _)| bits & (1 << i) != 0).fold(0, |a, (_, f)| a | f);
    let utf8 = input.byte() & 1 != 0;
    let pattern = input.part();
    let string = input.rest();
    if pattern.len() > 256 || string.len() > 256 || string.contains(&0) || pattern.iter().filter(|&&b| b == b'*').count() > 12 {
        return;
    }
    let loc = liella_fuzz::locale(utf8);

    let direct = fnmatch::fnmatch(pattern, string, flags, &loc);
    let Ok(compiled) = Pattern::new(pattern, flags, &loc) else { return };
    let via = compiled.matches(string, &loc);
    if let (Ok(a), Ok(b)) = (&direct, &via) {
        assert_eq!(a, b, "fnmatch and Pattern::matches disagree: {pattern:?} {string:?} flags {flags:#x}");
    }
    let Ok(answer) = via else { return };

    if let Some(want) = reference(pattern, string, flags, utf8) {
        assert_eq!(answer, want, "fnmatch differs from the reference model: {pattern:?} {string:?} flags {flags:#x} utf8 {utf8}");
    }

    if !utf8 && string.len() <= 48 {
        if let Ok(Some(v)) = compiled.prefix_matches(string, &loc) {
            assert_eq!(v.len(), string.len() + 1, "prefix_matches length");
            for e in 0..=string.len() {
                if let Ok(m) = compiled.matches(&string[..e], &loc) {
                    assert_eq!(v[e], m, "prefix_matches[{e}] differs: {pattern:?} {string:?} flags {flags:#x}");
                }
            }
        }
        if let Ok(Some(v)) = compiled.suffix_matches(string, &loc) {
            assert_eq!(v.len(), string.len() + 1, "suffix_matches length");
            for b in 0..=string.len() {
                if let Ok(m) = compiled.matches(&string[b..], &loc) {
                    assert_eq!(v[b], m, "suffix_matches[{b}] differs: {pattern:?} {string:?} flags {flags:#x}");
                }
            }
        }
    }
});
