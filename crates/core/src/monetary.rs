//! Monetary formatting (spec §7.5; D-7.11): `strfmon`'s format language over a locale's
//! `LC_MONETARY` conventions (§7.4). Rewritten for COLD-REWRITE2-cold-02 from the spec, D-7.11
//! and `spec/behaviour/strfmon.md`; the design choices are
//! D-7.15.
//!
//! **Format.** Text is copied; `%%` is a `%`. A conversion is `%`, flags, a field width, `#` and
//! a left precision, `.` and a right precision, an optional `L` (the amount is a `long double`)
//! and `n` (national) or `i` (international), in that order. Flags, in any order and
//! repeatable: `=f` (fill byte `f`, the last one wins), `^` (no grouping), `+` or `(` (sign
//! style; one of them, once), `!` (no currency symbol), `-` (left-justify). Anything else, a `#`
//! or `.` without digits, or the end of the format inside a conversion is `EINVAL`.
//!
//! **Number.** The signed amount is rounded to the right precision (default `frac_digits` or
//! `int_frac_digits`, 2 when unspecified) in the given rounding mode, by `format_float`. Its
//! integer digits are grouped by `mon_grouping` with `mon_thousands_sep` (unless `^`). A left
//! precision pads the integer part with the fill byte to the positions that many digits would
//! take, a separator counting as one position; infinities and NaNs (`inf`, `nan`) are padded
//! with spaces to the positions of the whole number, the decimal point one position.
//!
//! **Sign and symbol.** Negative means the sign bit is set (`-0.0` and `-nan` too). The parts
//! are the sign string (`negative_sign`, `-` if empty; `positive_sign`), the symbol
//! (`currency_symbol`, or the first three bytes of `int_curr_symbol`) and the number, ordered by
//! `cs_precedes` and `sign_posn`. `sep_by_space` 1 puts a space between the number and the
//! side the symbol is on; 2 a space between the symbol and the sign position next to it (an
//! empty sign still takes the position); the space is printed only with the symbol and only
//! between non-empty text, never leading or trailing. `(`, or `sign_posn` 0, drops the sign strings and puts a
//! negative amount and its symbol in parentheses. With a left precision, a form whose lead-in
//! (the text before the number) is shorter than the other sign's is padded in front.
//!
//! **Result.** A field width pads with spaces (left, or right with `-`); widths count bytes.
//! The result and its terminator must fit in the buffer, else `E2BIG`; errors are reported in
//! format order (output is produced as the format is read).

use crate::error::{Errno, Result};
use crate::locale::file::Cat;
use crate::locale::{Item, Locale};
use crate::mem::TryVec;
use crate::num::float::{Float, Format, Rounding};
use crate::num::format::{Case, Sink, Spec, Style, format_float};

/// Where the currency symbol and the sign go for one sign of amount (`struct lconv`'s
/// `cs_precedes`, `sep_by_space` and `sign_posn`; 0xff or 127: unspecified).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// Nonzero: the symbol precedes the number.
    pub cs_precedes: u8,
    /// 1: a space separates the symbol (with an adjacent sign) from the number; 2: a space
    /// separates the symbol from an adjacent sign; otherwise none.
    pub sep_by_space: u8,
    /// 0: parentheses; 1: sign first; 2: sign last; 3: sign just before the symbol; 4: sign just
    /// after it (other values: 1).
    pub sign_posn: u8,
}

/// A locale's `LC_MONETARY` conventions, as `strfmon` uses them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Conventions<'a> {
    /// `int_curr_symbol` (its fourth byte, the ISO 4217 separator, is not printed).
    pub int_curr_symbol: &'a [u8],
    /// `currency_symbol`.
    pub currency_symbol: &'a [u8],
    /// The decimal point: `mon_decimal_point`, else `LC_NUMERIC`'s, else `.`.
    pub decimal_point: &'a [u8],
    /// `mon_thousands_sep`.
    pub thousands_sep: &'a [u8],
    /// `mon_grouping`: group sizes from the right, the last repeating; 127 or more stops.
    pub grouping: &'a [u8],
    /// `positive_sign`.
    pub positive_sign: &'a [u8],
    /// `negative_sign`.
    pub negative_sign: &'a [u8],
    /// `int_frac_digits` (0xff or 127: unspecified).
    pub int_frac_digits: u8,
    /// `frac_digits` (0xff or 127: unspecified).
    pub frac_digits: u8,
    /// `p_*` and `n_*` placements.
    pub national: [Placement; 2],
    /// `int_p_*` and `int_n_*` placements.
    pub international: [Placement; 2],
}

/// `LC_MONETARY` item numbers (the order of `struct lconv`'s monetary members).
mod item {
    pub(super) const INT_CURR_SYMBOL: usize = 0;
    pub(super) const CURRENCY_SYMBOL: usize = 1;
    pub(super) const DECIMAL_POINT: usize = 2;
    pub(super) const THOUSANDS_SEP: usize = 3;
    pub(super) const GROUPING: usize = 4;
    pub(super) const POSITIVE_SIGN: usize = 5;
    pub(super) const NEGATIVE_SIGN: usize = 6;
    pub(super) const INT_FRAC_DIGITS: usize = 7;
    pub(super) const FRAC_DIGITS: usize = 8;
    /// `cs_precedes`, `sep_by_space`, `sign_posn` of a national positive amount.
    pub(super) const P: [usize; 3] = [9, 10, 13];
    /// The same, national negative.
    pub(super) const N: [usize; 3] = [11, 12, 14];
    /// The same, international positive.
    pub(super) const INT_P: [usize; 3] = [16, 17, 20];
    /// The same, international negative.
    pub(super) const INT_N: [usize; 3] = [18, 19, 21];
}

impl Conventions<'static> {
    /// The conventions of `l`'s `LC_MONETARY` (and its `LC_NUMERIC` decimal point).
    pub fn of(l: &Locale) -> Conventions<'static> {
        let s = |i| l.string(Cat::Monetary, i);
        let b = |i| match l.item(Cat::Monetary, i) {
            Item::Bytes(&[v, ..]) => v,
            _ => 0xff,
        };
        let p = |[cs, sep, posn]: [usize; 3]| Placement { cs_precedes: b(cs), sep_by_space: b(sep), sign_posn: b(posn) };
        let decimal_point = match (s(item::DECIMAL_POINT), l.string(Cat::Numeric, 0)) {
            (m, _) if !m.is_empty() => m,
            (_, n) if !n.is_empty() => n,
            _ => b".",
        };
        Conventions {
            int_curr_symbol: s(item::INT_CURR_SYMBOL),
            currency_symbol: s(item::CURRENCY_SYMBOL),
            decimal_point,
            thousands_sep: s(item::THOUSANDS_SEP),
            grouping: s(item::GROUPING),
            positive_sign: s(item::POSITIVE_SIGN),
            negative_sign: s(item::NEGATIVE_SIGN),
            int_frac_digits: b(item::INT_FRAC_DIGITS),
            frac_digits: b(item::FRAC_DIGITS),
            national: [p(item::P), p(item::N)],
            international: [p(item::INT_P), p(item::INT_N)],
        }
    }
}

/// Where the amounts come from: the next one, read as a `long double` or a `double`.
pub trait Amounts {
    /// The next amount.
    fn amount(&mut self, long_double: bool) -> Float;
}

/// One parsed conversion.
#[derive(Clone, Copy, Debug)]
struct Directive {
    fill: u8,
    group: bool,
    parens: bool,
    symbol: bool,
    left_justify: bool,
    width: usize,
    left_prec: Option<usize>,
    right_prec: Option<usize>,
    long_double: bool,
    international: bool,
}

/// Consume `b` if `rest` starts with it.
fn take(rest: &mut &[u8], b: u8) -> bool {
    match rest.split_first() {
        Some((&c, tail)) if c == b => {
            *rest = tail;
            true
        }
        _ => false,
    }
}

/// The digits `rest` starts with, consumed, as a saturating number; `None` without a digit.
fn digits(rest: &mut &[u8]) -> Option<usize> {
    let mut n = None;
    while let Some((&c, tail)) = rest.split_first()
        && c.is_ascii_digit()
    {
        n = Some(n.unwrap_or(0usize).saturating_mul(10).saturating_add(usize::from(c.wrapping_sub(b'0'))));
        *rest = tail;
    }
    n
}

/// The conversion `rest` starts with (just after its `%`; `None`: `%%`), consumed.
fn directive(rest: &mut &[u8]) -> Result<Option<Directive>> {
    if take(rest, b'%') {
        return Ok(None);
    }
    let mut d = Directive {
        fill: b' ',
        group: true,
        parens: false,
        symbol: true,
        left_justify: false,
        width: 0,
        left_prec: None,
        right_prec: None,
        long_double: false,
        international: false,
    };
    let mut sign_style = false;
    loop {
        let (&c, mut tail) = rest.split_first().ok_or(Errno::EINVAL)?;
        match c {
            b'=' => {
                let (&f, after) = tail.split_first().ok_or(Errno::EINVAL)?;
                d.fill = f;
                tail = after;
            }
            b'^' => d.group = false,
            s @ (b'+' | b'(') => {
                if sign_style {
                    return Err(Errno::EINVAL);
                }
                sign_style = true;
                d.parens = s == b'(';
            }
            b'!' => d.symbol = false,
            b'-' => d.left_justify = true,
            _ => break,
        }
        *rest = tail;
    }
    d.width = digits(rest).unwrap_or(0);
    if take(rest, b'#') {
        d.left_prec = Some(digits(rest).ok_or(Errno::EINVAL)?);
    }
    if take(rest, b'.') {
        d.right_prec = Some(digits(rest).ok_or(Errno::EINVAL)?);
    }
    d.long_double = take(rest, b'L');
    d.international = match rest.split_first() {
        Some((b'n', tail)) => {
            *rest = tail;
            false
        }
        Some((b'i', tail)) => {
            *rest = tail;
            true
        }
        _ => return Err(Errno::EINVAL),
    };
    Ok(Some(d))
}

/// The caller's buffer; the terminator is reserved at the end.
struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Out<'_> {
    fn room(&self) -> usize {
        self.buf.len().saturating_sub(self.len)
    }

    /// The next `n` bytes of the buffer, now counted as written; `E2BIG` if they do not fit.
    fn claim(&mut self, n: usize) -> Result<&mut [u8]> {
        let end = self.len.checked_add(n).ok_or(Errno::E2BIG)?;
        let dst = self.buf.get_mut(self.len..end).ok_or(Errno::E2BIG)?;
        self.len = end;
        Ok(dst)
    }

    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        self.claim(bytes.len())?.copy_from_slice(bytes);
        Ok(())
    }

    fn pad(&mut self, b: u8, n: usize) -> Result<()> {
        self.claim(n)?.fill(b);
        Ok(())
    }
}

/// A [`Sink`] collecting at most `cap` bytes; beyond that (or out of memory) it only records
/// the failure, so that a huge precision costs no work.
struct Collect {
    text: TryVec<u8>,
    cap: usize,
    failed: Option<Errno>,
}

impl Collect {
    fn reserve(&mut self, n: usize) -> bool {
        if self.failed.is_some() {
            return false;
        }
        if self.text.len().checked_add(n).is_none_or(|end| end > self.cap) {
            self.failed = Some(Errno::E2BIG);
            return false;
        }
        true
    }
}

impl Sink for Collect {
    fn put(&mut self, bytes: &[u8]) {
        if self.reserve(bytes.len())
            && let Err(e) = self.text.extend_from_slice(bytes)
        {
            self.failed = Some(e);
        }
    }

    fn repeat(&mut self, b: u8, n: usize) {
        if self.reserve(n) {
            let len = self.text.len();
            if let Err(e) = self.text.resize(len.saturating_add(n), b) {
                self.failed = Some(e);
            }
        }
    }
}

/// The digit counts (from the right) at which separators go in an integer of `n` digits, in
/// increasing order. `grouping` gives group sizes from the right; the last repeats, and a size
/// of 0 (or the end) repeats the previous one, 127 or more ends grouping.
fn separators(grouping: &[u8], n: usize, mut each: impl FnMut(usize)) {
    let mut at = 0usize;
    let mut size = 0usize;
    let mut sizes = grouping.iter();
    loop {
        match sizes.next() {
            Some(&g) if g >= 127 => return,
            Some(&g) if g > 0 => size = usize::from(g),
            _ if size == 0 => return,
            _ => {}
        }
        at = at.saturating_add(size);
        if at >= n {
            return;
        }
        each(at);
    }
}

fn separator_count(grouping: &[u8], n: usize) -> usize {
    let mut k = 0usize;
    separators(grouping, n, |_| k = k.saturating_add(1));
    k
}

/// The text before and after the number: (lead-in, trailer).
fn frame(conv: &Conventions<'_>, d: &Directive, neg: bool) -> Result<(TryVec<u8>, TryVec<u8>)> {
    let [positive, negative] = if d.international { conv.international } else { conv.national };
    let placement = if neg { negative } else { positive };
    let symbol = match (d.symbol, d.international) {
        (false, _) => &b""[..],
        (true, true) => conv.int_curr_symbol.get(..3).unwrap_or(conv.int_curr_symbol),
        (true, false) => conv.currency_symbol,
    };
    let parens = d.parens || placement.sign_posn == 0;
    let sign = match (parens, neg) {
        (true, _) => &b""[..],
        (false, true) if conv.negative_sign.is_empty() => b"-",
        (false, true) => conv.negative_sign,
        (false, false) => conv.positive_sign,
    };
    #[derive(Clone, Copy, PartialEq)]
    enum Part {
        Sign,
        Symbol,
        Number,
    }
    use Part::{Number as V, Sign as S, Symbol as C};
    let order: &[Part] = match (placement.cs_precedes != 0, if parens { 0 } else { placement.sign_posn }) {
        (true, 0) => &[C, V],
        (true, 2) => &[C, V, S],
        (true, 4) => &[C, S, V],
        (true, _) => &[S, C, V],
        (false, 0) => &[V, C],
        (false, 2 | 4) => &[V, C, S],
        (false, 3) => &[V, S, C],
        (false, _) => &[S, V, C],
    };
    let text = |p: Part| match p {
        S => sign,
        C => symbol,
        V => &b""[..],
    };
    let symbol_first = order.iter().position(|&p| p == C) < order.iter().position(|&p| p == V);
    let mut lead = TryVec::new();
    let mut trail = TryVec::new();
    if parens && neg {
        lead.push(b'(')?;
    }
    let mut seen_number = false;
    let mut prev = None;
    for (i, &p) in order.iter().enumerate() {
        if let Some(q) = prev.replace(p) {
            // The space belongs to the symbol and to the boundary, not to the sign: 1 puts it
            // between the number and the symbol's side, 2 between a symbol and the sign position
            // next to it (an empty sign included, so `JPY 1,235`). It is printed only with the
            // symbol and only with text on both sides (never leading or trailing; D-7.11).
            let wanted = match placement.sep_by_space {
                1 => if symbol_first { p == V } else { q == V },
                2 => matches!((q, p), (S, C) | (C, S)),
                _ => false,
            };
            let before = seen_number || !lead.is_empty();
            let after = order.get(i..).unwrap_or_default().iter().any(|&r| r == V || !text(r).is_empty());
            let space = wanted && !symbol.is_empty() && before && after;
            if space {
                if seen_number { trail.push(b' ')? } else { lead.push(b' ')? }
            }
        }
        if p == V {
            seen_number = true;
        } else if seen_number {
            trail.extend_from_slice(text(p))?;
        } else {
            lead.extend_from_slice(text(p))?;
        }
    }
    if parens && neg {
        trail.push(b')')?;
    }
    Ok((lead, trail))
}

/// The number of `x`: integer digits filled to the left precision and grouped, then the
/// fraction; at most `room` bytes, else `E2BIG`.
fn number(conv: &Conventions<'_>, d: &Directive, x: Float, fmt: Format, mode: Rounding, room: usize) -> Result<TryVec<u8>> {
    let frac = if d.international { conv.int_frac_digits } else { conv.frac_digits };
    let prec = d.right_prec.unwrap_or(if frac >= 127 { 2 } else { usize::from(frac) });
    if prec > room {
        return Err(Errno::E2BIG);
    }
    // Formatted signed (so that directed rounding sees the sign), the sign then dropped.
    let mut raw = Collect { text: TryVec::new(), cap: room.saturating_add(1), failed: None };
    let spec = Spec { style: Style::Fixed, case: Case::Lower, precision: Some(prec), alternate: false };
    format_float(&mut raw, x, fmt, spec, conv.decimal_point, mode);
    if let Some(e) = raw.failed {
        return Err(e);
    }
    let text = raw.text.as_slice();
    let text = if x.neg { text.get(1..).unwrap_or(b"") } else { text };
    // Separator positions come from mon_grouping, one position each whatever the separator text (even
    // empty: ruling #336, matches the reference's `#` padding).
    let grouping = if d.group { conv.grouping } else { &[][..] };
    let mut out = Collect { text: TryVec::new(), cap: room, failed: None };
    if !text.first().is_some_and(u8::is_ascii_digit) {
        // inf or nan: spaces to the whole number's positions.
        if let Some(lp) = d.left_prec {
            let fraction = if prec > 0 { prec.saturating_add(1) } else { 0 };
            let positions = lp.saturating_add(separator_count(grouping, lp)).saturating_add(fraction);
            out.repeat(b' ', positions.saturating_sub(text.len()));
        }
        out.put(text);
    } else {
        let n = text.iter().take_while(|c| c.is_ascii_digit()).count();
        // The printed separators take their own width: one position each, none when empty (the
        // reference's padding under xx_NOSEP), while the lp positions above reserve one each.
        let seps = if conv.thousands_sep.is_empty() { 0 } else { separator_count(grouping, n) };
        if let Some(lp) = d.left_prec {
            let positions = lp.saturating_add(separator_count(grouping, lp));
            out.repeat(d.fill, positions.saturating_sub(n.saturating_add(seps)));
        }
        let mut cuts = TryVec::new();
        let mut oom = Ok(());
        separators(grouping, n, |k| {
            if oom.is_ok() {
                oom = cuts.push(n.saturating_sub(k));
            }
        });
        oom?;
        let mut from = 0;
        for &cut in cuts.as_slice().iter().rev() {
            out.put(text.get(from..cut).unwrap_or_default());
            out.put(conv.thousands_sep);
            from = cut;
        }
        out.put(text.get(from..).unwrap_or_default());
    }
    match out.failed {
        Some(e) => Err(e),
        None => Ok(out.text),
    }
}

/// One conversion of the next amount into `out`.
fn convert(out: &mut Out<'_>, conv: &Conventions<'_>, d: &Directive, amounts: &mut dyn Amounts, mode: Rounding) -> Result<()> {
    let fmt = if d.long_double { Format::F80 } else { Format::F64 };
    let x = amounts.amount(d.long_double);
    let (lead, trail) = frame(conv, d, x.neg)?;
    let align = match d.left_prec {
        Some(_) => frame(conv, d, !x.neg)?.0.len().saturating_sub(lead.len()),
        None => 0,
    };
    let body = number(conv, d, x, fmt, mode, out.room())?;
    let len = align.saturating_add(lead.len()).saturating_add(body.len()).saturating_add(trail.len());
    let pad = d.width.saturating_sub(len);
    if !d.left_justify {
        out.pad(b' ', pad)?;
    }
    out.pad(b' ', align)?;
    out.put(lead.as_slice())?;
    out.put(body.as_slice())?;
    out.put(trail.as_slice())?;
    if d.left_justify {
        out.pad(b' ', pad)?;
    }
    Ok(())
}

/// `strfmon`: format the amounts `fmt` names into `buf` (its whole length is the limit) under
/// `conv`, rounding in `mode`; the length written, the terminator excluded.
pub fn format(buf: &mut [u8], fmt: &[u8], conv: &Conventions<'_>, amounts: &mut dyn Amounts, mode: Rounding) -> Result<usize> {
    let mut out = Out { buf, len: 0 };
    let mut rest = fmt;
    while !rest.is_empty() {
        let k = rest.iter().position(|&c| c == b'%').unwrap_or(rest.len());
        let (text, from_percent) = rest.split_at_checked(k).unwrap_or((rest, b""));
        out.put(text)?;
        // Past the `%`; `None`: no `%` was left.
        let Some(after) = from_percent.get(1..) else { break };
        rest = after;
        match directive(&mut rest)? {
            None => out.put(b"%")?,
            Some(d) => convert(&mut out, conv, &d, amounts, mode)?,
        }
    }
    let n = out.len;
    out.put(&[0])?;
    Ok(n)
}
