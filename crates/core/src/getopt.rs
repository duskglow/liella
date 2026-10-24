//! Command-line parsing (spec §1.6, §1.6a; D-1.6, D-1.6a): a re-entrant option parser whose whole
//! position lives in a caller-owned [`State`], and a non-destructive suboption reader.
//!
//! Rewritten for COLD-REWRITE2-W-far-07 from the spec, `spec/behaviour/getopt.md`, POSIX and
//! black-box probes (`tests/oracle/getopt_diff.c`); the design decisions are
//! D-1.11.
//!
//! **Elements.** An *operand* is an element that does not start with `-`, or is exactly `-`.
//! `--` ends the options. Any other element holds options: one or more letters after one `-`,
//! or, with a long-option table, a long option after `--` (and, for [`next`]'s `long_only`,
//! after one `-`).
//!
//! **Order** ([`Spec::new`]). An option string starting with `-` returns operands in place
//! (code 1); one starting with `+`, or any option string in POSIX mode, stops at the first
//! operand; otherwise operands are *permuted*: skipped, and moved behind the options found
//! after them, so that when the options are exhausted the operands follow them in their
//! original order and the index names the first. The move happens at the next element
//! boundary, so an index returned with an option is valid in the vector as it is at that
//! moment. A `:` after the order character (or first) makes the parser quiet and reports a
//! missing argument as `:`.
//!
//! **The end.** Nothing latches: like POSIX's description, each call reads from [`State::next`].
//! Where the options were exhausted a further call ends again at the same element; after a `--`
//! it reads on (the elements behind the `--`).
//!
//! **Letters.** Each letter of the option string is an option, except `:` and `;`; `:` after a
//! letter makes its argument required (the rest of the element, else the next element, whatever
//! it is), `::` optional (the rest of the element only). With a long-option table, `W;` makes
//! `-W name` (or `-Wname`) a long option; without one, `W` is an ordinary letter.
//!
//! **Long options.** The name is the text up to the first `=`; the argument the text after it,
//! or for a required argument the next element. An exact name wins; otherwise a unique prefix.
//! A prefix shared by several entries is ambiguous, except (`--` and `-W` forms of
//! `getopt_long`, and the `-W` form of `getopt_long_only`) when all of them are equivalent to the
//! first (same argument kind, flag and value), as the reference library behaves. In
//! `long_only` mode, an element with one `-` is tried as a long option first unless it is a
//! single character that appears in the option string; when no entry matches and its first
//! character appears in the option string, it is read as letters instead.
//!
//! **`optopt`** ([`State::optopt`]) changes only on errors: the letter for a letter error, the
//! entry's value for a long option's argument error, 0 for an unknown or ambiguous long option.

/// How operands are handled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// Skip operands and move them behind the options (GNU default).
    Permute,
    /// Stop at the first operand (`+`, POSIX mode).
    RequireOrder,
    /// Return each operand in place as option code 1 (`-`).
    ReturnInOrder,
}

/// Whether an option takes an argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HasArg {
    /// None.
    No,
    /// Required.
    Required,
    /// Optional (only attached: `-xARG`, `--name=ARG`).
    Optional,
}

impl HasArg {
    /// From the C table's `has_arg` (`no_argument` 0, `required_argument` 1,
    /// `optional_argument` 2); any other value takes no argument.
    pub fn of(v: i32) -> HasArg {
        match v {
            1 => HasArg::Required,
            2 => HasArg::Optional,
            _ => HasArg::No,
        }
    }
}

/// A parsed short-option specification.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    /// The whole option string (searched as text by `long_only`).
    text: &'a [u8],
    /// The letters and their `:` marks, after the order character and the quiet `:`.
    letters: &'a [u8],
    /// How operands are handled.
    pub order: Order,
    /// A leading `:`: no diagnostics, and `:` for a missing argument.
    pub quiet: bool,
}

/// What a letter of the option string means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Letter {
    /// An option letter.
    Plain(HasArg),
    /// `W;`: `-W name` is a long option when there is a table.
    W,
}

impl<'a> Spec<'a> {
    /// Parse `optstring`; `posix` (POSIX mode: `POSIXLY_CORRECT`, `__posix_getopt`) stops at the
    /// first operand unless the string asks for operands in place.
    pub fn new(optstring: &'a [u8], posix: bool) -> Spec<'a> {
        let (order, rest) = match optstring.split_first() {
            Some((b'-', rest)) => (Order::ReturnInOrder, rest),
            Some((b'+', rest)) => (Order::RequireOrder, rest),
            _ if posix => (Order::RequireOrder, optstring),
            _ => (Order::Permute, optstring),
        };
        let (quiet, letters) = match rest.split_first() {
            Some((b':', letters)) => (true, letters),
            _ => (false, rest),
        };
        Spec { text: optstring, letters, order, quiet }
    }

    /// Whether `c` appears anywhere in the option string.
    fn mentions(&self, c: u8) -> bool {
        self.text.contains(&c)
    }

    /// The meaning of letter `c` (its first occurrence), `None` when it is not an option.
    fn letter(&self, c: u8) -> Option<Letter> {
        if c == b':' || c == b';' {
            return None;
        }
        let at = self.letters.iter().position(|&l| l == c)?;
        let marks = self.letters.get(at.saturating_add(1)..).unwrap_or(&[]);
        Some(match marks {
            [b';', ..] if c == b'W' => Letter::W,
            [b':', b':', ..] => Letter::Plain(HasArg::Optional),
            [b':', ..] => Letter::Plain(HasArg::Required),
            _ => Letter::Plain(HasArg::No),
        })
    }
}

/// One entry of a long-option table.
#[derive(Clone, Copy, Debug)]
pub struct LongOpt<'n> {
    /// The name.
    pub name: &'n [u8],
    /// Its argument.
    pub has_arg: HasArg,
    /// The address of the variable that receives `val` (0: none; then `val` is returned).
    pub flag: usize,
    /// The value.
    pub val: i32,
}

impl LongOpt<'_> {
    /// Whether choosing `self` or `o` would make no difference.
    fn same_as(&self, o: &LongOpt<'_>) -> bool {
        self.has_arg == o.has_arg && self.flag == o.flag && self.val == o.val
    }
}

/// A long-option table.
pub trait Longs {
    /// Entry `i`, `None` at and after the terminator.
    fn entry(&self, i: usize) -> Option<LongOpt<'_>>;
}

impl Longs for [LongOpt<'_>] {
    fn entry(&self, i: usize) -> Option<LongOpt<'_>> {
        self.get(i).copied()
    }
}

/// An argument vector the parser may reorder.
pub trait Args {
    /// The number of elements.
    fn count(&self) -> usize;
    /// Element `i` (empty when out of range).
    fn text(&self, i: usize) -> &[u8];
    /// Exchange elements `i` and `j`.
    fn swap(&mut self, i: usize, j: usize);
}

impl<T: AsRef<[u8]>> Args for [T] {
    fn count(&self) -> usize {
        self.len()
    }
    fn text(&self, i: usize) -> &[u8] {
        self.get(i).map_or(&[], AsRef::as_ref)
    }
    fn swap(&mut self, i: usize, j: usize) {
        if i < self.len() && j < self.len() {
            <[T]>::swap(self, i, j);
        }
    }
}

/// Bytes `start..end` of element `index`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    /// The element.
    pub index: usize,
    /// The first byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// The bytes in `args`.
    pub fn of<'s, A: Args + ?Sized>(&self, args: &'s A) -> &'s [u8] {
        args.text(self.index).get(self.start..self.end).unwrap_or(&[])
    }

    /// The whole of element `index` of `args`.
    fn whole<A: Args + ?Sized>(args: &A, index: usize) -> Span {
        Span { index, start: 0, end: args.text(index).len() }
    }
}

/// How a long option was written (diagnostics repeat it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dash {
    /// `-name` (`long_only`).
    One,
    /// `--name`.
    Two,
    /// `-W name`.
    W,
}

impl Dash {
    /// The text written before the name.
    pub fn text(self) -> &'static [u8] {
        match self {
            Dash::One => b"-",
            Dash::Two => b"--",
            Dash::W => b"-W ",
        }
    }
}

/// What one call found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// No more options.
    End,
    /// An option letter, with its argument.
    Short {
        /// The letter.
        letter: u8,
        /// The argument.
        arg: Option<Span>,
    },
    /// A long option (entry index), with its argument.
    Long {
        /// The entry.
        entry: usize,
        /// The argument.
        arg: Option<Span>,
    },
    /// An operand returned in place (element index).
    Operand(usize),
    /// A letter that is not an option.
    Invalid(u8),
    /// A letter whose required argument is missing.
    MissingShort(u8),
    /// A long option no entry matches (`text`: as written, without the dashes).
    Unrecognized {
        /// How it was written.
        dash: Dash,
        /// The text.
        text: Span,
    },
    /// A prefix several entries share ([`candidates`] lists them).
    Ambiguous {
        /// How it was written.
        dash: Dash,
        /// The text as written, without the dashes.
        text: Span,
        /// The name (the text before any `=`).
        name: Span,
        /// Every prefix match counts (otherwise those equivalent to the first do not).
        strict: bool,
    },
    /// A long option that takes no argument, given one.
    NoArgument {
        /// How it was written.
        dash: Dash,
        /// The entry.
        entry: usize,
    },
    /// A long option whose required argument is missing.
    MissingLong {
        /// How it was written.
        dash: Dash,
        /// The entry.
        entry: usize,
    },
}

/// The parser's position, owned by the caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State {
    /// The next element to process (`optind`).
    pub next: usize,
    /// The offset of the next letter in element `next`; 0 at an element boundary.
    pub cursor: usize,
    /// The operands skipped and not yet moved (permuting order): elements `first..last`.
    pub first: usize,
    /// See [`State::first`].
    pub last: usize,
    /// The last call found the options exhausted ([`Event::End`]). Informational: a further call
    /// reads on from [`State::next`] (see the module documentation).
    pub ended: bool,
    /// The last call ended the options at a `--` element (argp's `quoted`).
    pub dashes: bool,
    /// The last error's option (`optopt`).
    pub optopt: i32,
}

impl Default for State {
    fn default() -> State {
        State::new(0)
    }
}

impl State {
    /// A parse starting at element 1 (element 0 is the program name), keeping `optopt`.
    pub fn new(optopt: i32) -> State {
        State { next: 1, cursor: 0, first: 1, last: 1, ended: false, dashes: false, optopt }
    }

    /// Continue from element `next` (the caller moved the index): the position within an element
    /// and the skipped operands are forgotten.
    pub fn moved(&mut self, next: usize) {
        *self = State { next, first: next, last: next, ..State::new(self.optopt) };
    }

    /// Step past element `next` to the boundary after it.
    fn take(&mut self) {
        self.next = self.next.saturating_add(1);
        self.cursor = 0;
    }
}

/// Whether element text `t` is an operand.
fn operand(t: &[u8]) -> bool {
    t.first() != Some(&b'-') || t.len() == 1
}

/// Exchange the `n` elements from `a` with the `n` elements from `b` (the ranges do not overlap).
fn swap_blocks<A: Args + ?Sized>(args: &mut A, a: usize, b: usize, n: usize) {
    for k in 0..n {
        args.swap(a.wrapping_add(k), b.wrapping_add(k));
    }
}

/// Move elements `mid..hi` in front of `lo..mid`, keeping the order within each, by block swaps:
/// each step puts the shorter block in its final place, `hi - lo - gcd` swaps in all.
fn rotate<A: Args + ?Sized>(args: &mut A, lo: usize, mid: usize, hi: usize) {
    let (mut at, mut left, mut right) = (lo, mid.saturating_sub(lo), hi.saturating_sub(mid));
    while left > 0 && right > 0 {
        if left <= right {
            // L R1 R2 with |R2| = |L| -> R2 R1 L: L is placed; R2 R1 remains.
            swap_blocks(args, at, at.wrapping_add(right), left);
            right = right.wrapping_sub(left);
        } else {
            // L1 L2 R with |L1| = |R| -> R L2 L1: R is placed; L2 L1 remains.
            swap_blocks(args, at, at.wrapping_add(left), right);
            at = at.wrapping_add(right);
            left = left.wrapping_sub(right);
        }
    }
}

/// Classify the next option of `args` (see the module documentation); `longs` is the long-option
/// table, if any.
pub fn next<A: Args + ?Sized, L: Longs + ?Sized>(st: &mut State, args: &mut A, spec: &Spec<'_>, longs: Option<&L>, long_only: bool) -> Event {
    st.ended = false;
    st.dashes = false;
    if st.cursor != 0 && st.cursor >= args.text(st.next).len() {
        // The element was cut short under the cursor: read it again from its start, at the
        // same index (probe of glibc 2.39; see the decision record, Proof run 2).
        st.cursor = 0;
    }
    if st.cursor == 0 {
        if let Some(e) = boundary(st, args, spec.order) {
            return e;
        }
        if let Some(table) = longs {
            if let Some(e) = long_element(st, args, spec, table, long_only) {
                return e;
            }
        }
        st.cursor = 1;
    }
    letter(st, args, spec, longs, long_only)
}

/// At an element boundary: move skipped operands, skip new ones, and handle `--`, the end and
/// operands. `None` when element `st.next` holds options.
fn boundary<A: Args + ?Sized>(st: &mut State, args: &mut A, order: Order) -> Option<Event> {
    let count = args.count();
    if order == Order::Permute {
        let pending = st.first < st.last && st.last <= st.next && st.next <= count;
        st.first = if pending {
            // The options parsed since the skip (`last..next`) go in front of the operands.
            rotate(args, st.first, st.last, st.next);
            st.first.saturating_add(st.next.saturating_sub(st.last))
        } else {
            st.next
        };
        let mut i = st.next;
        while i < count && operand(args.text(i)) {
            i = i.saturating_add(1);
        }
        st.last = i;
        st.next = i;
    }
    if st.next >= count {
        if order == Order::Permute {
            st.next = st.first;
            st.last = st.first;
        }
        st.ended = true;
        return Some(Event::End);
    }
    let t = args.text(st.next);
    if t == b"--" {
        st.next = st.next.saturating_add(1);
        st.ended = true;
        st.dashes = true;
        if order == Order::Permute {
            if st.first < st.last {
                // The `--` goes in front of the skipped operands; the index names the first.
                rotate(args, st.first, st.last, st.next);
                st.first = st.first.saturating_add(1);
            } else {
                st.first = st.next;
            }
            st.next = st.first;
            st.last = st.first;
        }
        return Some(Event::End);
    }
    if operand(t) {
        if order == Order::ReturnInOrder {
            let at = st.next;
            st.next = at.saturating_add(1);
            return Some(Event::Operand(at));
        }
        st.ended = true;
        return Some(Event::End);
    }
    None
}

/// Element `st.next` (which holds options) as a long option: `--name`, or `-name` in
/// `long_only` mode. `None` when it is to be read as letters.
fn long_element<A: Args + ?Sized, L: Longs + ?Sized>(st: &mut State, args: &A, spec: &Spec<'_>, table: &L, long_only: bool) -> Option<Event> {
    let t = args.text(st.next);
    let len = t.len();
    if t.starts_with(b"--") {
        let text = Span { index: st.next, start: 2, end: len };
        return Some(long(st, args, table, text, Dash::Two, long_only, true).unwrap_or_else(|| unrecognized(st, Dash::Two, text, true)));
    }
    if !long_only {
        return None;
    }
    let lead = t.get(1).is_some_and(|&c| spec.mentions(c));
    if lead && len == 2 {
        return None;
    }
    let text = Span { index: st.next, start: 1, end: len };
    long(st, args, table, text, Dash::One, true, true).or_else(|| (!lead).then(|| unrecognized(st, Dash::One, text, true)))
}

/// A long option no entry matches; `take`: its element is still to be taken.
fn unrecognized(st: &mut State, dash: Dash, text: Span, take: bool) -> Event {
    if take {
        st.take();
    }
    st.optopt = 0;
    Event::Unrecognized { dash, text }
}

/// The next letter of element `st.next`, at `st.cursor`.
fn letter<A: Args + ?Sized, L: Longs + ?Sized>(st: &mut State, args: &mut A, spec: &Spec<'_>, longs: Option<&L>, long_only: bool) -> Event {
    let index = st.next;
    let t = args.text(index);
    let c = t.get(st.cursor).copied().unwrap_or(0);
    let after = st.cursor.saturating_add(1);
    // The rest of the element after the letter, if any.
    let rest = (after < t.len()).then_some(Span { index, start: after, end: t.len() });
    let kind = spec.letter(c);
    if matches!(kind, None | Some(Letter::Plain(HasArg::No) | Letter::W)) && !(kind == Some(Letter::W) && longs.is_some()) {
        // A letter that takes nothing: the cluster goes on.
        if rest.is_some() {
            st.cursor = after;
        } else {
            st.take();
        }
        if kind.is_none() {
            st.optopt = i32::from(c);
            return Event::Invalid(c);
        }
        return Event::Short { letter: c, arg: None };
    }
    // The letter ends the element: its argument, if any, is the rest of it or the next element.
    st.take();
    match (kind, longs) {
        (Some(Letter::Plain(HasArg::Optional)), _) => Event::Short { letter: c, arg: rest },
        (Some(Letter::W), Some(table)) => {
            let Some(text) = separate(st, args, rest) else {
                st.optopt = i32::from(c);
                return Event::MissingShort(c);
            };
            long(st, args, table, text, Dash::W, long_only, false).unwrap_or_else(|| unrecognized(st, Dash::W, text, false))
        }
        _ => match separate(st, args, rest) {
            Some(arg) => Event::Short { letter: c, arg: Some(arg) },
            None => {
                st.optopt = i32::from(c);
                Event::MissingShort(c)
            }
        },
    }
}

/// A required argument: `attached`, else the whole of element `st.next` (taken), else `None`.
fn separate<A: Args + ?Sized>(st: &mut State, args: &A, attached: Option<Span>) -> Option<Span> {
    if attached.is_some() {
        return attached;
    }
    if st.next >= args.count() {
        return None;
    }
    let arg = Span::whole(args, st.next);
    st.take();
    Some(arg)
}

/// What a long-option name matches.
enum Match<'l> {
    /// One entry (exact, or the only prefix match up to equivalence).
    Entry(usize, LongOpt<'l>),
    /// Several entries.
    Ambiguous,
}

/// Look `name` up in `table` in one pass: an exact name wins at once; otherwise the first prefix
/// match, unless another one differs from it (any other one when `strict`).
fn lookup<'l, L: Longs + ?Sized>(table: &'l L, name: &[u8], strict: bool) -> Option<Match<'l>> {
    let mut first: Option<(usize, LongOpt<'l>)> = None;
    let mut ambiguous = false;
    let mut i = 0usize;
    while let Some(e) = table.entry(i) {
        if e.name.starts_with(name) {
            if e.name.len() == name.len() {
                return Some(Match::Entry(i, e));
            }
            match first {
                None => first = Some((i, e)),
                Some((_, f)) => ambiguous |= strict || !f.same_as(&e),
            }
        }
        i = i.saturating_add(1);
    }
    if ambiguous {
        return Some(Match::Ambiguous);
    }
    first.map(|(i, e)| Match::Entry(i, e))
}

/// The long option written as `text`: `None` when no entry matches (nothing is taken).
/// `take`: the element holding `text` is still to be taken (not for `-W`).
fn long<A: Args + ?Sized, L: Longs + ?Sized>(st: &mut State, args: &A, table: &L, text: Span, dash: Dash, long_only: bool, take: bool) -> Option<Event> {
    let written = text.of(args);
    let eq = written.iter().position(|&b| b == b'=');
    let name = Span { end: text.start.saturating_add(eq.unwrap_or(written.len())), ..text };
    let strict = long_only && dash != Dash::W;
    let found = lookup(table, name.of(args), strict)?;
    if take {
        st.take();
    }
    let (entry, e) = match found {
        Match::Entry(entry, e) => (entry, e),
        Match::Ambiguous => {
            st.optopt = 0;
            return Some(Event::Ambiguous { dash, text, name, strict });
        }
    };
    let attached = eq.map(|q| Span { start: text.start.saturating_add(q).saturating_add(1), ..text });
    Some(match (e.has_arg, attached) {
        (HasArg::No, Some(_)) => {
            st.optopt = e.val;
            Event::NoArgument { dash, entry }
        }
        (HasArg::Required, None) => match separate(st, args, None) {
            Some(arg) => Event::Long { entry, arg: Some(arg) },
            None => {
                st.optopt = e.val;
                Event::MissingLong { dash, entry }
            }
        },
        (_, arg) => Event::Long { entry, arg },
    })
}

/// The entries an [`Event::Ambiguous`] name matches, in table order, as its diagnostic lists
/// them: every entry the name is a prefix of when `strict`, otherwise the first and those not
/// equivalent to it.
pub fn candidates<'l, L: Longs + ?Sized>(table: &'l L, name: &[u8], strict: bool, mut each: impl FnMut(LongOpt<'l>)) {
    let mut first: Option<LongOpt<'l>> = None;
    let mut i = 0usize;
    while let Some(e) = table.entry(i) {
        if e.name.starts_with(name) {
            match first {
                None => {
                    first = Some(e);
                    each(e);
                }
                Some(f) if strict || !f.same_as(&e) => each(e),
                Some(_) => {}
            }
        }
        i = i.saturating_add(1);
    }
}

/// One suboption of a `getsubopt` text (offsets into the text).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subopt {
    /// The token it names (exactly), if any.
    pub token: Option<usize>,
    /// The end of the name (where `=` is, or [`Subopt::end`]).
    pub name_end: usize,
    /// The start of the value (after the first `=`), if there is one.
    pub value: Option<usize>,
    /// The end of the suboption (where `,` is, or the end of the text).
    pub end: usize,
    /// Where the next suboption starts (after the `,`, or the end of the text).
    pub next: usize,
}

/// The first suboption of `text` (`None` when it is empty), matched against `tokens`.
pub fn subopt<'t>(text: &[u8], tokens: impl IntoIterator<Item = &'t [u8]>) -> Option<Subopt> {
    if text.is_empty() {
        return None;
    }
    let comma = text.iter().position(|&b| b == b',');
    let end = comma.unwrap_or(text.len());
    let item = text.get(..end).unwrap_or(&[]);
    let eq = item.iter().position(|&b| b == b'=');
    let name_end = eq.unwrap_or(end);
    let name = item.get(..name_end).unwrap_or(&[]);
    Some(Subopt {
        token: tokens.into_iter().position(|t| t == name),
        name_end,
        value: eq.map(|q| q.saturating_add(1)),
        end,
        next: comma.map_or(end, |c| c.saturating_add(1)),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    fn opt(name: &'static str, has_arg: HasArg, val: i32) -> LongOpt<'static> {
        LongOpt { name: name.as_bytes(), has_arg, flag: 0, val }
    }

    /// Parse `argv` to the end: each event with the index after it.
    fn run(argv: &mut [&str], optstring: &str, longs: Option<&[LongOpt<'_>]>, long_only: bool) -> Vec<(Event, usize)> {
        let spec = Spec::new(optstring.as_bytes(), false);
        let mut st = State::new(0);
        let mut out = Vec::new();
        for _ in 0..40 {
            let e = next(&mut st, argv, &spec, longs, long_only);
            out.push((e, st.next));
            if e == Event::End {
                break;
            }
        }
        out
    }

    #[test]
    fn permutes_operands_behind_options() {
        let mut v = ["prog", "op1", "-a", "op2", "-b", "--", "-c", "op3"];
        let r = run(&mut v, "abc:", None, false);
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), [3, 5, 4]);
        assert_eq!(v, ["prog", "-a", "-b", "--", "op1", "op2", "-c", "op3"]);
        let mut v = ["prog", "a", "b", "-a", "c", "-b", "d", "-c", "e"];
        let r = run(&mut v, "abc:", None, false);
        assert_eq!(r.last(), Some(&(Event::End, 5)));
        assert_eq!(v, ["prog", "-a", "-b", "-c", "e", "a", "b", "c", "d"]);
    }

    #[test]
    fn rotation_keeps_both_blocks_in_order() {
        for n in 0..12usize {
            for mid in 0..=n {
                let mut v: Vec<String> = (0..n).map(|k| k.to_string()).collect();
                rotate(v.as_mut_slice(), 0, mid, n);
                let want: Vec<String> = (mid..n).chain(0..mid).map(|k| k.to_string()).collect();
                assert_eq!(v, want, "n {n} mid {mid}");
            }
        }
    }

    #[test]
    fn reads_on_after_a_double_dash() {
        let mut v = ["prog", "op1", "-a", "--", "-b", "op2"];
        let spec = Spec::new(b"ab", false);
        let mut st = State::new(0);
        let none = None::<&[LongOpt<'_>]>;
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'a', arg: None });
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::End);
        assert_eq!((st.next, st.dashes, st.ended), (3, true, true));
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'b', arg: None }, "POSIX: the next call reads from optind");
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::End);
        assert_eq!(v, ["prog", "-a", "--", "-b", "op1", "op2"]);
        assert_eq!((st.next, st.dashes), (4, false));
    }

    #[test]
    fn first_occurrence_of_a_letter_wins() {
        // Probe (host glibc): with "aa:" `-a x` is `a` without an argument; with "a:a", with `x`.
        let mut v = ["prog", "-a", "x"];
        assert_eq!(run(&mut v, "aa:", None, false)[0], (Event::Short { letter: b'a', arg: None }, 2));
        let mut v = ["prog", "-a", "x"];
        assert_eq!(run(&mut v, "a:a", None, false)[0], (Event::Short { letter: b'a', arg: Some(Span { index: 2, start: 0, end: 1 }) }, 3));
    }

    #[test]
    fn moved_index_forgets_skipped_operands() {
        // `a` is skipped before `-x`; the caller then moves the index past `b`. The skipped
        // operand is forgotten: nothing before the new index is moved again.
        let mut v = ["prog", "a", "-x", "b", "-y", "c"];
        let spec = Spec::new(b"xy", false);
        let mut st = State::new(0);
        let none = None::<&[LongOpt<'_>]>;
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'x', arg: None });
        assert_eq!((st.first, st.last, st.next), (1, 2, 3));
        st.moved(4);
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'y', arg: None });
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::End);
        assert_eq!(v, ["prog", "a", "-x", "b", "-y", "c"]);
        assert_eq!(st.next, 5);
    }

    #[test]
    fn element_cut_under_the_cursor_is_read_again() {
        // Probe (host glibc 2.39): `-ab` read to `a`, then cut in place to `-a`: the next call
        // reads that element again from its start (`a`, index 2), not the element after it.
        let mut v = ["prog", "-ab", "-c"];
        let spec = Spec::new(b"abc", false);
        let mut st = State::new(0);
        let none = None::<&[LongOpt<'_>]>;
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'a', arg: None });
        assert_eq!((st.next, st.cursor), (1, 2));
        v[1] = "-a";
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'a', arg: None });
        assert_eq!(st.next, 2);
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::Short { letter: b'c', arg: None });
        assert_eq!(next(&mut st, &mut v[..], &spec, none, false), Event::End);
    }

    #[test]
    fn orders_and_arguments() {
        let mut v = ["prog", "-ab", "x", "-cfoo", "-c", "bar", "-d", "-dz"];
        let r = run(&mut v, "+abc:d::", None, false);
        assert_eq!(r[0], (Event::Short { letter: b'a', arg: None }, 1));
        assert_eq!(r[1], (Event::Short { letter: b'b', arg: None }, 2));
        assert_eq!(r[2], (Event::End, 2));
        let mut v = ["prog", "x", "-c"];
        let r = run(&mut v, "-:c:", None, false);
        assert_eq!(r[0], (Event::Operand(1), 2));
        assert_eq!(r[1], (Event::MissingShort(b'c'), 3));
        assert!(Spec::new(b"-:c:", false).quiet);
        let mut v = ["prog", "-d", "-dz", "-;"];
        let r = run(&mut v, "d::W;", None, false);
        assert_eq!(r[0].0, Event::Short { letter: b'd', arg: None });
        assert_eq!(r[1].0, Event::Short { letter: b'd', arg: Some(Span { index: 2, start: 2, end: 3 }) });
        assert_eq!(r[2].0, Event::Invalid(b';'));
    }

    #[test]
    fn long_options() {
        let t = [opt("verbose", HasArg::No, 1), opt("file", HasArg::Required, 2), opt("fill", HasArg::Required, 3), opt("color", HasArg::Optional, 4), opt("colour", HasArg::Optional, 4)];
        let mut v = ["prog", "--verb", "--file", "x", "--col=red", "--fi", "--verbose=1", "--nope", "--file"];
        let r = run(&mut v, "ab", Some(&t), false);
        assert_eq!(r[0].0, Event::Long { entry: 0, arg: None });
        assert_eq!(r[1], (Event::Long { entry: 1, arg: Some(Span { index: 3, start: 0, end: 1 }) }, 4));
        assert_eq!(r[2].0, Event::Long { entry: 3, arg: Some(Span { index: 4, start: 6, end: 9 }) });
        assert!(matches!(r[3].0, Event::Ambiguous { strict: false, .. }));
        assert_eq!(r[4].0, Event::NoArgument { dash: Dash::Two, entry: 0 });
        assert!(matches!(r[5].0, Event::Unrecognized { dash: Dash::Two, .. }));
        assert_eq!(r[6].0, Event::MissingLong { dash: Dash::Two, entry: 1 });
        let mut names = Vec::new();
        candidates(&t[..], b"col", true, |e| names.push(e.name));
        assert_eq!(names, [b"color".as_slice(), b"colour"]);
        names.clear();
        candidates(&t[..], b"col", false, |e| names.push(e.name));
        assert_eq!(names, [b"color".as_slice()]);
        // long_only: one dash, a single option letter stays a letter, unknown text falls back.
        let mut v = ["prog", "-verbose", "-a", "-ab", "-col"];
        let r = run(&mut v, "ab", Some(&t), true);
        assert_eq!(r[0].0, Event::Long { entry: 0, arg: None });
        assert_eq!(r[1].0, Event::Short { letter: b'a', arg: None });
        assert_eq!(r[2].0, Event::Short { letter: b'a', arg: None });
        assert_eq!(r[3].0, Event::Short { letter: b'b', arg: None });
        assert!(matches!(r[4].0, Event::Ambiguous { dash: Dash::One, strict: true, .. }));
        // `-W name` with `W;`.
        let mut v = ["prog", "-W", "verbose", "-Wfile=x", "-W"];
        let r = run(&mut v, "W;", Some(&t), false);
        assert_eq!(r[0], (Event::Long { entry: 0, arg: None }, 3));
        assert_eq!(r[1].0, Event::Long { entry: 1, arg: Some(Span { index: 3, start: 7, end: 8 }) });
        assert_eq!(r[2].0, Event::MissingShort(b'W'));
    }

    #[test]
    fn suboptions() {
        let tokens: [&[u8]; 3] = [b"ro", b"rw", b"size"];
        let s = subopt(b"size=10,ro", tokens).unwrap();
        assert_eq!(s, Subopt { token: Some(2), name_end: 4, value: Some(5), end: 7, next: 8 });
        let s = subopt(b"bogus", tokens).unwrap();
        assert_eq!(s, Subopt { token: None, name_end: 5, value: None, end: 5, next: 5 });
        assert_eq!(subopt(b"", tokens), None);
    }
}
