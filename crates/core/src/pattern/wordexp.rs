//! Shell word expansion (spec §14.10; XCU 2.2 and 2.6; D-14.5 and the owner rulings of
//! 2026-10-03): tilde, parameter, command and arithmetic expansion, field splitting, pathname
//! expansion and quote removal, in process; only command substitution runs `/bin/sh` (the
//! [`Shell`] the caller supplies does).
//!
//! # Two passes
//!
//! 1. **Parse** the whole text into words before anything is expanded. Every syntactic error is
//!    found here, left to right: an unquoted `| & ; < > ( ) { }` or newline is
//!    `WRDE_BADCHAR`; an unterminated quote, `$(`, `$((`, `${` or backquote, a trailing
//!    backslash, or a parameter form POSIX does not define is `WRDE_SYNTAX`; and with
//!    `WRDE_NOCMD` any command substitution, wherever it is (inside `${...}`, inside `$((...))`,
//!    a backquote inside `${...}`, `$()`), is `WRDE_CMDSUB` (owner ruling: detection is
//!    syntactic and fails closed).
//! 2. **Expand** word by word, left to right, giving each finished field to the [`Words`]
//!    sink: `WRDE_BADVAL` (an unset variable with `WRDE_UNDEF`, or `${X:?}`), arithmetic errors
//!    (`WRDE_SYNTAX`, or `WRDE_BADVAL` for an unset variable with `WRDE_UNDEF`), and
//!    `WRDE_NOSPACE` when memory or the sink fails (the fields given so far are the partial
//!    result POSIX describes).
//!
//! # Expansion rules
//!
//! * Quotes protect from field splitting and pathname expansion; the results of unquoted
//!   expansions (parameters, command output, arithmetic) are split on `IFS` and, with the word's
//!   own unquoted text, expanded as pathnames (sorted; a pattern without a match stays as
//!   written). Inside an unquoted `${X:-word}`, the quoted parts of `word` stay protected
//!   (XCU 2.6.2).
//! * Field splitting (XCU 2.6.5): `IFS` unset means space, tab, newline; `IFS` white space at
//!   the start or end of an expansion's result is dropped, inside it a run of IFS white space
//!   (with at most one other IFS character) ends a field; each other IFS character ends a field,
//!   so two of them in a row delimit an empty one. A word whose expansion is empty and has no
//!   quoted part produces no field.
//! * Tilde (XCU 2.6.1): an unquoted `~` at the start of a word (or right after the first `=` of
//!   an assignment-like word), with the plain characters up to `/` or `:`: `~` is `$HOME` (the
//!   passwd entry when `HOME` is unset), `~user` the user's home; an unknown user leaves the
//!   text unchanged. The result is not split or expanded further.
//! * Parameters: `$name`, `${name}`, the POSIX forms `- = ? +` with and without `:`, `#`,
//!   `% %% # ##` (patterns as `fnmatch`), `${#name}`. `$0`-`$9`, `${n}`, `$#`, `$@`, `$*` are
//!   the program's arguments, `$$` the process ID (POSIX leaves them unspecified for `wordexp`;
//!   as glibc). `$?`, `$-`, `$!` stay literal text. `${X:=w}` sets `X` in the caller's
//!   environment (as glibc; reported to the owner). `${X:?w}` writes `X: w` to standard error and
//!   fails with `WRDE_BADVAL` (an error, never an empty success).
//! * Arithmetic (owner ruling): the POSIX operator set on signed 64-bit integers, see [`arith`].
//!   `$[...]` is accepted as an old form of `$((...))`.

#![allow(clippy::arithmetic_side_effects)] // offsets are bounded by the lengths of the texts; arithmetic on values uses checked operations

use crate::error::{Errno, Result};
use crate::locale::Locale;
use crate::mem::{InlineVec, TryVec};

use super::fnmatch::Pattern;
use super::regex::text::{self, Mode, Unit};

/// `WRDE_DOOFFS`.
pub const DOOFFS: i32 = 1;
/// `WRDE_APPEND`.
pub const APPEND: i32 = 1 << 1;
/// `WRDE_NOCMD`.
pub const NOCMD: i32 = 1 << 2;
/// `WRDE_REUSE`.
pub const REUSE: i32 = 1 << 3;
/// `WRDE_SHOWERR`.
pub const SHOWERR: i32 = 1 << 4;
/// `WRDE_UNDEF`.
pub const UNDEF: i32 = 1 << 5;

/// A failure, as the `WRDE_*` return values name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordError {
    /// `WRDE_NOSPACE` (1).
    NoSpace,
    /// `WRDE_BADCHAR` (2).
    BadChar,
    /// `WRDE_BADVAL` (3).
    BadVal,
    /// `WRDE_CMDSUB` (4).
    CmdSub,
    /// `WRDE_SYNTAX` (5).
    Syntax,
}

impl WordError {
    /// The return value.
    pub fn code(self) -> i32 {
        match self {
            WordError::NoSpace => 1,
            WordError::BadChar => 2,
            WordError::BadVal => 3,
            WordError::CmdSub => 4,
            WordError::Syntax => 5,
        }
    }
}

impl From<Errno> for WordError {
    fn from(_: Errno) -> WordError {
        WordError::NoSpace
    }
}

type Wr<T> = core::result::Result<T, WordError>;

/// A parameter's value or a home directory, as the shell gives it: inline for a usual one (no
/// allocation).
pub type ValBuf = InlineVec<u8, 192>;

/// What word expansion needs from the process.
pub trait Shell {
    /// The value of variable `name` into `out` (cleared first); `false` when it is unset.
    fn var(&mut self, name: &[u8], out: &mut ValBuf) -> Result<bool>;
    /// Set variable `name` (`${name:=word}`).
    fn set_var(&mut self, name: &[u8], value: &[u8]) -> Result<()>;
    /// The number of positional parameters (`$#`).
    fn arg_count(&mut self) -> usize;
    /// Positional parameter `i` (0: the program name) into `out`; `false` when there is none.
    fn arg(&mut self, i: usize, out: &mut ValBuf) -> Result<bool>;
    /// The process ID (`$$`).
    fn pid(&mut self) -> i32;
    /// The home directory of `user` into `out` (`None`: `$HOME`, even empty, or the caller's
    /// passwd entry when `HOME` is unset); `false` when unknown.
    fn home(&mut self, user: Option<&[u8]>, out: &mut ValBuf) -> Result<bool>;
    /// Run `command` with `/bin/sh -c` and capture its standard output into `out`; its
    /// standard error goes to `/dev/null` unless `show_err`.
    fn run(&mut self, command: &[u8], show_err: bool, out: &mut TryVec<u8>) -> Result<()>;
    /// Write `text` to standard error.
    fn stderr(&mut self, text: &[u8]);
    /// The paths matching `pattern` (backslash escapes active), sorted, into `out`; none when
    /// nothing matches.
    fn glob(&mut self, pattern: &[u8], out: &mut TryVec<TryVec<u8>>) -> Result<()>;
}

/// Where the fields go.
pub trait Words {
    /// One more field.
    fn push(&mut self, word: &[u8]) -> Result<()>;
}

// ------------------------------------------------------------------------------ the parse

/// A range of the parsed text: `start..end`.
type Span = (usize, usize);

#[derive(Clone, Copy, Debug)]
enum Name {
    /// A variable: its name's range in the input.
    Var(Span),
    Positional(usize),
    At,
    Star,
    Count,
    Pid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Default,
    Assign,
    Error,
    Alt,
    SuffixShort,
    SuffixLong,
    PrefixShort,
    PrefixLong,
}

/// A nested word: its segments' range in the parser's arena.
type Range = (usize, usize);

#[derive(Clone, Copy, Debug)]
enum Form {
    Plain,
    Length,
    Op { op: Op, colon: bool, word: Range },
}

/// Plain data (`Copy`): nested words are ranges of the arena, a command's text a range of the
/// input or of the parser's text, so that a call's segments live in inline buffers.
#[derive(Clone, Copy, Debug)]
enum Seg {
    /// Text of the word itself: a range of the input (`input`; text as it stands there, not
    /// copied) or of the parser's text buffer; `quoted` when it came from quotes or a backslash.
    Text { at: Span, quoted: bool, input: bool },
    /// A tilde prefix (the user name's range in the input, empty for `~`).
    Tilde(Span),
    /// A parameter expansion; `quoted` inside double quotes.
    Param { name: Name, form: Form, quoted: bool },
    /// A command substitution (the command text).
    Cmd { at: Span, input: bool, quoted: bool },
    /// An arithmetic expansion (its text, expanded as inside double quotes).
    Arith { expr: Range, quoted: bool },
}

impl Default for Seg {
    fn default() -> Seg {
        Seg::Text { at: (0, 0), quoted: false, input: false }
    }
}

/// A word list under construction: inline for a usual text.
type Segs = InlineVec<Seg, 12>;

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    nocmd: bool,
    depth: usize,
    /// The words' text, every `Seg::Text` a range of it: one buffer for the whole call (inline
    /// for a usual text: no allocation). Each word starts with a separator byte, so that no
    /// range of an earlier word reaches the current word's `floor`.
    text: InlineVec<u8, 128>,
    /// Where the current word's text starts in `text`: [`push_text`] never extends a segment
    /// that starts before it (one of an earlier word in the flat segment list).
    floor: usize,
    /// Where each word's segments end in the list [`Parser::words`] returns.
    ends: InlineVec<usize, 16>,
    /// The segments of nested words (`${x:-word}`, `$((expr))`), each stored when complete.
    arena: InlineVec<Seg, 4>,
}

const MAX_DEPTH: usize = 128;

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Append `b` to the text of `segs`: to its last segment when that is text quoted alike and of
/// the current word (it starts at or after `floor`; a segment that does not end the buffer is
/// first copied to its end), else as a new segment.
fn push_text(segs: &mut Segs, text: &mut InlineVec<u8, 128>, floor: usize, b: &[u8], quoted: bool) -> Wr<()> {
    if let Some(Seg::Text { at, quoted: q, input: false }) = segs.as_mut_slice().last_mut() {
        if *q == quoted && at.0 >= floor {
            if at.1 != text.len() {
                let start = text.len();
                for k in at.0..at.1 {
                    let c = text.as_slice().get(k).copied().unwrap_or(0);
                    text.push(c)?;
                }
                *at = (start, text.len());
            }
            text.extend_from_slice(b)?;
            at.1 = text.len();
            return Ok(());
        }
    }
    let start = text.len();
    text.extend_from_slice(b)?;
    segs.push(Seg::Text { at: (start, text.len()), quoted, input: false })?;
    Ok(())
}

/// Append the input's bytes `at` to the text of `segs` without copying them: to its last
/// segment when that is a range of the input quoted alike that ends where `at` starts (the two
/// ranges make one), else as a new segment. (Merging is not needed for the result: a word's text
/// is the concatenation of its segments either way.)
fn push_input(segs: &mut Segs, at: Span, quoted: bool) -> Wr<()> {
    if let Some(Seg::Text { at: last, quoted: q, input: true }) = segs.as_mut_slice().last_mut() {
        if *q == quoted && last.1 == at.0 {
            last.1 = at.1;
            return Ok(());
        }
    }
    segs.push(Seg::Text { at, quoted, input: true })?;
    Ok(())
}

/// Whether `b` ends a run of plain bytes in an unquoted word (it may need handling of its own).
fn ends_plain_run(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')' | b'{' | b'}' | b'\\' | b'\'' | b'"' | b'$' | b'`' | b'~' | b'=')
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<u8> {
        self.s.get(self.i + k).copied()
    }

    fn enter(&mut self) -> Wr<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(WordError::Syntax) } else { Ok(()) }
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// The words of the whole text: their segments in one list (one allocation for all the
    /// words, not one per word); where each word's segments end in it goes to `ends`.
    fn words(&mut self, segs: &mut Segs) -> Wr<()> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.i += 1;
            }
            if self.peek().is_none() {
                return Ok(());
            }
            // The separator: every range of the earlier words ends before the floor.
            self.text.push(0)?;
            self.floor = self.text.len();
            self.word(segs)?;
            self.ends.push(segs.len())?;
        }
    }

    /// A finished nested word into the arena: its range.
    fn stash(&mut self, segs: &Segs) -> Wr<Range> {
        let start = self.arena.len();
        self.arena.extend_from_slice(segs.as_slice())?;
        Ok((start, self.arena.len()))
    }

    /// A tilde prefix at the current `~`: the user name and the offset after it, when the
    /// characters up to `/`, `:`, a blank or the end are plain.
    fn tilde_prefix(&self) -> Option<(usize, usize)> {
        let start = self.i + 1;
        let mut k = start;
        loop {
            match self.s.get(k).copied() {
                None | Some(b'/' | b':' | b' ' | b'\t') => return Some((start, k)),
                Some(b'\\' | b'\'' | b'"' | b'$' | b'`' | b'\n' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')' | b'{' | b'}') => return None,
                Some(_) => k += 1,
            }
        }
    }

    /// One unquoted word, up to a blank or the end: its segments appended to `segs`.
    fn word(&mut self, segs: &mut Segs) -> Wr<()> {
        let word_start = self.i;
        // Whether a `~` here may start a tilde prefix (the word's start, or after the first `=`
        // of an assignment-like word).
        let mut tilde_ok = true;
        let mut assignment_seen = false;
        loop {
            let Some(b) = self.peek() else { return Ok(()) };
            let tilde_here = tilde_ok;
            tilde_ok = false;
            match b {
                b' ' | b'\t' => return Ok(()),
                b'\n' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')' | b'{' | b'}' => return Err(WordError::BadChar),
                b'\\' => {
                    let Some(n) = self.peek_at(1) else { return Err(WordError::Syntax) };
                    self.i += 2;
                    if n != b'\n' {
                        push_text(segs, &mut self.text, self.floor, &[n], true)?;
                    }
                }
                b'\'' => {
                    let start = self.i + 1;
                    let Some(len) = self.s.get(start..).and_then(|r| r.iter().position(|&c| c == b'\'')) else { return Err(WordError::Syntax) };
                    push_input(segs, (start, start + len), true)?;
                    self.i = start + len + 1;
                }
                b'"' => {
                    self.i += 1;
                    // A quoted piece even when empty.
                    push_input(segs, (self.i, self.i), true)?;
                    self.dquote(segs)?;
                }
                b'$' => self.dollar(segs, false)?,
                b'`' => self.backquote(segs, false)?,
                b'~' if tilde_here => match self.tilde_prefix() {
                    Some((a, z)) => {
                        segs.push(Seg::Tilde((a, z)))?;
                        self.i = z;
                    }
                    None => {
                        push_input(segs, (self.i, self.i + 1), false)?;
                        self.i += 1;
                    }
                },
                b'=' if !assignment_seen => {
                    assignment_seen = true;
                    let name = self.s.get(word_start..self.i).unwrap_or(&[]);
                    tilde_ok = name.first().is_some_and(|&c| is_name_start(c)) && name.iter().all(|&c| is_name(c));
                    push_input(segs, (self.i, self.i + 1), false)?;
                    self.i += 1;
                }
                _ => {
                    // This byte and the plain bytes after it, as one piece of text.
                    let start = self.i;
                    let run = self.s.get(start + 1..).and_then(|r| r.iter().position(|&c| ends_plain_run(c))).map_or(self.s.len(), |k| start + 1 + k);
                    push_input(segs, (start, run), false)?;
                    self.i = run;
                }
            }
        }
    }

    /// The inside of double quotes, after the opening `"`, up to and including the closing one.
    fn dquote(&mut self, segs: &mut Segs) -> Wr<()> {
        loop {
            let Some(b) = self.peek() else { return Err(WordError::Syntax) };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(());
                }
                b'\\' => {
                    let Some(n) = self.peek_at(1) else { return Err(WordError::Syntax) };
                    match n {
                        b'\n' => self.i += 2,
                        b'$' | b'`' | b'"' | b'\\' => {
                            push_text(segs, &mut self.text, self.floor, &[n], true)?;
                            self.i += 2;
                        }
                        _ => {
                            // The backslash stays, as it stands in the input.
                            push_input(segs, (self.i, self.i + 1), true)?;
                            self.i += 1;
                        }
                    }
                }
                b'$' => self.dollar(segs, true)?,
                b'`' => self.backquote(segs, true)?,
                _ => {
                    // This byte and the plain bytes after it, as one piece of text.
                    let start = self.i;
                    let run = self.s.get(start + 1..).and_then(|r| r.iter().position(|&c| matches!(c, b'"' | b'\\' | b'$' | b'`'))).map_or(self.s.len(), |k| start + 1 + k);
                    push_input(segs, (start, run), true)?;
                    self.i = run;
                }
            }
        }
    }

    /// A backquoted command substitution at the current `` ` ``.
    fn backquote(&mut self, segs: &mut Segs, quoted: bool) -> Wr<()> {
        let start = self.text.len();
        let mut k = self.i + 1;
        loop {
            match self.s.get(k).copied() {
                None => return Err(WordError::Syntax),
                Some(b'`') => break,
                Some(b'\\') => {
                    let Some(n) = self.s.get(k + 1).copied() else { return Err(WordError::Syntax) };
                    if n == b'$' || n == b'`' || n == b'\\' || (quoted && n == b'"') {
                        self.text.push(n)?;
                    } else {
                        self.text.push(b'\\')?;
                        self.text.push(n)?;
                    }
                    k += 2;
                }
                Some(c) => {
                    self.text.push(c)?;
                    k += 1;
                }
            }
        }
        self.i = k + 1;
        if self.nocmd {
            return Err(WordError::CmdSub);
        }
        segs.push(Seg::Cmd { at: (start, self.text.len()), input: false, quoted })?;
        Ok(())
    }

    /// The end of a `$(...)` whose text starts at `from`: the offset of its `)`.
    fn command_end(&self, from: usize) -> Wr<usize> {
        let mut depth = 1usize;
        let mut k = from;
        loop {
            match self.s.get(k).copied() {
                None => return Err(WordError::Syntax),
                Some(b'\\') => k += 2,
                Some(b'\'') => {
                    let Some(len) = self.s.get(k + 1..).and_then(|r| r.iter().position(|&c| c == b'\'')) else { return Err(WordError::Syntax) };
                    k += len + 2;
                }
                Some(b'"') => {
                    k += 1;
                    loop {
                        match self.s.get(k).copied() {
                            None => return Err(WordError::Syntax),
                            Some(b'\\') => k += 2,
                            Some(b'"') => break,
                            Some(_) => k += 1,
                        }
                    }
                    k += 1;
                }
                Some(b'`') => {
                    k += 1;
                    loop {
                        match self.s.get(k).copied() {
                            None => return Err(WordError::Syntax),
                            Some(b'\\') => k += 2,
                            Some(b'`') => break,
                            Some(_) => k += 1,
                        }
                    }
                    k += 1;
                }
                Some(b'(') => {
                    depth += 1;
                    k += 1;
                }
                Some(b')') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(k);
                    }
                    k += 1;
                }
                Some(_) => k += 1,
            }
        }
    }

    /// The text of an arithmetic expansion, from the current offset up to `close` (`))` for
    /// `$((`, `]` for `$[`), parsed as inside double quotes.
    fn arith(&mut self, square: bool) -> Wr<Range> {
        let mut segs = Segs::new();
        // The usual expression is plain text: one scan finds its end and it is one piece of text.
        // Anything that needs handling of its own (`\`, `$`, a backquote) takes the loop below
        // from the start; so does a malformed end (the loop gives the error).
        {
            let (open, close) = if square { (b'[', b']') } else { (b'(', b')') };
            let rest = self.s.get(self.i..).unwrap_or(&[]);
            let mut depth = 0usize;
            let mut k = 0usize;
            while let Some(&c) = rest.get(k) {
                if matches!(c, b'\\' | b'$' | b'`') {
                    break;
                }
                if c == open {
                    depth += 1;
                } else if c == close {
                    if depth > 0 {
                        depth -= 1;
                    } else if square {
                        push_text(&mut segs, &mut self.text, self.floor, rest.get(..k).unwrap_or(&[]), true)?;
                        self.i += k + 1;
                        return self.stash(&segs);
                    } else if rest.get(k + 1) == Some(&b')') {
                        push_text(&mut segs, &mut self.text, self.floor, rest.get(..k).unwrap_or(&[]), true)?;
                        self.i += k + 2;
                        return self.stash(&segs);
                    } else {
                        break;
                    }
                }
                k += 1;
            }
        }
        let mut depth = 0usize;
        loop {
            let Some(b) = self.peek() else { return Err(WordError::Syntax) };
            match b {
                b'(' if !square => {
                    depth += 1;
                    push_text(&mut segs, &mut self.text, self.floor, b"(", true)?;
                    self.i += 1;
                }
                b')' if !square => {
                    if depth == 0 {
                        if self.peek_at(1) == Some(b')') {
                            self.i += 2;
                            return self.stash(&segs);
                        }
                        return Err(WordError::Syntax);
                    }
                    depth -= 1;
                    push_text(&mut segs, &mut self.text, self.floor, b")", true)?;
                    self.i += 1;
                }
                b'[' if square => {
                    depth += 1;
                    push_text(&mut segs, &mut self.text, self.floor, b"[", true)?;
                    self.i += 1;
                }
                b']' if square => {
                    if depth == 0 {
                        self.i += 1;
                        return self.stash(&segs);
                    }
                    depth -= 1;
                    push_text(&mut segs, &mut self.text, self.floor, b"]", true)?;
                    self.i += 1;
                }
                b'\\' => {
                    let Some(n) = self.peek_at(1) else { return Err(WordError::Syntax) };
                    match n {
                        b'\n' => {}
                        b'$' | b'`' | b'"' | b'\\' => push_text(&mut segs, &mut self.text, self.floor, &[n], true)?,
                        _ => push_text(&mut segs, &mut self.text, self.floor, &[b'\\', n], true)?,
                    }
                    self.i += 2;
                }
                b'$' => self.dollar(&mut segs, true)?,
                b'`' => self.backquote(&mut segs, true)?,
                _ => {
                    // This byte and the plain bytes after it, as one piece of text.
                    let start = self.i;
                    let run = self.s.get(start + 1..).and_then(|r| r.iter().position(|&c| matches!(c, b'(' | b')' | b'[' | b']' | b'\\' | b'$' | b'`'))).map_or(self.s.len(), |k| start + 1 + k);
                    let t = self.s.get(start..run).unwrap_or(&[]);
                    push_text(&mut segs, &mut self.text, self.floor, t, true)?;
                    self.i = run;
                }
            }
        }
    }

    /// An expansion at the current `$`.
    fn dollar(&mut self, segs: &mut Segs, quoted: bool) -> Wr<()> {
        self.enter()?;
        let r = self.dollar_inner(segs, quoted);
        self.leave();
        r
    }

    fn dollar_inner(&mut self, segs: &mut Segs, quoted: bool) -> Wr<()> {
        let Some(n) = self.peek_at(1) else {
            push_text(segs, &mut self.text, self.floor, b"$", quoted)?;
            self.i += 1;
            return Ok(());
        };
        let simple = |name: Name| Seg::Param { name, form: Form::Plain, quoted };
        match n {
            b'(' if self.peek_at(2) == Some(b'(') => {
                self.i += 3;
                let expr = self.arith(false)?;
                segs.push(Seg::Arith { expr, quoted })?;
            }
            b'(' => {
                let end = self.command_end(self.i + 2)?;
                let at = (self.i + 2, end);
                self.i = end + 1;
                if self.nocmd {
                    return Err(WordError::CmdSub);
                }
                segs.push(Seg::Cmd { at, input: true, quoted })?;
            }
            b'[' => {
                self.i += 2;
                let expr = self.arith(true)?;
                segs.push(Seg::Arith { expr, quoted })?;
            }
            b'{' => {
                self.i += 2;
                let p = self.braced(quoted)?;
                segs.push(p)?;
            }
            c if is_name_start(c) => {
                let start = self.i + 1;
                let mut k = start;
                while self.s.get(k).is_some_and(|&c| is_name(c)) {
                    k += 1;
                }
                segs.push(simple(Name::Var((start, k))))?;
                self.i = k;
            }
            c if c.is_ascii_digit() => {
                segs.push(simple(Name::Positional(usize::from(c - b'0'))))?;
                self.i += 2;
            }
            b'@' | b'*' | b'#' | b'$' => {
                let name = match n {
                    b'@' => Name::At,
                    b'*' => Name::Star,
                    b'#' => Name::Count,
                    _ => Name::Pid,
                };
                segs.push(simple(name))?;
                self.i += 2;
            }
            b'?' | b'-' | b'!' => {
                // Left as written (as glibc; POSIX leaves them unspecified for wordexp).
                push_text(segs, &mut self.text, self.floor, &[b'$', n], quoted)?;
                self.i += 2;
            }
            _ => {
                push_text(segs, &mut self.text, self.floor, b"$", quoted)?;
                self.i += 1;
            }
        }
        Ok(())
    }

    /// A `${...}` after its `{`.
    fn braced(&mut self, quoted: bool) -> Wr<Seg> {
        let mut length = false;
        if self.peek() == Some(b'#') && self.peek_at(1).is_some_and(|c| c != b'}') {
            let after = self.peek_at(1).unwrap_or(0);
            if is_name_start(after) || after.is_ascii_digit() || matches!(after, b'@' | b'*' | b'$') {
                length = true;
                self.i += 1;
            }
        }
        let name = match self.peek() {
            Some(c) if is_name_start(c) => {
                let start = self.i;
                while self.peek().is_some_and(is_name) {
                    self.i += 1;
                }
                Name::Var((start, self.i))
            }
            Some(c) if c.is_ascii_digit() => {
                let mut v = 0usize;
                while let Some(d) = self.peek().filter(u8::is_ascii_digit) {
                    v = v.saturating_mul(10).saturating_add(usize::from(d - b'0'));
                    self.i += 1;
                }
                Name::Positional(v)
            }
            Some(b'@') => {
                self.i += 1;
                Name::At
            }
            Some(b'*') => {
                self.i += 1;
                Name::Star
            }
            Some(b'$') => {
                self.i += 1;
                Name::Pid
            }
            _ => return Err(WordError::Syntax),
        };
        if length {
            if self.peek() != Some(b'}') {
                return Err(WordError::Syntax);
            }
            self.i += 1;
            return Ok(Seg::Param { name, form: Form::Length, quoted });
        }
        let Some(c) = self.peek() else { return Err(WordError::Syntax) };
        if c == b'}' {
            self.i += 1;
            return Ok(Seg::Param { name, form: Form::Plain, quoted });
        }
        let (op, colon, len) = match (c, self.peek_at(1)) {
            (b':', Some(b'-')) => (Op::Default, true, 2),
            (b':', Some(b'=')) => (Op::Assign, true, 2),
            (b':', Some(b'?')) => (Op::Error, true, 2),
            (b':', Some(b'+')) => (Op::Alt, true, 2),
            (b'-', _) => (Op::Default, false, 1),
            (b'=', _) => (Op::Assign, false, 1),
            (b'?', _) => (Op::Error, false, 1),
            (b'+', _) => (Op::Alt, false, 1),
            (b'%', Some(b'%')) => (Op::SuffixLong, false, 2),
            (b'%', _) => (Op::SuffixShort, false, 1),
            (b'#', Some(b'#')) => (Op::PrefixLong, false, 2),
            (b'#', _) => (Op::PrefixShort, false, 1),
            _ => return Err(WordError::Syntax),
        };
        self.i += len;
        let word = self.brace_word(quoted)?;
        Ok(Seg::Param { name, form: Form::Op { op, colon, word }, quoted })
    }

    /// The word of a `${name<op>word}`, up to and including the `}` that closes it (brace
    /// levels counted, quoted strings and substitutions skipped: XCU 2.6.2).
    fn brace_word(&mut self, quoted: bool) -> Wr<Range> {
        self.enter()?;
        let mut segs = Segs::new();
        let mut depth = 0usize;
        let mut first = true;
        let r = loop {
            let Some(b) = self.peek() else { break Err(WordError::Syntax) };
            let at_start = first;
            first = false;
            match b {
                b'}' if depth == 0 => {
                    self.i += 1;
                    break Ok(());
                }
                b'}' => {
                    depth -= 1;
                    push_text(&mut segs, &mut self.text, self.floor, b"}", quoted)?;
                    self.i += 1;
                }
                b'{' => {
                    depth += 1;
                    push_text(&mut segs, &mut self.text, self.floor, b"{", quoted)?;
                    self.i += 1;
                }
                b'\\' => {
                    let Some(n) = self.peek_at(1) else { break Err(WordError::Syntax) };
                    self.i += 2;
                    if n == b'\n' {
                        continue;
                    }
                    if quoted && !matches!(n, b'$' | b'`' | b'"' | b'\\' | b'}') {
                        push_text(&mut segs, &mut self.text, self.floor, &[b'\\', n], true)?;
                    } else {
                        push_text(&mut segs, &mut self.text, self.floor, &[n], true)?;
                    }
                }
                b'\'' if !quoted => {
                    let start = self.i + 1;
                    let Some(len) = self.s.get(start..).and_then(|r| r.iter().position(|&c| c == b'\'')) else { break Err(WordError::Syntax) };
                    let t = self.s.get(start..start + len).unwrap_or(&[]);
                    let at = self.text.len();
                    self.text.extend_from_slice(t)?;
                    segs.push(Seg::Text { at: (at, self.text.len()), quoted: true, input: false })?;
                    self.i = start + len + 1;
                }
                b'"' => {
                    self.i += 1;
                    segs.push(Seg::Text { at: (self.text.len(), self.text.len()), quoted: true, input: false })?;
                    if let Err(e) = self.dquote(&mut segs) {
                        break Err(e);
                    }
                }
                b'$' => {
                    if let Err(e) = self.dollar(&mut segs, quoted) {
                        break Err(e);
                    }
                }
                b'`' => {
                    if let Err(e) = self.backquote(&mut segs, quoted) {
                        break Err(e);
                    }
                }
                b'~' if at_start && !quoted => match self.tilde_prefix_in_brace() {
                    Some((a, z)) => {
                        segs.push(Seg::Tilde((a, z)))?;
                        self.i = z;
                    }
                    None => {
                        push_text(&mut segs, &mut self.text, self.floor, b"~", false)?;
                        self.i += 1;
                    }
                },
                _ => {
                    push_text(&mut segs, &mut self.text, self.floor, &[b], quoted)?;
                    self.i += 1;
                }
            }
        };
        self.leave();
        r.and_then(|()| self.stash(&segs))
    }

    /// A tilde prefix at the start of a `${...}` word (ends at `/`, `:` or the closing `}`).
    fn tilde_prefix_in_brace(&self) -> Option<(usize, usize)> {
        let start = self.i + 1;
        let mut k = start;
        loop {
            match self.s.get(k).copied() {
                Some(b'/' | b':' | b'}') => return Some((start, k)),
                None | Some(b'\\' | b'\'' | b'"' | b'$' | b'`' | b'{' | b' ' | b'\t' | b'\n') => return None,
                Some(_) => k += 1,
            }
        }
    }
}

// ------------------------------------------------------------------------------ expansion

/// How a piece of a word's expansion behaves afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    /// Quoted: neither split nor expanded as a pathname.
    #[default]
    Quoted,
    /// The word's own unquoted text: expanded as a pathname, not split.
    Literal,
    /// The result of an unquoted expansion: split and expanded as a pathname.
    Expanded,
}

/// A word being expanded: its pieces (ranges of one buffer), and field breaks (`"$@"`). A
/// quoted piece, even empty, makes the word a field.
///
/// Inline buffers: a usual word expands without an allocation. The pieces are consecutive
/// ranges of `bytes`: where each ends, and its kind (two arrays of plain values, which are built
/// in place, not moved; WORDEXP-2).
struct Pieces {
    bytes: InlineVec<u8, 128>,
    ends: InlineVec<usize, 8>,
    kinds: InlineVec<Kind, 8>,
    breaks: InlineVec<usize, 2>,
}

impl Pieces {
    #[inline(always)]
    fn new() -> Pieces {
        Pieces { bytes: InlineVec::new(), ends: InlineVec::new(), kinds: InlineVec::new(), breaks: InlineVec::new() }
    }

    /// Empty again, keeping the allocations (the next word).
    fn clear(&mut self) {
        self.bytes.clear();
        self.ends.clear();
        self.kinds.clear();
        self.breaks.clear();
    }

    fn push(&mut self, b: &[u8], k: Kind) -> Result<()> {
        self.bytes.extend_from_slice(b)?;
        self.ends.push(self.bytes.len())?;
        self.kinds.push(k)
    }

    fn brk(&mut self) -> Result<()> {
        self.breaks.push(self.ends.len())
    }

    /// The pieces in order: bytes and kind.
    fn iter(&self) -> impl Iterator<Item = (&[u8], Kind)> {
        let bytes = self.bytes.as_slice();
        let ends = self.ends.as_slice();
        ends.iter().zip(self.kinds.as_slice()).enumerate().map(move |(i, (&z, &k))| {
            let a = i.checked_sub(1).and_then(|j| ends.get(j)).copied().unwrap_or(0);
            (bytes.get(a..z).unwrap_or(&[]), k)
        })
    }

    /// The concatenated text (for `${X:=w}`, `${X:?w}`, arithmetic): the pieces are
    /// consecutive ranges of `bytes`.
    fn text(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// The text as a pattern (`${X%word}`): quoted pieces escaped.
    fn pattern(&self) -> Result<TryVec<u8>> {
        let mut t = TryVec::new();
        for (b, k) in self.iter() {
            for &c in b.iter() {
                if k == Kind::Quoted && matches!(c, b'*' | b'?' | b'[' | b']' | b'\\') {
                    t.push(b'\\')?;
                }
                t.push(c)?;
            }
        }
        Ok(t)
    }
}

/// A field being built: its bytes, and the same as a pathname pattern (a backslash from an
/// expansion is literal; so is a quoted pattern character). Reused field after field.
///
/// The pattern is the bytes themselves until a byte needs an escape (a backslash, or an inactive
/// pattern character): only then is it kept apart (`same` false). `magic`: an active `*`, `?`
/// or `[` was added (without one the pattern has no metacharacter, so no pathname expansion).
struct Field {
    bytes: InlineVec<u8, 128>,
    pattern: TryVec<u8>,
    same: bool,
    magic: bool,
    started: bool,
}

impl Field {
    #[inline(always)]
    fn new() -> Field {
        Field { bytes: InlineVec::new(), pattern: TryVec::new(), same: true, magic: false, started: false }
    }

    /// Empty again, keeping the allocations.
    fn clear(&mut self) {
        self.bytes.clear();
        self.pattern.clear();
        self.same = true;
        self.magic = false;
        self.started = false;
    }

    /// Append `b`; `active`: its pattern characters are active.
    fn add(&mut self, b: &[u8], active: bool) -> Result<()> {
        self.started = true;
        // One scan answers both questions for the usual text (no pattern character at all).
        if self.same && !b.iter().any(|&c| matches!(c, b'\\' | b'*' | b'?' | b'[' | b']')) {
            return self.bytes.extend_from_slice(b);
        }
        if active && !self.magic {
            self.magic = b.iter().any(|&c| matches!(c, b'*' | b'?' | b'['));
        }
        let escape = |c: u8| c == b'\\' || (!active && matches!(c, b'*' | b'?' | b'[' | b']'));
        if self.same {
            if !b.iter().any(|&c| escape(c)) {
                return self.bytes.extend_from_slice(b);
            }
            // The first byte that needs an escape: the pattern goes its own way from here.
            self.pattern.clear();
            self.pattern.extend_from_slice(self.bytes.as_slice())?;
            self.same = false;
        }
        self.bytes.extend_from_slice(b)?;
        for &c in b {
            if escape(c) {
                self.pattern.push(b'\\')?;
            }
            self.pattern.push(c)?;
        }
        Ok(())
    }

    /// The field as a pathname pattern.
    fn pattern(&self) -> &[u8] {
        if self.same { self.bytes.as_slice() } else { &self.pattern }
    }
}

struct Expander<'a, S: Shell, W: Words> {
    sh: &'a mut S,
    out: &'a mut W,
    /// The text given to `wordexp` (names and tilde users are ranges of it).
    input: &'a [u8],
    /// The parser's text buffer (`Seg::Text` ranges).
    text: &'a [u8],
    /// The segments of nested words (`Form::Op`, `Seg::Arith`).
    arena: &'a [Seg],
    flags: i32,
    loc: &'a Locale,
    /// `IFS` (`None`: unset).
    ifs: Option<ValBuf>,
    /// `IFS` as a set of bytes, and the bytes below which it decides alone: every byte in the
    /// single-byte form; ASCII in UTF-8 (an ASCII byte is always a character of its own).
    ifs_set: Option<([u64; 4], usize)>,
}

/// The byte set of `ifs` (see `Expander::ifs_set`) in `loc`, when it decides membership.
fn ifs_set(ifs: Option<&[u8]>, loc: &Locale) -> Option<([u64; 4], usize)> {
    let limit = match Mode::of(loc) {
        Mode::Bytes => 256,
        Mode::Multi(crate::text::Encoding::Utf8) => 128,
        Mode::Multi(_) => return None,
    };
    let mut set = [0u64; 4];
    for &b in ifs.unwrap_or(b" \t\n") {
        if let Some(w) = set.get_mut(usize::from(b >> 6)) {
            *w |= 1 << (b & 63);
        }
    }
    Some((set, limit))
}

/// `v` in decimal.
fn decimal(v: i128) -> Result<InlineVec<u8, 48>> {
    let mut out = InlineVec::new();
    // Every value a caller passes fits 64 bits: divide in 64 bits (a 128-bit division is a
    // library call per digit).
    if let Ok(mut n) = u64::try_from(v.unsigned_abs()) {
        let mut buf = [0u8; 20];
        let mut k = buf.len();
        loop {
            k -= 1;
            if let Some(slot) = buf.get_mut(k) {
                *slot = b'0' + u8::try_from(n % 10).unwrap_or(0);
            }
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if v < 0 {
            out.push(b'-')?;
        }
        out.extend_from_slice(buf.get(k..).unwrap_or(&[]))?;
        return Ok(out);
    }
    let mut rev: InlineVec<u8, 48> = InlineVec::new();
    let mut n = v.unsigned_abs();
    loop {
        rev.push(b'0' + u8::try_from(n % 10).unwrap_or(0))?;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        rev.push(b'-')?;
    }
    rev.as_mut_slice().reverse();
    Ok(rev)
}

impl<'a, S: Shell, W: Words> Expander<'a, S, W> {
    fn undef(&self) -> bool {
        self.flags & UNDEF != 0
    }

    /// The input's bytes in `at`.
    fn input(&self, at: Span) -> &'a [u8] {
        self.input.get(at.0..at.1).unwrap_or(&[])
    }

    /// The value of a parameter into `v`; `false` when it is unset.
    fn lookup(&mut self, name: &Name, v: &mut ValBuf) -> Wr<bool> {
        v.clear();
        let set = match name {
            Name::Var(n) => self.sh.var(self.input(*n), v)?,
            Name::Positional(i) => self.sh.arg(*i, v)?,
            Name::Count => {
                v.extend_from_slice(decimal(i128::try_from(self.sh.arg_count()).unwrap_or(0))?.as_slice())?;
                true
            }
            Name::Pid => {
                v.extend_from_slice(decimal(i128::from(self.sh.pid()))?.as_slice())?;
                true
            }
            Name::At | Name::Star => {
                let n = self.sh.arg_count();
                let mut a = ValBuf::new();
                for i in 1..=n {
                    if i > 1 {
                        v.push(b' ')?;
                    }
                    self.sh.arg(i, &mut a)?;
                    v.extend_from_slice(&a)?;
                }
                true
            }
        };
        Ok(set)
    }

    /// The characters of `v` in the locale (bytes where it is not valid text).
    fn char_bounds(&self, v: &[u8]) -> Result<TryVec<usize>> {
        let mode = Mode::of(self.loc);
        let mut b = TryVec::new();
        let mut i = 0;
        loop {
            b.push(i)?;
            match text::unit_at(mode, v, i) {
                Unit::Char(_, n) => i += n.max(1),
                Unit::Invalid(_) => i += 1,
                Unit::End => return Ok(b),
            }
        }
    }

    /// The number of characters of `v` (bytes where it is not valid text), as
    /// [`Self::char_bounds`] counts them, without a list.
    fn char_count(&self, v: &[u8]) -> usize {
        let mode = Mode::of(self.loc);
        if v.is_ascii() && matches!(mode, Mode::Bytes | Mode::Multi(crate::text::Encoding::Utf8)) {
            return v.len();
        }
        let (mut i, mut n) = (0, 0usize);
        loop {
            match text::unit_at(mode, v, i) {
                Unit::Char(_, k) => i += k.max(1),
                Unit::Invalid(_) => i += 1,
                Unit::End => return n,
            }
            n += 1;
        }
    }

    fn param(&mut self, name: &Name, form: &Form, quoted: bool, p: &mut Pieces) -> Wr<()> {
        let kind = if quoted { Kind::Quoted } else { Kind::Expanded };
        let checks_undef = matches!(name, Name::Var(_) | Name::Positional(_));
        // `$@` and `$*`: one field per argument (unquoted, and `"$@"`).
        if matches!(form, Form::Plain) && matches!(name, Name::At | Name::Star) && !(quoted && matches!(name, Name::Star)) {
            let n = self.sh.arg_count();
            let mut a = ValBuf::new();
            for i in 1..=n {
                if i > 1 {
                    p.brk()?;
                }
                self.sh.arg(i, &mut a)?;
                p.push(&a, kind)?;
            }
            return Ok(());
        }
        if quoted && matches!(name, Name::Star) && matches!(form, Form::Plain) {
            let sep: TryVec<u8> = match &self.ifs {
                None => TryVec::from_slice(b" ")?,
                Some(i) => TryVec::from_slice(i.get(..1).unwrap_or(&[]))?,
            };
            let n = self.sh.arg_count();
            let mut v = TryVec::new();
            let mut a = ValBuf::new();
            for i in 1..=n {
                if i > 1 {
                    v.extend_from_slice(&sep)?;
                }
                self.sh.arg(i, &mut a)?;
                v.extend_from_slice(&a)?;
            }
            p.push(&v, kind)?;
            return Ok(());
        }
        let mut v = ValBuf::new();
        let set = self.lookup(name, &mut v)?;
        self.param_value(name, form, kind, checks_undef, set.then_some(v.as_slice()), p)
    }

    /// [`Self::param`] for a parameter whose value is `v` (`None`: unset).
    fn param_value(&mut self, name: &Name, form: &Form, kind: Kind, checks_undef: bool, v: Option<&[u8]>, p: &mut Pieces) -> Wr<()> {
        let quoted = kind == Kind::Quoted;
        match form {
            Form::Plain => {
                if v.is_none() && checks_undef && self.undef() {
                    return Err(WordError::BadVal);
                }
                p.push(v.unwrap_or(&[]), kind)?;
            }
            Form::Length => {
                let Some(v) = v else {
                    if checks_undef && self.undef() {
                        return Err(WordError::BadVal);
                    }
                    p.push(b"0", kind)?;
                    return Ok(());
                };
                let n = self.char_count(v);
                p.push(decimal(i128::try_from(n).unwrap_or(0))?.as_slice(), kind)?;
            }
            Form::Op { op, colon, word } => {
                let word = self.arena.get(word.0..word.1).unwrap_or(&[]);
                if v.is_none() && checks_undef && self.undef() {
                    return Err(WordError::BadVal);
                }
                let set = v.is_some();
                let null = v.as_ref().is_none_or(|x| x.is_empty());
                let test = if *colon { set && !null } else { set };
                match op {
                    Op::Default => {
                        if test {
                            p.push(v.unwrap_or(&[]), kind)?;
                        } else {
                            self.segs(word, quoted, true, p)?;
                        }
                    }
                    Op::Alt => {
                        if test {
                            self.segs(word, quoted, true, p)?;
                        } else {
                            p.push(b"", kind)?;
                        }
                    }
                    Op::Assign => {
                        if test {
                            p.push(v.unwrap_or(&[]), kind)?;
                        } else {
                            let mut w = Pieces::new();
                            self.segs(word, true, true, &mut w)?;
                            let value = w.text();
                            match name {
                                Name::Var(n) => self.sh.set_var(self.input(*n), value)?,
                                _ => return Err(WordError::Syntax),
                            }
                            p.push(value, kind)?;
                        }
                    }
                    Op::Error => {
                        if test {
                            p.push(v.unwrap_or(&[]), kind)?;
                        } else {
                            let mut w = Pieces::new();
                            self.segs(word, true, true, &mut w)?;
                            let mut msg = TryVec::new();
                            match name {
                                Name::Var(n) => msg.extend_from_slice(self.input(*n))?,
                                Name::Positional(i) => msg.extend_from_slice(decimal(i128::try_from(*i).unwrap_or(0))?.as_slice())?,
                                _ => {}
                            }
                            msg.extend_from_slice(b": ")?;
                            let text = w.text();
                            if word.is_empty() {
                                msg.extend_from_slice(b"parameter null or not set")?;
                            } else {
                                msg.extend_from_slice(text)?;
                            }
                            msg.push(b'\n')?;
                            self.sh.stderr(&msg);
                            return Err(WordError::BadVal);
                        }
                    }
                    Op::SuffixShort | Op::SuffixLong | Op::PrefixShort | Op::PrefixLong => {
                        let v = v.unwrap_or(&[]);
                        let mut w = Pieces::new();
                        self.segs(word, quoted, true, &mut w)?;
                        let pat = Pattern::new(&w.pattern()?, 0, self.loc)?;
                        let bounds = self.char_bounds(v)?;
                        let n = bounds.len();
                        let mut cut: Option<(usize, usize)> = None;
                        let loc = self.loc;
                        let mut try_cut = |a: usize, b: usize| -> Result<bool> {
                            if pat.matches(v.get(a..b).unwrap_or(&[]), loc)? {
                                cut = Some((a, b));
                                return Ok(true);
                            }
                            Ok(false)
                        };
                        let len = v.len();
                        // Prefix removal: one pass over the value tells which prefixes match
                        // (a match per prefix was quadratic in the value's length).
                        let prefix_ends = if matches!(op, Op::PrefixShort | Op::PrefixLong) { pat.prefix_matches(v, loc)? } else { None };
                        // Suffix removal likewise: one backward pass for all suffixes.
                        let suffix_starts = if matches!(op, Op::SuffixShort | Op::SuffixLong) { pat.suffix_matches(v, loc)? } else { None };
                        match op {
                            Op::PrefixShort | Op::PrefixLong if prefix_ends.is_some() => {
                                let hit = |&e: &usize| prefix_ends.as_ref().and_then(|m| m.get(e)).copied().unwrap_or(false);
                                let found = if matches!(op, Op::PrefixShort) { bounds.iter().find(|&e| hit(e)) } else { bounds.iter().rev().find(|&e| hit(e)) };
                                if let Some(&e) = found {
                                    cut = Some((0, e));
                                }
                            }
                            Op::SuffixShort | Op::SuffixLong if suffix_starts.is_some() => {
                                let hit = |&s: &usize| suffix_starts.as_ref().and_then(|m| m.get(s)).copied().unwrap_or(false);
                                // Shortest = the latest start, longest = the earliest.
                                let found = if matches!(op, Op::SuffixShort) { bounds.iter().rev().find(|&s| hit(s)) } else { bounds.iter().find(|&s| hit(s)) };
                                if let Some(&s) = found {
                                    cut = Some((s, len));
                                }
                            }
                            Op::PrefixShort => {
                                for &e in bounds.iter() {
                                    if try_cut(0, e)? {
                                        break;
                                    }
                                }
                            }
                            Op::PrefixLong => {
                                for k in (0..n).rev() {
                                    if try_cut(0, bounds.get(k).copied().unwrap_or(0))? {
                                        break;
                                    }
                                }
                            }
                            Op::SuffixShort => {
                                for k in (0..n).rev() {
                                    if try_cut(bounds.get(k).copied().unwrap_or(0), len)? {
                                        break;
                                    }
                                }
                            }
                            _ => {
                                for &s in bounds.iter() {
                                    if try_cut(s, len)? {
                                        break;
                                    }
                                }
                            }
                        }
                        let rest: &[u8] = match cut {
                            Some((0, b)) if matches!(op, Op::PrefixShort | Op::PrefixLong) => v.get(b..).unwrap_or(&[]),
                            Some((a, _)) if matches!(op, Op::SuffixShort | Op::SuffixLong) => v.get(..a).unwrap_or(&[]),
                            _ => v,
                        };
                        p.push(rest, kind)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Expand `segs` into `p`. `quoted`: inside double quotes; `in_word`: the word of a
    /// `${...}` (its unquoted text then behaves as an expansion result).
    fn segs(&mut self, segs: &[Seg], quoted: bool, in_word: bool, p: &mut Pieces) -> Wr<()> {
        for s in segs {
            match s {
                Seg::Text { at, quoted: q, input } => {
                    let bytes = seg_text(self.input, self.text, *at, *input);
                    let k = if *q || quoted {
                        Kind::Quoted
                    } else if in_word {
                        Kind::Expanded
                    } else {
                        Kind::Literal
                    };
                    p.push(bytes, k)?;
                }
                Seg::Tilde(user) => {
                    let user = self.input(*user);
                    let mut h = ValBuf::new();
                    let known = self.sh.home(if user.is_empty() { None } else { Some(user) }, &mut h)?;
                    if known {
                        p.push(&h, Kind::Quoted)?;
                    } else {
                        let mut t = TryVec::from_slice(b"~")?;
                        t.extend_from_slice(user)?;
                        p.push(&t, if in_word { Kind::Expanded } else { Kind::Literal })?;
                    }
                }
                Seg::Param { name, form, quoted: q } => self.param(name, form, *q || quoted, p)?,
                Seg::Cmd { at, input, quoted: q } => {
                    let mut o = TryVec::new();
                    let text = seg_text(self.input, self.text, *at, *input);
                    self.sh.run(text, self.flags & SHOWERR != 0, &mut o)?;
                    while o.last() == Some(&b'\n') {
                        o.pop();
                    }
                    p.push(&o, if *q || quoted { Kind::Quoted } else { Kind::Expanded })?;
                }
                Seg::Arith { expr, quoted: q } => {
                    let expr = self.arena.get(expr.0..expr.1).unwrap_or(&[]);
                    let undef = self.undef();
                    let v = if let [Seg::Text { at, input, .. }] = expr {
                        // Text alone (the usual expression): evaluated where it lies, no expansion.
                        let text = seg_text(self.input, self.text, *at, *input);
                        arith::evaluate(text, undef, &mut |name: &[u8], out: &mut ValBuf| self.sh.var(name, out))?
                    } else {
                        let mut e = Pieces::new();
                        self.segs(expr, true, false, &mut e)?;
                        let text = e.text();
                        arith::evaluate(text, undef, &mut |name: &[u8], out: &mut ValBuf| self.sh.var(name, out))?
                    };
                    p.push(decimal(i128::from(v))?.as_slice(), if *q || quoted { Kind::Quoted } else { Kind::Expanded })?;
                }
            }
        }
        Ok(())
    }

    /// Whether the byte at `v[i]` is in `IFS` when the IFS byte set decides it alone (a
    /// character of its own: see `ifs_set`); `None` otherwise.
    #[inline]
    fn ifs_byte(&self, v: &[u8], i: usize) -> Option<bool> {
        let (set, limit) = self.ifs_set.as_ref()?;
        let b = *v.get(i)?;
        if usize::from(b) >= *limit {
            return None;
        }
        Some(set.get(usize::from(b >> 6)).is_some_and(|w| w & (1 << (b & 63)) != 0))
    }

    /// Whether `c` (a character at `v[i..]`) is in `IFS`, and whether it is IFS white space.
    fn ifs_class(&self, v: &[u8], i: usize) -> (bool, bool, usize) {
        if let (Some((set, limit)), Some(&b)) = (&self.ifs_set, v.get(i)) {
            if usize::from(b) < *limit {
                let inifs = set.get(usize::from(b >> 6)).is_some_and(|w| w & (1 << (b & 63)) != 0);
                return (inifs, inifs && matches!(b, b' ' | b'\t' | b'\n'), 1);
            }
        }
        let mode = Mode::of(self.loc);
        let (c, n) = match text::unit_at(mode, v, i) {
            Unit::Char(c, n) => (Some(c), n.max(1)),
            _ => (None, 1),
        };
        let ifs: &[u8] = match &self.ifs {
            None => b" \t\n",
            Some(x) => x,
        };
        let Some(c) = c else {
            // An invalid byte is in IFS when IFS holds that byte.
            let b = v.get(i).copied().unwrap_or(0);
            return (ifs.contains(&b), false, 1);
        };
        let mut k = 0;
        while k < ifs.len() {
            match text::unit_at(mode, ifs, k) {
                Unit::Char(d, m) => {
                    if d == c {
                        let white = matches!(c, 0x20 | 0x09 | 0x0a);
                        return (true, white, n);
                    }
                    k += m.max(1);
                }
                _ => {
                    if ifs.get(k) == v.get(i) && n == 1 {
                        return (true, false, 1);
                    }
                    k += 1;
                }
            }
        }
        (false, false, n)
    }

    /// Split the pieces into fields and hand each, pathname-expanded, to the sink, in order
    /// (`cur` is the field being built, reused).
    fn emit(&mut self, p: &Pieces, cur: &mut Field) -> Wr<()> {
        cur.clear();
        let mut after_white = false;
        let splitting = self.ifs.as_ref().is_none_or(|i| !i.is_empty());
        for (idx, (bytes, kind)) in p.iter().enumerate() {
            if p.breaks.as_slice().contains(&idx) && idx > 0 {
                self.field(cur)?;
                cur.clear();
                cur.started = true;
                after_white = false;
            }
            match kind {
                Kind::Quoted => {
                    cur.add(bytes, false)?;
                    after_white = false;
                }
                Kind::Literal => {
                    cur.add(bytes, true)?;
                    after_white = false;
                }
                Kind::Expanded if !splitting => {
                    if !bytes.is_empty() {
                        cur.add(bytes, true)?;
                        after_white = false;
                    }
                }
                Kind::Expanded => {
                    // The result's characters, each (in IFS, IFS white space); IFS white space
                    // at the start and the end of the result is dropped: first the extent
                    // without it, then its characters.
                    let (mut first, mut end, mut k) = (None, 0, 0);
                    while k < bytes.len() {
                        // A byte the IFS set decides alone: no character decoding.
                        if let Some(inifs) = self.ifs_byte(bytes, k) {
                            if !(inifs && matches!(bytes.get(k), Some(b' ' | b'\t' | b'\n'))) {
                                first = first.or(Some(k));
                                end = k + 1;
                            }
                            k += 1;
                            continue;
                        }
                        let (inifs, white, n) = self.ifs_class(bytes, k);
                        if !(inifs && white) {
                            first = first.or(Some(k));
                            end = k + n;
                        }
                        k += n;
                    }
                    let mut k = first.unwrap_or(end);
                    while k < end {
                        // A run of bytes the IFS set decides alone and outside it: one piece
                        // (adding them one by one gives the same field).
                        let mut run = k;
                        while run < end && self.ifs_byte(bytes, run) == Some(false) {
                            run += 1;
                        }
                        if run > k {
                            cur.add(bytes.get(k..run).unwrap_or(&[]), true)?;
                            after_white = false;
                            k = run;
                            continue;
                        }
                        let (inifs, white, n) = self.ifs_class(bytes, k);
                        let ch = bytes.get(k..k + n).unwrap_or(&[]);
                        k += n;
                        if !inifs {
                            cur.add(ch, true)?;
                            after_white = false;
                        } else if white {
                            if cur.started {
                                self.field(cur)?;
                                cur.clear();
                                after_white = true;
                            }
                        } else {
                            // Another IFS character ends a field; with only IFS white space
                            // since the last field, it delimits that field (no empty one).
                            if cur.started {
                                self.field(cur)?;
                                cur.clear();
                            } else if !after_white {
                                self.out.push(&[])?;
                            }
                            after_white = false;
                        }
                    }
                }
            }
        }
        if cur.started {
            self.field(cur)?;
        }
        Ok(())
    }

    /// One field: pathname expansion when it has an active pattern character, then the sink.
    fn field(&mut self, f: &Field) -> Wr<()> {
        let pattern = f.pattern();
        if f.magic && super::glob::is_magic(pattern, true) {
            let mut found = TryVec::new();
            self.sh.glob(pattern, &mut found)?;
            if !found.is_empty() {
                for m in found.iter() {
                    self.out.push(m)?;
                }
                return Ok(());
            }
        }
        self.out.push(f.bytes.as_slice())?;
        Ok(())
    }
}

/// The field of a word of text alone (`w`, its segments; `text`, the parser's buffer) when no
/// field buffer is needed for it: its segments are consecutive ranges of `text` (they are, unless
/// one was moved) and none of its unquoted text holds a pattern character (no pathname expansion,
/// so the field is the bytes as they stand; quoted text is never active). `None` otherwise.
fn plain_word<'t>(w: &[Seg], input: &'t [u8], text: &'t [u8]) -> Option<&'t [u8]> {
    let (Some(Seg::Text { at: first, input: src, .. }), Some(Seg::Text { at: last, .. })) = (w.first(), w.last()) else { return None };
    let all = if *src { input } else { text };
    let mut next = first.0;
    for s in w {
        let Seg::Text { at, quoted, input } = s else { return None };
        if input != src || at.0 != next || (!*quoted && all.get(at.0..at.1)?.iter().any(|&c| matches!(c, b'*' | b'?' | b'['))) {
            return None;
        }
        next = at.1;
    }
    all.get(first.0..last.1)
}

/// The bytes of a `Seg::Text`: `at` in the input (`from_input`) or in the parser's text buffer.
#[inline]
fn seg_text<'t>(input: &'t [u8], text: &'t [u8], at: Span, from_input: bool) -> &'t [u8] {
    if from_input { input } else { text }.get(at.0..at.1).unwrap_or(&[])
}

/// `wordexp(words, flags)` in `loc`: the fields, in order, to `out`. On `WRDE_NOSPACE` the
/// fields given so far are the partial result; on any other error the caller discards them.
pub fn wordexp<S: Shell, W: Words>(words: &[u8], flags: i32, loc: &Locale, sh: &mut S, out: &mut W) -> core::result::Result<(), WordError> {
    // The words' text is about as long as the input: one allocation for it.
    let mut parser = Parser { s: words, i: 0, nocmd: flags & NOCMD != 0, depth: 0, text: InlineVec::new(), floor: 0, ends: InlineVec::new(), arena: InlineVec::new() };
    let mut segs = Segs::new();
    parser.words(&mut segs)?;
    let parsed = segs.as_slice();
    // The parser stays where it is: its buffers are borrowed, not moved.
    let (text, ends, arena) = (parser.text.as_slice(), parser.ends.as_slice(), parser.arena.as_slice());
    // `IFS` is looked up before the first expansion (a word of text alone never needs it: none
    // when every word is such text).
    let mut ex = Expander { sh, out, input: words, text, arena, flags, loc, ifs: None, ifs_set: None };
    let mut ifs_known = false;
    // Inline buffers: a usual word allocates nothing here.
    let mut p = Pieces::new();
    let mut field = Field::new();
    let mut start = 0;
    for &end in ends {
        let w = parsed.get(start..end).unwrap_or(&[]);
        start = end;
        if let Some(b) = plain_word(w, words, text) {
            // Its text as it stands is the field: straight to the sink (no field buffer).
            ex.out.push(b)?;
            continue;
        }
        if w.iter().all(|s| matches!(s, Seg::Text { .. })) {
            // Text alone (no expansion): nothing to split, so the pieces go straight into the
            // field, as `emit` would add them (quoted text inactive, the word's own active).
            field.clear();
            for s in w {
                if let Seg::Text { at, quoted, input } = s {
                    field.add(seg_text(words, text, *at, *input), !*quoted)?;
                }
            }
            if field.started {
                ex.field(&field)?;
            }
            continue;
        }
        if !ifs_known {
            ifs_known = true;
            let mut v = ValBuf::new();
            ex.ifs = if ex.sh.var(b"IFS", &mut v)? { Some(v) } else { None };
            ex.ifs_set = ifs_set(ex.ifs.as_deref(), loc);
        }
        p.clear();
        ex.segs(w, false, false, &mut p)?;
        ex.emit(&p, &mut field)?;
    }
    Ok(())
}

pub mod arith {
    //! Arithmetic expansion (XCU 2.6.4; owner ruling 2026-10-03, D-14.5): the POSIX shell
    //! arithmetic operators on signed 64-bit `long`, as in ISO C: unary `+ - ~ !`, `* / %`,
    //! `+ -`, `<< >>`, `< <= > >=`, `== !=`, `&`, `^`, `|`, `&&`, `||` (short-circuit), `?:`,
    //! parentheses; decimal, octal (`0` prefix) and hexadecimal (`0x`) constants; variables by
    //! name. `WRDE_SYNTAX` for anything else (`++`, `--`, `**`, `,`, a malformed constant or
    //! expression), for division or modulo by zero, a shift count outside 0..63, a left shift
    //! that loses bits, and signed overflow (`+ - *`, unary minus, `LONG_MIN / -1`,
    //! `LONG_MIN % -1`). Assignment operators are refused (owner ruling: expanding a string must
    //! not change the caller's state). A variable that is unset is 0, or `WRDE_BADVAL` with
    //! `WRDE_UNDEF`; one whose value is not an integer constant (an optional sign, then a
    //! constant) is `WRDE_SYNTAX`. The operands of an operator that short-circuiting skips are
    //! parsed but not evaluated (no error from them, no variable read).

    use super::WordError;
    use crate::error::Result;

    /// An operator's spelling as a number (its bytes, little-endian), so that operators are
    /// compared as integers, not as text.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Code(u32);

    /// The code of a spelling of one to three bytes (0 for any other).
    const fn code(s: &str) -> Code {
        match s.as_bytes() {
            [a] => Code(u32::from_le_bytes([*a, 0, 0, 0])),
            [a, b] => Code(u32::from_le_bytes([*a, *b, 0, 0])),
            [a, b, c] => Code(u32::from_le_bytes([*a, *b, *c, 0])),
            _ => Code(0),
        }
    }

    const OR: Code = code("||");
    const AND: Code = code("&&");
    const BIT_OR: Code = code("|");
    const BIT_XOR: Code = code("^");
    const BIT_AND: Code = code("&");
    const EQ: Code = code("==");
    const NE: Code = code("!=");
    const LT: Code = code("<");
    const LE: Code = code("<=");
    const GT: Code = code(">");
    const GE: Code = code(">=");
    const SHL: Code = code("<<");
    const SHR: Code = code(">>");
    const ADD: Code = code("+");
    const SUB: Code = code("-");
    const MUL: Code = code("*");
    const DIV: Code = code("/");
    const REM: Code = code("%");
    const NOT: Code = code("!");
    const INVERT: Code = code("~");
    const QUESTION: Code = code("?");
    const COLON: Code = code(":");
    const OPEN: Code = code("(");
    const CLOSE: Code = code(")");

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Tok {
        Num(i64),
        /// A name: offset and length in the text.
        Name(usize, usize),
        Op(Code),
        End,
    }

    /// The operator at the start of `rest`, longest spelling first: its code and length, or
    /// `WRDE_SYNTAX` for a byte that starts no operator and for the spellings that are refused
    /// (the assignment operators by the owner's ruling, `++`, `--`, `**`, `=` and `,`).
    /// A direct match on the first bytes: no search through a table of spellings.
    fn operator(rest: &[u8]) -> core::result::Result<(Code, usize), WordError> {
        let at = |k: usize| rest.get(k).copied();
        let (a, b) = (at(0).ok_or(WordError::Syntax)?, at(1));
        let two = |c: Code| Ok((c, 2));
        match (a, b) {
            (b'<', Some(b'<')) | (b'>', Some(b'>')) => {
                if at(2) == Some(b'=') {
                    Err(WordError::Syntax)
                } else if a == b'<' {
                    two(SHL)
                } else {
                    two(SHR)
                }
            }
            (b'<', Some(b'=')) => two(LE),
            (b'>', Some(b'=')) => two(GE),
            (b'=', Some(b'=')) => two(EQ),
            (b'!', Some(b'=')) => two(NE),
            (b'&', Some(b'&')) => two(AND),
            (b'|', Some(b'|')) => two(OR),
            (b'+', Some(b'+' | b'=')) | (b'-', Some(b'-' | b'=')) | (b'*', Some(b'*' | b'=')) => Err(WordError::Syntax),
            (b'/' | b'%' | b'&' | b'^' | b'|', Some(b'=')) => Err(WordError::Syntax),
            (b'+', _) => Ok((ADD, 1)),
            (b'-', _) => Ok((SUB, 1)),
            (b'*', _) => Ok((MUL, 1)),
            (b'/', _) => Ok((DIV, 1)),
            (b'%', _) => Ok((REM, 1)),
            (b'<', _) => Ok((LT, 1)),
            (b'>', _) => Ok((GT, 1)),
            (b'&', _) => Ok((BIT_AND, 1)),
            (b'^', _) => Ok((BIT_XOR, 1)),
            (b'|', _) => Ok((BIT_OR, 1)),
            (b'!', _) => Ok((NOT, 1)),
            (b'~', _) => Ok((INVERT, 1)),
            (b'?', _) => Ok((QUESTION, 1)),
            (b':', _) => Ok((COLON, 1)),
            (b'(', _) => Ok((OPEN, 1)),
            (b')', _) => Ok((CLOSE, 1)),
            _ => Err(WordError::Syntax),
        }
    }

    /// A variable lookup: the value into the buffer; `false` when unset.
    pub type Var<'v> = &'v mut dyn FnMut(&[u8], &mut super::ValBuf) -> Result<bool>;

    struct P<'t, 'v> {
        t: &'t [u8],
        i: usize,
        tok: Tok,
        undef: bool,
        var: Var<'v>,
        depth: usize,
        /// Precedence level of the current token when it is a binary operator (`NO_LEVEL` else),
        /// set by `advance` so the climbing loop does not classify the operator again.
        lvl: u8,
    }

    const NO_LEVEL: u8 = 255;

    /// A constant (`0x..`, `0..`, decimal) in `s`, whole.
    fn constant(s: &[u8]) -> Option<i64> {
        let (digits, radix) = match s {
            [b'0', b'x' | b'X', rest @ ..] => (rest, 16),
            [b'0', rest @ ..] if !rest.is_empty() => (rest, 8),
            _ => (s, 10),
        };
        if digits.is_empty() {
            return None;
        }
        let mut v: i64 = 0;
        for &d in digits {
            let x = char::from(d).to_digit(radix)?;
            v = v.checked_mul(i64::from(radix))?.checked_add(i64::from(x))?;
        }
        Some(v)
    }

    impl P<'_, '_> {
        /// The next token.
        fn advance(&mut self) -> core::result::Result<(), WordError> {
            while self.t.get(self.i).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n')) {
                self.i += 1;
            }
            self.lvl = NO_LEVEL;
            let Some(&b) = self.t.get(self.i) else {
                self.tok = Tok::End;
                return Ok(());
            };
            if b.is_ascii_digit() && b != b'0' {
                // A decimal constant (not octal or hexadecimal): digits only, folded as they are read.
                let mut j = self.i;
                let mut v: i64 = 0;
                let mut fits = true;
                while let Some(d) = self.t.get(j).copied().filter(u8::is_ascii_digit) {
                    match v.checked_mul(10).and_then(|x| x.checked_add(i64::from(d - b'0'))) {
                        Some(x) => v = x,
                        None => fits = false,
                    }
                    j += 1;
                }
                if fits && !self.t.get(j).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    self.i = j;
                    self.tok = Tok::Num(v);
                    return Ok(());
                }
            }
            if b.is_ascii_alphanumeric() || b == b'_' {
                let start = self.i;
                while self.t.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    self.i += 1;
                }
                let word = self.t.get(start..self.i).unwrap_or(&[]);
                self.tok = if b.is_ascii_digit() { Tok::Num(constant(word).ok_or(WordError::Syntax)?) } else { Tok::Name(start, self.i - start) };
                return Ok(());
            }
            let (code, len) = operator(self.t.get(self.i..).unwrap_or(&[]))?;
            self.i += len;
            self.tok = Tok::Op(code);
            self.lvl = Self::level(code);
            Ok(())
        }

        fn is(&self, op: Code) -> bool {
            self.tok == Tok::Op(op)
        }

        fn value_of(&mut self, a: usize, n: usize, eval: bool) -> core::result::Result<i64, WordError> {
            if !eval {
                return Ok(0);
            }
            let name = self.t.get(a..a + n).unwrap_or(&[]);
            let mut v = super::ValBuf::new();
            if !(self.var)(name, &mut v)? {
                return if self.undef { Err(WordError::BadVal) } else { Ok(0) };
            }
            let (neg, digits) = match v.split_first() {
                Some((b'-', d)) => (true, d),
                Some((b'+', d)) => (false, d),
                _ => (false, v.as_slice()),
            };
            let c = constant(digits).ok_or(WordError::Syntax)?;
            if neg { c.checked_neg().ok_or(WordError::Syntax) } else { Ok(c) }
        }

        fn primary(&mut self, eval: bool) -> core::result::Result<i64, WordError> {
            match self.tok {
                Tok::Num(v) => {
                    self.advance()?;
                    Ok(v)
                }
                Tok::Name(a, n) => {
                    self.advance()?;
                    self.value_of(a, n, eval)
                }
                Tok::Op(OPEN) => {
                    self.advance()?;
                    let v = self.cond(eval)?;
                    if !self.is(CLOSE) {
                        return Err(WordError::Syntax);
                    }
                    self.advance()?;
                    Ok(v)
                }
                _ => Err(WordError::Syntax),
            }
        }

        fn unary(&mut self, eval: bool) -> core::result::Result<i64, WordError> {
            self.depth += 1;
            if self.depth > 512 {
                return Err(WordError::Syntax);
            }
            let r = match self.tok {
                Tok::Op(op @ (ADD | SUB | INVERT | NOT)) => {
                    self.advance()?;
                    let v = self.unary(eval)?;
                    match op {
                        ADD => Ok(v),
                        SUB => {
                            if eval {
                                v.checked_neg().ok_or(WordError::Syntax)
                            } else {
                                Ok(0)
                            }
                        }
                        INVERT => Ok(!v),
                        _ => Ok(i64::from(v == 0)),
                    }
                }
                _ => self.primary(eval),
            };
            self.depth -= 1;
            r
        }

        /// The binary operators by precedence (0 binds loosest), all left-associative: `||`,
        /// `&&`, `|`, `^`, `&`, `== !=`, `< <= > >=`, `<< >>`, `+ -`, `* / %`.
        fn level(op: Code) -> u8 {
            match op {
                OR => 0,
                AND => 1,
                BIT_OR => 2,
                BIT_XOR => 3,
                BIT_AND => 4,
                EQ | NE => 5,
                LT | LE | GT | GE => 6,
                SHL | SHR => 7,
                ADD | SUB => 8,
                MUL | DIV | REM => 9,
                _ => NO_LEVEL,
            }
        }

        /// The operators of `level` and above, by precedence climbing: the same tree as one
        /// recursive function per level (each level's operators applied left to right, a
        /// tighter one's operands first), with one call per operand instead of one per level.
        fn binary(&mut self, level: usize, eval: bool) -> core::result::Result<i64, WordError> {
            if level >= 10 {
                return self.unary(eval);
            }
            let mut lhs = self.unary(eval)?;
            loop {
                let Tok::Op(op) = self.tok else { return Ok(lhs) };
                let l = usize::from(self.lvl);
                if self.lvl == NO_LEVEL || l < level {
                    return Ok(lhs);
                }
                self.advance()?;
                // Short-circuit: the right side of `&&`/`||` is evaluated only when needed.
                let rhs_eval = match op {
                    AND => eval && lhs != 0,
                    OR => eval && lhs == 0,
                    _ => eval,
                };
                let rhs = self.binary(l + 1, rhs_eval)?;
                if !eval {
                    lhs = 0;
                    continue;
                }
                lhs = match op {
                    OR => i64::from(lhs != 0 || rhs != 0),
                    AND => i64::from(lhs != 0 && rhs != 0),
                    BIT_OR => lhs | rhs,
                    BIT_XOR => lhs ^ rhs,
                    BIT_AND => lhs & rhs,
                    EQ => i64::from(lhs == rhs),
                    NE => i64::from(lhs != rhs),
                    LT => i64::from(lhs < rhs),
                    LE => i64::from(lhs <= rhs),
                    GT => i64::from(lhs > rhs),
                    GE => i64::from(lhs >= rhs),
                    SHL | SHR => {
                        let n = u32::try_from(rhs).ok().filter(|n| *n < 64).ok_or(WordError::Syntax)?;
                        if op == SHR {
                            lhs >> n
                        } else {
                            let r = lhs << n;
                            if r >> n != lhs {
                                return Err(WordError::Syntax);
                            }
                            r
                        }
                    }
                    ADD => lhs.checked_add(rhs).ok_or(WordError::Syntax)?,
                    SUB => lhs.checked_sub(rhs).ok_or(WordError::Syntax)?,
                    MUL => lhs.checked_mul(rhs).ok_or(WordError::Syntax)?,
                    DIV => lhs.checked_div(rhs).ok_or(WordError::Syntax)?,
                    REM => lhs.checked_rem(rhs).ok_or(WordError::Syntax)?,
                    _ => return Err(WordError::Syntax),
                };
            }
        }

        fn cond(&mut self, eval: bool) -> core::result::Result<i64, WordError> {
            self.depth += 1;
            if self.depth > 512 {
                return Err(WordError::Syntax);
            }
            let c = self.binary(0, eval)?;
            let r = if self.is(QUESTION) {
                self.advance()?;
                let a = self.cond(eval && c != 0)?;
                if !self.is(COLON) {
                    return Err(WordError::Syntax);
                }
                self.advance()?;
                let b = self.cond(eval && c == 0)?;
                Ok(if !eval { 0 } else if c != 0 { a } else { b })
            } else {
                Ok(c)
            };
            self.depth -= 1;
            r
        }
    }

    /// Evaluate `text` (already expanded). `var` reads a variable (`false`: unset).
    pub fn evaluate(text: &[u8], undef: bool, var: Var<'_>) -> core::result::Result<i64, WordError> {
        let mut p = P { t: text, i: 0, tok: Tok::End, undef, var, depth: 0, lvl: NO_LEVEL };
        p.advance()?;
        let v = p.cond(true)?;
        if p.tok != Tok::End {
            return Err(WordError::Syntax);
        }
        Ok(v)
    }
}
