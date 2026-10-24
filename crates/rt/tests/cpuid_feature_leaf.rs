//! `__x86_get_cpuid_feature_leaf` table (spec §27.5): the raw CPUID words and active bits that
//! glibc 2.39 reported on four CPUs (probe X86-CPUID-FEATURE-LEAF, 2026-10-22), replayed through
//! a fake CPU that answers with the same raw words.

use liella_kernel::machine::{Machine, X87Env};
use liella_rt::cpu::{feature_leaves, FeatureLeaf, FEATURE_LEAVES};

struct Cpu {
    max: u32,
    leaves: Vec<((u32, u32), [u32; 4])>,
    xcr0: u64,
}

impl Machine for Cpu {
    fn thread_pointer(&self) -> usize { 0 }
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        match leaf {
            0 => [self.max, 0, 0, 0],
            0x8000_0000 => [0x8000_0008, 0, 0, 0],
            _ => self.leaves.iter().find(|(k, _)| *k == (leaf, subleaf)).map_or([0; 4], |(_, r)| *r),
        }
    }
    fn xcr0(&self) -> Option<u64> { Some(self.xcr0) }
    fn mxcsr(&self) -> u32 { 0x1f80 }
    fn set_mxcsr(&self, _: u32) {}
    fn x87_control(&self) -> u16 { 0x37f }
    fn set_x87_control(&self, _: u16) {}
    fn x87_status(&self) -> u16 { 0 }
    fn x87_clear_exceptions(&self) {}
    fn x87_store_env(&self) -> X87Env { X87Env::default() }
    fn x87_load_env(&self, _: &X87Env) {}
    fn pkru(&self) -> Option<u32> { None }
    fn set_pkru(&self, _: u32) -> bool { false }
    fn barrier(&self, _: &[u8]) {}
}

const KEYS: [(u32, u32); FEATURE_LEAVES] =
    [(1, 0), (7, 0), (0x8000_0001, 0), (0xd, 1), (0x8000_0007, 0), (0x8000_0008, 0), (7, 1), (0x19, 0), (0x14, 0), (0x24, 0)];

/// Build the fake from the probed table (its raw words) and check the decoded table equals it.
fn replay(name: &str, max: u32, xcr0: u64, rows: &[(usize, FeatureLeaf)], extra: &[((u32, u32), [u32; 4])]) {
    let mut want = [[0u32; 8]; FEATURE_LEAVES];
    let mut leaves = extra.to_vec();
    for &(i, row) in rows {
        if let (Some(w), Some(&k)) = (want.get_mut(i), KEYS.get(i)) {
            *w = row;
            let [a, b, c, d, ..] = row;
            leaves.push((k, [a, b, c, d]));
        }
    }
    let got = feature_leaves(&Cpu { max, leaves, xcr0 });
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert_eq!(g, w, "{name}: index {i}: got {g:08x?} want {w:08x?}");
    }
}

/// Word `w` of entry `i`.
fn word(t: &[FeatureLeaf; FEATURE_LEAVES], i: usize, w: usize) -> u32 {
    t.get(i).and_then(|e| e.get(w)).copied().unwrap_or(0)
}

#[test]
fn sumire_skylake_avx512_tsx_force_abort() {
    // no clause ID (spec §27.5 states no clause for the table; probe X86-CPUID-FEATURE-LEAF)
    // spec §27.5: AVX-512 active with XCR0 AVX-512 state; RTM/HLE present but inactive under
    // TSX_FORCE_ABORT, RTM_ALWAYS_ABORT reported active; XSAVES present but inactive.
    replay("sumire", 0x16, 0x2ff, &[
        (0, [0x0005_0654, 0x5640_0800, 0x7ffe_fbff, 0xbfeb_fbff, 0, 0, 0x7ed8_3203, 0x1788_8110]),
        (1, [0, 0xd39f_fffb, 0x18, 0xbc00_2400, 0, 0xd18f_0329, 0x18, 0x800]),
        (2, [0, 0, 0x121, 0x2c10_0800, 0, 0, 0x121, 0x0800_0000]),
        (3, [0xf, 0xa08, 0x100, 0, 0x7, 0, 0, 0]),
        (4, [0, 0, 0, 0x100, 0, 0, 0, 0]),
        (5, [0x302e, 0, 0, 0, 0, 0, 0, 0]),
        (8, [1, 0xf, 7, 0, 0, 0, 0, 0]),
    ], &[((0x19, 0), [0xd48, 0xfa0, 0x64, 0]), ((0x24, 0), [0xd48, 0xfa0, 0x64, 0]), ((7, 1), [1, 1, 1, 1])]);
}

#[test]
fn shiki_haswell() {
    // no clause ID (spec §27.5 states no clause for the table; probe X86-CPUID-FEATURE-LEAF)
    // spec §27.5
    replay("shiki", 0xd, 0x7, &[
        (0, [0x0003_06f2, 0x1720_0800, 0xfffa_b223, 0x1f8b_fbff, 0, 0, 0x7ed8_3203, 0x1788_8110]),
        (1, [0, 0x27ab, 4, 0xbc00_0400, 0, 0x329, 0, 0]),
        (2, [0x0003_06f2, 0, 0x21, 0x2c10_0800, 0, 0, 0x21, 0x0800_0000]),
        (3, [1, 0x340, 0, 0, 1, 0, 0, 0]),
        (5, [0x002e_302e, 0x0100_d000, 0, 0, 0, 0, 0, 0]),
    ], &[]);
}

#[test]
fn kanon_broadwell_tsx_active() {
    // no clause ID (spec §27.5 states no clause for the table; probe X86-CPUID-FEATURE-LEAF)
    // spec §27.5: HLE and RTM active without an abort bit.
    replay("kanon", 0xd, 0x7, &[
        (0, [0x0004_06f1, 0x0f20_0800, 0xfffa_b223, 0x1f8b_fbff, 0, 0, 0x7ed8_3203, 0x1788_8110]),
        (1, [0, 0x001c_2fbb, 4, 0xbc00_0400, 0, 0x000c_0b39, 0, 0]),
        (2, [0x0004_06f1, 0, 0x121, 0x2c10_0800, 0, 0, 0x121, 0x0800_0000]),
        (3, [1, 0x340, 0, 0, 1, 0, 0, 0]),
        (5, [0x002e_302e, 0x0100_d000, 0, 0, 0, 0, 0, 0]),
    ], &[]);
}

#[test]
fn keke_epyc_zen1() {
    // no clause ID (spec §27.5 states no clause for the table; probe X86-CPUID-FEATURE-LEAF)
    // spec §27.5: SHA and SSE4A active; CLZERO etc. (0x80000008) present only.
    replay("keke", 0xd, 0x7, &[
        (0, [0x0080_0f12, 0x1728_0800, 0xfff8_3203, 0x178b_fbff, 0, 0, 0x7ed8_3203, 0x1788_8110]),
        (1, [0, 0x209c_01ab, 0, 0, 0, 0x208c_0129, 0, 0]),
        (2, [0x0080_0f12, 0, 0x0080_03f7, 0x2fd3_fbff, 0, 0, 0x161, 0x0800_0000]),
        (3, [7, 0x340, 0, 0, 7, 0, 0, 0]),
        (4, [0, 3, 0, 0, 0, 0, 0, 0]),
        (5, [0x0030_3030, 0x0200_1005, 0x6027, 0, 0, 0, 0, 0]),
    ], &[]);
}

#[test]
fn avx_bits_need_os_state() {
    // no clause ID (spec §27.5 states no clause for the table; probe X86-CPUID-FEATURE-LEAF)
    // spec §27.5: "active" requires OS support via XCR0.
    let row0 = [0x0005_0654, 0x5640_0800, 0x7ffe_fbff, 0xbfeb_fbff];
    let row1 = [0, 0xd39f_fffb, 0x18, 0];
    let cpu = |xcr0| Cpu { max: 7, leaves: vec![((1, 0), row0), ((7, 0), row1)], xcr0 };
    let sse_only = feature_leaves(&cpu(0x3));
    assert_eq!(word(&sse_only, 0, 6) & 0x3000_1000, 0, "FMA/AVX/F16C need AVX state");
    assert_eq!(word(&sse_only, 1, 5) & 0xd003_0020, 0, "AVX2 and AVX-512 need their state");
    let avx_only = feature_leaves(&cpu(0x7));
    assert_eq!(word(&avx_only, 1, 5) & 0xd003_0020, 0x20, "AVX-512 needs opmask/ZMM state");
    assert_eq!(word(&avx_only, 1, 5) & 0x810, 0x810, "HLE/RTM active without an abort bit");
}
