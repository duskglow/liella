//! `liella_ld::dlopen`'s rules that need no process (phase 14 DL-16a, plan/ph14/dl-16-dlopen.md §1;
//! spec/behaviour/dynlink.md "dlerror texts", probes P-1 and the gaps probe; rulings #5294): the order of
//! the checks on a candidate file, the refusals of malformed headers, the mode word's precedence and
//! the open count's saturation. The candidates are mutated copies of this test's own executable (a
//! position-independent executable, as the x86-64 toolchain links it).
// ARCH(x86_64): the candidate is this x86-64 test program, checked as a native object.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]

use liella_ld::arch::Native;
use liella_ld::dlopen::{check, mode, reopen, ModeError, Refusal, KNOWN, RTLD_LAZY, RTLD_NOW};

const PAGE: u64 = 4096;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;

fn exe() -> Vec<u8> {
    std::fs::read(std::env::current_exe().unwrap()).unwrap()
}

fn u16_at(f: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(f[o..o + 2].try_into().unwrap())
}

/// The file offset of the first program header of type `t`.
fn phdr(f: &[u8], t: u32) -> usize {
    let off = u64::from_le_bytes(f[32..40].try_into().unwrap()) as usize;
    let n = usize::from(u16_at(f, 56));
    (0..n).map(|i| off + i * 56).find(|&p| f[p..p + 4] == t.to_le_bytes()).unwrap()
}

fn verdict(f: &[u8]) -> Refusal {
    check::<Native>(f, PAGE).err().unwrap()
}

/// s27_1_f_0 (dlopen fails with a text): each check in the probed order; a file failing an earlier
/// check and a later one gets the earlier one's refusal. `e_ehsize` other than 64 and a `p_align`
/// that is not a power of two are refused as malformed (ruling #5294, the differences rows).
#[test]
fn s27_1_f_0_check_order_and_malformed_headers() {
    let base = exe();
    // The untouched executable reaches the last stage that refuses it: a PIE.
    assert_eq!(verdict(&base), Refusal::PieExecutable);
    let mut f = base.clone();
    f[52..54].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Malformed, "e_ehsize 0");
    // The header outranks the executable type.
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Malformed, "e_ehsize 0 and ET_EXEC");
    f[52..54].copy_from_slice(&64u16.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Executable, "ET_EXEC");
    // `p_align` is a header rule of the reader: it outranks the executable type too.
    let load = phdr(&base, PT_LOAD);
    let mut f = base.clone();
    f[load + 48..load + 56].copy_from_slice(&3u64.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Malformed, "PT_LOAD p_align 3");
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Malformed, "p_align 3 and ET_EXEC");
    // The executable type outranks the segment layout, which outranks the dynamic section and the
    // PIE flag: a first PT_LOAD whose p_vaddr and p_offset differ modulo the page.
    let mut f = base.clone();
    let v = u64::from_le_bytes(f[load + 16..load + 24].try_into().unwrap()) + 1;
    f[load + 16..load + 24].copy_from_slice(&v.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Segments, "p_vaddr off the page of p_offset");
    f[16..18].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::Executable, "misaligned PT_LOAD and ET_EXEC");
    let dynamic = phdr(&base, PT_DYNAMIC);
    let mut f = base.clone();
    f[dynamic..dynamic + 4].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(verdict(&f), Refusal::NoDynamic, "no PT_DYNAMIC, before the PIE flag");
    assert_eq!(Refusal::Malformed.text(), b"malformed ELF object");
}

/// s27_1_f_0 (probe P-1): an unknown bit is refused before the binding mode is looked at, so a mode
/// with an unknown bit and no binding bit gets `invalid mode parameter`, not the binding text.
#[test]
fn s27_1_f_0_unknown_mode_bit_outranks_a_missing_binding() {
    for bit in 0..32 {
        let m = (1u32 << bit) as i32;
        if m & KNOWN == 0 {
            assert_eq!(mode(m), Err(ModeError::Unknown), "bit {bit} alone");
        }
    }
    assert_eq!(mode(0), Err(ModeError::NoBinding));
    assert!(mode(RTLD_LAZY | RTLD_NOW).is_ok());
    assert!(!ModeError::Unknown.prefixed() && ModeError::NoBinding.prefixed());
}

/// no spec clause (plan §1, D-27.32): the open count saturates at `u32::MAX` and pins the object, so
/// a wrapped count can never unload a live object.
#[test]
fn open_count_saturates_and_pins() {
    assert_eq!(reopen(0), (1, false));
    assert_eq!(reopen(u32::MAX - 2), (u32::MAX - 1, false));
    assert_eq!(reopen(u32::MAX - 1), (u32::MAX, true));
    assert_eq!(reopen(u32::MAX), (u32::MAX, true));
}
