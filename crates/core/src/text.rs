//! Multibyte and wide text: codecs with explicit state, string conversion, display width
//! (spec §6). Wide characters are Unicode code points (`__STDC_ISO_10646__`).
//!
//! The locale's multibyte encoding is an [`Encoding`]: ASCII for the built-in `"C"` locale
//! (D-6.7), UTF-8 for UTF-8 locales (phase 7). Conversions keep their partial state in an
//! [`MbState`] that serialises into the 8-byte ABI `mbstate_t` (D-6.1); the all-zero state is
//! the initial state.

use crate::locale::Locale;

/// A multibyte encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    /// 7-bit ASCII: bytes 0x80..=0xFF are invalid.
    Ascii,
    /// UTF-8 (RFC 3629): no overlongs, no surrogates, nothing above U+10FFFF.
    Utf8,
    /// ISO-8859-1: each byte is the code point (built in, D-6.8).
    Latin1,
    /// A charset read from the installed charmaps (D-6.8).
    Charmap(CharmapRef),
}

/// A loaded charmap, compared by identity (each is loaded once per process).
#[derive(Clone, Copy)]
pub struct CharmapRef(pub &'static crate::charmap::Charmap);

impl PartialEq for CharmapRef {
    fn eq(&self, other: &CharmapRef) -> bool {
        core::ptr::eq(self.0, other.0)
    }
}

impl Eq for CharmapRef {}

impl core::fmt::Debug for CharmapRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Charmap({})", core::str::from_utf8(self.0.name()).unwrap_or("?"))
    }
}

impl Encoding {
    /// The encoding of `loc`'s `LC_CTYPE`.
    #[inline]
    pub fn of(loc: &Locale) -> Encoding {
        loc.encoding()
    }

    /// Work out the encoding of `loc`'s `LC_CTYPE` from its codeset item and attached charmap:
    /// the validation that [`Encoding::of`] used to repeat on every call. The locale runs it
    /// once, when it is built (LV-1).
    pub(crate) fn derive(loc: &Locale) -> Encoding {
        match loc.ctype() {
            crate::locale::Ctype::C => Encoding::Ascii,
            crate::locale::Ctype::CUtf8 => Encoding::Utf8,
            crate::locale::Ctype::File(f) => match builtin_charset(f.codeset()) {
                Some(e) => e,
                // Other codesets come from their charmap (D-6.8), which loading attached.
                None => loc.charmap().map_or(Encoding::Ascii, |c| Encoding::Charmap(CharmapRef(c))),
            },
        }
    }
}

/// The built-in encoding a codeset name denotes, if any (UTF-8, ASCII, ISO-8859-1).
pub fn builtin_charset(codeset: &[u8]) -> Option<Encoding> {
    use crate::locale::name::same_codeset;
    if same_codeset(codeset, b"UTF-8") {
        Some(Encoding::Utf8)
    } else if same_codeset(codeset, b"ANSI_X3.4-1968") || same_codeset(codeset, b"ASCII") || same_codeset(codeset, b"US-ASCII") {
        Some(Encoding::Ascii)
    } else if same_codeset(codeset, b"ISO-8859-1") || same_codeset(codeset, b"LATIN1") {
        Some(Encoding::Latin1)
    } else {
        None
    }
}

/// Conversion state (`mbstate_t`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MbState {
    /// Accumulated code point bits, or a pending surrogate / code point.
    acc: u32,
    /// UTF-8 continuation bytes still expected.
    need: u8,
    /// Bytes consumed so far for the pending character (for the minimum-value check).
    have: u8,
    /// What `acc` holds: 0 nothing, 1 a partial UTF-8 decode, 2 a pending low surrogate
    /// (`mbrtoc16`), 3 a pending high surrogate (`c16rtomb`), 4 pending UTF-8 output units
    /// (`mbrtoc8`; `need` units of `acc` remain), 5 the first `have` bytes of a charmap
    /// character (little-endian in `acc`).
    kind: u8,
}

impl MbState {
    /// The initial state.
    pub const INITIAL: MbState = MbState { acc: 0, need: 0, have: 0, kind: 0 };

    /// True in the initial state (`mbsinit`).
    pub fn is_initial(&self) -> bool {
        self.kind == 0
    }

    /// The 8-byte ABI form. What says the state is not initial (`need`, `have`, `kind`) is in
    /// the bytes of the header's `__count` (offset 0), the accumulated value in `__value`
    /// (offset 4): programs and libraries that keep their own state in an `mbstate_t` (gnulib's
    /// replacement converters) mark it in `__count` and ask [`count_is_initial`] (validation
    /// T18).
    pub fn to_bytes(self) -> [u8; 8] {
        let a = self.acc.to_le_bytes();
        [self.need, self.have, self.kind, 0, a[0], a[1], a[2], a[3]]
    }

    /// Read the ABI form. Bytes this library never writes (an unknown kind) read as the
    /// initial state rather than as garbage.
    pub fn from_bytes(b: [u8; 8]) -> MbState {
        let [need, have, kind, _, a0, a1, a2, a3] = b;
        let s = MbState { acc: u32::from_le_bytes([a0, a1, a2, a3]), need, have, kind };
        if kind > 5 || need > 3 || have > 4 { MbState::INITIAL } else { s }
    }
}

/// `mbsinit` over the 8 ABI bytes: initial when `__count` (the first 4) is zero, whatever
/// `__value` holds, as glibc 2.39 answers for a state a program wrote (black-box probe,
/// validation T18). Every non-initial state this library writes has a non-zero `kind` there.
pub fn count_is_initial(b: &[u8; 8]) -> bool {
    b.get(..4) == Some(&[0u8; 4][..])
}

/// Result of decoding one character (D-6.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoded {
    /// A character and the bytes consumed by this call.
    Char(u32, usize),
    /// The null character (one byte consumed).
    Null,
    /// A valid prefix: all input consumed into the state (`(size_t)-2`).
    Incomplete,
    /// An invalid sequence (`(size_t)-1`, `EILSEQ`).
    Invalid,
    /// The second half of a character already delivered (`(size_t)-3`).
    Continuation(u32),
}

fn utf8_lead(b: u8) -> Option<(u32, u8)> {
    match b {
        0xc2..=0xdf => Some((u32::from(b & 0x1f), 1)),
        0xe0..=0xef => Some((u32::from(b & 0x0f), 2)),
        0xf0..=0xf4 => Some((u32::from(b & 0x07), 3)),
        _ => None,
    }
}

fn utf8_valid_next(acc: u32, have: u8, need_total: u8, b: u8) -> bool {
    if b & 0xc0 != 0x80 {
        return false;
    }
    // The second byte bounds overlongs, surrogates and the upper limit (RFC 3629 §4).
    if have == 1 {
        return match (need_total, acc) {
            (2, 0x0) => b >= 0xa0,
            (2, 0xd) => b <= 0x9f,
            (3, 0x0) => b >= 0x90,
            (3, 0x4) => b <= 0x8f,
            _ => true,
        };
    }
    true
}

/// One UTF-8 character from the initial state, read from `next(0)`, `next(1)`, ... and decoded
/// in the same pass (MB-K2; AUD-FIX gathered the bytes first and then ran [`decode`] over them):
/// the answer and the state [`decode`] gives for the bytes a growing window would hold. It reads
/// the lead byte, then continuation bytes until the character is complete, up to and including
/// the first byte that proves it invalid (the terminator included), or until `next` returns
/// `None` (the caller's limit: `Incomplete`, the bytes so far kept in the state as [`decode`]
/// keeps them). `next(i)` is called only after bytes `0..i` were accepted (none was the
/// terminator), and never past the character's last byte.
#[inline]
pub fn decode_utf8_initial(mut next: impl FnMut(usize) -> Option<u8>) -> (Decoded, MbState) {
    let Some(b0) = next(0) else { return (Decoded::Incomplete, MbState::INITIAL) };
    if b0 < 0x80 {
        return (if b0 == 0 { Decoded::Null } else { Decoded::Char(u32::from(b0), 1) }, MbState::INITIAL);
    }
    let Some((lead, need)) = utf8_lead(b0) else { return (Decoded::Invalid, MbState::INITIAL) };
    let mut acc = lead;
    // `have` = bytes read so far (1..=need); `need` <= 3, so nothing below saturates.
    for have in 1..=need {
        let Some(b) = next(usize::from(have)) else {
            return (Decoded::Incomplete, MbState { acc, need: need.saturating_add(1).saturating_sub(have), have, kind: 1 });
        };
        if !utf8_valid_next(lead, have.min(2), need, b) {
            return (Decoded::Invalid, MbState::INITIAL);
        }
        acc = (acc << 6) | u32::from(b & 0x3f);
    }
    (Decoded::Char(acc, usize::from(need).saturating_add(1)), MbState::INITIAL)
}

/// `mbrtoc16`'s split of a decoded character: one outside the BMP is delivered as its high
/// surrogate, the low one kept in `state` for the next call.
pub fn split_utf16(state: &mut MbState, d: Decoded) -> Decoded {
    match d {
        Decoded::Char(c, n) if c > 0xffff => {
            let v = c.wrapping_sub(0x10000); // c > 0xFFFF (the match guard)
            *state = MbState { acc: 0xdc00 | (v & 0x3ff), need: 0, have: 0, kind: 2 };
            Decoded::Char(0xd800 | (v >> 10), n)
        }
        d => d,
    }
}

/// `mbrtoc8`'s split of a decoded character: its first UTF-8 unit now, the others kept in
/// `state` for the next calls.
pub fn split_utf8_units(state: &mut MbState, d: Decoded) -> Decoded {
    match d {
        Decoded::Char(c, n) => {
            let mut buf = [0u8; 4];
            let len = encode_utf8(c, &mut buf).unwrap_or(1);
            if len > 1 {
                *state = MbState { acc: c, need: u8::try_from(len.saturating_sub(1)).unwrap_or(0), have: 0, kind: 4 };
            }
            Decoded::Char(u32::from(buf[0]), n)
        }
        d => d,
    }
}

/// Decode one character of `enc` from `input` (`mbrtowc`). An empty input with no pending state
/// is `Incomplete`.
pub fn decode(state: &mut MbState, input: &[u8], enc: Encoding) -> Decoded {
    match enc {
        Encoding::Ascii => {
            *state = MbState::INITIAL;
            match input.first() {
                None => Decoded::Incomplete,
                Some(0) => Decoded::Null,
                Some(&b) if b < 0x80 => Decoded::Char(u32::from(b), 1),
                Some(_) => Decoded::Invalid,
            }
        }
        Encoding::Utf8 => decode_utf8(state, input),
        Encoding::Latin1 => {
            *state = MbState::INITIAL;
            match input.first() {
                None => Decoded::Incomplete,
                Some(0) => Decoded::Null,
                Some(&b) => Decoded::Char(u32::from(b), 1),
            }
        }
        Encoding::Charmap(c) => decode_charmap(state, input, c.0),
    }
}

/// A charmap character from the bytes pending in `state` and then `input`.
fn decode_charmap(state: &mut MbState, input: &[u8], cm: &crate::charmap::Charmap) -> Decoded {
    use crate::charmap::Step;
    let pending = if state.kind == 5 { usize::from(state.have) } else { 0 };
    let mut buf = [0u8; crate::charmap::MAX_BYTES * 2];
    let pend = state.acc.to_le_bytes();
    let mut n = 0usize;
    for &b in pend.iter().take(pending).chain(input.iter().take(crate::charmap::MAX_BYTES)) {
        if let Some(slot) = buf.get_mut(n) {
            *slot = b;
            n = n.saturating_add(1);
        }
    }
    let seq = buf.get(..n).unwrap_or(&[]);
    match cm.decode(seq) {
        Step::Char(cp, len) => {
            *state = MbState::INITIAL;
            // The bytes of this call's input that the character took.
            let used = len.saturating_sub(pending).max(1);
            if cp == 0 { Decoded::Null } else { Decoded::Char(cp, used) }
        }
        Step::Incomplete if !input.is_empty() => {
            // Keep every byte so far (fewer than the longest sequence, so they fit).
            let mut acc = [0u8; 4];
            for (a, b) in acc.iter_mut().zip(seq) {
                *a = *b;
            }
            *state = MbState { acc: u32::from_le_bytes(acc), need: 0, have: u8::try_from(seq.len().min(3)).unwrap_or(3), kind: 5 };
            Decoded::Incomplete
        }
        Step::Incomplete => Decoded::Incomplete,
        Step::Invalid => {
            *state = MbState::INITIAL;
            Decoded::Invalid
        }
    }
}

fn decode_utf8(state: &mut MbState, input: &[u8]) -> Decoded {
    if state.kind != 1 {
        *state = MbState::INITIAL;
    }
    let mut used = 0usize;
    for &b in input {
        used = used.saturating_add(1);
        if state.kind == 0 {
            match b {
                0 => return Decoded::Null,
                0x01..=0x7f => return Decoded::Char(u32::from(b), 1),
                _ => match utf8_lead(b) {
                    Some((v, n)) => *state = MbState { acc: v, need: n, have: 1, kind: 1 },
                    None => {
                        *state = MbState::INITIAL;
                        return Decoded::Invalid;
                    }
                },
            }
        } else {
            let total = state.have.saturating_sub(1).saturating_add(state.need);
            if !utf8_valid_next(state.acc, state.have, total, b) {
                *state = MbState::INITIAL;
                return Decoded::Invalid;
            }
            state.acc = (state.acc << 6) | u32::from(b & 0x3f);
            state.have = state.have.saturating_add(1);
            state.need = state.need.saturating_sub(1);
            if state.need == 0 {
                let c = state.acc;
                *state = MbState::INITIAL;
                return Decoded::Char(c, used);
            }
        }
    }
    Decoded::Incomplete
}

/// Encode code point `c` in `enc` into `out` (`wcrtomb`); returns the byte count, or `None`
/// for a character the encoding cannot represent (`EILSEQ`). `out` must hold
/// [`MAX_ENCODED`] bytes; a shorter one is `None` too.
pub fn encode(c: u32, enc: Encoding, out: &mut [u8]) -> Option<usize> {
    match enc {
        Encoding::Ascii => {
            let b = u8::try_from(c).ok().filter(|b| b.is_ascii())?;
            *out.first_mut()? = b;
            Some(1)
        }
        Encoding::Utf8 => encode_utf8(c, out),
        Encoding::Latin1 => {
            *out.first_mut()? = u8::try_from(c).ok()?;
            Some(1)
        }
        Encoding::Charmap(cm) => cm.0.encode(c, out),
    }
}

/// The longest encoding of one character in any supported encoding.
pub const MAX_ENCODED: usize = 4;

fn encode_utf8(c: u32, out: &mut [u8]) -> Option<usize> {
    let ch = char::from_u32(c)?;
    let n = ch.len_utf8();
    ch.encode_utf8(out.get_mut(..n)?);
    Some(n)
}

/// `mbrtoc16`: decode one character into UTF-16 code units; a character outside the BMP is
/// delivered as its high surrogate, then (on the next call) its low surrogate as
/// `Continuation`.
pub fn decode_utf16(state: &mut MbState, input: &[u8], enc: Encoding) -> Decoded {
    if state.kind == 2 {
        let low = state.acc;
        *state = MbState::INITIAL;
        return Decoded::Continuation(low);
    }
    let d = decode(state, input, enc);
    split_utf16(state, d)
}

/// Result of feeding one code unit to an encoder that buffers (`c16rtomb`, `c8rtomb`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fed {
    /// This many bytes were written.
    Wrote(usize),
    /// The unit was stored in the state; nothing written.
    Stored,
    /// Invalid input (`EILSEQ`).
    Invalid,
}

/// `c16rtomb`: encode one UTF-16 unit; a high surrogate is stored until its low surrogate.
pub fn encode_utf16(state: &mut MbState, unit: u16, enc: Encoding, out: &mut [u8]) -> Fed {
    let u = u32::from(unit);
    match (state.kind, u) {
        (3, 0xdc00..=0xdfff) => {
            // A stored high surrogate (kind 3) and a low one (the match arm): no overflow.
            let c = 0x10000u32.wrapping_add(state.acc.wrapping_sub(0xd800).wrapping_shl(10)).wrapping_add(u.wrapping_sub(0xdc00));
            *state = MbState::INITIAL;
            encode(c, enc, out).map_or(Fed::Invalid, Fed::Wrote)
        }
        (3, _) => {
            *state = MbState::INITIAL;
            Fed::Invalid
        }
        (_, 0xd800..=0xdbff) => {
            *state = MbState { acc: u, need: 0, have: 0, kind: 3 };
            Fed::Stored
        }
        (_, 0xdc00..=0xdfff) => Fed::Invalid,
        _ => encode(u, enc, out).map_or(Fed::Invalid, Fed::Wrote),
    }
}

/// `mbrtoc8`: decode one character and deliver its UTF-8 units one per call; the units after
/// the first come back as `Continuation`.
pub fn decode_utf8_units(state: &mut MbState, input: &[u8], enc: Encoding) -> Decoded {
    if state.kind == 4 {
        let mut buf = [0u8; 4];
        let n = encode_utf8(state.acc, &mut buf).unwrap_or(0);
        let idx = n.saturating_sub(usize::from(state.need));
        let unit = buf.get(idx).copied().unwrap_or(0);
        state.need = state.need.saturating_sub(1);
        if state.need == 0 {
            *state = MbState::INITIAL;
        }
        return Decoded::Continuation(u32::from(unit));
    }
    let d = decode(state, input, enc);
    split_utf8_units(state, d)
}

/// `c8rtomb`: feed one UTF-8 unit; complete characters are encoded in `enc`.
pub fn encode_utf8_units(state: &mut MbState, unit: u8, enc: Encoding, out: &mut [u8]) -> Fed {
    match decode_utf8(state, &[unit]) {
        Decoded::Char(c, _) => encode(c, enc, out).map_or(Fed::Invalid, Fed::Wrote),
        Decoded::Null => encode(0, enc, out).map_or(Fed::Invalid, Fed::Wrote),
        Decoded::Incomplete => Fed::Stored,
        Decoded::Invalid | Decoded::Continuation(_) => Fed::Invalid,
    }
}

/// Outcome of a string conversion (D-6.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Converted {
    /// Output units written (or that would be written when measuring), excluding the
    /// terminator.
    pub written: usize,
    /// Input units consumed (the position the `r` forms report).
    pub consumed: usize,
    /// True if the terminator was converted (the `r` forms then report `NULL`).
    pub finished: bool,
    /// True if an invalid sequence stopped the conversion (`EILSEQ`).
    pub invalid: bool,
}

/// Multibyte to wide (`mbsnrtowcs`): decode `input` (up to `limit` bytes, stopping after a
/// terminator) into `out` (`None` measures without writing; then the output is unbounded).
/// The output unit is a code point (`u32`) or the ABI's `wchar_t` (`i32`).
pub fn mbs_to_wcs<W: TryFrom<u32> + Copy>(state: &mut MbState, input: &[u8], out: Option<&mut [W]>, enc: Encoding) -> Converted {
    let cap = out.as_ref().map_or(usize::MAX, |o| o.len());
    let mut out = out;
    let mut r = Converted { written: 0, consumed: 0, finished: false, invalid: false };
    let lane = matches!(enc, Encoding::Utf8 | Encoding::Ascii);
    while r.written < cap {
        if lane && *state == MbState::INITIAL {
            if mbs_lane(input, out.as_deref_mut(), cap, enc, &mut r) {
                r.finished = true;
                break;
            }
            if r.written >= cap {
                break;
            }
        }
        let rest = input.get(r.consumed..).unwrap_or(&[]);
        let (c, n) = match decode(state, rest, enc) {
            Decoded::Char(c, n) => (c, n),
            Decoded::Null => (0, 1),
            Decoded::Incomplete => {
                r.consumed = input.len();
                break;
            }
            Decoded::Invalid | Decoded::Continuation(_) => {
                r.invalid = true;
                break;
            }
        };
        if let (Some(slot), Ok(w)) = (out.as_mut().and_then(|o| o.get_mut(r.written)), W::try_from(c)) {
            *slot = w;
        }
        r.consumed = r.consumed.saturating_add(n);
        if c == 0 {
            r.finished = true;
            break;
        }
        r.written = r.written.saturating_add(1);
    }
    r
}

/// The common characters of [`mbs_to_wcs`] without the general decoder (STRXFRM-3): from the
/// initial state in a UTF-8 or ASCII locale, bytes 0x01..=0x7F (eight at a time when the next
/// eight all are and the output has room for them) and, in UTF-8, well-formed two-byte
/// characters (lead 0xC2..=0xDF, as [`decode`] accepts them). Each is a complete character from
/// the initial state, after which the decoder leaves the state initial, so the state is not
/// touched. The terminator is converted too (stored if there is room; `true` is then returned,
/// with the terminator consumed but not counted, as the general loop does). Stops at anything
/// else - a byte starting a longer or invalid sequence, the end of `input`, a full output - and
/// the general loop goes on from there.
#[inline(always)]
fn mbs_lane<W: TryFrom<u32> + Copy>(input: &[u8], mut out: Option<&mut [W]>, cap: usize, enc: Encoding, r: &mut Converted) -> bool {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let utf8 = enc == Encoding::Utf8;
    let (mut at, mut done) = (r.consumed, r.written);
    let finished = 'lane: loop {
        if cap.saturating_sub(done) >= 8 {
            if let Some(chunk) = input.get(at..at.saturating_add(8)).and_then(|c| <[u8; 8]>::try_from(c).ok()) {
                let x = u64::from_le_bytes(chunk);
                // No byte with its top bit set, and (the zero-byte test, exact once no top bit
                // is set) no terminator.
                if (x | (x.wrapping_sub(ONES) & !x)) & HIGH == 0 {
                    if let Some(o) = out.as_deref_mut().and_then(|o| o.get_mut(done..done.saturating_add(8))) {
                        for (slot, &b) in o.iter_mut().zip(&chunk) {
                            if let Ok(w) = W::try_from(u32::from(b)) {
                                *slot = w;
                            }
                        }
                    }
                    at = at.saturating_add(8);
                    done = done.saturating_add(8);
                    continue;
                }
            }
        }
        // Up to eight characters one at a time before the next eight-byte test (mixed text
        // would otherwise pay that test at every character).
        for _ in 0..8 {
            if done >= cap {
                break 'lane false;
            }
            let Some(&b0) = input.get(at) else { break 'lane false };
            let (c, n) = if b0 < 0x80 {
                (u32::from(b0), 1)
            } else if utf8 && (0xc2..=0xdf).contains(&b0) {
                match input.get(at.saturating_add(1)) {
                    Some(&b1) if b1 & 0xc0 == 0x80 => ((u32::from(b0 & 0x1f) << 6) | u32::from(b1 & 0x3f), 2),
                    _ => break 'lane false,
                }
            } else {
                break 'lane false;
            };
            if let (Some(slot), Ok(w)) = (out.as_deref_mut().and_then(|o| o.get_mut(done)), W::try_from(c)) {
                *slot = w;
            }
            at = at.saturating_add(n);
            if c == 0 {
                break 'lane true;
            }
            done = done.saturating_add(1);
        }
    };
    r.consumed = at;
    r.written = done;
    finished
}

/// Wide to multibyte (`wcsnrtombs`): encode `input` (stopping after a terminator) into `out`
/// (`None` measures). A character that does not fit entirely is not written.
/// The input unit is a code point (`u32`) or a `wchar_t` (`i32`; negative values are invalid).
pub fn wcs_to_mbs<W: TryInto<u32> + Copy>(input: &[W], out: Option<&mut [u8]>, enc: Encoding) -> Converted {
    let cap = out.as_ref().map_or(usize::MAX, |o| o.len());
    let mut out = out;
    let mut r = Converted { written: 0, consumed: 0, finished: false, invalid: false };
    for &w in input {
        let c: u32 = w.try_into().unwrap_or(u32::MAX);
        let mut buf = [0u8; MAX_ENCODED];
        let Some(n) = encode(c, enc, &mut buf) else {
            r.invalid = true;
            break;
        };
        if c == 0 {
            r.finished = true;
            if r.written < cap {
                if let Some(slot) = out.as_mut().and_then(|o| o.get_mut(r.written)) {
                    *slot = 0;
                }
                r.consumed = r.consumed.saturating_add(1);
            }
            break;
        }
        if r.written.saturating_add(n) > cap {
            break;
        }
        if let Some(dst) = out.as_mut().and_then(|o| o.get_mut(r.written..r.written.saturating_add(n))) {
            dst.copy_from_slice(buf.get(..n).unwrap_or(&[]));
        }
        r.written = r.written.saturating_add(n);
        r.consumed = r.consumed.saturating_add(1);
    }
    r
}

/// Columns occupied by `c` (`wcwidth`): 0 for the null character, -1 for a character that is
/// not printable in `loc`, 1 otherwise (the `"C"` locale has no wide or combining characters).
pub fn display_width(c: u32, loc: &Locale) -> i32 {
    match loc.ctype() {
        crate::locale::Ctype::C => {
            if c == 0 {
                0
            } else if crate::ctype::wclasses(c, loc).intersects(crate::ctype::Class::PRINT) {
                1
            } else {
                -1
            }
        }
        crate::locale::Ctype::CUtf8 => crate::unicode::width(c),
        crate::locale::Ctype::File(f) => f.width(c),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    /// The standard library's verdict on the first character of `inp`.
    fn std_first(inp: &[u8]) -> Decoded {
        match core::str::from_utf8(inp) {
            Ok(s) => s.chars().next().map_or(Decoded::Incomplete, |c| if c == '\0' { Decoded::Null } else { Decoded::Char(c as u32, c.len_utf8()) }),
            Err(e) if e.valid_up_to() > 0 => {
                let s = core::str::from_utf8(&inp[..e.valid_up_to()]).unwrap();
                let c = s.chars().next().unwrap();
                if c == '\0' { Decoded::Null } else { Decoded::Char(c as u32, c.len_utf8()) }
            }
            Err(e) => if e.error_len().is_none() { Decoded::Incomplete } else { Decoded::Invalid },
        }
    }

    #[test]
    fn utf8_decoder_agrees_with_the_standard_library() {
        let mut inputs: Vec<Vec<u8>> = (0u16..256).map(|b| vec![b as u8]).collect();
        for a in 0xc0u16..=0xff {
            for b in 0x70u16..=0xc0 {
                inputs.push(vec![a as u8, b as u8]);
                for c in [0x7fu8, 0x80, 0xbf, 0xc0] {
                    inputs.push(vec![a as u8, b as u8, c]);
                    inputs.push(vec![a as u8, b as u8, c, 0x80]);
                }
            }
        }
        for inp in inputs {
            let mut st = MbState::INITIAL;
            assert_eq!(decode(&mut st, &inp, Encoding::Utf8), std_first(&inp), "{inp:02x?}");
        }
    }

    /// [`mbs_to_wcs`] as one [`decode`] per character (no lane): the reference for the lane.
    fn mbs_reference(state: &mut MbState, input: &[u8], mut out: Option<&mut [i32]>, enc: Encoding) -> Converted {
        let cap = out.as_ref().map_or(usize::MAX, |o| o.len());
        let mut r = Converted { written: 0, consumed: 0, finished: false, invalid: false };
        while r.written < cap {
            let (c, n) = match decode(state, &input[r.consumed..], enc) {
                Decoded::Char(c, n) => (c, n),
                Decoded::Null => (0, 1),
                Decoded::Incomplete => {
                    r.consumed = input.len();
                    break;
                }
                Decoded::Invalid | Decoded::Continuation(_) => {
                    r.invalid = true;
                    break;
                }
            };
            if let Some(slot) = out.as_mut().and_then(|o| o.get_mut(r.written)) {
                *slot = c as i32;
            }
            r.consumed += n;
            if c == 0 {
                r.finished = true;
                break;
            }
            r.written += 1;
        }
        r
    }

    #[test]
    fn k_mbs_lane_equals_the_decoder_loop() {
        // Trace: STRXFRM-3 (the mbs_to_wcs lane: eight-byte ASCII runs and two-byte UTF-8).
        let pieces: [&[u8]; 9] = [b"abcdefgh", b"x", b"\xc3\xa9", b"\xe2\x82\xac", b"\x80", b"\xc3", b"\xc1\x81", b"\0", b"\xf0\x9f\x98\x80"];
        let mut inputs: Vec<Vec<u8>> = Vec::new();
        for a in 0..pieces.len() {
            for b in 0..pieces.len() {
                for c in 0..pieces.len() {
                    for pre in [0usize, 3, 7, 8, 9, 15, 16] {
                        let mut v: Vec<u8> = (0..pre).map(|i| b'a' + (i % 26) as u8).collect();
                        v.extend_from_slice(pieces[a]);
                        v.extend_from_slice(pieces[b]);
                        v.extend_from_slice(b"0123456789");
                        v.extend_from_slice(pieces[c]);
                        inputs.push(v.clone());
                        v.push(0);
                        inputs.push(v);
                    }
                }
            }
        }
        for inp in &inputs {
            for enc in [Encoding::Utf8, Encoding::Ascii, Encoding::Latin1] {
                for cap in (0..inp.len() + 2).map(Some).chain([None]) {
                    for start in [MbState::INITIAL, { let mut s = MbState::INITIAL; decode(&mut s, &[0xe2], Encoding::Utf8); s }] {
                        let (mut s1, mut s2) = (start, start);
                        let (mut o1, mut o2) = (vec![-7i32; cap.unwrap_or(0)], vec![-7i32; cap.unwrap_or(0)]);
                        let r1 = mbs_to_wcs(&mut s1, inp, cap.map(|_| &mut o1[..]), enc);
                        let r2 = mbs_reference(&mut s2, inp, cap.map(|_| &mut o2[..]), enc);
                        assert_eq!((r1, s1, &o1), (r2, s2, &o2), "{inp:02x?} {enc:?} cap {cap:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn k_mbs_lane_every_two_byte_pair_at_every_lane_position() {
        // Trace: STRXFRM-3 (the lane's two-byte decode). Every (lead, second) byte pair, at each
        // offset of an eight-byte block (so a lead sits at every lane slot, the last one included),
        // followed by a tail, against the general decoder loop: covers every valid pair and every
        // invalid continuation (0x00..=0xFF, not only 0x80..=0xBF), in a UTF-8 and an ASCII locale.
        // (A mask of 0x7f instead of 0x3f on the second byte is not observable: the guard has
        // already established that bit 6 is clear, so both masks give the same six bits.)
        for lead in 0..=255u8 {
            for second in 0..=255u8 {
                for pre in [0usize, 1, 6, 7, 8, 13, 15] {
                    let mut v: Vec<u8> = (0..pre).map(|i| b'a' + (i % 26) as u8).collect();
                    v.extend_from_slice(&[lead, second, b'z', b'y', b'x', b'w', b'v', b'u', b't', 0]);
                    for enc in [Encoding::Utf8, Encoding::Ascii] {
                        for cap in [None, Some(v.len() + 1), Some(pre + 1)] {
                            let (mut s1, mut s2) = (MbState::INITIAL, MbState::INITIAL);
                            let (mut o1, mut o2) = (vec![-7i32; cap.unwrap_or(0)], vec![-7i32; cap.unwrap_or(0)]);
                            let r1 = mbs_to_wcs(&mut s1, &v, cap.map(|_| &mut o1[..]), enc);
                            let r2 = mbs_reference(&mut s2, &v, cap.map(|_| &mut o2[..]), enc);
                            assert_eq!((r1, s1, &o1), (r2, s2, &o2), "{v:02x?} {enc:?} cap {cap:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn state_round_trips_through_the_abi_form() {
        let mut st = MbState::INITIAL;
        assert_eq!(decode(&mut st, &[0xe2, 0x82], Encoding::Utf8), Decoded::Incomplete);
        let mut back = MbState::from_bytes(st.to_bytes());
        assert_eq!(decode(&mut back, &[0xac], Encoding::Utf8), Decoded::Char(0x20ac, 1));
        assert_eq!(MbState::INITIAL.to_bytes(), [0; 8]);
    }
}
