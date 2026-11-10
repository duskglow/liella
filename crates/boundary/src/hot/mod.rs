//! Hot kernels and the libm boundary wrappers (plan S2 category *hot-kernel*, D-4.22): one
//! module, split by family (LIBM-HOT-PER-FAMILY) so that independent stories edit different files.
//!
//! * `mem`: the small-size copy, fill and compare kernels behind `memcpy`, `memmove`, `mempcpy`,
//!   `memset`, `memcmp`, `bcmp` and `__memcmpeq`, and their AVX2 builds (`avx2`); the KERNEL
//!   CONTRACT is at the top of that file.
//! * `delim`: the `getdelim` fused scan and copy.
//! * `fpregs`: the libm envelope's machine-register intrinsics (category *machine-register*,
//!   D-X.44): the inline MXCSR read and the empty `xmm` barrier, the only inline assembly here.
//! * one file per libm family (`exp_log`, `trig`, `invtrig`, `hyperbolic`, `erf`, `gamma`,
//!   `bessel`, and `f32_<family>` for the `float` ones): the build-dispatching wrappers of that
//!   family's fast paths.
//!
//! FLOW RULE: a libm story adds its boundary wrappers in its family's file, never here. This
//! file holds only what two or more families share (the inline square root below) and the
//! re-exports, so every item keeps its `crate::hot::<name>` path.

mod bessel;
mod delim;
mod erf;
mod exp_log;
mod f32_bessel;
mod f32_erf;
mod f32_exp_log;
mod f32_gamma;
mod f32_hyperbolic;
mod f32_invtrig;
mod f32_trig;
pub mod fpregs;
mod gamma;
mod hyperbolic;
mod invtrig;
mod mem;
mod trig;

pub(crate) use delim::*;
pub(crate) use exp_log::*;
pub(crate) use hyperbolic::*;
pub(crate) use invtrig::*;
pub use mem::*;

// The families with no wrapper yet: declared here once so that a story adding the first one
// edits only its family's file (the `allow` does nothing once a family has items).
#[allow(unused_imports)]
pub(crate) use {bessel::*, erf::*, f32_bessel::*, f32_erf::*, f32_exp_log::*, f32_gamma::*, f32_hyperbolic::*, f32_invtrig::*, f32_trig::*, gamma::*, trig::*};

/// `sqrtsd` for the family kernels (`invtrig`, `hyperbolic`) (correctly rounded; they run in the default MXCSR), issued
/// inline. The `core::arch` intrinsics are safe only inside a `#[target_feature]` function on
/// the pinned compiler (a plain function, even with SSE2 in the target baseline, needs
/// `unsafe`), so the root lives in one: the SSE2 lanes' upper half is `+0`, whose root is exact
/// and raises nothing. LLVM inlines it into callers with at least SSE2 (every x86-64 function,
/// and the FMA builds), so no call is left (the out-of-line `liella_machine_sqrtsd` cost a call,
/// a return and the caller-saved XMM spills around it).
#[target_feature(enable = "sse2")]
#[inline]
fn sqrtsd_sse2(x: f64) -> f64 {
    // ARCH(x86_64): SSE2 square root (hot-kernel); aarch64 needs `fsqrt` or a portable root (A-8)
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_set_sd, _mm_sqrt_pd};
    _mm_cvtsd_f64(_mm_sqrt_pd(_mm_set_sd(x)))
}

/// [`sqrtsd_sse2`] from a function without `#[target_feature]`. Also `sqrt`'s in-place root
/// (`mathret`, D-10.71): the caller pins the operand behind its mode test first.
#[inline(always)]
pub(crate) fn sqrtsd(x: f64) -> f64 {
    // UNSAFE(hot-kernel) SAFETY: the callee needs only SSE2, which the x86-64 baseline
    // guarantees on every CPU this library runs on.
    #[allow(unsafe_code)]
    unsafe {
        sqrtsd_sse2(x)
    }
}

/// `sqrtss` issued inline, [`sqrtsd_sse2`]'s `float` form: the upper lanes are `+0`, whose root
/// is exact and raises nothing in any MXCSR mode (FTZ and DAZ included). `sqrtf`'s in-place root
/// (`mathret`, D-10.71).
#[target_feature(enable = "sse")]
#[inline]
fn sqrtss_sse(x: f32) -> f32 {
    // ARCH(x86_64): SSE square root (hot-kernel); aarch64 needs `fsqrt` or a portable root (A-8)
    use core::arch::x86_64::{_mm_cvtss_f32, _mm_set_ss, _mm_sqrt_ps};
    _mm_cvtss_f32(_mm_sqrt_ps(_mm_set_ss(x)))
}

/// [`sqrtss_sse`] from a function without `#[target_feature]`.
#[inline(always)]
pub(crate) fn sqrtss(x: f32) -> f32 {
    // UNSAFE(hot-kernel) SAFETY: the callee needs only SSE, which the x86-64 baseline
    // guarantees on every CPU this library runs on.
    #[allow(unsafe_code)]
    unsafe {
        sqrtss_sse(x)
    }
}
