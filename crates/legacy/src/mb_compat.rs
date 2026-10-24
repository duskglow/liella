//! The C shapes of spec §6.2–§6.3 over `liella::text`: in-band sentinels (`(size_t)-2`,
//! `(size_t)-3`), `NULL` arguments that reset or measure, hidden states for the non-restartable
//! forms and for a `NULL` `mbstate_t *`, and the `*src` cursor of the `r` string forms.
//!
//! Inputs arrive as the text before the terminator, read no further than the caller's byte
//! limit (`n`, `nms`); `bounded` says whether a terminator follows inside that limit. One
//! character never needs more than [`text::MAX_ENCODED`] bytes, so the single-character forms
//! look at no more than that, and read their input ([`MbInput`]) one byte at a time, only as
//! far as the character needs (AUD-M4: no scan over all of `n`, so a loop over a buffer stays
//! O(N), and no byte past the character is touched, as the reference does).

use liella::error::{Errno, Result};
use liella::locale::Locale;
use liella::text::{self, Converted, Decoded, Encoding, Fed, MbState};

use crate::cstring::Dest;
use crate::mb_state::{self, Slot};
use crate::string_compat::Cursor;

/// `(size_t)-2`: an incomplete but valid prefix, consumed into the state.
pub const INCOMPLETE: usize = usize::MAX - 1;
/// `(size_t)-3`: the second half of a character already delivered.
pub const CONTINUED: usize = usize::MAX - 2;

/// Where a conversion state lives.
pub enum State<'a> {
    /// The caller's `mbstate_t` (its 8 ABI bytes).
    User(&'a mut [u8; 8]),
    /// A hidden state (the non-restartable forms, or a `NULL` `mbstate_t *`).
    Hidden(Slot),
    /// A private initial state (`mbstowcs`, `wcstombs`).
    Fresh(MbState),
}

impl State<'_> {
    /// The caller's state, or the hidden `slot` if it passed `NULL`.
    pub fn of(user: Option<&mut [u8; 8]>, slot: Slot) -> State<'_> {
        user.map_or(State::Hidden(slot), State::User)
    }

    fn load(&self) -> MbState {
        match self {
            State::User(b) => mb_state::from_abi(**b),
            State::Hidden(s) => mb_state::load(*s),
            State::Fresh(m) => *m,
        }
    }

    fn store(&mut self, m: MbState) {
        match self {
            State::User(b) => **b = mb_state::to_abi(m),
            State::Hidden(s) => mb_state::store(*s, m),
            State::Fresh(f) => *f = m,
        }
    }
}

/// Text read at an address, no further than a limit (the boundary converts the address under
/// the export's contract).
pub trait ReadAt<T> {
    /// The units at `addr` before the terminator, at most `limit` of them.
    fn read_at(&mut self, addr: usize, limit: usize) -> Option<&[T]>;
}

/// A cursor held in a local variable (`mbstowcs` converts as if through `mbsrtowcs`).
pub struct Local(pub usize);

impl Cursor for Local {
    fn get(&self) -> usize {
        self.0
    }
    fn set(&mut self, addr: usize) {
        self.0 = addr;
    }
}

/// `MB_CUR_MAX` (`__ctype_get_mb_cur_max`).
pub fn mb_cur_max(loc: &Locale) -> usize {
    loc.mb_cur_max()
}

/// The input of a single-character form, read lazily: `byte(i)` is byte `i` of the text, the
/// terminator (0) when the text ends there inside the caller's limit, or `None` at the limit.
/// The caller reads byte `i` only after bytes `0..i` were read and none was the terminator.
pub trait MbInput {
    /// Byte `i`, or `None` past the caller's limit.
    fn byte(&self, i: usize) -> Option<u8>;
}

impl<T: MbInput + ?Sized> MbInput for &T {
    fn byte(&self, i: usize) -> Option<u8> {
        (**self).byte(i)
    }
}

/// A measured text and whether its terminator follows inside the limit.
impl MbInput for (&[u8], bool) {
    fn byte(&self, i: usize) -> Option<u8> {
        match self.0.get(i) {
            Some(&b) => Some(b),
            None if i == self.0.len() && self.1 => Some(0),
            None => None,
        }
    }
}

/// One step of a restartable decoder: the C return value and the unit to store, if any. `None`
/// (a `NULL` `s`) is the C rule's `s = ""`, `n = 1`.
///
/// The window grows one byte at a time while the decoder needs more: an incomplete prefix, or,
/// for a multibyte charmap, a character that took every byte so far (a longer one may share its
/// prefix: the longest listed sequence wins). At most [`text::MAX_ENCODED`] bytes, then the
/// terminator if it comes next, as a whole-text window would hold; the decoders decide on the
/// bytes in order, so the answer is the one that window gives. `split` is what `dec` does to a
/// character decoded from the initial state (the UTF-8 fast path decodes it itself).
fn step<I: MbInput>(s: Option<&I>, mut st: State<'_>, dec: fn(&mut MbState, &[u8], Encoding) -> Decoded, split: fn(&mut MbState, Decoded) -> Decoded, loc: &Locale) -> Result<(usize, Option<u32>)> {
    let enc = Encoding::of(loc);
    let longer = matches!(enc, Encoding::Charmap(c) if c.0.max_len() > 1);
    let byte = |i: usize| match s {
        Some(s) => s.byte(i),
        None => (i == 0).then_some(0),
    };
    let start = st.load();
    // UTF-8 from the initial state (the common case): the lead byte gives the length, so read
    // exactly the bytes the per-byte loop below would read and decode them in the same pass
    // (`text::decode_utf8_initial` applies the decoder's own rules; AUD-FIX, MB-K2).
    if enc == Encoding::Utf8 && start.is_initial() {
        let (d, mut m) = text::decode_utf8_initial(byte);
        let d = split(&mut m, d);
        st.store(m);
        return answer(d);
    }
    let mut buf = [0u8; text::MAX_ENCODED + 1];
    let mut k = 0usize;
    let mut open = true;
    loop {
        if open {
            match byte(k) {
                // Past MAX_ENCODED bytes only the terminator joins the window.
                Some(b) if k < text::MAX_ENCODED || b == 0 => {
                    if let Some(slot) = buf.get_mut(k) {
                        *slot = b;
                        k = k.saturating_add(1);
                    }
                    open = b != 0 && k < buf.len();
                }
                _ => open = false,
            }
        }
        let mut m = start;
        let d = dec(&mut m, buf.get(..k).unwrap_or(&[]), enc);
        let more = match d {
            Decoded::Incomplete => true,
            Decoded::Char(_, used) => longer && used >= k,
            _ => false,
        };
        if !(open && more) {
            st.store(m);
            return answer(d);
        }
    }
}

/// The split of `mbrtowc`, `mbrlen` and `mbtowc`: none (one call delivers the character).
fn whole(_: &mut MbState, d: Decoded) -> Decoded {
    d
}

/// The C return value and the unit to store for a decoder's answer.
fn answer(d: Decoded) -> Result<(usize, Option<u32>)> {
    match d {
        Decoded::Char(c, used) => Ok((used, Some(c))),
        Decoded::Null => Ok((0, Some(0))),
        Decoded::Incomplete => Ok((INCOMPLETE, None)),
        Decoded::Invalid => Err(Errno::EILSEQ),
        Decoded::Continuation(u) => Ok((CONTINUED, Some(u))),
    }
}

/// Store a decoded unit. A `NULL` `s` is the C rule's `mbrtowc(NULL, "", 1, ps)` (C17
/// 7.29.6.3.2, 7.28.1): the caller's output pointer is ignored, so nothing is stored.
fn put<W: TryFrom<u32>, I>(out: Option<&mut W>, s: Option<&I>, v: Option<u32>) {
    if s.is_none() {
        return;
    }
    if let (Some(o), Some(w)) = (out, v.and_then(|v| W::try_from(v).ok())) {
        *o = w;
    }
}

/// `mbrtowc`/`mbrtoc32`: `s` is the input text and whether a terminator follows within `n`.
pub fn mbrtowc<W: TryFrom<u32>>(out: Option<&mut W>, s: Option<impl MbInput>, st: State<'_>, loc: &Locale) -> Result<usize> {
    let (r, v) = step(s.as_ref(), st, text::decode, whole, loc)?;
    put(out, s.as_ref(), v);
    Ok(r)
}

/// `mbrtoc16`: characters outside the BMP take two calls (the second returns `(size_t)-3`).
pub fn mbrtoc16(out: Option<&mut u16>, s: Option<impl MbInput>, st: State<'_>, loc: &Locale) -> Result<usize> {
    let (r, v) = step(s.as_ref(), st, text::decode_utf16, text::split_utf16, loc)?;
    put(out, s.as_ref(), v);
    Ok(r)
}

/// `mbrtoc8`: one UTF-8 code unit per call.
pub fn mbrtoc8(out: Option<&mut u8>, s: Option<impl MbInput>, st: State<'_>, loc: &Locale) -> Result<usize> {
    let (r, v) = step(s.as_ref(), st, text::decode_utf8_units, text::split_utf8_units, loc)?;
    put(out, s.as_ref(), v);
    Ok(r)
}

/// `mbrlen`.
pub fn mbrlen(s: Option<impl MbInput>, st: State<'_>, loc: &Locale) -> Result<usize> {
    step(s.as_ref(), st, text::decode, whole, loc).map(|(r, _)| r)
}

/// `mblen`/`mbtowc`: the non-restartable forms. A `NULL` `s` resets the hidden state and
/// reports that the encoding has no shift states (0); an incomplete character is an error.
pub fn mbtowc<W: TryFrom<u32>>(out: Option<&mut W>, s: Option<impl MbInput>, slot: Slot, loc: &Locale) -> Result<i64> {
    let Some(s) = s else {
        mb_state::store(slot, MbState::INITIAL);
        return Ok(0);
    };
    let (r, v) = step(Some(&s), State::Hidden(slot), text::decode, whole, loc)?;
    if r == INCOMPLETE {
        mb_state::store(slot, MbState::INITIAL);
        return Err(Errno::EILSEQ);
    }
    put(out, Some(&s), v);
    Ok(i64::try_from(r).unwrap_or(i64::MAX))
}

/// Store an encoded character through `d` (`None`: the C rule for a `NULL` output, which
/// encodes the null character into an internal buffer and resets the state).
fn emit(bytes: &[u8], d: Option<impl Dest<u8>>) -> Result<usize> {
    if let Some(mut d) = d {
        d.get(bytes.len())?.copy_from_slice(bytes);
    }
    Ok(bytes.len())
}

/// `wcrtomb`/`c32rtomb`.
pub fn wcrtomb(d: Option<impl Dest<u8>>, wc: u32, mut st: State<'_>, loc: &Locale) -> Result<usize> {
    let wc = if d.is_some() { wc } else { 0 };
    st.store(MbState::INITIAL);
    let mut buf = [0u8; text::MAX_ENCODED];
    let n = text::encode(wc, Encoding::of(loc), &mut buf).ok_or(Errno::EILSEQ)?;
    emit(buf.get(..n).unwrap_or(&[]), d)
}

/// `wctomb`: the non-restartable form; a `NULL` `s` resets and reports no shift states.
pub fn wctomb(d: Option<impl Dest<u8>>, wc: u32, loc: &Locale) -> Result<i64> {
    if d.is_none() {
        mb_state::store(Slot::Wctomb, MbState::INITIAL);
        return Ok(0);
    }
    wcrtomb(d, wc, State::Hidden(Slot::Wctomb), loc).map(|n| i64::try_from(n).unwrap_or(i64::MAX))
}

fn fed(d: Option<impl Dest<u8>>, mut st: State<'_>, f: impl FnOnce(&mut MbState, &mut [u8]) -> Fed) -> Result<usize> {
    let mut m = st.load();
    let mut buf = [0u8; text::MAX_ENCODED];
    let r = f(&mut m, &mut buf);
    st.store(m);
    match r {
        Fed::Wrote(n) => emit(buf.get(..n).unwrap_or(&[]), d),
        Fed::Stored => Ok(0),
        Fed::Invalid => Err(Errno::EILSEQ),
    }
}

/// `c16rtomb`: a high surrogate is kept in the state until its low surrogate arrives.
pub fn c16rtomb(d: Option<impl Dest<u8>>, unit: u16, st: State<'_>, loc: &Locale) -> Result<usize> {
    let unit = if d.is_some() { unit } else { 0 };
    fed(d, st, |m, buf| text::encode_utf16(m, unit, Encoding::of(loc), buf))
}

/// `c8rtomb`: one UTF-8 code unit per call.
pub fn c8rtomb(d: Option<impl Dest<u8>>, unit: u8, st: State<'_>, loc: &Locale) -> Result<usize> {
    let unit = if d.is_some() { unit } else { 0 };
    fed(d, st, |m, buf| text::encode_utf8_units(m, unit, Encoding::of(loc), buf))
}

/// `btowc`: the wide character of a single byte, `WEOF` if the byte is not a character on its
/// own (or is `EOF`).
pub fn btowc(c: i32, loc: &Locale) -> u32 {
    let Ok(b) = u8::try_from(c) else { return u32::MAX };
    let mut m = MbState::INITIAL;
    match text::decode(&mut m, &[b], Encoding::of(loc)) {
        Decoded::Char(w, 1) => w,
        Decoded::Null => 0,
        _ => u32::MAX,
    }
}

/// `wctob`: the single byte of a wide character, `EOF` if it needs more (or is invalid).
pub fn wctob(wc: u32, loc: &Locale) -> i32 {
    let mut buf = [0u8; text::MAX_ENCODED];
    match text::encode(wc, Encoding::of(loc), &mut buf) {
        Some(1) => i32::from(buf[0]),
        _ => -1,
    }
}

/// `mbsinit`: `NULL` counts as the initial state; otherwise the header's `__count` decides
/// ([`text::count_is_initial`]).
pub fn mbsinit(st: Option<&[u8; 8]>) -> bool {
    st.is_none_or(text::count_is_initial)
}

/// `wcwidth`.
pub fn wcwidth(wc: i32, loc: &Locale) -> i32 {
    u32::try_from(wc).map_or(-1, |c| text::display_width(c, loc))
}

/// `wcswidth`: the width of the first `n` characters (fewer at a terminator); -1 if any of
/// them is not printable.
pub fn wcswidth(s: &[i32], loc: &Locale) -> i32 {
    s.iter().try_fold(0i32, |acc, &c| match wcwidth(c, loc) {
        -1 => None,
        w => Some(acc.saturating_add(w)),
    }).unwrap_or(-1)
}

/// The terminator step after converting the text before it: a pending partial character makes
/// it invalid; otherwise it is converted if there is room.
fn finish(r: &mut Converted, m: &MbState, bounded: bool, room: bool) {
    if r.invalid || !bounded || !room {
        return;
    }
    if m.is_initial() {
        r.finished = true;
    } else {
        r.invalid = true;
    }
}

/// `mbsrtowcs`/`mbsnrtowcs`: convert the text at `*src` (at most `limit` bytes of it) into
/// `out` (`None` measures). When writing, `*src` advances to where conversion stopped, or
/// becomes `NULL` after the terminator; the state is updated only when writing.
pub fn mbsrtowcs(mut cur: impl Cursor, limit: usize, out: Option<&mut [i32]>, mut st: State<'_>, mut mem: impl ReadAt<u8>, loc: &Locale) -> Result<usize> {
    let addr = cur.get();
    let input = mem.read_at(addr, limit).ok_or(Errno::EFAULT)?;
    let bounded = input.len() < limit;
    let writing = out.is_some();
    let mut m = st.load();
    let mut out = out;
    let mut r = text::mbs_to_wcs(&mut m, input, out.as_deref_mut(), Encoding::of(loc));
    let room = out.as_ref().is_none_or(|o| r.written < o.len());
    let at_end = r.consumed >= input.len();
    finish(&mut r, &m, bounded && at_end, room);
    if r.finished {
        if let Some(slot) = out.as_mut().and_then(|o| o.get_mut(r.written)) {
            *slot = 0;
        }
        m = MbState::INITIAL;
    }
    if writing {
        st.store(m);
        cur.set(if r.finished { 0 } else { addr.saturating_add(r.consumed) });
    }
    if r.invalid { Err(Errno::EILSEQ) } else { Ok(r.written) }
}

/// `wcsrtombs`/`wcsnrtombs`: convert the wide text at `*src` (at most `limit` units) into `out`
/// (`None` measures), never splitting a character.
pub fn wcsrtombs(mut cur: impl Cursor, limit: usize, out: Option<&mut [u8]>, mut st: State<'_>, mut mem: impl ReadAt<i32>, loc: &Locale) -> Result<usize> {
    let addr = cur.get();
    let input = mem.read_at(addr, limit).ok_or(Errno::EFAULT)?;
    let bounded = input.len() < limit;
    let writing = out.is_some();
    let mut out = out;
    let mut r = text::wcs_to_mbs(input, out.as_deref_mut(), Encoding::of(loc));
    let room = out.as_ref().is_none_or(|o| r.written < o.len());
    let at_end = r.consumed >= input.len();
    finish(&mut r, &MbState::INITIAL, bounded && at_end, room);
    if r.finished {
        if let Some(slot) = out.as_mut().and_then(|o| o.get_mut(r.written)) {
            *slot = 0;
        }
    }
    if writing {
        st.store(MbState::INITIAL);
        cur.set(if r.finished { 0 } else { addr.saturating_add(r.consumed.saturating_mul(4)) });
    }
    if r.invalid { Err(Errno::EILSEQ) } else { Ok(r.written) }
}
