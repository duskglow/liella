//! Argument parsing framework (spec §1.7; D-1.7): a declarative command-line parser over a tree
//! of option tables, which returns a structured result or error, and the help and usage texts
//! generated from the same tree.
//!
//! Written under the clean-room rules (implementation plan §1.4) from the spec, the public
//! header's documentation, `spec/behaviour/argp.md` and black-box probes of the reference.
//!
//! **The tree** is a list of [`Parser`]s in preorder: the root first, each child after its
//! parent and after the descendants of its earlier siblings. Each has its option table, its
//! documentation, and the header and group its parent's child entry gives it. [`Defaults`] adds
//! the standard options: `-?`/`--help`, `--usage`, the hidden `--program-name=NAME` and
//! `--HANG[=SECS]`, and `-V`/`--version`, merged into the root's level in group -1.
//!
//! **Options.** An option that is not an alias starts an *entry*; the `OPTION_ALIAS` options
//! after it add names to it and share its argument (and its `OPTION_ARG_OPTIONAL`); an alias
//! without a key reports its entry's key. An entry whose first option is `OPTION_DOC` is
//! documentation, never parsed. An option with neither name nor key is a group header. Each
//! option's group is its own when not 0; otherwise the previous option's, and for a header the
//! previous group plus one. A short option is a printable key (`:` and `;` included, though getopt never parses them). When two
//! options claim one short letter or one long name, the first in tree order keeps it and the
//! later one loses that name, in parsing and in help alike.
//!
//! **Parsing** ([`parse`]) runs [`crate::getopt`] over the union of the tables: operands are
//! permuted behind the options, returned in place with `IN_ORDER`, or end the options with
//! `NO_ARGS`. The events go to the parsers through the [`Host`], in this order: `INIT` (preorder;
//! a child's input is the slot its parent's `child_inputs` holds after the parent's `INIT`),
//! each option to the parser that owns it, each operand as `ARG` to each parser in preorder until
//! one takes it, a parser declining `ARG` being offered `ARGS` (the rest of the vector) before
//! the next parser is asked; then `NO_ARGS` (preorder, to each parser that took no operand),
//! `END` (reverse preorder), `SUCCESS` and `FINI` (reverse preorder). An operand no parser takes
//! stops the parse there: with a requested index, the parse succeeds without `NO_ARGS` and `END`;
//! without one, it is a usage error (`Too many arguments`). A parser that lowers `next` during
//! `ARG` has the operand not counted, and parsing resumes at the new `next`. An error (a
//! parser's code other than `ARGP_ERR_UNKNOWN`, or `EINVAL` for a usage error) sends `ERROR`
//! (preorder) and `FINI` (reverse preorder), and is the result.
//!
//! **Help** ([`help`]) prints, as the flags ask, the usage lines, the first non-empty text
//! before a vertical tab in a parser's documentation (preorder), the `Try ...` line, the option
//! list, every parser's text after the vertical tab, and the bug-report line, a blank line
//! separating the option list, each later text and the bug-report line from what precedes them.
//! [`Format`] holds the columns (`ARGP_HELP_FMT`, and `COLUMNS` for the right margin).
//!
//! **The option list.** A child with a header or a non-zero group forms a *cluster*; other
//! children merge into their parent's level. Groups order 0, 1, 2, ..., then the negative ones up
//! to -1. At the root's level, a cluster is placed at its group after the root's own entries of the
//! group; inside a cluster, the cluster's own entries (by group) come before its sub-clusters (by
//! group); sibling clusters of one group come in reverse order (probed, ARGP-HEADER-WHITESPACE-2).
//! A header or keyless entry prints its text after a blank line (an empty text prints nothing);
//! another entry follows a blank line when its group differs from the previous entry's, then its
//! cluster's header after a blank line when the previous entry is in another cluster (so a cluster
//! whose first entry is a header shows no header of its own). In a group, entries sort by the first character
//! of their first shown name (a short option, else a long name; ignoring case), entries known
//! only by long names first on a tie, then by name ignoring case, lower case first;
//! documentation entries whose name does not start with `-` (after spaces) come last. An entry
//! shows its short options, then its long options with the argument (`--name=ARG`,
//! `--name[=ARG]`; short options show it only with `dup-args`), then its text from the text
//! column (or 3 spaces after names that end at most 3 columns past it, or on the next line),
//! wrapped at the right margin. Long usage lists the short options without an argument
//! together, then each with one, then each long option, leaving out documentation entries.
//! A usage unit with a space inside (an option with its argument, the operands) must end
//! before the right margin; others may end at it.

use core::cmp::Ordering;

use crate::error::{Errno, Result};
use crate::getopt::{self, Args, Event, HasArg, LongOpt, Longs, Span, Spec, State};
use crate::mem::TryVec;

// ------------------------------------------------------------------------------ interface values

/// `OPTION_ARG_OPTIONAL`: the option's argument may be left out.
pub const OPTION_ARG_OPTIONAL: i32 = 0x1;
/// `OPTION_HIDDEN`: not shown in help or usage.
pub const OPTION_HIDDEN: i32 = 0x2;
/// `OPTION_ALIAS`: another name for the closest earlier option that is not an alias.
pub const OPTION_ALIAS: i32 = 0x4;
/// `OPTION_DOC`: documentation shown like an option, never parsed.
pub const OPTION_DOC: i32 = 0x8;
/// `OPTION_NO_USAGE`: left out of the long usage text.
pub const OPTION_NO_USAGE: i32 = 0x10;

/// `ARGP_PARSE_ARGV0`: parse element 0 too (only together with [`NO_ERRS`]).
pub const PARSE_ARGV0: u32 = 0x01;
/// `ARGP_NO_ERRS`: no messages (and no exits).
pub const NO_ERRS: u32 = 0x02;
/// `ARGP_NO_ARGS`: operands end the options and are not parsed.
pub const NO_ARGS: u32 = 0x04;
/// `ARGP_IN_ORDER`: operands in place, among the options.
pub const IN_ORDER: u32 = 0x08;
/// `ARGP_NO_HELP`: no standard options.
pub const NO_HELP: u32 = 0x10;
/// `ARGP_NO_EXIT`: no exits.
pub const NO_EXIT: u32 = 0x20;
/// `ARGP_LONG_ONLY`: long options with one dash too.
pub const LONG_ONLY: u32 = 0x40;

/// `ARGP_KEY_ARG`: an operand.
pub const KEY_ARG: i32 = 0;
/// `ARGP_KEY_END`: no more arguments.
pub const KEY_END: i32 = 0x100_0001;
/// `ARGP_KEY_NO_ARGS`: the parser took no operand.
pub const KEY_NO_ARGS: i32 = 0x100_0002;
/// `ARGP_KEY_INIT`: before parsing.
pub const KEY_INIT: i32 = 0x100_0003;
/// `ARGP_KEY_SUCCESS`: the parse succeeded.
pub const KEY_SUCCESS: i32 = 0x100_0004;
/// `ARGP_KEY_ERROR`: the parse failed.
pub const KEY_ERROR: i32 = 0x100_0005;
/// `ARGP_KEY_ARGS`: the remaining operands, at `next`.
pub const KEY_ARGS: i32 = 0x100_0006;
/// `ARGP_KEY_FINI`: the last call.
pub const KEY_FINI: i32 = 0x100_0007;

/// `ARGP_KEY_HELP_PRE_DOC`: a help filter's key for the text before the options.
pub const KEY_HELP_PRE_DOC: i32 = 0x200_0001;
/// `ARGP_KEY_HELP_POST_DOC`: the text after the options.
pub const KEY_HELP_POST_DOC: i32 = 0x200_0002;
/// `ARGP_KEY_HELP_HEADER`: a header.
pub const KEY_HELP_HEADER: i32 = 0x200_0003;
/// `ARGP_KEY_HELP_EXTRA`: extra text after a parser's documentation (no text given).
pub const KEY_HELP_EXTRA: i32 = 0x200_0004;
/// `ARGP_KEY_HELP_ARGS_DOC`: the operands' description.
pub const KEY_HELP_ARGS_DOC: i32 = 0x200_0006;

/// `ARGP_ERR_UNKNOWN` (`E2BIG`): "not mine".
pub const ERR_UNKNOWN: i32 = Errno::E2BIG.get();

/// `ARGP_HELP_USAGE`: the usage lines with every option.
pub const HELP_USAGE: u32 = 0x01;
/// `ARGP_HELP_SHORT_USAGE`: the usage lines with `[OPTION...]`.
pub const HELP_SHORT_USAGE: u32 = 0x02;
/// `ARGP_HELP_SEE`: the `Try ...` line.
pub const HELP_SEE: u32 = 0x04;
/// `ARGP_HELP_LONG`: the option list.
pub const HELP_LONG: u32 = 0x08;
/// `ARGP_HELP_PRE_DOC`: the text before the options.
pub const HELP_PRE_DOC: u32 = 0x10;
/// `ARGP_HELP_POST_DOC`: the texts after the options.
pub const HELP_POST_DOC: u32 = 0x20;
/// `ARGP_HELP_BUG_ADDR`: the bug-report line.
pub const HELP_BUG_ADDR: u32 = 0x40;
/// `ARGP_HELP_EXIT_ERR` (`argp_state_help` only): exit with `argp_err_exit_status`.
pub const HELP_EXIT_ERR: u32 = 0x100;
/// `ARGP_HELP_EXIT_OK` (`argp_state_help` only): exit with 0.
pub const HELP_EXIT_OK: u32 = 0x200;
/// `ARGP_HELP_STD_ERR`.
pub const HELP_STD_ERR: u32 = HELP_SEE | HELP_EXIT_ERR;
/// `ARGP_HELP_STD_USAGE`.
pub const HELP_STD_USAGE: u32 = HELP_SHORT_USAGE | HELP_SEE | HELP_EXIT_ERR;
/// `ARGP_HELP_STD_HELP`.
pub const HELP_STD_HELP: u32 = HELP_SHORT_USAGE | HELP_LONG | HELP_EXIT_OK | HELP_PRE_DOC | HELP_POST_DOC | HELP_BUG_ADDR;

// ------------------------------------------------------------------------------ the tree

/// One `struct argp_option`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opt<'a> {
    /// The long name.
    pub name: Option<&'a [u8]>,
    /// The key (a printable key is also the short option).
    pub key: i32,
    /// The argument's name: the option takes an argument.
    pub arg: Option<&'a [u8]>,
    /// `OPTION_*` flags.
    pub flags: i32,
    /// The help text.
    pub doc: Option<&'a [u8]>,
    /// The help group.
    pub group: i32,
}

impl Opt<'_> {
    /// The table's terminator: no name, key, text or group.
    pub fn is_end(&self) -> bool {
        self.name.is_none() && self.key == 0 && self.doc.is_none() && self.group == 0
    }

    /// A group header: neither name nor key.
    fn is_header(&self) -> bool {
        self.name.is_none() && self.key == 0
    }
}

/// The short option a key denotes. Probed (ARGP-HELP-FILTER-KEYS): `:`, `;` and `-` are short
/// options too, as the GNU C Library manual says of every printable key: help and usage list
/// them (`-:`, `-;`, `--`) and `-` parses in a cluster (`-a-`); `:` and `;` never parse (see
/// `optstring`).
fn short(key: i32) -> Option<u8> {
    u8::try_from(key).ok().filter(|c| (0x20..0x7f).contains(c))
}

/// One `struct argp` of the tree.
#[derive(Debug, Default)]
pub struct Parser<'a> {
    /// The option table (without its terminator).
    pub options: TryVec<Opt<'a>>,
    /// The operands' description (alternatives separated by newlines).
    pub args_doc: Option<&'a [u8]>,
    /// The text before (and after a vertical tab, after) the options.
    pub doc: Option<&'a [u8]>,
    /// The parent's index (`None` for the root).
    pub parent: Option<usize>,
    /// The header the parent's child entry gives.
    pub header: Option<&'a [u8]>,
    /// The group the parent's child entry gives.
    pub group: i32,
    /// It has a parser function.
    pub has_parser: bool,
    /// It has a help filter.
    pub has_filter: bool,
}

/// The standard options to add.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    /// `--help`, `--usage`, `--program-name`, `--HANG`.
    pub help: bool,
    /// `--version`.
    pub version: bool,
}

/// A standard option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// `-?`, `--help`.
    Help,
    /// `--usage`.
    Usage,
    /// `--program-name=NAME`.
    ProgramName,
    /// `--HANG[=SECS]`.
    Hang,
    /// `-V`, `--version`.
    Version,
}

/// The texts of the standard options.
const STANDARD: [Opt<'static>; 4] = [
    Opt { name: Some(b"help".as_slice()), key: 0x3f, arg: None, flags: 0, doc: Some(b"Give this help list".as_slice()), group: -1 },
    Opt { name: Some(b"usage".as_slice()), key: 0, arg: None, flags: 0, doc: Some(b"Give a short usage message".as_slice()), group: 0 },
    Opt { name: Some(b"program-name".as_slice()), key: 0, arg: Some(b"NAME".as_slice()), flags: OPTION_HIDDEN, doc: Some(b"Set the program name".as_slice()), group: 0 },
    Opt { name: Some(b"HANG".as_slice()), key: 0, arg: Some(b"SECS".as_slice()), flags: OPTION_ARG_OPTIONAL | OPTION_HIDDEN, doc: Some(b"Hang for SECS seconds (default 3600)".as_slice()), group: 0 },
];
/// [`STANDARD`]'s meanings.
const STANDARD_KINDS: [Builtin; 4] = [Builtin::Help, Builtin::Usage, Builtin::ProgramName, Builtin::Hang];
/// `--version` (key `V`).
const VERSION: [Opt<'static>; 1] = [Opt { name: Some(b"version".as_slice()), key: 0x56, arg: None, flags: 0, doc: Some(b"Print program version".as_slice()), group: -1 }];

// ------------------------------------------------------------------------------ the compiled table

/// Who answers an entry's options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    /// The parser with this index.
    Parser(usize),
    /// A standard option, answered by the [`Host`].
    Builtin(Builtin),
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// An option to parse.
    Option,
    /// A documentation entry (`OPTION_DOC`), never parsed.
    Doc,
    /// A group header (neither name nor key).
    Header,
}

/// One entry: a non-alias option and the aliases after it.
#[derive(Clone, Copy, Debug)]
struct Entry<'a> {
    /// Who answers it.
    owner: Owner,
    /// The parser whose table holds it (the root for the standard options).
    parser: usize,
    /// Its level (an index into [`Table::levels`]).
    level: usize,
    /// The first option's key.
    key: i32,
    /// The argument's name.
    arg: Option<&'a [u8]>,
    /// `OPTION_ARG_OPTIONAL`.
    optional: bool,
    /// `OPTION_NO_USAGE`.
    no_usage: bool,
    /// What it is.
    kind: Kind,
    /// The help text.
    text: Option<&'a [u8]>,
    /// The help group.
    group: i32,
    /// Its names: `names[first..end]`.
    first: usize,
    /// See [`Entry::first`].
    end: usize,
}

/// One name of an entry: a short letter or a long name.
#[derive(Clone, Copy, Debug)]
struct Name<'a> {
    /// The entry.
    entry: usize,
    /// The short letter.
    short: Option<u8>,
    /// The long name.
    long: Option<&'a [u8]>,
    /// The key a parser is called with (an alias without a key reports its entry's).
    key: i32,
    /// No earlier option claimed it (always true for documentation names).
    claimed: bool,
    /// `OPTION_HIDDEN` on its own option (an alias of a hidden option is shown).
    hidden: bool,
}

impl Name<'_> {
    /// Shown in help and usage.
    fn visible(&self) -> bool {
        self.claimed && !self.hidden
    }
}

/// One step of a level's path: `(negative group, group, 0 own entries | 1 a cluster, cluster parser)`.
/// Paths compare lexicographically through [`step_key`]; groups order 0, 1, 2, ... then -k .. -1.
type Step = (bool, i32, u8, usize);

/// The step of a level's own entries of group `g`.
fn own(g: i32) -> Step {
    (g < 0, g, 0, 0)
}

/// The sort key of step `s` at depth `i` of a path. At the root's level (depth 0) a group's own
/// entries come before its clusters; inside a cluster its own entries come before all its
/// sub-clusters; sibling clusters of one group come later parser first (probed against the
/// reference, ARGP-HEADER-WHITESPACE-2).
fn step_key(i: usize, s: Step) -> (u8, bool, i32, u8, usize) {
    let (neg, g, kind, p) = s;
    let back = usize::MAX.saturating_sub(p);
    if i == 0 { (0, neg, g, kind, back) } else { (kind, neg, g, 0, back) }
}

/// The root's level, or a cluster: a child with a header or a non-zero group.
#[derive(Clone, Copy, Debug)]
struct Level<'a> {
    /// The parser whose filter sees the header (the cluster's parent).
    filter: usize,
    /// The header the parent's child entry gives.
    header: Option<&'a [u8]>,
    /// Its path: `steps[path..path + depth]`.
    path: usize,
    /// See [`Level::path`].
    depth: usize,
}

/// The union of a tree's option tables, with the standard options: what the parser and the
/// help text work from. As a [`Longs`] table, an entry's `val` is the index of its name.
#[derive(Debug)]
pub struct Table<'t, 'a> {
    tree: &'t [Parser<'a>],
    entries: TryVec<Entry<'a>>,
    names: TryVec<Name<'a>>,
    levels: TryVec<Level<'a>>,
    steps: TryVec<Step>,
    /// Name index + 1 per printable letter (0: none).
    by_letter: [usize; 96],
    /// The claimed long names; `val` is the name's index.
    longs: TryVec<LongOpt<'a>>,
}

impl Longs for Table<'_, '_> {
    fn entry(&self, i: usize) -> Option<LongOpt<'_>> {
        self.longs.get(i).copied()
    }
}

impl<'t, 'a> Table<'t, 'a> {
    /// Compile `tree` with the standard options `d`: one pass in preorder, the standard
    /// options last in the root's level. The first claimant of a short letter or long name keeps it.
    fn new(tree: &'t [Parser<'a>], d: Defaults) -> Result<Table<'t, 'a>> {
        let mut t = Table { tree, entries: TryVec::new(), names: TryVec::new(), levels: TryVec::new(), steps: TryVec::new(), by_letter: [0; 96], longs: TryVec::new() };
        t.levels.push(Level { filter: 0, header: None, path: 0, depth: 0 })?;
        let mut level_of: TryVec<usize> = TryVec::with_capacity(tree.len())?;
        for (p, parser) in tree.iter().enumerate() {
            let parent = parser.parent.filter(|&q| q < p);
            let level = match parent.and_then(|q| level_of.get(q).copied()) {
                Some(pl) if parser.header.is_some() || parser.group != 0 => t.cluster(pl, p, parent.unwrap_or(0))?,
                Some(pl) => pl,
                None => 0,
            };
            level_of.push(level)?;
            t.add(&parser.options, p, level, |_| Owner::Parser(p))?;
        }
        if d.help {
            t.add(&STANDARD, 0, 0, |k| Owner::Builtin(STANDARD_KINDS.get(k).copied().unwrap_or(Builtin::Help)))?;
        }
        if d.version {
            t.add(&VERSION, 0, 0, |_| Owner::Builtin(Builtin::Version))?;
        }
        Ok(t)
    }

    /// A new cluster level for parser `p` (parent `q`) inside level `pl`.
    fn cluster(&mut self, pl: usize, p: usize, q: usize) -> Result<usize> {
        let (from, depth) = self.levels.get(pl).map_or((0, 0), |l| (l.path, l.depth));
        let path = self.steps.len();
        for k in from..from.saturating_add(depth) {
            if let Some(&s) = self.steps.get(k) {
                self.steps.push(s)?;
            }
        }
        let group = self.tree.get(p).map_or(0, |t| t.group);
        self.steps.push((group < 0, group, 1, p))?;
        let header = self.tree.get(p).and_then(|t| t.header);
        self.levels.push(Level { filter: q, header, path, depth: depth.saturating_add(1) })?;
        Ok(self.levels.len().saturating_sub(1))
    }

    /// Add one option table (parser `parser`, level `level`); `owner(k)` answers option `k`.
    fn add(&mut self, opts: &[Opt<'a>], parser: usize, level: usize, owner: impl Fn(usize) -> Owner) -> Result<()> {
        let mut group = 0i32;
        let mut current: Option<usize> = None;
        for (k, o) in opts.iter().enumerate() {
            let header = o.is_header();
            if o.flags & OPTION_ALIAS != 0 && !header {
                if let Some(e) = current {
                    self.names_of(o, e)?;
                    continue;
                }
            }
            group = if o.group != 0 { o.group } else if header { group.wrapping_add(1) } else { group };
            let kind = if header { Kind::Header } else if o.flags & OPTION_DOC != 0 { Kind::Doc } else { Kind::Option };
            let at = self.names.len();
            self.entries.push(Entry {
                owner: owner(k),
                parser,
                level,
                key: o.key,
                arg: o.arg,
                optional: o.flags & OPTION_ARG_OPTIONAL != 0,
                no_usage: o.flags & OPTION_NO_USAGE != 0,
                kind,
                text: o.doc,
                group,
                first: at,
                end: at,
            })?;
            let e = self.entries.len().saturating_sub(1);
            current = Some(e);
            self.names_of(o, e)?;
        }
        Ok(())
    }

    /// Add the names option `o` gives entry `e`.
    fn names_of(&mut self, o: &Opt<'a>, e: usize) -> Result<()> {
        let Some(&entry) = self.entries.get(e) else { return Ok(()) };
        let key = if o.key != 0 { o.key } else { entry.key };
        let hidden = o.flags & OPTION_HIDDEN != 0;
        let has_arg = match (entry.arg, entry.optional) {
            (None, _) => HasArg::No,
            (Some(_), false) => HasArg::Required,
            (Some(_), true) => HasArg::Optional,
        };
        match entry.kind {
            Kind::Header => {}
            Kind::Doc => {
                if let Some(long) = o.name {
                    self.names.push(Name { entry: e, short: None, long: Some(long), key, claimed: true, hidden })?;
                }
            }
            Kind::Option => {
                if let Some(c) = short(o.key) {
                    let at = self.names.len();
                    let slot = self.by_letter.get_mut(usize::from(c).wrapping_sub(0x20));
                    let claimed = slot.as_ref().is_some_and(|s| **s == 0);
                    if let (true, Some(s)) = (claimed, slot) {
                        *s = at.saturating_add(1);
                    }
                    self.names.push(Name { entry: e, short: Some(c), long: None, key, claimed, hidden })?;
                }
                if let Some(long) = o.name {
                    let at = self.names.len();
                    let claimed = !self.longs.iter().any(|l| l.name == long);
                    if claimed {
                        let val = i32::try_from(at).map_err(|_| Errno::ENOMEM)?;
                        self.longs.push(LongOpt { name: long, has_arg, flag: 0, val })?;
                    }
                    self.names.push(Name { entry: e, short: None, long: Some(long), key, claimed, hidden })?;
                }
            }
        }
        if let Some(en) = self.entries.get_mut(e) {
            en.end = self.names.len();
        }
        Ok(())
    }

    /// The getopt option string: the order character, then each claimed letter with its marks.
    fn optstring(&self, flags: u32) -> Result<TryVec<u8>> {
        let mut s = TryVec::new();
        if flags & NO_ARGS != 0 {
            s.push(b'+')?;
        } else if flags & IN_ORDER != 0 {
            s.push(b'-')?;
        }
        for n in self.names.iter().filter(|n| n.claimed) {
            let Some(c) = n.short else { continue };
            if c == b':' || c == b';' {
                // Never options to getopt, as in the reference (`-:` and `-;` are invalid). Left
                // out, so a leading `:` does not silence the diagnostics (D-1.7, kept difference).
                continue;
            }
            if s.is_empty() && (c == b'+' || c == b'-') {
                // A leading `+` or `-` would be read as the order character; `;` is never an option.
                s.push(b';')?;
            }
            s.push(c)?;
            match self.entries.get(n.entry).map(|e| (e.arg.is_some(), e.optional)) {
                Some((true, false)) => s.push(b':')?,
                Some((true, true)) => s.extend_from_slice(b"::")?,
                _ => {}
            }
        }
        Ok(s)
    }

    /// The name short letter `c` denotes.
    fn short_name(&self, c: u8) -> Option<Name<'a>> {
        let i = self.by_letter.get(usize::from(c).wrapping_sub(0x20)).copied()?.checked_sub(1)?;
        self.names.get(i).copied()
    }

    /// The name long-table entry `i` denotes.
    fn long_name(&self, i: usize) -> Option<Name<'a>> {
        let n = usize::try_from(self.longs.get(i)?.val).ok()?;
        self.names.get(n).copied()
    }

    /// The names of entry `e`.
    fn names(&self, e: &Entry<'_>) -> &[Name<'a>] {
        self.names.get(e.first..e.end).unwrap_or(&[])
    }

    /// The path of level `l`.
    fn path(&self, l: usize) -> &[Step] {
        self.levels.get(l).and_then(|lv| self.steps.get(lv.path..lv.path.saturating_add(lv.depth))).unwrap_or(&[])
    }
}

// ------------------------------------------------------------------------------ parsing

/// The argument of a parser call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// None.
    None,
    /// An option's argument: these bytes of an element.
    Span(Span),
    /// An operand: the whole element.
    Element(usize),
}

/// The fields of `struct argp_state` the parser keeps: written before each call, read back
/// after it (`next`, `flags`, `input` and `hook` may be changed by the parser).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// `next`: the element to parse next.
    pub next: usize,
    /// `arg_num`: the operands the called parser has taken.
    pub arg_num: u32,
    /// `quoted`: the element after `--` (0 before one is seen).
    pub quoted: usize,
    /// `input` of the called parser.
    pub input: usize,
    /// `hook` of the called parser.
    pub hook: usize,
    /// `flags`.
    pub flags: u32,
}

/// A usage error the parse found itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem<'p> {
    /// An operand no parser takes, and no index was requested.
    TooManyArguments,
    /// An option its parser declined: as written, `-c` (`letter`) or `--name`.
    Unhandled {
        /// The short option used.
        letter: Option<u8>,
        /// The long name used.
        name: &'p [u8],
    },
}

/// What [`parse`] needs from its caller: the argument vector, the parser functions, and the
/// standard options and diagnostics, which belong to the C interface.
pub trait Host {
    /// The argument vector.
    type Args: Args + ?Sized;
    /// The argument vector as it is now (a parser may replace it).
    fn args(&mut self) -> &mut Self::Args;
    /// Call parser `parser` with `key` and `arg`, the state set from `view`; afterwards `view`
    /// holds what the parser left. The parser's result.
    fn call(&mut self, parser: usize, key: i32, arg: Arg, view: &mut View) -> i32;
    /// Slot `slot` of parser `parser`'s `child_inputs`.
    fn child_input(&mut self, parser: usize, slot: usize) -> usize;
    /// Run a standard option (help and version texts, which may end the process).
    fn builtin(&mut self, which: Builtin, arg: Option<Span>, view: &mut View);
    /// Report an option error `getopt` found (`table` lists the long options).
    fn option_error(&mut self, event: Event, table: &Table<'_, '_>, view: &mut View);
    /// Report a usage error the parse found.
    fn usage_error(&mut self, problem: Problem<'_>, view: &mut View);
}

/// A successful parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The first element not parsed.
    pub index: usize,
    /// Parsing stopped at an operand no parser took.
    pub stopped: bool,
}

/// A failed parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failed {
    /// The error: a parser's code, `EINVAL` for a usage error, `ENOMEM`.
    pub code: i32,
    /// The first element not parsed, when the arguments were all read before the failure.
    pub index: Option<usize>,
}

/// The parser-side state of one parser.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    /// `input`.
    input: usize,
    /// `hook`.
    hook: usize,
    /// `arg_num`: the operands it took.
    arg_num: u32,
    /// Its children whose input has been read (the next `child_inputs` slot).
    kids: usize,
}

/// How the operands from getopt's end finished.
enum Tail {
    /// All taken.
    Done,
    /// No parser took the operand here (or `NO_ARGS` forbids it).
    Stop(usize),
    /// A parser moved `next` here: options are read again from it.
    Moved(usize),
}

/// One parse in progress.
struct Run<'r, 't, 'a, H: Host + ?Sized> {
    tree: &'t [Parser<'a>],
    table: &'r Table<'t, 'a>,
    host: &'r mut H,
    slots: TryVec<Slot>,
    /// `next` as the parsers see it.
    next: usize,
    /// `quoted`: the element after `--` (0 before one is seen).
    quoted: usize,
    /// `flags` as the parsers leave them.
    flags: u32,
}

/// `EINVAL`, a usage error's code.
const EINVAL: i32 = Errno::EINVAL.get();

/// A parser's result that is an error (neither 0 nor `ARGP_ERR_UNKNOWN`).
fn failed(r: i32) -> bool {
    r != 0 && r != ERR_UNKNOWN
}

impl<H: Host + ?Sized> Run<'_, '_, '_, H> {
    /// The state parser `p` sees.
    fn view(&self, p: usize) -> View {
        let s = self.slots.get(p).copied().unwrap_or_default();
        View { next: self.next, arg_num: s.arg_num, quoted: self.quoted, input: s.input, hook: s.hook, flags: self.flags }
    }

    /// Call parser `p` (a parser without a function knows no key).
    fn call(&mut self, p: usize, key: i32, arg: Arg) -> i32 {
        if !self.tree.get(p).is_some_and(|t| t.has_parser) {
            return ERR_UNKNOWN;
        }
        let mut v = self.view(p);
        let r = self.host.call(p, key, arg, &mut v);
        if let Some(s) = self.slots.get_mut(p) {
            s.input = v.input;
            s.hook = v.hook;
        }
        self.next = v.next;
        self.flags = v.flags;
        r
    }

    /// The number of elements.
    fn argc(&mut self) -> usize {
        self.host.args().count()
    }

    /// Add `n` taken operands to parser `p`'s count.
    fn count(&mut self, p: usize, n: usize) {
        if let Some(s) = self.slots.get_mut(p) {
            s.arg_num = s.arg_num.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
        }
    }

    /// The error path: `ERROR` (preorder), `FINI` (reverse preorder).
    fn fail(&mut self, code: i32, index: Option<usize>) -> Failed {
        for p in 0..self.tree.len() {
            self.call(p, KEY_ERROR, Arg::None);
        }
        for p in (0..self.tree.len()).rev() {
            self.call(p, KEY_FINI, Arg::None);
        }
        Failed { code, index }
    }

    /// Offer operand `i` to each parser in preorder: `ARG`, then `ARGS`. `Some(n)`: taken,
    /// continue at element `n`; `None`: nobody took it.
    fn operand(&mut self, i: usize) -> core::result::Result<Option<usize>, i32> {
        let after = i.saturating_add(1);
        for p in 0..self.tree.len() {
            self.next = after;
            let r = self.call(p, KEY_ARG, Arg::Element(i));
            if r == 0 {
                let argc = self.argc();
                let n = self.next;
                let resume = if n > i {
                    // Raised `next`: the elements skipped count as taken.
                    self.count(p, n.min(argc.max(after)).saturating_sub(i));
                    n
                } else if n == i {
                    // Ruling #397: lowered to the operand's own index, the reference resumes one earlier.
                    i.saturating_sub(1)
                } else {
                    n
                };
                self.next = resume;
                return Ok(Some(resume));
            }
            if failed(r) {
                return Err(r);
            }
            self.next = i;
            let r = self.call(p, KEY_ARGS, Arg::None);
            if r == 0 {
                // ARGS consumes the rest, whatever the parser left in `next`.
                let argc = self.argc();
                self.count(p, argc.saturating_sub(i));
                self.next = argc;
                return Ok(Some(argc));
            }
            if failed(r) {
                return Err(r);
            }
        }
        self.next = i;
        Ok(None)
    }

    /// The operands from element `from` on, after getopt ended the options there.
    fn operands(&mut self, from: usize, flags: u32) -> core::result::Result<Tail, i32> {
        let mut i = from;
        loop {
            if i >= self.argc() {
                return Ok(Tail::Done);
            }
            if flags & NO_ARGS != 0 {
                return Ok(Tail::Stop(i));
            }
            match self.operand(i)? {
                None => return Ok(Tail::Stop(i)),
                Some(n) if n != i.saturating_add(1) && (self.quoted == 0 || n < self.quoted) => return Ok(Tail::Moved(n)),
                Some(n) => i = n,
            }
        }
    }

    /// An option (`name`), with its argument; `letter` when written as a short option.
    fn option(&mut self, name: Option<Name<'_>>, arg: Option<Span>, letter: Option<u8>) -> core::result::Result<(), i32> {
        let Some(name) = name else { return Err(EINVAL) };
        let Some(entry) = self.table.entries.get(name.entry).copied() else { return Err(EINVAL) };
        match entry.owner {
            Owner::Builtin(b) => {
                let mut v = self.view(0);
                self.host.builtin(b, arg, &mut v);
                self.next = v.next;
                self.flags = v.flags;
                Ok(())
            }
            Owner::Parser(p) => match self.call(p, name.key, arg.map_or(Arg::None, Arg::Span)) {
                0 => Ok(()),
                ERR_UNKNOWN => {
                    let mut v = self.view(p);
                    self.host.usage_error(Problem::Unhandled { letter, name: name.long.unwrap_or(&[]) }, &mut v);
                    Err(EINVAL)
                }
                r => Err(r),
            },
        }
    }

    /// The whole parse after the table is built (Decision 2's phases).
    fn go(&mut self, spec: &Spec<'_>, flags: u32, input: usize, want_index: bool) -> core::result::Result<Parsed, Failed> {
        let n = self.tree.len();
        // Init: preorder; a child's input is read after its parent's INIT.
        if let Some(s) = self.slots.get_mut(0) {
            s.input = input;
        }
        for p in 0..n {
            if let Some(q) = self.tree.get(p).and_then(|t| t.parent).filter(|&q| q < p) {
                let k = self.slots.get(q).map_or(0, |s| s.kids);
                if let Some(s) = self.slots.get_mut(q) {
                    s.kids = k.saturating_add(1);
                }
                let v = self.host.child_input(q, k);
                if let Some(s) = self.slots.get_mut(p) {
                    s.input = v;
                }
            }
            self.next = 0;
            let r = self.call(p, KEY_INIT, Arg::None);
            if failed(r) {
                return Err(self.fail(r, None));
            }
        }
        // Options, with the operands getopt returns in place or leaves at its end.
        let start = usize::from(!(flags & PARSE_ARGV0 != 0 && flags & NO_ERRS != 0));
        let mut st = State::new(0);
        st.moved(start);
        self.next = start;
        let long_only = flags & LONG_ONLY != 0;
        let stop = loop {
            let ev = getopt::next(&mut st, self.host.args(), spec, Some(self.table), long_only);
            self.next = st.next;
            let r = match ev {
                Event::End => {
                    if st.dashes {
                        self.quoted = st.next;
                    }
                    match self.operands(st.next, flags) {
                        Ok(Tail::Done) => break None,
                        Ok(Tail::Stop(i)) => break Some(i),
                        Ok(Tail::Moved(m)) => {
                            st.moved(m);
                            continue;
                        }
                        Err(c) => Err(c),
                    }
                }
                Event::Operand(i) => match self.operand(i) {
                    Ok(Some(m)) => {
                        if m != st.next {
                            st.moved(m);
                        }
                        continue;
                    }
                    Ok(None) => break Some(i),
                    Err(c) => Err(c),
                },
                Event::Short { letter, arg } => self.option(self.table.short_name(letter), arg, Some(letter)),
                Event::Long { entry, arg } => self.option(self.table.long_name(entry), arg, None),
                other => {
                    let mut v = self.view(0);
                    self.host.option_error(other, self.table, &mut v);
                    Err(EINVAL)
                }
            };
            if let Err(c) = r {
                return Err(self.fail(c, None));
            }
            if self.next != st.next {
                st.moved(self.next);
            }
        };
        // Finish: NO_ARGS (preorder, parsers that took no operand), END (reverse preorder).
        if let Some(i) = stop {
            self.next = i;
            if !want_index {
                let mut v = self.view(0);
                self.host.usage_error(Problem::TooManyArguments, &mut v);
                return Err(self.fail(EINVAL, None));
            }
        } else {
            self.next = self.argc();
            for p in 0..n {
                if self.slots.get(p).is_some_and(|s| s.arg_num == 0) {
                    let r = self.call(p, KEY_NO_ARGS, Arg::None);
                    if failed(r) {
                        let at = self.next;
                        return Err(self.fail(r, Some(at)));
                    }
                }
            }
            for p in (0..n).rev() {
                let r = self.call(p, KEY_END, Arg::None);
                if failed(r) {
                    let at = self.next;
                    return Err(self.fail(r, Some(at)));
                }
            }
        }
        // Success: SUCCESS, FINI (reverse preorder).
        let index = self.next;
        for p in (0..n).rev() {
            self.call(p, KEY_SUCCESS, Arg::None);
        }
        for p in (0..n).rev() {
            self.call(p, KEY_FINI, Arg::None);
        }
        Ok(Parsed { index, stopped: stop.is_some() })
    }
}

/// Parse the argument vector of `host` with `tree` (see the module documentation). `flags` are
/// the `ARGP_*` flags, `input` the root's input, `want_index` whether the caller asks for the
/// index of the first element not parsed (an operand no parser takes then ends the parse
/// successfully), `posix` POSIX mode (operands end the options).
pub fn parse<H: Host + ?Sized>(tree: &[Parser<'_>], d: Defaults, flags: u32, input: usize, want_index: bool, posix: bool, host: &mut H) -> core::result::Result<Parsed, Failed> {
    let nomem = Failed { code: Errno::ENOMEM.get(), index: None };
    let table = Table::new(tree, d).map_err(|_| nomem)?;
    let optstring = table.optstring(flags).map_err(|_| nomem)?;
    let spec = Spec::new(&optstring, posix);
    let mut slots = TryVec::with_capacity(tree.len()).map_err(|_| nomem)?;
    for _ in tree {
        slots.push(Slot::default()).map_err(|_| nomem)?;
    }
    let mut run = Run { tree, table: &table, host, slots, next: 0, quoted: 0, flags };
    run.go(&spec, flags, input, want_index)
}


// ------------------------------------------------------------------------------ help format

/// The help layout (`ARGP_HELP_FMT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// `short-opt-col`: where short options start.
    pub short_opt_col: usize,
    /// `long-opt-col`: where long options start.
    pub long_opt_col: usize,
    /// `doc-opt-col`: where documentation entries start.
    pub doc_opt_col: usize,
    /// `opt-doc-col`: where the options' texts start.
    pub opt_doc_col: usize,
    /// `header-col`: where headers start.
    pub header_col: usize,
    /// `usage-indent`: the indentation of continued usage lines.
    pub usage_indent: usize,
    /// `rmargin`: the longest line (a longer word stands alone on its line).
    pub rmargin: usize,
    /// `dup-args`: show the argument with short options too.
    pub dup_args: bool,
    /// `dup-args-note`: without `dup-args`, explain that the argument applies to both forms.
    pub dup_args_note: bool,
}

impl Default for Format {
    fn default() -> Format {
        Format { short_opt_col: 2, long_opt_col: 6, doc_opt_col: 2, opt_doc_col: 29, header_col: 1, usage_indent: 12, rmargin: 79, dup_args: false, dup_args_note: true }
    }
}

/// A problem in `ARGP_HELP_FMT` (the setting is ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError<'t> {
    /// A name that is no parameter.
    Unknown(&'t [u8]),
    /// A numeric parameter without a value, or with `no-`.
    NeedsValue(&'t [u8]),
    /// A value that is not a number (name, value).
    BadValue(&'t [u8], &'t [u8]),
    /// Text that names nothing (the rest of the setting).
    Garbage(&'t [u8]),
}

/// A decimal number up to 65535.
fn number(v: &[u8]) -> Option<usize> {
    if v.is_empty() || v.len() > 5 || !v.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = v.iter().fold(0usize, |n, &d| n.wrapping_mul(10).wrapping_add(usize::from(d.wrapping_sub(b'0'))));
    (n <= 65535).then_some(n)
}

/// A separator between `ARGP_HELP_FMT` items.
fn separator(c: u8) -> bool {
    c == b',' || c.is_ascii_whitespace()
}

impl Format {
    /// The layout from `ARGP_HELP_FMT` (`fmt`) and `COLUMNS` (`columns`, which sets the right
    /// margin to one less than the width it gives; `rmargin` overrides it). The setting is a
    /// list of `name`, `no-name` and `name=value` items separated by commas or white space;
    /// each problem goes to `report`.
    pub fn new(fmt: Option<&[u8]>, columns: Option<&[u8]>, mut report: impl FnMut(FormatError<'_>)) -> Format {
        let mut f = Format::default();
        if let Some(c) = columns.and_then(number).filter(|&c| c >= 2) {
            f.rmargin = c.saturating_sub(1);
        }
        let Some(mut rest) = fmt else { return f };
        loop {
            let skip = rest.iter().take_while(|&&c| separator(c)).count();
            rest = rest.get(skip..).unwrap_or(&[]);
            if rest.is_empty() {
                return f;
            }
            let len = rest.iter().take_while(|&&c| c.is_ascii_alphanumeric() || c == b'-').count();
            let (name, after) = rest.split_at(len.min(rest.len()));
            let (value, after) = match after.split_first() {
                _ if name.is_empty() => {
                    report(FormatError::Garbage(rest));
                    return f;
                }
                Some((b'=', v)) => {
                    let vl = v.iter().take_while(|&&c| !separator(c)).count();
                    let (value, after) = v.split_at(vl.min(v.len()));
                    (Some(value), after)
                }
                Some((&c, _)) if !separator(c) => {
                    report(FormatError::Garbage(rest));
                    return f;
                }
                _ => (None, after),
            };
            f.set(name, value, &mut report);
            rest = after;
        }
    }

    /// Apply one item.
    fn set(&mut self, given: &[u8], value: Option<&[u8]>, report: &mut impl FnMut(FormatError<'_>)) {
        let (negated, name) = match given.strip_prefix(b"no-") {
            Some(n) => (true, n),
            None => (false, given),
        };
        let flag = match name {
            b"dup-args" => Some(&mut self.dup_args),
            b"dup-args-note" => Some(&mut self.dup_args_note),
            _ => None,
        };
        if let Some(flag) = flag {
            match (negated, value) {
                (true, _) => *flag = false,
                (false, None) => *flag = true,
                (false, Some(v)) => match number(v) {
                    Some(n) => *flag = n != 0,
                    None => report(FormatError::BadValue(given, v)),
                },
            }
            return;
        }
        let field = match name {
            b"short-opt-col" => &mut self.short_opt_col,
            b"long-opt-col" => &mut self.long_opt_col,
            b"doc-opt-col" => &mut self.doc_opt_col,
            b"opt-doc-col" => &mut self.opt_doc_col,
            b"header-col" => &mut self.header_col,
            b"usage-indent" => &mut self.usage_indent,
            b"rmargin" => &mut self.rmargin,
            _ => return report(FormatError::Unknown(given)),
        };
        let least = usize::from(name == b"rmargin");
        match (negated, value) {
            (false, Some(v)) => match number(v).filter(|&n| n >= least) {
                Some(n) => *field = n,
                None => report(FormatError::BadValue(given, v)),
            },
            _ => report(FormatError::NeedsValue(given)),
        }
    }
}

// ------------------------------------------------------------------------------ help text

/// What a help filter made of a text.
#[derive(Debug)]
pub enum Filtered {
    /// Use the text as it is.
    Same,
    /// Print nothing.
    Omit,
    /// Use this text instead.
    Text(TryVec<u8>),
}

/// A text to print: none, the original, or a filter's replacement.
enum Txt<'x> {
    /// Nothing.
    No,
    /// The original.
    Ref(&'x [u8]),
    /// A replacement.
    Own(TryVec<u8>),
}

impl Txt<'_> {
    /// The text, when there is a non-empty one.
    fn get(&self) -> Option<&[u8]> {
        match self {
            Txt::No => None,
            Txt::Ref(t) => Some(*t),
            Txt::Own(t) => Some(t.as_slice()),
        }
        .filter(|t| !t.is_empty())
    }
}

/// The help output: the buffer and the current column.
struct Out<'o> {
    buf: &'o mut TryVec<u8>,
    col: usize,
}

impl Out<'_> {
    /// Text without newlines.
    fn put(&mut self, b: &[u8]) -> Result<()> {
        self.buf.extend_from_slice(b)?;
        self.col = self.col.saturating_add(b.len());
        Ok(())
    }

    /// End the line.
    fn nl(&mut self) -> Result<()> {
        self.buf.push(b'\n')?;
        self.col = 0;
        Ok(())
    }

    /// Spaces up to column `to`.
    fn pad(&mut self, to: usize) -> Result<()> {
        while self.col < to {
            self.put(b" ")?;
        }
        Ok(())
    }

    /// Spaces up to column `to`, at least one.
    fn gap(&mut self, to: usize) -> Result<()> {
        self.put(b" ")?;
        self.pad(to)
    }

    /// End the line unless it is empty.
    fn finish(&mut self) -> Result<()> {
        if self.col > 0 { self.nl() } else { Ok(()) }
    }

    /// `text` from the current column, words wrapped at `rmargin` to lines indented to `lmargin`
    /// (a word longer than the line stands alone); its newlines start new lines.
    fn wrap(&mut self, text: &[u8], lmargin: usize, rmargin: usize) -> Result<()> {
        for (k, line) in text.split(|&c| c == b'\n').enumerate() {
            if k > 0 {
                self.nl()?;
            }
            let mut first = true;
            let mut rest = line;
            loop {
                let g = rest.iter().take_while(|&&c| c == b' ').count();
                let (gap, tail) = rest.split_at(g.min(rest.len()));
                let w = tail.iter().take_while(|&&c| c != b' ').count();
                let (word, tail) = tail.split_at(w.min(tail.len()));
                rest = tail;
                if word.is_empty() {
                    break;
                }
                if self.col == 0 {
                    self.pad(lmargin)?;
                }
                if !first && self.col > lmargin && self.col.saturating_add(gap.len()).saturating_add(word.len()) > rmargin {
                    self.nl()?;
                    self.pad(lmargin)?;
                } else {
                    self.put(gap)?;
                }
                self.put(word)?;
                first = false;
            }
        }
        Ok(())
    }

    /// A blank line, when something precedes it (from `base` on).
    fn separate(&mut self, base: usize) -> Result<()> {
        if self.buf.len() > base { self.nl() } else { Ok(()) }
    }

    /// A usage unit: on this line when it fits (one with a space inside must end before the
    /// margin, others may end at it), else on a continuation line.
    fn unit(&mut self, u: &[u8], fmt: &Format) -> Result<()> {
        let end = self.col.saturating_add(1).saturating_add(u.len());
        let fits = if u.contains(&b' ') { end < fmt.rmargin } else { end <= fmt.rmargin };
        if fits {
            self.put(b" ")?;
        } else {
            self.nl()?;
            self.pad(fmt.usage_indent)?;
        }
        self.put(u)
    }
}

/// The help-side view of a tree: the table, its help order and the filter.
struct Help<'h, 't, 'a, F> {
    table: &'h Table<'t, 'a>,
    order: TryVec<usize>,
    fmt: &'h Format,
    filter: F,
}

/// The text before the vertical tab and the text after it.
fn split_doc(doc: Option<&[u8]>) -> (Option<&[u8]>, Option<&[u8]>) {
    let Some(doc) = doc else { return (None, None) };
    let (pre, post) = match doc.iter().position(|&c| c == 0x0b) {
        Some(at) => (doc.get(..at), doc.get(at.saturating_add(1)..)),
        None => (Some(doc), None),
    };
    (pre.filter(|t| !t.is_empty()), post.filter(|t| !t.is_empty()))
}

/// Byte-wise comparison ignoring case, then lower case first.
fn cmp_names(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
        .then_with(|| a.iter().map(|&c| swap_case(c)).cmp(b.iter().map(|&c| swap_case(c))))
}

/// Upper case to lower and lower to upper (so that lower case sorts first).
fn swap_case(c: u8) -> u8 {
    if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }
}

/// The explanation printed after the option list without `dup-args`.
const DUP_ARGS_NOTE: &[u8] = b"Mandatory or optional arguments to long options are also mandatory or optional for any corresponding short options.";

impl<'t, 'a> Table<'t, 'a> {
    /// Whether the help lists entry `e`.
    fn listed(&self, e: &Entry<'_>) -> bool {
        match e.kind {
            Kind::Header => true,
            Kind::Doc | Kind::Option => self.keyless(e) || self.names(e).iter().any(Name::visible),
        }
    }

    /// Whether `e` has no name at all (no long name, no printable key): the help lists such an
    /// entry as a header line carrying its text, even when it is hidden, as the reference does.
    fn keyless(&self, e: &Entry<'_>) -> bool {
        e.kind != Kind::Header && self.names(e).is_empty()
    }

    /// Whether the help prints `e` as a header line: a header or a keyless entry.
    fn headed(&self, e: &Entry<'_>) -> bool {
        e.kind == Kind::Header || self.keyless(e)
    }

    /// An entry's sort name: `(last, name, long only)`. A documentation entry sorts by its name
    /// without leading spaces and dashes, last unless it starts with `-`.
    fn sort_name(&self, e: &Entry<'_>) -> (bool, &[u8], bool) {
        let names = self.names(e);
        let mut shown = names.iter().filter(|n| n.visible());
        if e.kind == Kind::Doc {
            let n = shown.find_map(|n| n.long).unwrap_or(&[]);
            let n = n.get(n.iter().take_while(|&&c| c == b' ').count()..).unwrap_or(&[]);
            let rest = n.get(n.iter().take_while(|&&c| c == b'-').count()..).unwrap_or(&[]);
            return if rest.len() < n.len() { (false, rest, true) } else { (true, n, true) };
        }
        match names.iter().find(|n| n.visible() && n.short.is_some()) {
            Some(n) => (false, n.short.as_slice(), false),
            None => (false, shown.find_map(|n| n.long).unwrap_or(&[]), true),
        }
    }

    /// The help order of entries `a` and `b`: by place (section), then by name, then table order. A header
    /// sorts first in its group through its empty name, with no rule of its own: probed, a nameless
    /// entry listed before a header of its group stays before it (COLD-REWRITE2-W-far-01-PROOF-2).
    fn cmp_help(&self, a: usize, b: usize) -> Ordering {
        let (Some(ea), Some(eb)) = (self.entries.get(a), self.entries.get(b)) else { return a.cmp(&b) };
        let pa = self.path(ea.level).iter().copied().chain(core::iter::once(own(ea.group))).enumerate().map(|(i, s)| step_key(i, s));
        let pb = self.path(eb.level).iter().copied().chain(core::iter::once(own(eb.group))).enumerate().map(|(i, s)| step_key(i, s));
        pa.cmp(pb)
            .then_with(|| (!self.headed(ea)).cmp(&!self.headed(eb)))
            .then_with(|| {
                if self.headed(ea) {
                    return Ordering::Equal;
                }
                let (la, na, oa) = self.sort_name(ea);
                let (lb, nb, ob) = self.sort_name(eb);
                la.cmp(&lb).then_with(|| {
                    if la {
                        return Ordering::Equal;
                    }
                    let first = |n: &[u8]| n.first().map_or(0, u8::to_ascii_lowercase);
                    first(na).cmp(&first(nb)).then_with(|| ob.cmp(&oa)).then_with(|| cmp_names(na, nb))
                })
            })
            .then_with(|| a.cmp(&b))
    }

    /// The listed entries in help order (the order is computed once, from the place keys).
    fn help_order(&self) -> Result<TryVec<usize>> {
        let mut v = TryVec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if self.listed(e) {
                v.push(i)?;
            }
        }
        v.as_mut_slice().sort_unstable_by(|&a, &b| self.cmp_help(a, b));
        Ok(v)
    }
}

impl<F: FnMut(usize, i32, Option<&[u8]>) -> Filtered> Help<'_, '_, '_, F> {
    /// Parser `p`'s filter applied to `text` (no filter: the text).
    fn resolve<'x>(&mut self, p: Option<usize>, key: i32, text: Option<&'x [u8]>) -> Txt<'x> {
        let filtered = p.filter(|&p| self.table.tree.get(p).is_some_and(|t| t.has_filter));
        match filtered.map(|p| (self.filter)(p, key, text)) {
            None | Some(Filtered::Same) => text.map_or(Txt::No, Txt::Ref),
            Some(Filtered::Omit) => Txt::No,
            Some(Filtered::Text(t)) => Txt::Own(t),
        }
    }

    /// A header line at `header-col`, after a blank line when `lead` (the start of the option
    /// list) says one is due and the filter keeps the text.
    fn header(&mut self, o: &mut Out<'_>, p: Option<usize>, key: i32, text: Option<&[u8]>, lead: Option<usize>) -> Result<()> {
        let fmt = self.fmt;
        if let Some(t) = self.resolve(p, key, text).get() {
            if let Some(mark) = lead {
                o.separate(mark)?;
            }
            // Probed (ARGP-HEADER-WHITESPACE): every line of a header starts at header-col, an empty
            // one included, and keeps its spaces, trailing ones included, so a header of only spaces
            // prints a line of header-col plus those spaces. (Not probed: the empty line after a
            // final newline, left unpadded as before.)
            let lines = t.split(|&c| c == b'\n').count();
            for (k, line) in t.split(|&c| c == b'\n').enumerate() {
                if k > 0 {
                    o.nl()?;
                    if line.is_empty() && k.saturating_add(1) == lines {
                        break;
                    }
                }
                o.pad(fmt.header_col)?;
                o.wrap(line, fmt.header_col, fmt.rmargin)?;
                let spaces = line.iter().rev().take_while(|&&c| c == b' ').count();
                o.put(line.get(line.len().saturating_sub(spaces)..).unwrap_or(&[]))?;
            }
            o.finish()?;
        }
        Ok(())
    }

    /// One entry's line(s); whether it shows an argument under both a short and a long name.
    fn entry(&mut self, o: &mut Out<'_>, e: &Entry<'_>) -> Result<bool> {
        let (fmt, table) = (self.fmt, self.table);
        let names = table.names(e);
        let shown = || names.iter().filter(|n| n.visible());
        let has_long = shown().any(|n| n.long.is_some());
        let mut shorts = false;
        if e.kind == Kind::Doc {
            o.pad(fmt.doc_opt_col)?;
            for (k, n) in shown().filter_map(|n| n.long).enumerate() {
                o.put(if k == 0 { b"" } else { b", " })?;
                o.put(n)?;
            }
        } else {
            for c in shown().filter_map(|n| n.short) {
                if shorts {
                    o.put(b", ")?;
                } else {
                    o.pad(fmt.short_opt_col)?;
                }
                shorts = true;
                o.put(&[b'-', c])?;
                match e.arg {
                    Some(a) if fmt.dup_args || !has_long => {
                        o.put(if e.optional { b"[" } else { b" " })?;
                        o.put(a)?;
                        o.put(if e.optional { b"]" } else { b"" })?;
                    }
                    _ => {}
                }
            }
            for (k, n) in shown().filter_map(|n| n.long).enumerate() {
                match (k, shorts) {
                    (0, true) => {
                        o.put(b",")?;
                        o.gap(fmt.long_opt_col)?;
                    }
                    (0, false) => o.pad(fmt.long_opt_col)?,
                    _ => o.put(b", ")?,
                }
                o.put(b"--")?;
                o.put(n)?;
                if let Some(a) = e.arg {
                    o.put(if e.optional { b"[=" } else { b"=" })?;
                    o.put(a)?;
                    o.put(if e.optional { b"]" } else { b"" })?;
                }
            }
        }
        let p = match e.owner {
            Owner::Parser(p) => Some(p),
            Owner::Builtin(_) => None,
        };
        if let Some(t) = self.resolve(p, e.key, e.text).get() {
            let col = o.col;
            if col < fmt.opt_doc_col {
                o.pad(fmt.opt_doc_col)?;
            } else if col <= fmt.opt_doc_col.saturating_add(3) && col.saturating_add(3) < fmt.rmargin {
                o.put(b"   ")?;
            } else {
                o.nl()?;
                o.pad(fmt.opt_doc_col)?;
            }
            o.wrap(t, fmt.opt_doc_col, fmt.rmargin)?;
        }
        o.finish()?;
        Ok(e.kind == Kind::Option && e.arg.is_some() && shorts && has_long)
    }

    /// The option list in help order (probed against the reference, ARGP-HEADER-WHITESPACE-2): a
    /// header or keyless entry prints its text after a blank line (an empty text prints nothing);
    /// another entry follows a blank line when its group differs from the previous entry's and a
    /// header has come before it (ARGP-HELP-FILTER-KEYS-2: a group header or keyless entry, an
    /// empty one included, or a child's non-empty header; before the first, groups run on without one),
    /// then its cluster's header after a blank line when the previous entry is in another level.
    /// Then the `dup-args` note.
    fn options(&mut self, o: &mut Out<'_>) -> Result<()> {
        let table = self.table;
        let (mark, mut note) = (o.buf.len(), false);
        let mut prev: Option<(usize, i32)> = None;
        let mut headed = false;
        for k in 0..self.order.len() {
            let Some(e) = self.order.get(k).and_then(|&i| table.entries.get(i)).copied() else { continue };
            let last = prev.replace((e.level, e.group));
            if e.kind == Kind::Header {
                // Probed (ARGP-HELP-FILTER-KEYS): a group header reaches the filter with
                // `ARGP_KEY_HELP_HEADER`, like a keyless entry and a child's header.
                self.header(o, Some(e.parser), KEY_HELP_HEADER, e.text, Some(mark))?;
                headed = true;
                continue;
            }
            if table.keyless(&e) {
                self.header(o, Some(e.parser), KEY_HELP_HEADER, e.text, Some(mark))?;
                headed = true;
                continue;
            }
            if headed && last.is_some_and(|(_, g)| g != e.group) {
                o.separate(mark)?;
            }
            if e.level != 0 && !matches!(last, Some((l, _)) if l == e.level) {
                let lv = table.levels.get(e.level).copied();
                // Probed: a child's empty header does not count, unlike an empty group header.
                headed |= lv.is_some_and(|lv| lv.header.is_some_and(|h| !h.is_empty()));
                self.header(o, lv.map(|lv| lv.filter), KEY_HELP_HEADER, lv.and_then(|lv| lv.header), Some(mark))?;
            }
            note |= self.entry(o, &e)?;
        }
        if note && !self.fmt.dup_args && self.fmt.dup_args_note {
            o.separate(mark)?;
            o.wrap(DUP_ARGS_NOTE, 0, self.fmt.rmargin)?;
            o.finish()?;
        }
        Ok(())
    }

    /// The usage lines: one per combination of the parsers' `args_doc` alternatives (preorder,
    /// the root's varying slowest); `long` lists the options on the first.
    fn usage(&mut self, o: &mut Out<'_>, name: &[u8], long: bool) -> Result<()> {
        let (fmt, table) = (self.fmt, self.table);
        let mut docs: TryVec<TryVec<u8>> = TryVec::new();
        for p in 0..table.tree.len() {
            let text = table.tree.get(p).and_then(|t| t.args_doc);
            if let Some(t) = self.resolve(Some(p), KEY_HELP_ARGS_DOC, text).get() {
                docs.push(TryVec::from_slice(t)?)?;
            }
        }
        let mut digits: TryVec<usize> = TryVec::new();
        digits.resize(docs.len(), 0)?;
        fn alts(d: &[u8]) -> impl Iterator<Item = &[u8]> {
            d.split(|&c| c == b'\n')
        }
        let mut unit: TryVec<u8> = TryVec::new();
        let mut line = 0usize;
        loop {
            o.put(if line == 0 { b"Usage: " } else { b"  or:  " })?;
            o.put(name)?;
            if line == 0 && long {
                self.option_units(o, &mut unit)?;
            } else {
                o.unit(b"[OPTION...]", fmt)?;
            }
            unit.clear();
            for (d, &k) in docs.iter().zip(digits.iter()) {
                if let Some(a) = alts(d).nth(k).filter(|a| !a.is_empty()) {
                    if !unit.is_empty() {
                        unit.push(b' ')?;
                    }
                    unit.extend_from_slice(a)?;
                }
            }
            if !unit.is_empty() {
                o.unit(&unit, fmt)?;
            }
            o.nl()?;
            line = line.saturating_add(1);
            // The next combination: the last parser's alternative varies fastest.
            let mut carry = true;
            for (d, k) in docs.iter().zip(digits.as_mut_slice().iter_mut()).rev() {
                if !carry {
                    break;
                }
                *k = k.saturating_add(1);
                carry = *k >= alts(d).count();
                if carry {
                    *k = 0;
                }
            }
            if carry {
                return Ok(());
            }
        }
    }

    /// The option units of long usage: the short options without an argument together, each
    /// short option with one, each long option; documentation entries and `OPTION_NO_USAGE` left out.
    fn option_units(&mut self, o: &mut Out<'_>, unit: &mut TryVec<u8>) -> Result<()> {
        let (fmt, table) = (self.fmt, self.table);
        let used = |i: &usize| table.entries.get(*i).filter(|e| e.kind == Kind::Option && !e.no_usage);
        unit.clear();
        for e in self.order.iter().filter_map(used).filter(|e| e.arg.is_none()) {
            for c in table.names(e).iter().filter(|n| n.visible()).filter_map(|n| n.short) {
                unit.extend_from_slice(if unit.is_empty() { b"[-" } else { b"" })?;
                unit.push(c)?;
            }
        }
        if !unit.is_empty() {
            unit.push(b']')?;
            o.unit(unit, fmt)?;
        }
        for e in self.order.iter().filter_map(used) {
            let Some(a) = e.arg else { continue };
            for c in table.names(e).iter().filter(|n| n.visible()).filter_map(|n| n.short) {
                unit.clear();
                unit.extend_from_slice(&[b'[', b'-', c])?;
                unit.extend_from_slice(if e.optional { b"[" } else { b" " })?;
                unit.extend_from_slice(a)?;
                unit.extend_from_slice(if e.optional { b"]]" } else { b"]" })?;
                o.unit(unit, fmt)?;
            }
        }
        for e in self.order.iter().filter_map(used) {
            for n in table.names(e).iter().filter(|n| n.visible()).filter_map(|n| n.long) {
                unit.clear();
                unit.extend_from_slice(b"[--")?;
                unit.extend_from_slice(n)?;
                if let Some(a) = e.arg {
                    unit.extend_from_slice(if e.optional { b"[=" } else { b"=" })?;
                    unit.extend_from_slice(a)?;
                    unit.extend_from_slice(if e.optional { b"]" } else { b"" })?;
                }
                unit.push(b']')?;
                o.unit(unit, fmt)?;
            }
        }
        Ok(())
    }
}

/// The help text for `tree` with the standard options `d`, as the `ARGP_HELP_*` `flags` ask
/// (see the module documentation), appended to `out`. `name` is the program name, `bug` the
/// bug-report address; `filter(parser, key, text)` is that parser's help filter (called only
/// for parsers that have one).
#[allow(clippy::too_many_arguments)]
pub fn help<F: FnMut(usize, i32, Option<&[u8]>) -> Filtered>(tree: &[Parser<'_>], d: Defaults, fmt: &Format, flags: u32, name: &[u8], bug: Option<&[u8]>, filter: F, out: &mut TryVec<u8>) -> Result<()> {
    let table = Table::new(tree, d)?;
    let order = table.help_order()?;
    let mut h = Help { table: &table, order, fmt, filter };
    let base = out.len();
    let mut o = Out { buf: out, col: 0 };
    if flags & (HELP_USAGE | HELP_SHORT_USAGE) != 0 {
        h.usage(&mut o, name, flags & HELP_SHORT_USAGE == 0)?;
    }
    if flags & HELP_PRE_DOC != 0 {
        for p in 0..tree.len() {
            let (pre, _) = split_doc(tree.get(p).and_then(|t| t.doc));
            if let Some(t) = h.resolve(Some(p), KEY_HELP_PRE_DOC, pre).get() {
                o.wrap(t, 0, fmt.rmargin)?;
                o.finish()?;
                break;
            }
        }
    }
    if flags & HELP_SEE != 0 {
        for part in [&b"Try `"[..], name, b" --help' or `", name, b" --usage' for more information."] {
            o.put(part)?;
        }
        o.nl()?;
    }
    if flags & HELP_LONG != 0 {
        let at = o.buf.len();
        o.separate(base)?;
        let listed = o.buf.len();
        h.options(&mut o)?;
        if o.buf.len() == listed {
            o.buf.truncate(at);
        }
    }
    if flags & HELP_POST_DOC != 0 {
        for p in 0..tree.len() {
            let (_, post) = split_doc(tree.get(p).and_then(|t| t.doc));
            for (key, text) in [(KEY_HELP_POST_DOC, post), (KEY_HELP_EXTRA, None)] {
                if key == KEY_HELP_EXTRA && !tree.get(p).is_some_and(|t| t.has_filter) {
                    continue;
                }
                if let Some(t) = h.resolve(Some(p), key, text).get() {
                    o.separate(base)?;
                    o.wrap(t, 0, fmt.rmargin)?;
                    o.finish()?;
                }
            }
        }
    }
    if let Some(b) = bug.filter(|_| flags & HELP_BUG_ADDR != 0) {
        o.separate(base)?;
        o.put(b"Report bugs to ")?;
        o.put(b)?;
        o.put(b".")?;
        o.nl()?;
    }
    Ok(())
}
