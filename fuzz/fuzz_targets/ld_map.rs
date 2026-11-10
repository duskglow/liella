//! The loader's mapper validation (`liella_ld::map`, phase 14 DL-7, D-27.15): the planner over any
//! file at three page sizes, and `map::load` against a mapper that only records the calls, so the
//! reservation, alignment, `.bss` and RELRO arithmetic run on every input the planner accepts.
//! Every error is a value; a panic, an overflow or a call outside the reservation is a finding.
#![no_main]

use libfuzzer_sys::fuzz_target;
use liella_ld::arch::X86_64;
use liella_ld::map::{self, Mapper, PROT_EXEC, PROT_WRITE};

/// A mapper with no memory: checks each call lies inside the one reservation.
struct Calls {
    page: u64,
    res: Option<(u64, u64)>,
}

impl Calls {
    fn inside(&self, at: u64, len: u64) {
        let (s, l) = self.res.expect("a mapping before the reservation");
        assert!(at >= s && at.checked_add(len).is_some_and(|e| e <= s + l), "outside the reservation");
        assert_eq!(at % self.page, 0);
    }
}

impl Mapper for Calls {
    fn reserve(&mut self, at: Option<u64>, len: u64) -> Result<u64, ()> {
        let a = at.unwrap_or(0x5555_0000_0000 + 3 * self.page);
        if a == 0 || a.checked_add(len).is_none_or(|e| e > 1 << 47) {
            return Err(());
        }
        self.res = Some((a, len));
        Ok(a)
    }
    fn map_file(&mut self, at: u64, len: u64, prot: i32, _offset: u64) -> Result<(), ()> {
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0);
        self.inside(at, len);
        Ok(())
    }
    fn map_zero(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0);
        self.inside(at, len);
        Ok(())
    }
    fn store(&mut self, _at: u64, _len: u64, _bytes: Option<&[u8]>) {}
    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        assert!(prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0);
        self.inside(at, len);
        Ok(())
    }
    fn unmap(&mut self, _at: u64, _len: u64) {}
}

fuzz_target!(|data: &[u8]| {
    for page in [0x1000u64, 0x4000, 0x1_0000] {
        if map::plan_file::<X86_64>(data, page).is_ok() {
            let mut m = Calls { page, res: None };
            if let Ok(l) = map::load::<X86_64, _>(&mut m, data, page) {
                let _ = l.protect_relro(&mut m);
            }
        }
    }
});
