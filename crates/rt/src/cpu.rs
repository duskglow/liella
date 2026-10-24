//! CPU feature decoding from `Machine::cpuid`/`xcr0` (spec §27.5, IP-26).
//!
//! Features are decoded once from the CPUID leaves and the OS-enabled state in XCR0 (a feature
//! that needs AVX or AVX-512 register state counts only if the OS saves that state). The
//! x86-64 microarchitecture levels select the hwcaps build variants (P4).
//!
//! The value keeps the raw registers; each query is one bit test on them, so the boundary can
//! cache the whole value once at startup (D-22.12 (6)).

use liella_kernel::machine::Machine;

/// Decoded CPU features.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Features {
    /// Raw CPUID leaf 1 ECX/EDX and leaf 7 (subleaf 0) EBX/ECX, and leaf 0x80000001 ECX.
    pub leaf1_ecx: u32,
    /// CPUID.1:EDX.
    pub leaf1_edx: u32,
    /// CPUID.(7,0):EBX.
    pub leaf7_ebx: u32,
    /// CPUID.(7,0):ECX.
    pub leaf7_ecx: u32,
    /// CPUID leaf 7 (subleaf 0) EDX (FSRM).
    pub leaf7_edx: u32,
    /// CPUID.80000001h:ECX.
    pub ext1_ecx: u32,
    /// OS-enabled state components (XCR0), 0 if `xgetbv` is unavailable.
    pub xcr0: u64,
}

/// Bit `b` of `v`.
#[inline]
fn bit(v: u32, b: u32) -> bool {
    v.checked_shr(b).is_some_and(|x| x & 1 != 0)
}

/// XCR0 components for SSE (1) and AVX (2) state.
const XCR0_AVX: u64 = 0b110;
/// XCR0 components for AVX-512: SSE and AVX, plus opmask (5), ZMM_Hi256 (6), Hi16_ZMM (7).
const XCR0_AVX512: u64 = 0b1110_0110;

/// CPUID.1:ECX bits.
mod l1c {
    pub(super) const SSE3: u32 = 0;
    pub(super) const SSSE3: u32 = 9;
    pub(super) const FMA: u32 = 12;
    pub(super) const CX16: u32 = 13;
    pub(super) const SSE41: u32 = 19;
    pub(super) const SSE42: u32 = 20;
    pub(super) const MOVBE: u32 = 22;
    pub(super) const POPCNT: u32 = 23;
    pub(super) const AVX: u32 = 28;
    pub(super) const F16C: u32 = 29;
}
/// CPUID.(7,0):EBX bits.
mod l7b {
    pub(super) const BMI1: u32 = 3;
    pub(super) const AVX2: u32 = 5;
    pub(super) const BMI2: u32 = 8;
    pub(super) const ERMS: u32 = 9;
    pub(super) const AVX512F: u32 = 16;
    pub(super) const AVX512DQ: u32 = 17;
    pub(super) const AVX512CD: u32 = 28;
    pub(super) const AVX512BW: u32 = 30;
    pub(super) const AVX512VL: u32 = 31;
}
/// CPUID.1:EDX.SSE2.
const L1D_SSE2: u32 = 26;
/// CPUID.7:EDX.FSRM (fast short `rep movsb`).
const L7D_FSRM: u32 = 4;
/// CPUID.80000001h:ECX bits.
const E1C_LAHF: u32 = 0;
const E1C_LZCNT: u32 = 5;

impl Features {
    /// Read the CPU.
    pub fn detect(m: &impl Machine) -> Features {
        // At most five queries (startup cost); leaves past the reported maximum are not read,
        // as real parts answer them with another leaf's data.
        let [max, ..] = m.cpuid(0, 0);
        let [_, _, leaf1_ecx, leaf1_edx] = if max >= 1 { m.cpuid(1, 0) } else { [0; 4] };
        let [_, leaf7_ebx, leaf7_ecx, leaf7_edx] = if max >= 7 { m.cpuid(7, 0) } else { [0; 4] };
        let [max_ext, ..] = m.cpuid(0x8000_0000, 0);
        let [_, _, ext1_ecx, _] = if max_ext >= 0x8000_0001 { m.cpuid(0x8000_0001, 0) } else { [0; 4] };
        let xcr0 = m.xcr0().unwrap_or(0);
        Features { leaf1_ecx, leaf1_edx, leaf7_ebx, leaf7_ecx, leaf7_edx, ext1_ecx, xcr0 }
    }

    /// SSE2 (baseline for x86-64).
    pub fn sse2(&self) -> bool {
        bit(self.leaf1_edx, L1D_SSE2)
    }
    /// SSE3.
    pub fn sse3(&self) -> bool {
        bit(self.leaf1_ecx, l1c::SSE3)
    }
    /// SSSE3.
    pub fn ssse3(&self) -> bool {
        bit(self.leaf1_ecx, l1c::SSSE3)
    }
    /// SSE4.1.
    pub fn sse41(&self) -> bool {
        bit(self.leaf1_ecx, l1c::SSE41)
    }
    /// SSE4.2.
    pub fn sse42(&self) -> bool {
        bit(self.leaf1_ecx, l1c::SSE42)
    }
    /// POPCNT.
    pub fn popcnt(&self) -> bool {
        bit(self.leaf1_ecx, l1c::POPCNT)
    }
    /// CMPXCHG16B.
    pub fn cx16(&self) -> bool {
        bit(self.leaf1_ecx, l1c::CX16)
    }
    /// LAHF/SAHF in 64-bit mode.
    pub fn lahf(&self) -> bool {
        bit(self.ext1_ecx, E1C_LAHF)
    }
    /// LZCNT (ABM).
    pub fn lzcnt(&self) -> bool {
        bit(self.ext1_ecx, E1C_LZCNT)
    }
    /// MOVBE.
    pub fn movbe(&self) -> bool {
        bit(self.leaf1_ecx, l1c::MOVBE)
    }
    /// FMA3 (VEX-encoded: needs the OS-enabled AVX state).
    pub fn fma(&self) -> bool {
        bit(self.leaf1_ecx, l1c::FMA) && self.avx_state()
    }
    /// F16C (VEX-encoded: needs the OS-enabled AVX state).
    pub fn f16c(&self) -> bool {
        bit(self.leaf1_ecx, l1c::F16C) && self.avx_state()
    }
    /// XSAVE state for SSE and AVX is enabled by the OS.
    fn avx_state(&self) -> bool {
        self.xcr0 & XCR0_AVX == XCR0_AVX
    }
    /// AVX-512 state (opmask, upper ZMM) enabled by the OS.
    fn avx512_state(&self) -> bool {
        self.xcr0 & XCR0_AVX512 == XCR0_AVX512
    }
    /// AVX.
    pub fn avx(&self) -> bool {
        bit(self.leaf1_ecx, l1c::AVX) && self.avx_state()
    }
    /// AVX2.
    pub fn avx2(&self) -> bool {
        bit(self.leaf7_ebx, l7b::AVX2) && self.avx_state()
    }
    /// BMI1.
    pub fn bmi1(&self) -> bool {
        bit(self.leaf7_ebx, l7b::BMI1)
    }
    /// BMI2.
    pub fn bmi2(&self) -> bool {
        bit(self.leaf7_ebx, l7b::BMI2)
    }
    /// AVX-512 F, BW, CD, DQ, VL (the x86-64-v4 set).
    pub fn avx512_v4(&self) -> bool {
        let set = [l7b::AVX512F, l7b::AVX512BW, l7b::AVX512CD, l7b::AVX512DQ, l7b::AVX512VL];
        set.iter().all(|&b| bit(self.leaf7_ebx, b)) && self.avx512_state()
    }
    /// Enhanced `rep movsb`/`rep stosb` (CPUID.7:EBX.ERMS).
    pub fn erms(&self) -> bool {
        bit(self.leaf7_ebx, l7b::ERMS)
    }
    /// Fast short `rep movsb` (CPUID.7:EDX.FSRM).
    pub fn fsrm(&self) -> bool {
        bit(self.leaf7_edx, L7D_FSRM)
    }
    /// AVX-512 F with the OS-enabled state.
    pub fn avx512f(&self) -> bool {
        bit(self.leaf7_ebx, l7b::AVX512F) && self.avx512_state()
    }

    /// The highest x86-64 microarchitecture level supported (1 to 4), which selects the hwcaps
    /// build variant: v2 (CX16, LAHF, POPCNT, SSE3, SSE4.1, SSE4.2, SSSE3), v3 (adds AVX, AVX2,
    /// BMI1, BMI2, F16C, FMA, LZCNT, MOVBE, OS-enabled AVX state), v4 (adds AVX-512 F/BW/CD/DQ/VL
    /// with OS-enabled state).
    pub fn level(&self) -> u8 {
        let v2 = self.cx16() && self.lahf() && self.popcnt() && self.sse3() && self.sse41() && self.sse42() && self.ssse3();
        // `avx`, `avx2`, `fma` and `f16c` already require the OS-enabled AVX state.
        let v3 = v2 && self.avx() && self.avx2() && self.bmi1() && self.bmi2() && self.f16c() && self.fma() && self.lzcnt() && self.movbe();
        let v4 = v3 && self.avx512_v4();
        match (v2, v3, v4) {
            (_, _, true) => 4,
            (_, true, _) => 3,
            (true, _, _) => 2,
            _ => 1,
        }
    }
}

/// Number of leaves `__x86_get_cpuid_feature_leaf` reports (indices `0..FEATURE_LEAVES`).
pub const FEATURE_LEAVES: usize = 10;

/// One reported leaf, `struct cpuid_feature` of `<sys/platform/x86.h>` (32 bytes, align 4):
/// the raw CPUID words `[eax, ebx, ecx, edx]`, then the "active" words in the same order.
pub type FeatureLeaf = [u32; 8];

/// CPUID (leaf, subleaf) behind each index, as the header's `CPU_FEATURE_PRESENT` macros
/// compile them (probe X86-CPUID-FEATURE-LEAF): 1, 7.0, 0x80000001, 0xd.1, 0x80000007,
/// 0x80000008, 7.1, 0x19, 0x14.0, 0x24.
const LEAF_OF: [(u32, u32); FEATURE_LEAVES] = [
    (1, 0), (7, 0), (0x8000_0001, 0), (0xd, 1), (0x8000_0007, 0), (0x8000_0008, 0), (7, 1), (0x19, 0), (0x14, 0), (0x24, 0),
];

/// Bits that are active whenever present, per index and register (probed on four CPUs; bits
/// never seen present on a probed CPU are left inactive, the safe answer).
const ACTIVE_ALWAYS: [[u32; 4]; FEATURE_LEAVES] = [
    // 1: SSE3 PCLMULQDQ SSSE3 CX16 SSE4.1 SSE4.2 MOVBE POPCNT AES XSAVE OSXSAVE RDRAND;
    //    TSC CX8 CMOV CLFSH MMX FXSR SSE SSE2 HTT.
    [0, 0, 0x4ed8_2203, 0x1788_8110],
    // 7.0: FSGSBASE BMI1 BMI2 ERMS RDSEED ADX CLFLUSHOPT CLWB SHA; PKU OSPKE.
    [0, 0x218c_0309, 0x18, 0],
    // 0x80000001: LAHF LZCNT SSE4A PREFETCHW; RDTSCP.
    [0, 0, 0x161, 0x0800_0000],
    // 0xd.1: XSAVEOPT XSAVEC XGETBV_ECX_1 (XSAVES stays inactive).
    [0x7, 0, 0, 0],
    [0; 4],
    [0; 4],
    [0; 4],
    [0; 4],
    [0; 4],
    [0; 4],
];

/// Leaf-1 ECX bits active only with OS-enabled AVX state: FMA, AVX, F16C.
const L1C_AVX_STATE: u32 = 0x3000_1000;
/// Leaf-7 EBX AVX2, active only with OS-enabled AVX state.
const L7B_AVX_STATE: u32 = 1 << l7b::AVX2;
/// Leaf-7 EBX AVX-512 F/DQ/CD/BW/VL, active only with OS-enabled AVX-512 state.
const L7B_AVX512_STATE: u32 = 0xd003_0000;
/// Leaf-7 EBX HLE and RTM.
const L7B_TSX: u32 = (1 << 4) | (1 << 11);
/// Leaf-7 EDX RTM_ALWAYS_ABORT (11) and TSX_FORCE_ABORT (13).
const L7D_RTM_ALWAYS_ABORT: u32 = 1 << 11;
const L7D_TSX_FORCE_ABORT: u32 = 1 << 13;

/// The `__x86_get_cpuid_feature_leaf` table (spec §27.5): raw CPUID words of each leaf (zero
/// for a leaf or subleaf past the CPU's reported maximum) and its "active" (usable) bits.
///
/// Active rules (probe X86-CPUID-FEATURE-LEAF, four CPUs): the `ACTIVE_ALWAYS` bits when
/// present; FMA/AVX/F16C/AVX2 only with XCR0 SSE+AVX state; AVX-512 F/DQ/CD/BW/VL only with
/// XCR0 AVX-512 state; HLE/RTM unless the CPU reports RTM_ALWAYS_ABORT or TSX_FORCE_ABORT, in
/// which case they are inactive and RTM_ALWAYS_ABORT is reported active instead.
pub fn feature_leaves(m: &impl Machine) -> [FeatureLeaf; FEATURE_LEAVES] {
    let [max, ..] = m.cpuid(0, 0);
    let [max_ext, ..] = m.cpuid(0x8000_0000, 0);
    let mut out = [[0u32; 8]; FEATURE_LEAVES];
    let mut sub7_max = 0;
    for (i, &(leaf, sub)) in LEAF_OF.iter().enumerate() {
        let top = if leaf >= 0x8000_0000 { max_ext } else { max };
        let listed = leaf <= top && (leaf != 7 || sub <= sub7_max);
        let regs = if listed { m.cpuid(leaf, sub) } else { [0; 4] };
        if (leaf, sub) == (7, 0) {
            sub7_max = regs[0];
        }
        let always = ACTIVE_ALWAYS.get(i).copied().unwrap_or_default();
        if let Some(e) = out.get_mut(i) {
            let [a0, a1, a2, a3] = always;
            let [r0, r1, r2, r3] = regs;
            *e = [r0, r1, r2, r3, r0 & a0, r1 & a1, r2 & a2, r3 & a3];
        }
    }
    let xcr0 = m.xcr0().unwrap_or(0);
    let avx = xcr0 & XCR0_AVX == XCR0_AVX;
    let avx512 = xcr0 & XCR0_AVX512 == XCR0_AVX512;
    let [l1, l7, ..] = &mut out;
    if avx {
        l1[6] |= l1[2] & L1C_AVX_STATE;
        l7[5] |= l7[1] & L7B_AVX_STATE;
    }
    if avx512 {
        l7[5] |= l7[1] & L7B_AVX512_STATE;
    }
    if l7[3] & (L7D_RTM_ALWAYS_ABORT | L7D_TSX_FORCE_ABORT) != 0 {
        l7[7] |= L7D_RTM_ALWAYS_ABORT;
    } else {
        l7[5] |= l7[1] & L7B_TSX;
    }
    out
}
