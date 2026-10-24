//! Regular expressions (spec §14.9; D-14.4): the POSIX basic and extended syntaxes and the GNU
//! syntaxes selected by `RE_*` bits, compiled to an automaton and matched with POSIX
//! leftmost-longest semantics, multibyte- and locale-aware.
//!
//! # Engine
//!
//! * **Parser** (`parse`): the pattern under the syntax bits becomes a tree; errors are the
//!   `regcomp` codes. Bracket expressions are compiled in the locale (`text`): character
//!   ranges by code point without collation rules, by collation-sequence value with them;
//!   classes, equivalence classes (equal primary weights) and collating symbols (any single
//!   character of the locale, `text::collating_element`, shared with `fnmatch` and `glob`).
//! * **Automaton** (`program`): a Thompson automaton. Repetitions whose body can match the empty
//!   string or holds groups run each iteration in a *scope* (`Enter`/`Exit`), which carries the
//!   iteration rule of XBD 9.1/9.3.6 (an empty iteration is allowed, but nothing follows it beyond
//!   the minimum count) and the reset of the body's groups. The program serialises to words,
//!   which is what a `regex_t` holds.
//! * **Matcher** (`exec`): every path in parallel, position by position, over states
//!   (instruction, empty-iteration depth) visited in topological order. The overall match is
//!   found first without subexpressions (earliest start, then longest end); subexpressions are
//!   then found by a second run anchored on that match. Patterns with back-references run one
//!   pass with subexpressions, because the referenced texts are part of each thread's future.
//! * **Submatch policy** (`policy`): the one component that decides which thread survives a
//!   merge; [`policy::Posix`] implements XBD 9.1 (owner ruling 2026-10-03). The matcher is
//!   generic over it, so a glibc-compatible alternation-priority mode would be one more
//!   implementation (designed for, not built: see `policy`).
//!
//! # Time
//!
//! Without back-references the work per subject position is bounded by the size of the
//! automaton, so matching is linear in the subject for a fixed pattern (no backtracking, no
//! exponential case). With back-references the work is counted against a budget derived from
//! `re_max_failures`; past it the search fails with `REG_ESPACE` instead of running unbounded.

mod dfa;
/// Which execution path a search took (RE-SEARCH-3): test-only counters, so that a later merge
/// cannot silently drop a fast path. Global atomics: a test that reads them runs alone.
#[cfg(feature = "selftest")]
pub mod paths {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// One execution path of a search.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(usize)]
    pub enum Path {
        /// The DFA found the span and answered alone (no subexpressions wanted).
        DfaOnly,
        /// The DFA found the span; the one-pass walk collected the subexpressions on it.
        SpanOnePass,
        /// The thread-set pass collected the subexpressions (on a DFA span or after the fast pass).
        ThreadSet,
        /// The fast overall pass found the match, no subexpressions wanted.
        FastOnly,
        /// The fast overall pass found the match; the one-pass walk collected the subexpressions.
        FastOnePass,
        /// The back-reference matcher (budgeted, with captures).
        Backref,
        /// `re_search`: the prefilter's required bytes are absent.
        GnuNoMatch,
        /// `re_search`: the prefilter found a plain string's match.
        GnuMatch,
        /// `re_search`: the engine started at the leading bytes' first occurrence.
        GnuFrom,
        /// `re_search`: the engine from the window's start (backward range, translate table, no bytes).
        GnuEngine,
    }
    const ALL: [Path; 10] = [
        Path::DfaOnly,
        Path::SpanOnePass,
        Path::ThreadSet,
        Path::FastOnly,
        Path::FastOnePass,
        Path::Backref,
        Path::GnuNoMatch,
        Path::GnuMatch,
        Path::GnuFrom,
        Path::GnuEngine,
    ];
    static COUNTS: [AtomicU64; ALL.len()] = [const { AtomicU64::new(0) }; ALL.len()];
    /// The counter of `p`.
    fn counter(p: Path) -> Option<&'static AtomicU64> {
        ALL.iter().position(|&q| q == p).and_then(|i| COUNTS.get(i))
    }
    /// Count one use of `p`.
    pub fn hit(p: Path) {
        if let Some(c) = counter(p) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// How often `p` ran so far.
    pub fn count(p: Path) -> u64 {
        counter(p).map_or(0, |c| c.load(Ordering::Relaxed))
    }
}
mod exec;
mod parse;
mod policy;
mod program;
mod required;
mod scratch;
pub(crate) mod text;

pub use program::{ProgRef, Program};

use crate::locale::Locale;
use crate::mem::TryVec;

/// The syntax bits (`reg_syntax_t`, `<regex.h>`).
pub mod syntax {
    /// `\` quotes the next character inside a bracket expression.
    pub const BACKSLASH_ESCAPE_IN_LISTS: u64 = 1;
    /// `\+` and `\?` are operators (`+` and `?` literal); otherwise the reverse.
    pub const BK_PLUS_QM: u64 = 1 << 1;
    /// `[:class:]` is recognised in bracket expressions.
    pub const CHAR_CLASSES: u64 = 1 << 2;
    /// `^` and `$` are anchors everywhere.
    pub const CONTEXT_INDEP_ANCHORS: u64 = 1 << 3;
    /// Repetition operators are operators everywhere (with nothing before them they repeat the
    /// empty expression).
    pub const CONTEXT_INDEP_OPS: u64 = 1 << 4;
    /// A repetition operator with nothing before it is an error.
    pub const CONTEXT_INVALID_OPS: u64 = 1 << 5;
    /// `.` matches a newline.
    pub const DOT_NEWLINE: u64 = 1 << 6;
    /// `.` does not match NUL.
    pub const DOT_NOT_NULL: u64 = 1 << 7;
    /// `[^...]` does not match a newline.
    pub const HAT_LISTS_NOT_NEWLINE: u64 = 1 << 8;
    /// Intervals are recognised.
    pub const INTERVALS: u64 = 1 << 9;
    /// `+`, `?` and `|` are not operators.
    pub const LIMITED_OPS: u64 = 1 << 10;
    /// A newline is an alternation operator.
    pub const NEWLINE_ALT: u64 = 1 << 11;
    /// `{...}` (not `\{...\}`) is an interval.
    pub const NO_BK_BRACES: u64 = 1 << 12;
    /// `(...)` (not `\(...\)`) is a group.
    pub const NO_BK_PARENS: u64 = 1 << 13;
    /// `\digit` is the digit, not a back-reference.
    pub const NO_BK_REFS: u64 = 1 << 14;
    /// `|` (not `\|`) is alternation.
    pub const NO_BK_VBAR: u64 = 1 << 15;
    /// A reversed range is an error (otherwise it is empty).
    pub const NO_EMPTY_RANGES: u64 = 1 << 16;
    /// An unmatched `)` is an ordinary character.
    pub const UNMATCHED_RIGHT_PAREN_ORD: u64 = 1 << 17;
    /// (GNU: stop at the first match; matching is always leftmost-longest here.)
    pub const NO_POSIX_BACKTRACKING: u64 = 1 << 18;
    /// The GNU operators (`\w`, `\b`, `` \` ``, ...) are ordinary characters.
    pub const NO_GNU_OPS: u64 = 1 << 19;
    /// (GNU debugging; ignored.)
    pub const DEBUG: u64 = 1 << 20;
    /// A malformed interval is ordinary text.
    pub const INVALID_INTERVAL_ORD: u64 = 1 << 21;
    /// Case is ignored.
    pub const ICASE: u64 = 1 << 22;
    /// (GNU internal; ignored.)
    pub const CARET_ANCHORS_HERE: u64 = 1 << 23;
    /// An interval cannot open the expression, a branch or a group, and `*` or an interval
    /// cannot follow another repetition.
    pub const CONTEXT_INVALID_DUP: u64 = 1 << 24;
    /// `re_compile_pattern` sets `no_sub`.
    pub const NO_SUB: u64 = 1 << 25;

    const POSIX_COMMON: u64 = CHAR_CLASSES | DOT_NEWLINE | DOT_NOT_NULL | INTERVALS | NO_EMPTY_RANGES;
    /// `RE_SYNTAX_POSIX_BASIC` (`regcomp` without `REG_EXTENDED`).
    pub const POSIX_BASIC: u64 = POSIX_COMMON | BK_PLUS_QM | CONTEXT_INVALID_DUP;
    /// `RE_SYNTAX_POSIX_EXTENDED` (`regcomp` with `REG_EXTENDED`).
    pub const POSIX_EXTENDED: u64 = POSIX_COMMON
        | CONTEXT_INDEP_ANCHORS
        | CONTEXT_INDEP_OPS
        | NO_BK_BRACES
        | NO_BK_PARENS
        | NO_BK_VBAR
        | CONTEXT_INVALID_OPS
        | UNMATCHED_RIGHT_PAREN_ORD;
}

/// A compilation or matching failure, as the `regcomp`/`regexec` codes name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReError {
    /// `REG_BADPAT`.
    BadPat,
    /// `REG_ECOLLATE`.
    ECollate,
    /// `REG_ECTYPE`.
    ECtype,
    /// `REG_EESCAPE`.
    EEscape,
    /// `REG_ESUBREG`.
    ESubreg,
    /// `REG_EBRACK`.
    EBrack,
    /// `REG_EPAREN`.
    EParen,
    /// `REG_EBRACE`.
    EBrace,
    /// `REG_BADBR`.
    BadBr,
    /// `REG_ERANGE`.
    ERange,
    /// `REG_ESPACE`.
    ESpace,
    /// `REG_BADRPT`.
    BadRpt,
    /// `REG_ERPAREN`: an unmatched `)` (the GNU interface reports it; `regcomp` reports
    /// `REG_EPAREN`).
    ERParen,
}

impl ReError {
    /// The `<regex.h>` code.
    pub fn code(self) -> i32 {
        match self {
            ReError::BadPat => 2,
            ReError::ECollate => 3,
            ReError::ECtype => 4,
            ReError::EEscape => 5,
            ReError::ESubreg => 6,
            ReError::EBrack => 7,
            ReError::EParen => 8,
            ReError::EBrace => 9,
            ReError::BadBr => 10,
            ReError::ERange => 11,
            ReError::ESpace => 12,
            ReError::BadRpt => 13,
            ReError::ERParen => 16,
        }
    }
}

/// `REG_NOMATCH`.
pub const NOMATCH: i32 = 1;

/// The `regcomp` flags (`cflags`).
pub mod cflags {
    /// `REG_EXTENDED`.
    pub const EXTENDED: i32 = 1;
    /// `REG_ICASE`.
    pub const ICASE: i32 = 2;
    /// `REG_NEWLINE`.
    pub const NEWLINE: i32 = 4;
    /// `REG_NOSUB`.
    pub const NOSUB: i32 = 8;
}

/// The syntax `regcomp` compiles with for `cflags` (unknown bits are ignored): POSIX basic or
/// extended; `REG_ICASE` ignores case; `REG_NEWLINE` keeps `.` and `[^...]` off newlines (and
/// makes `^`/`$` match at them, which is the caller's `newline_anchor`).
pub fn regcomp_syntax(flags: i32) -> u64 {
    let mut syn = if flags & cflags::EXTENDED != 0 { syntax::POSIX_EXTENDED } else { syntax::POSIX_BASIC };
    if flags & cflags::ICASE != 0 {
        syn |= syntax::ICASE;
    }
    if flags & cflags::NEWLINE != 0 {
        syn = (syn & !syntax::DOT_NEWLINE) | syntax::HAT_LISTS_NOT_NEWLINE;
    }
    syn
}

/// The code `regcomp` returns for `e`: an unmatched `)` is `REG_EPAREN` (`REG_ERPAREN` is never
/// returned by `regcomp`).
pub fn regcomp_code(e: ReError) -> i32 {
    if e == ReError::ERParen { ReError::EParen.code() } else { e.code() }
}

const MESSAGES: [&[u8]; 17] = [
    b"Success\0",
    b"No match\0",
    b"Invalid regular expression\0",
    b"Invalid collation character\0",
    b"Invalid character class name\0",
    b"Trailing backslash\0",
    b"Invalid back reference\0",
    b"Unmatched [, [^, [:, [., or [=\0",
    b"Unmatched ( or \\(\0",
    b"Unmatched \\{\0",
    b"Invalid content of \\{\\}\0",
    b"Invalid range end\0",
    b"Memory exhausted\0",
    b"Invalid preceding regular expression\0",
    b"Premature end of regular expression\0",
    b"Regular expression too big\0",
    b"Unmatched ) or \\)\0",
];

/// The text of an error code that no message names (`regerror` for a code outside 0-16), with
/// its terminator.
const UNKNOWN_MESSAGE: &[u8] = b"Unknown error\0";

/// The message for `code` (`regerror`, `re_compile_pattern`) followed by a NUL byte.
pub fn message_z(code: i32) -> &'static [u8] {
    usize::try_from(code).ok().and_then(|i| MESSAGES.get(i)).copied().unwrap_or(UNKNOWN_MESSAGE)
}

/// The message for `code`, without the terminator.
pub fn message(code: i32) -> &'static [u8] {
    let m = message_z(code);
    m.split_last().map_or(m, |(_, text)| text)
}

/// Compile `pattern` under the syntax bits `syntax` in `loc` (whose character set and
/// collation the program keeps using: pass the same locale to [`exec`]).
pub fn compile(pattern: &[u8], syntax: u64, loc: &Locale) -> Result<Program, ReError> {
    let parsed = parse::parse(pattern, syntax, loc)?;
    Program::build(parsed, loc, syntax & syntax::ICASE != 0)
}

/// How a search sees the subject.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// `^` does not match at the start of the subject (`REG_NOTBOL`, `not_bol`).
    pub not_bol: bool,
    /// `$` does not match at its end (`REG_NOTEOL`, `not_eol`).
    pub not_eol: bool,
    /// `^` and `$` also match after and before a newline (`REG_NEWLINE`, `newline_anchor`).
    pub newline_anchor: bool,
    /// The work budget for patterns with back-references (thread insertions).
    pub budget: u64,
    /// The accelerations a search may use (all on by default). A test knob: every setting gives
    /// the same answers, which the oracle diff and the fuzz targets check by running both ways.
    #[doc(hidden)]
    pub accel: Accel,
}

/// The accelerations of a search ([`Options::accel`]): each can be switched off, so that tests
/// can compare the results with and without it.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accel {
    /// [`prefilter`] answers from the pattern's [`required_string`] before the engine runs.
    pub prefilter: bool,
    /// The lazy DFA takes the existence and span scans (D-14.12 reserves the knob; no engine
    /// reads it yet). Groundwork for RX-T2-1: on by default, so turning it off is a no-op today.
    pub dfa: bool,
}

impl Default for Accel {
    fn default() -> Self {
        Self { prefilter: true, dfa: true }
    }
}

impl Accel {
    /// Every acceleration off: the engine alone.
    pub const OFF: Self = Self { prefilter: false, dfa: false };
}

/// Where a match may start.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// The first start position tried.
    pub first: usize,
    /// The last start position tried.
    pub last: usize,
    /// Prefer the latest start (a backward `re_search`) instead of the earliest.
    pub backward: bool,
}

/// A match.
#[derive(Debug)]
pub struct Match {
    /// Its start and end (byte offsets in the subject).
    pub start: usize,
    /// End (exclusive).
    pub end: usize,
    /// `(so, eo)` of subexpressions 1, 2, ... (-1, -1 for one that did not take part); empty
    /// when they were not asked for.
    pub groups: Groups,
}

/// The subexpressions of a [`Match`]: the first [`GROUPS_INLINE`] without an allocation.
pub type Groups = crate::mem::InlineVec<(i64, i64), GROUPS_INLINE>;

/// Subexpressions a [`Match`] holds inline (`regmatch_t` arrays rarely have more than ten
/// entries, the whole match included).
pub const GROUPS_INLINE: usize = 9;

/// The budget per unit of `re_max_failures` (thread insertions).
pub const BUDGET_PER_FAILURE: u64 = 1 << 16;

/// Search `subject` (the whole visible text: the anchors and word operators see its edges) for
/// the match of `prog` that starts in `window`: the earliest start (the latest for a backward
/// window), then the longest; with `subexpressions`, the subexpressions as spec §14.9 selects
/// them. `loc` must be the locale the program was compiled in.
pub fn exec(prog: &Program, loc: &Locale, subject: &[u8], window: Window, opts: &Options, subexpressions: bool) -> Result<Option<Match>, ReError> {
    exec_in(&prog.view(), loc, subject, window, opts, subexpressions)
}

/// Element counts of the scratch a search asks a [`ScratchSource`] for.
#[derive(Clone, Copy, Debug)]
pub struct ScratchNeed {
    /// `u32` elements.
    pub u32s: usize,
    /// `u64` elements.
    pub u64s: usize,
    /// `i32` elements.
    pub i32s: usize,
    /// `usize` elements.
    pub usizes: usize,
}

/// Scratch memory for one search, at least as large as the [`ScratchNeed`] it answers; its
/// contents on entry do not matter.
#[derive(Debug)]
pub struct ScratchMem<'m> {
    /// `u32` elements.
    pub u32s: &'m mut [u32],
    /// `u64` elements.
    pub u64s: &'m mut [u64],
    /// `i32` elements.
    pub i32s: &'m mut [i32],
    /// `usize` elements.
    pub usizes: &'m mut [usize],
}

/// Where a search gets scratch that does not fit its small stack buffers: `regexec` gives
/// each thread its own blocks, kept and grown between calls (D-14.11). `None`: the search
/// allocates its own for this call.
pub trait ScratchSource {
    /// Scratch for `need`, or `None`.
    fn scratch(&mut self, need: &ScratchNeed) -> Option<ScratchMem<'_>>;

    /// The calling thread's lazy-DFA state store (D-14.12): a fixed `&mut [u32]` the DFA reuses
    /// across calls, or `None` when this source has none (a core caller without a per-thread
    /// source, or a re-entrant call on a busy thread). The default is `None`, so a caller that
    /// does not override it keeps today's exec path exactly; no engine requests it yet.
    fn dfa_store(&mut self) -> Option<&mut [u32]> {
        None
    }
}

/// A [`ScratchSource`] that has none.
#[derive(Debug, Default)]
pub struct NoScratch;

impl ScratchSource for NoScratch {
    fn scratch(&mut self, _: &ScratchNeed) -> Option<ScratchMem<'_>> {
        None
    }
}

/// [`exec`] over a program read in place (a `regex_t` block, [`ProgRef::from_bytes`]).
pub fn exec_in(prog: &ProgRef<'_>, loc: &Locale, subject: &[u8], window: Window, opts: &Options, subexpressions: bool) -> Result<Option<Match>, ReError> {
    exec_with(prog, loc, subject, window, opts, subexpressions, &mut NoScratch)
}

/// [`exec_in`] with scratch from `src` for searches too large for the stack.
pub fn exec_with(prog: &ProgRef<'_>, loc: &Locale, subject: &[u8], window: Window, opts: &Options, subexpressions: bool, src: &mut dyn ScratchSource) -> Result<Option<Match>, ReError> {
    search_with(prog, loc, subject, window, opts, subexpressions, 0, src)
}

/// [`exec_with`] for a compiled block with a per-regcomp `serial` (the lazy DFA's store key,
/// D-14.12): with [`Accel::dfa`], a forward window and a store from `src`, the DFA's span
/// passes find the overall match (D-14.16) and the matcher only collects the subexpressions on
/// that span; the matcher answers alone whenever the DFA cannot (see `dfa`). `serial` 0 is
/// [`exec_with`] exactly.
#[allow(clippy::too_many_arguments)] // exec_with's inputs and the store key
pub fn search_with(prog: &ProgRef<'_>, loc: &Locale, subject: &[u8], window: Window, opts: &Options, subexpressions: bool, serial: u64, src: &mut dyn ScratchSource) -> Result<Option<Match>, ReError> {
    let mode = if prog.multibyte { text::Mode::Multi(crate::text::Encoding::of(loc)) } else { text::Mode::Bytes };
    let ctx = exec::Ctx { not_bol: opts.not_bol, not_eol: opts.not_eol, newline_anchor: opts.newline_anchor };
    let m = exec::Matcher { prog, mode, loc, subject, ctx, policy: &policy::Posix };
    let caps = subexpressions && prog.ngroups > 0;
    let base = exec::Run { caps: false, first_start: window.first, last_start: window.last, late: window.backward, end: None, budget: None };
    let mut groups = Groups::new();
    // The subexpressions, from the groups that hold them (hidden groups are not reported).
    let mut out = |c: &[i32]| -> Result<(), ReError> {
        groups.clear();
        for k in 1..=u32::try_from(prog.nsub()).unwrap_or(0) {
            let at = usize::try_from(prog.group_of(k)).unwrap_or(0).wrapping_sub(1).wrapping_mul(2);
            let (so, eo) = match c.get(at..at.wrapping_add(2)) {
                Some(&[so, eo]) if so >= 0 && eo >= 0 => (i64::from(so), i64::from(eo)),
                _ => (-1, -1),
            };
            groups.push((so, eo)).map_err(|_| ReError::ESpace)?;
        }
        Ok(())
    };
    if opts.accel.dfa && serial != 0 && !window.backward && prog.brefs == 0 && prog.has_preds() {
        let d = dfa::Dfa { prog, mode, loc, ctx };
        if let Some(span) = src.dfa_store().and_then(|store| d.span(subject, window.first, window.last, store, serial)) {
            let Some((start, end)) = span else { return Ok(None) };
            if !caps {
                #[cfg(feature = "selftest")]
                paths::hit(paths::Path::DfaOnly);
                return Ok(Some(Match { start, end, groups }));
            }
            // The span is known: the matcher runs on it alone (`Run::caps` set = no overall pass).
            let known = exec::Run { caps: true, first_start: start, last_start: start, late: false, end: Some(end), budget: None };
            let Some((start, end)) = m.search(&known, true, opts.budget, src, &mut out)? else { return Ok(None) };
            return Ok(Some(Match { start, end, groups }));
        }
    }
    let Some((start, end)) = m.search(&base, caps, opts.budget, src, &mut out)? else { return Ok(None) };
    Ok(Some(Match { start, end, groups }))
}

/// Whether any match of `prog` starts in `window` (`regexec` without subexpressions: `REG_NOSUB`
/// or `nmatch` 0): [`exec_with`]'s answer, `is_some()`. With a store from `src`
/// ([`ScratchSource::dfa_store`]) and a nonzero per-regcomp `serial` (the store's key), the lazy
/// DFA answers (D-14.13); it hands the search to the matcher when it cannot (see `dfa`).
pub fn exists_with(prog: &ProgRef<'_>, loc: &Locale, subject: &[u8], window: Window, opts: &Options, serial: u64, src: &mut dyn ScratchSource) -> Result<bool, ReError> {
    if opts.accel.dfa && serial != 0 && !window.backward && window.last >= subject.len() {
        let mode = if prog.multibyte { text::Mode::Multi(crate::text::Encoding::of(loc)) } else { text::Mode::Bytes };
        let ctx = exec::Ctx { not_bol: opts.not_bol, not_eol: opts.not_eol, newline_anchor: opts.newline_anchor };
        let d = dfa::Dfa { prog, mode, loc, ctx };
        if let Some(found) = src.dfa_store().and_then(|store| d.exists(subject, window.first, store, serial)) {
            return Ok(found);
        }
    }
    Ok(exec_with(prog, loc, subject, window, opts, false, src)?.is_some())
}

/// The bytes every match of a pattern begins with: the chain of single bytes from the start
/// (groups opened or closed on the way consume nothing), with `true` when the chain reaches the
/// match with no group at all, a plain string: its match is then the earliest occurrence of the
/// bytes, whatever the search's options (the anchors and `REG_NEWLINE` act only through
/// operators the chain lacks). `None` when no byte is certain. Only for a pattern matched byte
/// by byte in `loc`: without `REG_ICASE`, and in a multibyte locale only ASCII bytes in UTF-8
/// (where an ASCII byte is always a character of its own).
pub fn leading_string(prog: &Program, loc: &Locale) -> Option<(TryVec<u8>, bool)> {
    let prog = prog.view();
    if prog.icase || (prog.multibyte && crate::text::Encoding::of(loc) != crate::text::Encoding::Utf8) {
        return None;
    }
    let mut bytes = TryVec::new();
    let mut pc = prog.start;
    let mut whole = false;
    for _ in 0..=prog.ninsts() {
        if usize::try_from(pc).map_or(true, |k| k >= prog.ninsts()) {
            break;
        }
        let i = prog.inst(pc);
        match i.op {
            program::OP_BYTE if !prog.multibyte || i.a < 0x80 => {
                let Ok(b) = u8::try_from(i.a) else { break };
                if bytes.push(b).is_err() {
                    return None;
                }
                pc = i.b;
            }
            program::OP_OPEN | program::OP_CLOSE => pc = i.b,
            program::OP_MATCH => {
                whole = prog.ngroups == 0;
                break;
            }
            _ => break,
        }
    }
    (!bytes.is_empty()).then_some((bytes, whole))
}

/// The bytes every match of a pattern contains, for [`prefilter`]: the leading bytes
/// ([`leading_string`]) with `true` when they are the whole pattern (a plain string); otherwise
/// the longer of them and the longest string of bytes every match holds contiguously (at most 32
/// bytes, from the dominators of the match in the program: `required`), with `false`. `None`
/// when no byte is certain. A pattern with back-references keeps the leading bytes alone: its
/// budgeted search answers `REG_ESPACE` past `re_max_failures`, and an earlier no-match from
/// bytes inside the pattern would change that answer.
pub fn required_string(prog: &Program, loc: &Locale) -> Option<(TryVec<u8>, bool)> {
    let lead = leading_string(prog, loc);
    if lead.as_ref().is_some_and(|(_, whole)| *whole) || prog.has_backrefs() {
        return lead;
    }
    match (lead, required::required(&prog.view())) {
        (Some((l, _)), Some(r)) if r.len() > l.len() => Some((r, false)),
        (Some(l), _) => Some(l),
        (None, r) => r.map(|r| (r, false)),
    }
}

/// What [`prefilter`] decides.
#[derive(Debug, PartialEq, Eq)]
pub enum Prefilter {
    /// The match (start, end): a plain string's earliest occurrence.
    Match(usize, usize),
    /// No match.
    NoMatch,
    /// No match starts before this offset (the bytes lead every match and first occur here):
    /// the engine decides from it.
    From(usize),
    /// The engine decides.
    Engine,
}

/// Whether `bytes` (a pattern's [`required_string`]) begin every match: they are its
/// [`leading_string`], so the earliest match starts at an occurrence of them and a search may
/// begin at the first one ([`Prefilter::From`]). Never for a pattern with back-references: its
/// budgeted search could answer `REG_ESPACE` from the earlier start (as in [`required_string`]).
pub fn leads_every_match(prog: &Program, loc: &Locale, bytes: &[u8]) -> bool {
    !prog.has_backrefs() && leading_string(prog, loc).is_some_and(|(l, _)| l.as_slice() == bytes)
}

/// What `regexec` decides before the engine, from a pattern's [`required_string`] `req`, for a
/// search of `subject[first..]` (the window ends where the subject does): a plain string's match
/// is its earliest occurrence; when the bytes do not occur, there is no match (a match lies in
/// the window and holds them); when they begin every match (`leading`, [`leads_every_match`]),
/// the engine decides from their first occurrence; otherwise from `first`. Always the engine when
/// `opts.accel.prefilter` is off. `find(window, bytes)` is the first occurrence of `bytes` in
/// `window` (`bytes::find_seq` with the identity fold, or a faster search with the same answer).
#[inline(never)] // inlined into `regexec` its scan loop spills to the stack (+4 loads per byte, RX-T2-0d)
pub fn prefilter(req: Option<(&[u8], bool)>, leading: bool, subject: &[u8], first: usize, opts: &Options, find: impl Fn(&[u8], &[u8]) -> Option<usize>) -> Prefilter {
    let Some((bytes, whole)) = req.filter(|_| opts.accel.prefilter) else { return Prefilter::Engine };
    match (subject.get(first..).and_then(|w| find(w, bytes)), whole) {
        (None, _) => Prefilter::NoMatch,
        (Some(k), true) => Prefilter::Match(first.saturating_add(k), first.saturating_add(k).saturating_add(bytes.len())),
        (Some(k), false) if leading => Prefilter::From(first.saturating_add(k)),
        (Some(_), false) => Prefilter::Engine,
    }
}

/// The bytes `L` of a pattern that is exactly "the bytes `L`, then `$`" (RE-SO-3): the chain of
/// [`leading_string`] (same `REG_ICASE` and multibyte limits) ending in the end-of-line
/// assertion and then the match, with no subexpression and no back-reference. Its matches are
/// then the occurrences of `L` the end-of-line assertion holds after, settled by
/// [`end_prefilter`] without the engine. `None` for any other shape.
pub fn end_literal(prog: &Program, loc: &Locale) -> Option<TryVec<u8>> {
    let view = prog.view();
    if view.ngroups != 0 || prog.has_backrefs() || view.icase || (view.multibyte && crate::text::Encoding::of(loc) != crate::text::Encoding::Utf8) {
        return None;
    }
    let mut bytes = TryVec::new();
    let mut pc = view.start;
    let mut eol = false;
    for _ in 0..=view.ninsts() {
        if usize::try_from(pc).map_or(true, |k| k >= view.ninsts()) {
            return None;
        }
        let i = view.inst(pc);
        match i.op {
            program::OP_BYTE if !eol && (!view.multibyte || i.a < 0x80) => {
                bytes.push(u8::try_from(i.a).ok()?).ok()?;
                pc = i.b;
            }
            program::OP_OPEN | program::OP_CLOSE => pc = i.b,
            // `$` (assertion code 1, `program::assert_code`), once, after at least one byte.
            program::OP_ASSERT if !eol && i.a == 1 && !bytes.is_empty() => {
                eol = true;
                pc = i.b;
            }
            program::OP_MATCH if eol => return Some(bytes),
            _ => return None,
        }
    }
    None
}

/// [`prefilter`] for an [`end_literal`] pattern: `lit_nl` is its bytes `L` followed by one
/// `'\n'`. A match is an occurrence of `L` at `p >= first` where `$` holds at its end: the end
/// of the subject unless `opts.not_eol`, or (with `opts.newline_anchor`) a `'\n'` right after
/// it. The earliest such `p` is the match, as the engine would find it (the pattern has no
/// alternative and no subexpression); none = no match. Always the engine when
/// `opts.accel.prefilter` is off. Every read is bounds-checked: a wrong `lit_nl` (a damaged
/// block) can change the answer but never reads outside `subject`.
pub fn end_prefilter(lit_nl: &[u8], subject: &[u8], first: usize, opts: &Options, find: impl Fn(&[u8], &[u8]) -> Option<usize>) -> Prefilter {
    let Some(lit) = lit_nl.split_last().map(|(_, l)| l).filter(|l| opts.accel.prefilter && !l.is_empty()) else { return Prefilter::Engine };
    let Some(window) = subject.get(first..) else { return Prefilter::NoMatch };
    let n = lit.len();
    // `L` ending the subject.
    let tail = window.len().checked_sub(n).filter(|&k| !opts.not_eol && window.get(k..) == Some(lit));
    // `L` before a newline: only a start before the tail's can be earlier.
    let before_nl = if opts.newline_anchor { find(window.get(..tail.map_or(window.len(), |k| k.saturating_add(n)).min(window.len())).unwrap_or(window), lit_nl) } else { None };
    match before_nl.or(tail) {
        Some(k) => Prefilter::Match(first.saturating_add(k), first.saturating_add(k).saturating_add(n)),
        None => Prefilter::NoMatch,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::type_complexity)]
mod plain_tests {
    use super::*;

    extern crate std;
    use std::vec::Vec;

    /// The leading bytes are found exactly (a plain string when the whole pattern is one), and
    /// they agree with `exec` for every window over every subject of up to six bytes of
    /// `ab.\nx`, with and without `REG_NEWLINE`, `REG_NOTBOL` and `REG_NOTEOL`: a plain string's
    /// earliest occurrence is the match, and a pattern whose leading bytes do not occur in the
    /// window has none.
    /// GATE-SPEED: the sweep runs in case ranges (one test each, below); `REQUIRED_SHARDS` tiles all 26 cases
    /// (checked by `the_required_strings_shards_cover_every_case`), so the coverage is the one sweep's.
    const REQUIRED_SHARDS: [(usize, usize); 7] = [(0, 4), (4, 8), (8, 12), (12, 16), (16, 20), (20, 24), (24, 26)];

    fn required_strings_in_every_match(lo: usize, hi: usize) {
        // (pattern, flags, required bytes: `None` when not checked exactly, `Some(b"")` for none)
        let cases: [(&[u8], i32, Option<&[u8]>); 26] = [
            (b".*ab", 0, Some(b"ab")),
            (b"(ab)?xa", cflags::EXTENDED, Some(b"xa")),
            (b"a*xb*", 0, Some(b"x")),
            (b"(ab|ab)x", cflags::EXTENDED, Some(b"x")),
            (b"a\\{2,3\\}", 0, Some(b"aa")),
            (b"\\(ab\\)*x", 0, Some(b"x")),
            (b"x\\(ab\\)*", 0, Some(b"x")),
            (b".ab.", 0, Some(b"ab")),
            (b"[ab]x\\.a", 0, Some(b"x.a")),
            (b"a.b.xa", 0, Some(b"xa")),
            (b"(a|b)*", cflags::EXTENDED, Some(b"")),
            (b"^x.a$", 0, Some(b"x")),
            (b"\\<ab\\>", 0, Some(b"ab")),
            (b"\\(a\\)x\\1", 0, Some(b"ax")),
            // With back-references only the leading bytes (here none), not the `c` every match
            // holds; without them, the `c`.
            (b"\\(a*\\)*\\1c", 0, Some(b"")),
            (b"\\(a*\\)*c", 0, Some(b"c")),
            (b"x\\{0,1\\}ab", 0, Some(b"ab")),
            (b"ab", cflags::ICASE, Some(b"")),
            (b"a|b", cflags::EXTENDED, Some(b"")),
            (b"(a.|.b)x(a|ab)", cflags::EXTENDED, Some(b"x")),
            (b"(a*)*b+.a", cflags::EXTENDED, None),
            (b"a.\\.", cflags::ICASE, None),
            (b"(x|$)a", cflags::EXTENDED, None),
            (b"\\(a*\\)\\{2,3\\}x\\1", 0, None),
            (b"\xc3\xa9x?a", 0, None),
            (b"\n.\nb", 0, None),
        ];
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        let mut subjects: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = subjects.clone();
        for _ in 0..6 {
            let mut next = Vec::new();
            for s in &level {
                for &c in b"ab.\nx" {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            subjects.extend(next.iter().cloned());
            level = next;
        }
        for (pat, flags, want) in cases.into_iter().skip(lo).take(hi.saturating_sub(lo)) {
            for loc in [&Locale::C, &utf8] {
                for nl in [0, cflags::NEWLINE] {
                    let prog = compile(pat, regcomp_syntax(flags | nl), loc).unwrap();
                    let got = required_string(&prog, loc);
                    if let Some(w) = want {
                        assert_eq!(got.as_ref().map_or(&[][..], |(b, _)| b.as_slice()), w, "{pat:?} flags {flags}");
                    }
                    let Some((p, _)) = got else { continue };
                    for s in &subjects {
                        for first in 0..=s.len() {
                            if crate::bytes::find_seq(&s[first..], &p, core::convert::identity).is_some() {
                                continue;
                            }
                            for (not_bol, not_eol) in [(false, false), (true, true)] {
                                let opts = Options { not_bol, not_eol, newline_anchor: nl != 0, budget: 1 << 20, accel: Accel::OFF };
                                let m = exec(&prog, loc, s, Window { first, last: s.len(), backward: false }, &opts, false).unwrap().map(|m| (m.start, m.end));
                                assert_eq!(m, None, "{pat:?} on {s:?} from {first}: no {p:?}, so no match");
                            }
                        }
                    }
                }
            }
        }
    }

    /// RE-SO-3: an end-anchored literal is recognised exactly, and `end_prefilter` gives the
    /// engine's answer for every window start over every subject of up to five bytes of `ab\nx`,
    /// with and without `REG_NEWLINE`, `REG_NOTEOL` and `REG_NOTBOL`, in C and UTF-8.
    #[test]
    fn end_literals_agree_with_the_engine() {
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        let mut subjects: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = subjects.clone();
        for _ in 0..5 {
            let mut next = Vec::new();
            for s in &level {
                for &c in b"ab\nx" {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            subjects.extend(next.iter().cloned());
            level = next;
        }
        let cases: [(&[u8], i32, Option<&[u8]>); 9] = [
            (b"ab$", 0, Some(b"ab")),
            (b"b$", cflags::EXTENDED, Some(b"b")),
            (b"a\nb$", 0, Some(b"a\nb")),
            (b"\\(ab\\)$", 0, None),
            (b"ab$", cflags::ICASE, None),
            (b"a*b$", 0, None),
            (b"ab$c", cflags::EXTENDED, None),
            (b"$", 0, None),
            (b"(ab)$", cflags::EXTENDED, None),
        ];
        let mut settled = 0usize;
        for (pat, flags, want) in cases {
            for loc in [&Locale::C, &utf8] {
                for nl in [0, cflags::NEWLINE] {
                    let prog = compile(pat, regcomp_syntax(flags | nl), loc).unwrap();
                    let got = end_literal(&prog, loc);
                    assert_eq!(got.as_ref().map(|l| l.as_slice()), want, "{pat:?} flags {flags}");
                    let Some(mut l) = got else { continue };
                    l.push(b'\n').unwrap();
                    for s in &subjects {
                        for first in 0..=s.len() {
                            for (not_bol, not_eol) in [(false, false), (false, true), (true, false)] {
                                let on = Options { not_bol, not_eol, newline_anchor: nl != 0, budget: 1 << 20, ..Options::default() };
                                let off = Options { not_bol, not_eol, newline_anchor: nl != 0, budget: 1 << 20, accel: Accel::OFF };
                                let want = exec(&prog, loc, s, Window { first, last: s.len(), backward: false }, &off, false).unwrap().map(|m| (m.start, m.end));
                                let got = match end_prefilter(&l, s, first, &on, |w, b| crate::bytes::find_seq(w, b, core::convert::identity)) {
                                    Prefilter::Match(a, z) => Some(Some((a, z))),
                                    Prefilter::NoMatch => Some(None),
                                    Prefilter::From(_) | Prefilter::Engine => None,
                                };
                                assert_eq!(got, Some(want), "{pat:?} nl {nl} on {s:?} from {first} not_bol {not_bol} not_eol {not_eol}");
                                assert_eq!(end_prefilter(&l, s, first, &off, |_, _| None), Prefilter::Engine);
                                settled += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(settled > 10_000, "{settled}");
    }

    #[test]
    fn required_strings_are_in_every_match_0() {
        let (lo, hi) = REQUIRED_SHARDS[0];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_1() {
        let (lo, hi) = REQUIRED_SHARDS[1];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_2() {
        let (lo, hi) = REQUIRED_SHARDS[2];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_3() {
        let (lo, hi) = REQUIRED_SHARDS[3];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_4() {
        let (lo, hi) = REQUIRED_SHARDS[4];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_5() {
        let (lo, hi) = REQUIRED_SHARDS[5];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn required_strings_are_in_every_match_6() {
        let (lo, hi) = REQUIRED_SHARDS[6];
        required_strings_in_every_match(lo, hi);
    }

    #[test]
    fn the_required_strings_shards_cover_every_case() {
        let mut next = 0;
        for (lo, hi) in REQUIRED_SHARDS {
            assert_eq!(lo, next);
            assert!(hi > lo);
            next = hi;
        }
        assert_eq!(next, 26);
    }

    /// GATE-SPEED: as `REQUIRED_SHARDS`, for the 22 cases of the leading-bytes sweep.
    const LEADING_SHARDS: [(usize, usize); 6] = [(0, 4), (4, 8), (8, 12), (12, 16), (16, 20), (20, 22)];

    fn leading_strings_agree_with_the_engine(lo: usize, hi: usize) {
        // (pattern, flags, leading bytes, whole pattern, only in a single-byte locale)
        let cases: [(&[u8], i32, &[u8], bool, bool); 22] = [
            (b"a", 0, b"a", true, false),
            (b"ab", cflags::EXTENDED, b"ab", true, false),
            (b"aab", 0, b"aab", true, false),
            (b"\\.", 0, b".", true, false),
            (b"a.b", 0, b"a", false, false),
            (b"^a", 0, b"", false, false),
            (b"a$", cflags::EXTENDED, b"a", false, false),
            (b"[a]", 0, b"", false, false),
            (b"a*", 0, b"", false, false),
            (b"ab*", 0, b"a", false, false),
            (b"(a)", cflags::EXTENDED, b"a", false, false),
            (b"a\\(b\\)x", 0, b"abx", false, false),
            (b"ab.*x", 0, b"ab", false, false),
            // A bounded repetition of a byte is compiled as copies: the string `aa`.
            (b"a\\{2\\}", 0, b"aa", true, false),
            (b"ab", cflags::ICASE, b"", false, false),
            (b"a|b", cflags::EXTENDED, b"", false, false),
            (b"ab|ax", cflags::EXTENDED, b"", false, false),
            (b"x\nb", 0, b"x\nb", true, false),
            (b"\xc3\xa9", 0, b"\xc3\xa9", true, true),
            (b"a\xc3\xa9", 0, b"a\xc3\xa9", true, true),
            (b"", 0, b"", false, false),
            (b"\\(ab\\)\\1", 0, b"ab", false, false),
        ];
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        let mut subjects: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = subjects.clone();
        for _ in 0..6 {
            let mut next = Vec::new();
            for s in &level {
                for &c in b"ab.\nx" {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            subjects.extend(next.iter().cloned());
            level = next;
        }
        for (pat, flags, lead, whole, single_only) in cases.into_iter().skip(lo).take(hi.saturating_sub(lo)) {
            for (loc, multibyte) in [(&Locale::C, false), (&utf8, true)] {
                for nl in [0, cflags::NEWLINE] {
                    let prog = compile(pat, regcomp_syntax(flags | nl), loc).unwrap();
                    let got = leading_string(&prog, loc);
                    // In UTF-8 the bytes stop before the first non-ASCII one.
                    let want_lead: Vec<u8> = if multibyte && single_only { lead.iter().copied().take_while(u8::is_ascii).collect() } else { lead.to_vec() };
                    let want = (!want_lead.is_empty()).then_some((want_lead, whole && !(multibyte && single_only)));
                    assert_eq!(got.as_ref().map(|(b, w)| (b.to_vec(), *w)), want, "{pat:?} flags {flags} multibyte {multibyte}");
                    let Some((p, whole)) = got else { continue };
                    for s in &subjects {
                        for first in 0..=s.len() {
                            for (not_bol, not_eol) in [(false, false), (true, true)] {
                                let opts = Options { not_bol, not_eol, newline_anchor: nl != 0, budget: 1 << 20, ..Options::default() };
                                let m = exec(&prog, loc, s, Window { first, last: s.len(), backward: false }, &opts, false).unwrap().map(|m| (m.start, m.end));
                                let quick = crate::bytes::find_seq(&s[first..], &p, core::convert::identity).map(|k| (first.wrapping_add(k), first.wrapping_add(k).wrapping_add(p.len())));
                                if whole {
                                    assert_eq!(m, quick, "{pat:?} on {s:?} from {first}");
                                } else if quick.is_none() {
                                    assert_eq!(m, None, "{pat:?} on {s:?} from {first}: no leading bytes, no match");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn leading_strings_agree_with_the_engine_0() {
        let (lo, hi) = LEADING_SHARDS[0];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn leading_strings_agree_with_the_engine_1() {
        let (lo, hi) = LEADING_SHARDS[1];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn leading_strings_agree_with_the_engine_2() {
        let (lo, hi) = LEADING_SHARDS[2];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn leading_strings_agree_with_the_engine_3() {
        let (lo, hi) = LEADING_SHARDS[3];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn leading_strings_agree_with_the_engine_4() {
        let (lo, hi) = LEADING_SHARDS[4];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn leading_strings_agree_with_the_engine_5() {
        let (lo, hi) = LEADING_SHARDS[5];
        leading_strings_agree_with_the_engine(lo, hi);
    }

    #[test]
    fn the_leading_strings_shards_cover_every_case() {
        let mut next = 0;
        for (lo, hi) in LEADING_SHARDS {
            assert_eq!(lo, next);
            assert!(hi > lo);
            next = hi;
        }
        assert_eq!(next, 22);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::type_complexity)]
mod block_tests {
    use super::*;

    extern crate std;
    use std::vec::Vec;

    fn find(p: &ProgRef<'_>, s: &[u8], caps: bool) -> Option<(usize, usize, Vec<(i64, i64)>)> {
        let opts = Options { not_bol: false, not_eol: false, newline_anchor: false, budget: 1 << 20, ..Options::default() };
        exec_in(p, &Locale::C, s, Window { first: 0, last: s.len(), backward: false }, &opts, caps).unwrap().map(|m| (m.start, m.end, m.groups.to_vec()))
    }

    /// The serialised program is matched in place, from a copy of its bytes (as a `regex_t`
    /// block is), with the same answers as the compiled program; foreign, cut or extended
    /// bytes are refused rather than misread.
    #[test]
    fn a_block_is_matched_in_place_and_damage_is_refused() {
        let prog = compile(br"\(a*\)\(b\|[c-e]\{1,2\}\)\2", regcomp_syntax(0), &Locale::C).unwrap();
        let copy: Vec<u8> = prog.as_bytes().to_vec();
        let view = ProgRef::from_bytes(&copy).unwrap();
        assert_eq!(view.nsub(), 2);
        for s in [&b"xaabb"[..], b"cdcd", b"aaced", b"", b"ab"] {
            assert_eq!(find(&view, s, true), find(&prog.view(), s, true), "{s:?}");
        }
        assert_eq!(find(&view, b"xaabb", true), Some((1, 5, std::vec![(1, 3), (3, 4)])));
        assert!(ProgRef::from_bytes(&copy[..copy.len() - 4]).is_none());
        let mut longer = copy.clone();
        longer.extend_from_slice(&[0; 4]);
        assert!(ProgRef::from_bytes(&longer).is_none());
        let mut foreign = copy.clone();
        foreign[0] ^= 1;
        assert!(ProgRef::from_bytes(&foreign).is_none());
        // Damage inside the sections is read within the bytes: any answer, never a panic.
        for k in (program::HEADER * 4..copy.len()).step_by(7) {
            let mut d = copy.clone();
            d[k] = 0xff;
            if let Some(v) = ProgRef::from_bytes(&d) {
                let _ = find(&v, b"aabbcdcd", true);
            }
        }
    }

    /// One block searched by several threads at once gives every thread the same answers.
    #[test]
    fn searches_that_outgrow_their_scratch_run_again_with_the_same_answer() {
        // Every search starts with the least room (RX-CAP): the answers must be those of the
        // default room, and the reruns must happen.
        let cases: &[(&[u8], i32, &[&[u8]])] = &[
            (br"^\([a-z0-9_]*\) = \(.*\)$", 0, &[b"max_connections = 1024", b"x = ", b"= y", b"no match here"]),
            (br"\([a-z]*\)_\([0-9]*\)", 0, &[b"the value_42 is here", b"_", b"a_b_1_2"]),
            (br"([a-z]+)@([a-z]+)\.(com|org)", cflags::EXTENDED, &[b"mail bob@example.org now", b"a@b.com c@d.org", b"none"]),
            (br"(a|ab)(c|bcd)(d*)", cflags::EXTENDED, &[b"abcd", b"xxabcdddd", b"acd"]),
            (br"((a*)*b)*c", cflags::EXTENDED, &[b"aababc", b"bbbc", b"c"]),
            (br"\(a*\)\1b", 0, &[b"aaaab", b"aab", b"b", b"aaa"]),
            (br"\([a-z]*\) \1", 0, &[b"say hello hello world", b"a b"]),
            (br"(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)", cflags::EXTENDED, &[b"--xyzxyzxyzxyz--"]),
        ];
        let mut grown = 0;
        for &(pat, fl, subjects) in cases {
            let prog = compile(pat, regcomp_syntax(fl), &Locale::C).unwrap();
            let v = prog.view();
            for &s in subjects {
                for caps in [false, true] {
                    let want = find(&v, s, caps);
                    exec::test_room::TINY.with(|t| t.set(true));
                    exec::test_room::GROWN.with(|g| g.set(0));
                    let got = find(&v, s, caps);
                    grown += exec::test_room::GROWN.with(core::cell::Cell::get);
                    exec::test_room::TINY.with(|t| t.set(false));
                    assert_eq!(got, want, "{:?} on {:?}", core::str::from_utf8(pat), core::str::from_utf8(s));
                }
            }
        }
        assert!(grown > 0);
    }

    #[test]
    fn the_benchmarked_patterns_search_without_an_allocation() {
        // RX-CAP: the REG_NOSUB re_* rows of cbench_opt search in the stack scratch, in a
        // single-byte and a multibyte locale (the capture rows use the thread's blocks).
        let rows: &[(&[u8], i32, bool)] = &[
            (b"Accepted", 0, false),
            (b"^[[:space:]]*#", 0, false),
            (br"[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+", cflags::EXTENDED, false),
            (br"(sshd|cron|kernel)\[[0-9]+\]", cflags::EXTENDED, false),
            (b"error|warning", cflags::EXTENDED | cflags::ICASE, false),
            (b"user.*from.*port", 0, false),
            (br"^\([a-z0-9_]*\) = \(.*\)$", 0, true),
            (br"\([a-z]*\)_\([0-9]*\)", 0, true),
            (br"([a-z]+)@([a-z]+)\.(com|org)", cflags::EXTENDED, true),
        ];
        let mut misses = Vec::new();
        for loc in [Locale::C, Locale::c_utf8(b"C.UTF-8")] {
            for &(pat, fl, caps) in rows {
                let prog = compile(pat, regcomp_syntax(fl), &loc).unwrap();
                let v = prog.view();
                let mode = if v.multibyte { text::Mode::Multi(crate::text::Encoding::of(&loc)) } else { text::Mode::Bytes };
                let m = exec::Matcher { prog: &v, mode, loc: &loc, subject: b"", ctx: exec::Ctx::default(), policy: &policy::Posix };
                let (fits, need) = m.inline_need(caps);
                std::println!("{:?} {}: {need}", core::str::from_utf8(pat), v.multibyte);
                if !fits && !caps {
                    misses.push((core::str::from_utf8(pat), v.multibyte, need));
                }
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    #[test]
    fn threads_share_one_block() {
        let prog = compile(br"([a-z]+)@([a-z]+)\.(com|org)", regcomp_syntax(cflags::EXTENDED), &Locale::C).unwrap();
        let bytes = prog.as_bytes();
        let want = find(&ProgRef::from_bytes(bytes).unwrap(), b"mail bob@example.org now", true);
        assert_eq!(want, Some((5, 20, std::vec![(5, 8), (9, 16), (17, 20)])));
        std::thread::scope(|sc| {
            for _ in 0..4 {
                sc.spawn(|| {
                    let v = ProgRef::from_bytes(bytes).unwrap();
                    for _ in 0..200 {
                        assert_eq!(find(&v, b"mail bob@example.org now", true), want);
                    }
                });
            }
        });
    }
}
