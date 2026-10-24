//! Translation for spec §10: the floating-point environment (§10.6, D-10.5). The math functions
//! themselves are generated (`s10_math_gen.rs`, `cargo xtask math-gen`).
//!
//! The logic is `liella_rt::fpu`, which keeps the SSE and x87 units in step; these exports only
//! convert. `FE_DFL_ENV`, `FE_NOMASK_ENV` and `FE_DFL_MODE` arrive as the addresses −1 and −2
//! (`conv::fe_arg`). The trap functions return the previously enabled set, as glibc documents
//! (`fenv(3)`), not a status.

use liella_abi::ctypes::c_int;
use liella_abi::layout::{femode_t, fenv_t, fexcept_t};
use liella_rt::fpu::{self, Flags, Rounding};

use crate::conv;
use crate::export;
use crate::mathret as mret;
use crate::sys::CSys;

export! { libm "GLIBC_2.2.5";
pub extern "C" fn feclearexcept(excepts: c_int) -> c_int {
    mret::done(|| fpu::clear(&CSys, Flags::from_c(excepts)))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn feraiseexcept(excepts: c_int) -> c_int {
    mret::done(|| fpu::raise_c(&CSys, Flags::from_c(excepts)))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fetestexcept(excepts: c_int) -> c_int {
    fpu::test(&CSys, Flags::from_c(excepts)).to_c()
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fegetexceptflag(flagp: Option<&mut fexcept_t>, excepts: c_int) -> c_int {
    mret::store_fexcept(flagp, fpu::test(&CSys, Flags::from_c(excepts)))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fesetexceptflag(flagp: Option<&fexcept_t>, excepts: c_int) -> c_int {
    mret::done(|| fpu::restore_flags(&CSys, mret::fexcept(flagp), Flags::from_c(excepts)))
}}

export! { libm "GLIBC_2.25";
pub extern "C" fn fetestexceptflag(flagp: Option<&fexcept_t>, excepts: c_int) -> c_int {
    mret::fexcept(flagp).and(Flags::from_c(excepts)).to_c()
}}

export! { libm "GLIBC_2.25";
pub extern "C" fn fesetexcept(excepts: c_int) -> c_int {
    mret::done(|| fpu::set(&CSys, Flags::from_c(excepts)))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fegetround() -> c_int {
    fpu::rounding(&CSys).to_c()
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fesetround(round: c_int) -> c_int {
    mret::set_round(Rounding::from_c(round))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fegetenv(envp: Option<&mut fenv_t>) -> c_int {
    mret::store_env(envp, fpu::get_env(&CSys))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fesetenv(envp: *const fenv_t) -> c_int {
    // UNSAFE(slice) SAFETY: envp is FE_DFL_ENV, FE_NOMASK_ENV or an environment (§10.6).
    mret::done(|| fpu::set_env(&CSys, &mret::env(unsafe { conv::fe_arg(envp) })))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn feholdexcept(envp: Option<&mut fenv_t>) -> c_int {
    mret::store_env(envp, fpu::hold(&CSys))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn feupdateenv(envp: *const fenv_t) -> c_int {
    // UNSAFE(slice) SAFETY: envp is FE_DFL_ENV, FE_NOMASK_ENV or an environment (§10.6).
    mret::done(|| fpu::update(&CSys, &mret::env(unsafe { conv::fe_arg(envp) })))
}}

export! { libm "GLIBC_2.25";
pub extern "C" fn fegetmode(modep: Option<&mut femode_t>) -> c_int {
    mret::store_mode(modep, fpu::get_mode(&CSys))
}}

export! { libm "GLIBC_2.25";
pub extern "C" fn fesetmode(modep: *const femode_t) -> c_int {
    // UNSAFE(slice) SAFETY: modep is FE_DFL_MODE or control modes (§10.6).
    mret::done(|| fpu::set_mode(&CSys, &mret::modes(unsafe { conv::fe_arg(modep) })))
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn feenableexcept(excepts: c_int) -> c_int {
    fpu::enable(&CSys, Flags::from_c(excepts)).to_c()
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fedisableexcept(excepts: c_int) -> c_int {
    fpu::disable(&CSys, Flags::from_c(excepts)).to_c()
}}

export! { libm "GLIBC_2.2.5";
pub extern "C" fn fegetexcept() -> c_int {
    fpu::enabled(&CSys).to_c()
}}
