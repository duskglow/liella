//! TEST-ONLY ORACLE: a verbatim copy of `crates/core/src/pattern/regex.rs` and
//! `crates/core/src/pattern/regex/*.rs` from main's working tree on 2026-10-05 (the engine
//! before the optimization pass, with the T5 and T6 fixes), compiled only for tests. Do not
//! edit it to follow the optimized engine: it is what the optimized engine is compared with.
//!
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

mod exec;
mod parse;
mod policy;
mod program;
pub(crate) mod text;

pub use program::Program;

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
    pub groups: TryVec<(i64, i64)>,
}

/// The budget per unit of `re_max_failures` (thread insertions).
pub const BUDGET_PER_FAILURE: u64 = 1 << 16;

/// Search `subject` (the whole visible text: the anchors and word operators see its edges) for
/// the match of `prog` that starts in `window`: the earliest start (the latest for a backward
/// window), then the longest; with `subexpressions`, the subexpressions as spec §14.9 selects
/// them. `loc` must be the locale the program was compiled in.
pub fn exec(prog: &Program, loc: &Locale, subject: &[u8], window: Window, opts: &Options, subexpressions: bool) -> Result<Option<Match>, ReError> {
    let mode = if prog.multibyte { text::Mode::Multi(crate::text::Encoding::of(loc)) } else { text::Mode::Bytes };
    let ctx = exec::Ctx { not_bol: opts.not_bol, not_eol: opts.not_eol, newline_anchor: opts.newline_anchor };
    let m = exec::Matcher { prog, mode, loc, subject, ctx, policy: &policy::Posix };
    let caps = subexpressions && prog.ngroups > 0;
    let base = exec::Run { caps: false, first_start: window.first, last_start: window.last, late: window.backward, end: None, budget: None };
    let found = if prog.has_backrefs() {
        m.run(&exec::Run { caps: true, budget: Some(opts.budget.max(1)), ..base })?
    } else {
        let Some(overall) = m.run(&base)? else { return Ok(None) };
        if caps {
            m.run(&exec::Run { caps: true, first_start: overall.start, last_start: overall.start, late: false, end: Some(overall.end), budget: None })?
        } else {
            Some(overall)
        }
    };
    let Some(f) = found else { return Ok(None) };
    let mut groups = TryVec::new();
    if caps {
        // The subexpressions, from the groups that hold them (hidden groups are not reported).
        for k in 1..=u32::try_from(prog.nsub()).unwrap_or(0) {
            let at = usize::try_from(prog.group_of(k)).unwrap_or(0).wrapping_sub(1).wrapping_mul(2);
            let (so, eo) = match f.caps.get(at..at.wrapping_add(2)) {
                Some(&[so, eo]) if so >= 0 && eo >= 0 => (i64::from(so), i64::from(eo)),
                _ => (-1, -1),
            };
            groups.push((so, eo)).map_err(|_| ReError::ESpace)?;
        }
    }
    Ok(Some(Match { start: f.start, end: f.end, groups }))
}
