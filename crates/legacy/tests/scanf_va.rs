//! `liella_legacy::scanf_va::Pointers`: scanf's pointer arguments, read from a `va_list` on demand
//! and kept for positional access (spec §12.7, clause `s12_7_c_3`; D-12.13).
//!
//! The source is a counting fake of the `va_list`: slot `k` (0-based) holds `0x1000 * (k + 1)`.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::va::VaSource;
use liella_legacy::scanf_va::{Pointers, ARG_MAX};

struct Fake {
    reads: usize,
    /// Reads of any class but integer: scanf arguments are pointers, so there must be none.
    other: usize,
}

impl VaSource for Fake {
    fn int(&mut self) -> u64 {
        self.reads += 1;
        0x1000 * self.reads as u64
    }
    fn double(&mut self) -> u64 {
        self.other += 1;
        0
    }
    fn long_double(&mut self) -> u128 {
        self.other += 1;
        0
    }
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn nothing_is_read_until_asked() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let _p = Pointers::new(&mut f);
    }
    assert_eq!(f.reads, 0);
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn sequential_reads_take_one_slot_each() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(1), Some(0x1000));
        assert_eq!(p.get(2), Some(0x2000));
        assert_eq!(p.get(3), Some(0x3000));
    }
    assert_eq!(f.reads, 3);
    assert_eq!(f.other, 0);
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn a_later_index_reads_on_up_to_it_and_earlier_ones_are_kept() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(3), Some(0x3000));
        assert_eq!(p.get(1), Some(0x1000));
        assert_eq!(p.get(2), Some(0x2000));
        assert_eq!(p.get(3), Some(0x3000));
        assert_eq!(p.get(1), Some(0x1000));
    }
    assert_eq!(f.reads, 3, "each slot of the va_list is read once");
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn the_limit_is_nl_argmax() {
    assert_eq!(ARG_MAX, 4096);
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(ARG_MAX), Some(0x1000 * ARG_MAX));
        assert_eq!(p.get(ARG_MAX + 1), None);
        assert_eq!(p.get(usize::MAX), None);
        assert_eq!(p.get(1), Some(0x1000));
    }
    assert_eq!(f.reads, ARG_MAX, "a position past the limit reads nothing");
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn past_the_limit_without_prior_reads_reads_nothing() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(ARG_MAX + 1), None);
        assert_eq!(p.get(usize::MAX), None);
    }
    assert_eq!(f.reads, 0);
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn index_zero_has_no_pointer_and_reads_nothing() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(0), None);
        assert_eq!(p.get(1), Some(0x1000));
    }
    assert_eq!(f.reads, 1);
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn the_full_register_slot_is_kept() {
    struct Wide;
    impl VaSource for Wide {
        fn int(&mut self) -> u64 {
            0xffff_8000_dead_beef
        }
        fn double(&mut self) -> u64 {
            0
        }
        fn long_double(&mut self) -> u128 {
            0
        }
    }
    let mut w = Wide;
    let mut p = Pointers::new(&mut w);
    assert_eq!(p.get(1), Some(0xffff_8000_dead_beef_u64 as usize));
}

#[test]
// Trace: s12_7_c_3 (pointer arguments for successive and positional conversions; D-12.13).
fn many_positions_keep_their_values_in_any_order() {
    let mut f = Fake { reads: 0, other: 0 };
    {
        let mut p = Pointers::new(&mut f);
        assert_eq!(p.get(9), Some(0x9000));
        for n in (1..=9).rev() {
            assert_eq!(p.get(n), Some(0x1000 * n));
        }
        assert_eq!(p.get(300), Some(0x1000 * 300));
        for n in [1, 8, 9, 10, 17, 299, 300] {
            assert_eq!(p.get(n), Some(0x1000 * n));
        }
    }
    assert_eq!(f.reads, 300);
}
