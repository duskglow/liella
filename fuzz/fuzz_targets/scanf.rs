//! scanf formats (spec §12.7, D-12.6): the scanning engine over a string, byte and wide, under
//! each symbol set (`%a` as GNU allocation, C99, C23), with simulated destination objects. The
//! engine must never read past the input, nor push back more than the unit it just read, and
//! the assignment count must not exceed the pointers the format names.
//!
//! Input: a header byte (bit 0: wide; bit 1: `C.UTF-8`; bits 2-3: symbol set; bit 4: every
//! pointer NULL after the third), then `format NUL input`.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::error::Result;
use liella::num::float::Rounding;
use liella::scanf::{self, Ctx, FmtUnit, Outcome, Set, Sink};
use liella::text::Encoding;
use liella_fuzz::Input;

/// The text being scanned: units with one unit of pushback, checked.
struct Text<'a> {
    units: &'a [u32],
    pos: usize,
    pushed: bool,
}

impl scanf::Input for Text<'_> {
    fn next(&mut self) -> Result<Option<u32>> {
        self.pushed = false;
        let u = self.units.get(self.pos).copied();
        if u.is_some() {
            self.pos += 1;
        }
        Ok(u)
    }

    fn back(&mut self, unit: u32) {
        assert!(!self.pushed, "two units pushed back");
        assert!(self.pos > 0 && self.units[self.pos - 1] == unit, "pushed back a unit not just read");
        self.pos -= 1;
        self.pushed = true;
    }
}

struct Objects {
    null_after: Option<usize>,
    stores: usize,
}

impl Sink for Objects {
    fn pointer(&mut self, n: usize) -> usize {
        // As `legacy::scanf_va::Pointers`: positions beyond NL_ARGMAX (4096) have no pointer.
        match self.null_after {
            Some(k) if n > k => 0,
            _ if n > 4096 => 0,
            _ => 0x1000 * n,
        }
    }
    fn int(&mut self, addr: usize, size: usize, _value: u64) {
        let _ = addr;
        assert!((1..=16).contains(&size), "int store of size {size}");
        self.stores += 1;
    }
    fn bytes(&mut self, _addr: usize, _data: &[u8]) {
        self.stores += 1;
    }
    fn wide(&mut self, _addr: usize, _data: &[u32]) {
        self.stores += 1;
    }
    fn alloc_bytes(&mut self, _addr: usize, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn alloc_wide(&mut self, _addr: usize, _data: &[u32]) -> Result<()> {
        Ok(())
    }
    fn null(&mut self, _addr: usize) {}
}

fn run<F: FmtUnit>(fmt: &[F], units: &[u32], ctx: Ctx<'_>, null_after: Option<usize>) {
    let mut text = Text { units, pos: 0, pushed: false };
    let mut objects = Objects { null_after, stores: 0 };
    if let Ok(s) = scanf::scan(fmt, &mut text, &mut objects, ctx) {
        assert!(text.pos <= units.len());
        if let Outcome::Assigned(n) = s.outcome {
            let conversions = fmt.iter().filter(|u| u.code() == u32::from(b'%')).count();
            assert!(n <= conversions, "{n} assignments from {conversions} conversion specifications");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let header = input.byte();
    let fmt = input.part();
    let text = input.rest();
    let loc = liella_fuzz::locale(header & 2 != 0);
    let set = [Set::Gnu, Set::C99, Set::C23, Set::C99][usize::from((header >> 2) & 3)];
    let ctx = Ctx { loc: &loc, enc: Encoding::of(&loc), rounding: Rounding::Nearest, set };
    let null_after = (header & 0x10 != 0).then_some(3);
    if header & 1 == 0 {
        let units: Vec<u32> = text.iter().map(|&b| u32::from(b)).collect();
        run::<u8>(fmt, &units, ctx, null_after);
    } else {
        // Wide format and input: each byte a unit, with a few wide characters mixed in.
        let widen = |b: u8| -> u32 { if b >= 0xf8 { 0x100 + u32::from(b) * 0x1000 } else { u32::from(b) } };
        let wfmt: Vec<i32> = fmt.iter().map(|&b| i32::try_from(widen(b)).unwrap_or(0)).collect();
        let units: Vec<u32> = text.iter().map(|&b| widen(b)).collect();
        run::<i32>(&wfmt, &units, ctx, null_after);
    }
});
