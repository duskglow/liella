//! printf formats (spec §12.6, D-12.4, D-12.5): the format parser and the renderer, byte and
//! wide, with arguments of the classes the format asks for. Argument memory is simulated: a
//! `%s`/`%ls` pointer selects one of a few fixed strings, `%n` stores nowhere. The renderer must
//! report exactly the units it output.
//!
//! Input: a header byte (bit 0: wide format and output; bit 1: `C.UTF-8`; bits 2-3: rounding
//! mode), then `format NUL argument bytes` (8 bytes per integer or `double`, 10 per
//! `long double`, reused cyclically).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::error::Result;
use liella::locale::file::Cat;
use liella::locale::Locale;
use liella::mem::TryVec;
use liella::num::float::Rounding;
use liella::printf::{self as pf, Arg, Class, Count, Len, Out, Piece, Plain, Unit};
use liella::text::{self, Decoded, Encoding, MbState};
use liella_fuzz::Input;

/// The largest width or precision rendered: wider fields are valid but only cost time.
const MAX_FIELD: usize = 1 << 14;

static TEXTS: [&[u8]; 5] = [b"", b"hello", b"\xc3\xa9t\xc3\xa9 \xff\xfe", b"a%sb%nc", b"0123456789012345678901234567890123456789012345678901234567890123456789"];
static WIDES: [&[i32]; 4] = [&[], &[0x68, 0x69], &[0xe9, 0x20ac, 0x1f600, -1, 0x11_0000], &[0xd800, 0x41]];

struct Env {
    loc: Locale,
    enc: Encoding,
    rounding: Rounding,
}

impl pf::Env for Env {
    fn text(&mut self, addr: usize, max: Option<usize>) -> &'static [u8] {
        let t = TEXTS[addr % TEXTS.len()];
        &t[..max.unwrap_or(t.len()).min(t.len())]
    }

    fn wide(&mut self, addr: usize, max: Option<usize>) -> &'static [i32] {
        let t = WIDES[addr % WIDES.len()];
        &t[..max.unwrap_or(t.len()).min(t.len())]
    }

    fn store_count(&mut self, _addr: usize, _len: Len, _count: usize) {}

    fn error_text(&mut self, out: &mut TryVec<u8>) -> Result<()> {
        out.extend_from_slice(b"No such file or directory")
    }

    fn radix(&self) -> &[u8] {
        self.loc.string(Cat::Numeric, 0)
    }

    fn thousands(&self) -> &[u8] {
        b","
    }

    fn grouping(&self) -> &[u8] {
        b"\x03\x02"
    }

    fn digit(&self, d: u8) -> &[u8] {
        const ASCII: &[u8; 10] = b"0123456789";
        let i = usize::from(d.min(9));
        &ASCII[i..i + 1]
    }

    fn encode(&self, wc: u32, out: &mut [u8; 16]) -> Option<usize> {
        text::encode(wc, self.enc, out)
    }

    fn decode(&self, input: &[u8]) -> Option<(u32, usize)> {
        match text::decode(&mut { MbState::INITIAL }, input, self.enc) {
            Decoded::Char(c, n) => Some((c, n)),
            Decoded::Null => Some((0, 1)),
            _ => None,
        }
    }

    fn rounding(&self) -> Rounding {
        self.rounding
    }
}

struct Counter(usize);

impl<U: Unit> Out<U> for Counter {
    fn put(&mut self, s: &[U]) {
        self.0 += s.len();
    }
}

/// The arguments the parsed format reads, from `bytes`; `*` counts kept within `MAX_FIELD`.
fn arguments(parsed: &pf::Parsed, bytes: &[u8]) -> Option<Vec<Arg>> {
    let mut at = 0usize;
    let mut take = |n: usize| -> [u8; 16] {
        let mut out = [0u8; 16];
        for b in out.iter_mut().take(n) {
            *b = if bytes.is_empty() { 0 } else { bytes[at % bytes.len()] };
            at += 1;
        }
        out
    };
    let mut args: Vec<Arg> = parsed
        .classes
        .iter()
        .map(|c| match c {
            Class::Int | Class::User { .. } => Arg::Int(u64::from_le_bytes(take(8)[..8].try_into().unwrap_or_default())),
            Class::Double => Arg::Double(u64::from_le_bytes(take(8)[..8].try_into().unwrap_or_default())),
            Class::LongDouble => Arg::LongDouble(u128::from_le_bytes(take(10)) & ((1u128 << 80) - 1)),
        })
        .collect();
    for piece in parsed.pieces.iter() {
        let Piece::Conv(c) = piece else { continue };
        for count in [c.width, c.prec] {
            match count {
                Count::Fixed(n) if n > MAX_FIELD => return None,
                Count::Arg(i) => {
                    if let Some(Arg::Int(v)) = args.get_mut(i) {
                        let x = i32::from_ne_bytes(u32::try_from(*v & 0xffff_ffff).unwrap_or(0).to_ne_bytes());
                        let clamped = x.clamp(-(MAX_FIELD as i32), MAX_FIELD as i32);
                        *v = u64::from(u32::from_ne_bytes(clamped.to_ne_bytes()));
                    }
                }
                _ => {}
            }
        }
    }
    Some(args)
}

fn run<U: Unit>(fmt: &[U], arg_bytes: &[u8], env: &mut Env) {
    let Ok(parsed) = pf::parse(fmt, &mut Plain) else { return };
    let Some(args) = arguments(&parsed, arg_bytes) else { return };
    let mut out = Counter(0);
    if let Ok(n) = pf::render(fmt, &parsed, &args, env, &mut out) {
        assert_eq!(n, out.0, "the count returned is the units output");
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let header = input.byte();
    let fmt = input.part();
    let arg_bytes = input.rest();
    let loc = liella_fuzz::locale(header & 2 != 0);
    let rounding = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero][usize::from((header >> 2) & 3)];
    let mut env = Env { loc, enc: Encoding::of(&loc), rounding };
    if header & 1 == 0 {
        run::<u8>(fmt, arg_bytes, &mut env);
    } else {
        let wide: Vec<u32> = fmt.iter().map(|&b| u32::from(b)).collect();
        run::<u32>(&wide, arg_bytes, &mut env);
    }
});
