//! Byte arrays and text: copy, fill, compare, search, span, tokenize (spec §4).
//!
//! Every function works on length-delimited slices of a code unit `T` (`u8` for bytes, `i32`
//! for `wchar_t`, decision D-4.1): the C delimiting conventions (count, terminator, both,
//! capacity) are the boundary's input conversions, not separate algorithms here. Searches
//! return indices, never pointers (D-4.10). Nothing here panics or allocates.

use core::cmp::Ordering;

/// A code unit: a byte or a wide character. `ZERO` is the terminator value.
pub trait Unit: Copy + Eq + Ord + 'static {
    /// The terminator (0).
    const ZERO: Self;
    /// The unit as a byte, if it fits in one (for byte-wise case folding).
    fn as_byte(self) -> Option<u8>;
    /// A byte as a unit.
    fn from_byte(b: u8) -> Self;
    /// Copy into the start of `dst` (`liella_mem`, D-4.11); the units copied.
    fn copy_units(dst: &mut [Self], src: &[Self]) -> usize;
    /// As `copy_units`, whole blocks in descending address order (D-4.14); the units copied.
    fn copy_units_back(dst: &mut [Self], src: &[Self]) -> usize;
    /// Overlap-safe move within one buffer (`liella_mem`); `false` if out of range.
    fn move_units(buf: &mut [Self], src: usize, dst: usize, len: usize) -> bool;
    /// Fill (`liella_mem`).
    fn fill_units(dst: &mut [Self], v: Self);
    /// Lexicographic order (`liella_mem`).
    fn compare_units(a: &[Self], b: &[Self]) -> Ordering;
    /// The first index of `x` (`memchr`, `wmemchr`).
    fn find_unit(s: &[Self], x: Self) -> Option<usize> {
        s.iter().position(|&c| c == x)
    }
}

impl Unit for u8 {
    const ZERO: u8 = 0;
    fn as_byte(self) -> Option<u8> {
        Some(self)
    }
    fn from_byte(b: u8) -> u8 {
        b
    }
    fn copy_units(dst: &mut [u8], src: &[u8]) -> usize {
        liella_mem::copy_u8(dst, src)
    }
    fn copy_units_back(dst: &mut [u8], src: &[u8]) -> usize {
        liella_mem::copy_back_u8(dst, src)
    }
    fn move_units(buf: &mut [u8], src: usize, dst: usize, len: usize) -> bool {
        liella_mem::move_u8(buf, src, dst, len)
    }
    fn fill_units(dst: &mut [u8], v: u8) {
        liella_mem::fill_u8(dst, v);
    }
    fn compare_units(a: &[u8], b: &[u8]) -> Ordering {
        liella_mem::compare_u8(a, b)
    }
    fn find_unit(s: &[u8], x: u8) -> Option<usize> {
        liella_mem::find_u8(s, x)
    }
}

/// `wchar_t` units compare as signed `int`, the type of `wchar_t` on x86_64 (§4.1 "the same
/// contracts over `wchar_t` units").
impl Unit for i32 {
    const ZERO: i32 = 0;
    fn as_byte(self) -> Option<u8> {
        u8::try_from(self).ok()
    }
    fn from_byte(b: u8) -> i32 {
        i32::from(b)
    }
    fn copy_units(dst: &mut [i32], src: &[i32]) -> usize {
        liella_mem::copy_i32(dst, src)
    }
    fn copy_units_back(dst: &mut [i32], src: &[i32]) -> usize {
        liella_mem::copy_back_i32(dst, src)
    }
    fn move_units(buf: &mut [i32], src: usize, dst: usize, len: usize) -> bool {
        liella_mem::move_i32(buf, src, dst, len)
    }
    fn fill_units(dst: &mut [i32], v: i32) {
        liella_mem::fill_i32(dst, v);
    }
    fn compare_units(a: &[i32], b: &[i32]) -> Ordering {
        liella_mem::compare_i32(a, b)
    }
}

// ------------------------------------------------------------------------------ copy and fill

/// Copy `src` into the start of `dst`; returns the units copied (`min` of the lengths).
/// Like every primitive the `mem*` exports reach, it never lowers to the C runtime symbols
/// (D-4.11).
pub fn copy<T: Unit>(dst: &mut [T], src: &[T]) -> usize {
    T::copy_units(dst, src)
}

/// [`copy`] with the whole blocks in descending address order (the boundary's choice for a
/// destination a short distance above the source modulo 4 KiB, D-4.14).
pub fn copy_back<T: Unit>(dst: &mut [T], src: &[T]) -> usize {
    T::copy_units_back(dst, src)
}

/// Move `len` units from offset `src` to offset `dst` within one buffer, correct under
/// overlap (`memmove`; also `memcpy` per D-4.2). Out-of-range requests copy nothing and
/// return `false`.
pub fn move_within<T: Unit>(buf: &mut [T], src: usize, dst: usize, len: usize) -> bool {
    T::move_units(buf, src, dst, len)
}

/// The source and destination of a move that may overlap, as the boundary can express them:
/// two disjoint views, or one buffer spanning both (Rust cannot hold an overlapping `&mut` and
/// `&` pair).
pub enum Move<'a, T> {
    /// Disjoint destination and source of equal length.
    Disjoint(&'a mut [T], &'a [T]),
    /// One buffer: units move from offset `.1` to offset `.2`, `buffer.len() - max(.1, .2)` of
    /// them.
    Within(&'a mut [T], usize, usize),
}

/// Perform a move (`memmove`, `memcpy` per D-4.2); returns the units moved.
pub fn apply_move<T: Unit>(m: Move<'_, T>) -> usize {
    match m {
        Move::Disjoint(d, s) => copy(d, s),
        Move::Within(buf, src, dst) => {
            let len = buf.len().saturating_sub(src.max(dst));
            if move_within(buf, src, dst, len) { len } else { 0 }
        }
    }
}

/// Fill `dst` with `v`.
pub fn fill<T: Unit>(dst: &mut [T], v: T) {
    T::fill_units(dst, v);
}

/// `memccpy`: copy from `src` up to and including the first `stop`; returns the number of
/// units copied if `stop` was found (the C result points just after it), `None` otherwise
/// (then all of `src` was copied).
pub fn copy_until<T: Unit>(dst: &mut [T], src: &[T], stop: T) -> Option<usize> {
    let found = find(src, stop).map(|i| i.saturating_add(1));
    let n = found.unwrap_or(src.len());
    copy(dst, src.get(..n).unwrap_or(src));
    found
}

/// Result of a bounded, terminated copy (`strlcpy`, `strlcat`, the internal default).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Copied {
    /// Units copied, excluding the terminator.
    pub len: usize,
    /// True if the source did not fit.
    pub truncated: bool,
}

/// Copy `src` into `dst` (capacity `dst.len()`), always terminating if `dst` is non-empty.
pub fn copy_truncating<T: Unit>(dst: &mut [T], src: &[T]) -> Copied {
    let Some(room) = dst.len().checked_sub(1) else {
        return Copied { len: 0, truncated: !src.is_empty() };
    };
    let n = copy(dst.get_mut(..room).unwrap_or(&mut []), src);
    if let Some(t) = dst.get_mut(n) {
        *t = T::ZERO;
    }
    Copied { len: n, truncated: n < src.len() }
}

/// `strncpy` semantics: copy at most `dst.len()` units of `src`, then zero-pad the rest of
/// `dst`. If `src` fills `dst`, nothing is terminated (⚑ HIST). Returns the units copied
/// from `src` (the `stpncpy` offset).
pub fn copy_padded<T: Unit>(dst: &mut [T], src: &[T]) -> usize {
    let n = copy(dst, src);
    if let Some(rest) = dst.get_mut(n..) {
        rest.fill(T::ZERO);
    }
    n
}

/// Swap adjacent byte pairs from `src` into `dst` (`swab`); an odd final byte is dropped.
pub fn swap_pairs(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(2).zip(src.chunks_exact(2)) {
        if let ([d0, d1], [s0, s1]) = (d, s) {
            *d0 = *s1;
            *d1 = *s0;
        }
    }
}

/// Zero `buf` so that the stores are not elided (`explicit_bzero`): the buffer is then passed
/// to `barrier` (the machine's compiler barrier, which the optimiser cannot see through).
pub fn wipe(buf: &mut [u8], barrier: impl FnOnce(&[u8])) {
    buf.fill(0);
    barrier(buf);
}

/// XOR every byte with 42 (`memfrob`, ⚑ HIST).
pub fn frob(buf: &mut [u8]) {
    for b in buf {
        *b ^= 42;
    }
}

/// Fisher–Yates shuffle (`strfry`); `next(bound)` returns a uniform value in `[0, bound)`.
pub fn shuffle<T>(buf: &mut [T], mut next: impl FnMut(u32) -> u32) {
    let mut i = buf.len();
    while i > 1 {
        let j = usize::try_from(next(u32::try_from(i).unwrap_or(u32::MAX))).unwrap_or(0);
        i = i.saturating_sub(1);
        buf.swap(i, j.min(i));
    }
}

// ------------------------------------------------------------------------------ compare

/// `memcmp`/`wmemcmp`: lexicographic order of two equal-length sequences.
pub fn compare<T: Unit>(a: &[T], b: &[T]) -> Ordering {
    T::compare_units(a, b)
}

/// `strcmp`/`wcscmp` order of two texts, as if each were followed by its terminator (so the
/// terminator takes part in the comparison, which matters for negative `wchar_t` values).
pub fn compare_text<T: Unit>(a: &[T], b: &[T]) -> Ordering {
    compare_text_by(a, b, |x| x)
}

/// [`compare_text`] after mapping each unit through `fold` (`strcasecmp` with `tolower`).
pub fn compare_text_by<T: Unit>(a: &[T], b: &[T], fold: impl Fn(T) -> T) -> Ordering {
    let end = a.len().max(b.len());
    let mut i = 0usize;
    while i <= end {
        let x = fold(a.get(i).copied().unwrap_or(T::ZERO));
        let y = fold(b.get(i).copied().unwrap_or(T::ZERO));
        if x != y {
            return x.cmp(&y);
        }
        i = i.saturating_add(1);
    }
    Ordering::Equal
}

/// [`compare_text_by`] on bytes, for a `fold` that maps every ASCII byte (0…127) as
/// [`u8::to_ascii_lowercase`] does (the caller checks: `ctype::ascii_case`, PJ-23). Eight bytes
/// at a time: equal words are skipped, and words of ASCII bytes are folded with arithmetic; a
/// word that differs and holds a byte of 128 or more goes through `fold` byte by byte, as does
/// the tail. The answer is [`compare_text_by`]'s.
pub fn compare_text_ascii_folded(a: &[u8], b: &[u8], fold: impl Fn(u8) -> u8) -> Ordering {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let mut i = 0usize;
    for (x, y) in a.chunks_exact(8).zip(b.chunks_exact(8)) {
        let (Ok(xb), Ok(yb)) = (<[u8; 8]>::try_from(x), <[u8; 8]>::try_from(y)) else { break };
        let (xw, yw) = (u64::from_le_bytes(xb), u64::from_le_bytes(yb));
        if xw != yw {
            if (xw | yw) & HIGH == 0 {
                let (fx, fy) = (ascii_lower_word(xw), ascii_lower_word(yw));
                if fx != fy {
                    // The first differing byte is the lowest one (little-endian load).
                    let k = usize::try_from((fx ^ fy).trailing_zeros() >> 3).unwrap_or(0);
                    return fx.to_le_bytes().get(k).cmp(&fy.to_le_bytes().get(k));
                }
            } else {
                for (&p, &q) in xb.iter().zip(&yb) {
                    let (p, q) = (fold(p), fold(q));
                    if p != q {
                        return p.cmp(&q);
                    }
                }
            }
        }
        i = i.wrapping_add(8);
    }
    compare_text_by(a.get(i..).unwrap_or_default(), b.get(i..).unwrap_or_default(), fold)
}

/// Eight ASCII bytes (each below 128) with `A`…`Z` lowered: adding 0x3f sets a byte's top bit
/// from `A` (0x41) up, adding 0x25 from `[` (0x5b) up; neither sum carries into the next byte.
const fn ascii_lower_word(w: u64) -> u64 {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let from_a = w.wrapping_add(0x3f3f_3f3f_3f3f_3f3f);
    let past_z = w.wrapping_add(0x2525_2525_2525_2525);
    w | (((from_a ^ past_z) & HIGH) >> 2)
}

/// `strverscmp`: like [`compare_text`], except that runs of decimal digits compare by value.
/// A run with a leading zero is a *fractional* part: it sorts before any run without one
/// ("000" < "00" < "01" < "010" < "09" < "0" < "1" < "9" < "10": the GNU C Library manual and
/// strverscmp(3)). Of two fractional runs, the one with more leading zeros is smaller; with as
/// many, a run that is only zeros is larger than one with more digits, and otherwise the digits
/// after the zeros compare one by one. When those of one run are a prefix of the other's, the
/// comparison goes on with the characters after the shorter run, as text ("01" < "010" <
/// "01a"). Matches glibc 2.39 on every pair of strings of up to 4 characters over `012a.`
/// (black-box probe, validation T18).
pub fn compare_versions(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(j).copied().unwrap_or(0);
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let (o, na, nb) = compare_digit_runs(digit_run(a, i), digit_run(b, j));
            if o != Ordering::Equal {
                return o;
            }
            i = i.saturating_add(na);
            j = j.saturating_add(nb);
            continue;
        }
        if x != y {
            return x.cmp(&y);
        }
        if x == 0 {
            return Ordering::Equal;
        }
        i = i.saturating_add(1);
        j = j.saturating_add(1);
    }
}

fn digit_run(s: &[u8], from: usize) -> &[u8] {
    let rest = s.get(from..).unwrap_or(&[]);
    let n = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    rest.get(..n).unwrap_or(&[])
}

/// The order of two digit runs, or `Equal` and how many bytes of each to step over before the
/// comparison goes on.
fn compare_digit_runs(a: &[u8], b: &[u8]) -> (Ordering, usize, usize) {
    let fa = a.first() == Some(&b'0');
    let fb = b.first() == Some(&b'0');
    match (fa, fb) {
        // Integral parts: longer is larger, then lexical.
        (false, false) => (a.len().cmp(&b.len()).then_with(|| a.cmp(b)), a.len(), b.len()),
        // A fractional part sorts before an integral one.
        (true, false) => (Ordering::Less, 0, 0),
        (false, true) => (Ordering::Greater, 0, 0),
        (true, true) => {
            let za = a.iter().take_while(|&&c| c == b'0').count();
            let zb = b.iter().take_while(|&&c| c == b'0').count();
            if za != zb {
                return (zb.cmp(&za), 0, 0);
            }
            let (ra, rb) = (a.get(za..).unwrap_or(&[]), b.get(zb..).unwrap_or(&[]));
            match (ra.is_empty(), rb.is_empty()) {
                (true, true) => (Ordering::Equal, a.len(), b.len()),
                (true, false) => (Ordering::Greater, 0, 0),
                (false, true) => (Ordering::Less, 0, 0),
                (false, false) => match ra.iter().zip(rb).find(|(x, y)| x != y) {
                    Some((x, y)) => (x.cmp(y), 0, 0),
                    // One is a prefix of the other: the text after the shorter run decides.
                    None => {
                        let k = za.saturating_add(ra.len().min(rb.len()));
                        (Ordering::Equal, k, k)
                    }
                },
            }
        }
    }
}

// ------------------------------------------------------------------------------ search

/// First index of `x` (`memchr`, `strchr` over the text plus terminator).
pub fn find<T: Unit>(s: &[T], x: T) -> Option<usize> {
    T::find_unit(s, x)
}

/// Last index of `x` (`memrchr`, `strrchr`).
pub fn rfind<T: Unit>(s: &[T], x: T) -> Option<usize> {
    s.iter().rposition(|&c| c == x)
}

/// First index in `s` of any unit for which `member` holds (`strpbrk`).
pub fn find_any<T: Unit>(s: &[T], member: impl Fn(T) -> bool) -> Option<usize> {
    s.iter().position(|&c| member(c))
}

/// `strchr` on a terminated text: the terminator itself is searchable, so searching for zero
/// yields the length.
pub fn find_z<T: Unit>(s: &[T], x: T) -> Option<usize> {
    if x == T::ZERO { Some(s.len()) } else { find(s, x) }
}

/// `strrchr`: as [`find_z`], the last occurrence.
pub fn rfind_z<T: Unit>(s: &[T], x: T) -> Option<usize> {
    if x == T::ZERO { Some(s.len()) } else { rfind(s, x) }
}

/// `strchrnul`: the first occurrence or the end.
pub fn find_z_or_end<T: Unit>(s: &[T], x: T) -> usize {
    find(s, x).unwrap_or(s.len())
}

/// Membership in a wide set given as text (`wcsspn`, `wcstok`); zero is never a member.
pub fn member_of<T: Unit>(set: &[T]) -> impl Fn(T) -> bool + '_ {
    move |c| c != T::ZERO && set.contains(&c)
}

/// Non-membership in a wide set (`wcscspn`).
pub fn not_member_of<T: Unit>(set: &[T]) -> impl Fn(T) -> bool + '_ {
    move |c| !set.contains(&c)
}

/// Length of the prefix of `s` whose units all satisfy `member` (`strspn`; `strcspn` with
/// the negated predicate).
pub fn span<T: Unit>(s: &[T], member: impl Fn(T) -> bool) -> usize {
    s.iter().take_while(|&&c| member(c)).count()
}

/// A set of bytes for the span functions (the terminator is never a member, ⚑ HIST).
#[derive(Clone, Copy)]
pub struct ByteSet([u64; 4]);

impl ByteSet {
    /// The set of the bytes of `set`.
    pub fn new(set: &[u8]) -> ByteSet {
        let mut bits = [0u64; 4];
        for &b in set {
            if b != 0 {
                if let Some(w) = bits.get_mut(usize::from(b >> 6)) {
                    *w |= 1u64 << (b & 63);
                }
            }
        }
        ByteSet(bits)
    }

    /// Membership.
    pub fn contains(&self, b: u8) -> bool {
        self.0.get(usize::from(b >> 6)).is_some_and(|w| w & (1u64 << (b & 63)) != 0)
    }

    /// The membership predicate (`strspn`, `strpbrk`, the tokenizers).
    pub fn predicate(self) -> impl Fn(u8) -> bool {
        move |b| self.contains(b)
    }

    /// The non-membership predicate (`strcspn`).
    pub fn absent(self) -> impl Fn(u8) -> bool {
        move |b| !self.contains(b)
    }
}

/// First occurrence of `needle` in `hay` (`memmem`, `strstr`, `wcsstr`), comparing units
/// after `fold` (identity, or `tolower` for `strcasestr`). Worst-case linear time
/// (Crochemore–Perrin two-way matching). An empty needle matches at 0.
pub fn find_seq<T: Unit>(hay: &[T], needle: &[T], fold: impl Fn(T) -> T) -> Option<usize> {
    let m = needle.len();
    if m == 0 {
        return Some(0);
    }
    let last = hay.len().checked_sub(m)?;
    let n_at = |i: usize| needle.get(i).map(|&c| fold(c));
    let h_at = |i: usize| hay.get(i).map(|&c| fold(c));
    // Critical factorization: the larger of the two maximal suffixes.
    let (l1, p1) = maximal_suffix(m, &n_at, false);
    let (l2, p2) = maximal_suffix(m, &n_at, true);
    let (ell, per) = if l1 >= l2 { (l1, p1) } else { (l2, p2) };
    // Is the needle periodic with period `per` (needle[..ell] == needle[per..per+ell])?
    let periodic = per.checked_add(ell).is_some_and(|e| e <= m) && (0..ell).all(|i| n_at(i) == n_at(i.wrapping_add(per)));
    let mut pos = 0usize;
    if periodic {
        let mut memory = 0usize;
        while pos <= last {
            let mut i = ell.max(memory);
            while i < m && n_at(i) == h_at(pos.wrapping_add(i)) {
                i = i.saturating_add(1);
            }
            if i >= m {
                let mut j = ell;
                while j > memory && n_at(j.saturating_sub(1)) == h_at(pos.wrapping_add(j.saturating_sub(1))) {
                    j = j.saturating_sub(1);
                }
                if j <= memory {
                    return Some(pos);
                }
                pos = pos.saturating_add(per);
                memory = m.saturating_sub(per);
            } else {
                pos = pos.saturating_add(i.saturating_sub(ell).saturating_add(1));
                memory = 0;
            }
        }
    } else {
        let shift = ell.max(m.saturating_sub(ell)).saturating_add(1);
        while pos <= last {
            let mut i = ell;
            while i < m && n_at(i) == h_at(pos.wrapping_add(i)) {
                i = i.saturating_add(1);
            }
            if i >= m {
                let mut j = ell;
                while j > 0 && n_at(j.saturating_sub(1)) == h_at(pos.wrapping_add(j.saturating_sub(1))) {
                    j = j.saturating_sub(1);
                }
                if j == 0 {
                    return Some(pos);
                }
                pos = pos.saturating_add(shift);
            } else {
                pos = pos.saturating_add(i.saturating_sub(ell).saturating_add(1));
            }
        }
    }
    None
}

/// Maximal suffix of the needle under the unit order (or its reverse): returns (start, period).
fn maximal_suffix<T: Ord>(m: usize, at: &impl Fn(usize) -> Option<T>, reverse: bool) -> (usize, usize) {
    let mut ms = 0usize;
    let mut j = 1usize;
    let mut k = 1usize;
    let mut p = 1usize;
    while j.saturating_add(k) <= m {
        let a = at(j.saturating_add(k).saturating_sub(1));
        let b = at(ms.saturating_add(k).saturating_sub(1));
        let ord = if reverse { b.cmp(&a) } else { a.cmp(&b) };
        match ord {
            Ordering::Less => {
                j = j.saturating_add(k);
                k = 1;
                p = j.saturating_sub(ms);
            }
            Ordering::Equal => {
                if k == p {
                    j = j.saturating_add(p);
                    k = 1;
                } else {
                    k = k.saturating_add(1);
                }
            }
            Ordering::Greater => {
                ms = j;
                j = ms.saturating_add(1);
                k = 1;
                p = 1;
            }
        }
    }
    (ms, p)
}

// ------------------------------------------------------------------------------ tokens

/// Non-destructive tokenizer (D-4.5): yields `(start, len)` of the tokens of `s` separated by
/// units for which `is_delim` holds. With `skip_empty` (the `strtok` contract) runs of
/// delimiters separate one token and empty tokens are never produced; without it (`strsep`)
/// every delimiter ends a token, so empty tokens appear.
pub struct Tokens<'a, T, F> {
    s: &'a [T],
    pos: usize,
    is_delim: F,
    skip_empty: bool,
    done: bool,
}

impl<'a, T: Unit, F: Fn(T) -> bool> Tokens<'a, T, F> {
    /// Tokens of `s`.
    pub fn new(s: &'a [T], is_delim: F, skip_empty: bool) -> Self {
        Tokens { s, pos: 0, is_delim, skip_empty, done: false }
    }
}

impl<T: Unit, F: Fn(T) -> bool> Iterator for Tokens<'_, T, F> {
    type Item = (usize, usize);
    fn next(&mut self) -> Option<(usize, usize)> {
        if self.done {
            return None;
        }
        if self.skip_empty {
            let rest = self.s.get(self.pos..).unwrap_or(&[]);
            self.pos = self.pos.saturating_add(span(rest, &self.is_delim));
            if self.pos >= self.s.len() {
                self.done = true;
                return None;
            }
        }
        let rest = self.s.get(self.pos..).unwrap_or(&[]);
        let len = span(rest, |c| !(self.is_delim)(c));
        let start = self.pos;
        let end = start.saturating_add(len);
        if end >= self.s.len() {
            self.done = true;
        }
        self.pos = end.saturating_add(1);
        Some((start, len))
    }
}

// ------------------------------------------------------------------------------ paths

/// The GNU `basename`: index of the part after the last `/` (empty for `"a/"`); never
/// modifies.
pub fn basename_gnu(path: &[u8]) -> usize {
    rfind(path, b'/').map_or(0, |i| i.saturating_add(1))
}

/// What a mutating path function produced (XPG `basename`, `dirname`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathPart {
    /// The result starts at `start` in the input; a terminator was written at `end` (if `end`
    /// is inside the input; otherwise the existing terminator ends it).
    InPlace {
        /// Start of the result in the input.
        start: usize,
        /// Where the result ends.
        end: usize,
    },
    /// The static string `"."`.
    Dot,
}

/// XPG `basename`/`__xpg_basename`: strip trailing slashes (writing a terminator), return the
/// last component; `"."` for an empty input; `"/"` for an all-slash input.
pub fn basename_xpg(path: &mut [u8]) -> PathPart {
    if path.is_empty() {
        return PathPart::Dot;
    }
    let mut end = path.len();
    while end > 1 && path.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    if end == 1 && path.first() == Some(&b'/') {
        terminate(path, 1);
        return PathPart::InPlace { start: 0, end: 1 };
    }
    let start = rfind(path.get(..end).unwrap_or(&[]), b'/').map_or(0, |i| i.saturating_add(1));
    terminate(path, end);
    PathPart::InPlace { start, end }
}

/// `dirname`: the parent directory component, writing a terminator into the input. `"/"`
/// yields `"/"`; a name without a slash, or the empty name, yields `"."`. Purely lexical
/// (⚑ ARB: `dirname("a/..")` is `"a"`).
pub fn dirname(path: &mut [u8]) -> PathPart {
    let mut end = path.len();
    while end > 1 && path.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    let Some(slash) = rfind(path.get(..end).unwrap_or(&[]), b'/') else {
        return PathPart::Dot;
    };
    let mut cut = slash;
    while cut > 0 && path.get(cut.saturating_sub(1)) == Some(&b'/') {
        cut = cut.saturating_sub(1);
    }
    let cut = cut.max(1);
    terminate(path, cut);
    PathPart::InPlace { start: 0, end: cut }
}

fn terminate(path: &mut [u8], at: usize) {
    if let Some(b) = path.get_mut(at) {
        *b = 0;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    fn naive(h: &[u8], n: &[u8]) -> Option<usize> {
        if n.is_empty() {
            return Some(0);
        }
        h.windows(n.len()).position(|w| w == n)
    }

    #[test]
    fn copy_move_fill_every_length_and_overlap() {
        let fresh: Vec<u8> = (0..300).map(|i| (i * 11 + 5) as u8).collect();
        for n in 0..=96usize {
            let mut d = fresh.clone();
            assert_eq!(copy(&mut d[150..150 + n], &fresh[..n]), n);
            let mut want = fresh.clone();
            want[150..150 + n].copy_from_slice(&fresh[..n]);
            assert_eq!(d, want);
            for (s, t) in [(50usize, 51usize), (51, 50), (50, 50 + n / 3), (50 + n / 3, 50)] {
                let mut b = fresh.clone();
                assert!(move_within(&mut b, s, t, n));
                let mut want = fresh.clone();
                want.copy_within(s..s + n, t);
                assert_eq!(b, want, "n {n} s {s} t {t}");
            }
            let mut f = fresh.clone();
            fill(&mut f[9..9 + n], 0xee);
            let mut want = fresh.clone();
            want[9..9 + n].fill(0xee);
            assert_eq!(f, want);
        }
    }

    #[test]
    fn two_way_agrees_with_naive_search() {
        let alphabet = [b'a', b'b', b'c'];
        let hays: [&[u8]; 6] = [b"", b"a", b"abcabcabca", b"aaaaaaaaab", b"abababababcab", b"cbacbacbabcbacbbcacbbca"];
        let mut needle = Vec::new();
        for len in 0..=6u32 {
            for code in 0..3u32.pow(len) {
                needle.clear();
                let mut c = code;
                for _ in 0..len {
                    needle.push(alphabet[(c % 3) as usize]);
                    c /= 3;
                }
                for h in hays {
                    assert_eq!(find_seq(h, &needle, |x| x), naive(h, &needle), "{:?} in {:?}", needle, h);
                }
            }
        }
    }
}
