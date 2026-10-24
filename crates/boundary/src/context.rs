//! `makecontext` and `__longjmp_chk` (spec §21.1–§21.2; D-21.1–D-21.3): the parts of
//! non-local transfer that need decisions. The register save and restore is the C layer's
//! (`machine/setjmp.S`, `machine/context.S`).

#![allow(clippy::arithmetic_side_effects)] // word offsets inside the caller's context and stack

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella_kernel::errno::KResult;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::SS_ONSTACK;

use crate::sys::CSys;
use crate::{ffi, tb};

// `ucontext_t` as words (spec/abi/layouts.tsv): uc_link 1, uc_stack.ss_sp 2, ss_size 4, the
// general registers from word 5 (`REG_*` order).
const W_LINK: usize = 1;
const W_SS_SP: usize = 2;
const W_SS_SIZE: usize = 4;
const W_GREGS: usize = 5;
const REG_R8: usize = 0;
const REG_R9: usize = 1;
const REG_RDI: usize = 8;
const REG_RSI: usize = 9;
const REG_RBX: usize = 11;
const REG_RDX: usize = 12;
const REG_RCX: usize = 14;
const REG_RSP: usize = 15;
const REG_RIP: usize = 16;
/// The registers that carry the first six arguments.
const ARG_REGS: [usize; 6] = [REG_RDI, REG_RSI, REG_RDX, REG_RCX, REG_R8, REG_R9];

fn get(w: &[AtomicUsize], i: usize) -> usize {
    w.get(i).map_or(0, |x| x.load(Relaxed))
}

fn put(w: &[AtomicUsize], i: usize, v: usize) {
    if let Some(x) = w.get(i) {
        x.store(v, Relaxed);
    }
}

/// Where a context function whose `uc_link` is NULL goes: the thread exits (spec §21.2).
extern "C" fn context_end() -> ! {
    crate::cancel::unwind(0)
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn end_addr() -> usize {
    context_end as extern "C" fn() -> ! as usize
}

#[allow(clippy::as_conversions)] // as above
fn start_context_addr() -> usize {
    ffi::liella_machine_start_context as extern "C" fn() as usize
}

/// `makecontext(uc, func, argc, ...)`: `uc` resumes in `func` with `argc` register-sized
/// arguments on `uc.uc_stack` (glibc accepts 64-bit values on x86-64, §21.2), returning to
/// `uc.uc_link` or ending the thread. `stack` is the stack's words (from its lowest 8-aligned
/// address); `ap` is the adapter's `va_list *`.
pub fn make(uc: KResult<&[AtomicUsize]>, stack: KResult<(usize, &[AtomicUsize])>, func: usize, argc: i32, ap: usize) {
    let (Ok(uc), Ok((base, stack))) = (uc, stack) else { return };
    let Ok(argc) = usize::try_from(argc) else { return };
    let top = get(uc, W_SS_SP) + get(uc, W_SS_SIZE);
    // Above the return address: the arguments past the sixth, then uc_link and the exit
    // function (the trampoline finds them through %rbx).
    let extra = argc.saturating_sub(6);
    let link_idx = extra + 1;
    let Some(sp) = top.checked_sub((link_idx + 2) * 8).map(|v| (v & !15) - 8) else { return };
    if sp < base || (sp - base) / 8 + link_idx + 1 >= stack.len() {
        return;
    }
    let at = |i: usize| (sp - base) / 8 + i;
    put(stack, at(0), start_context_addr());
    put(stack, at(link_idx), get(uc, W_LINK));
    put(stack, at(link_idx + 1), end_addr());
    put(uc, W_GREGS + REG_RIP, func);
    put(uc, W_GREGS + REG_RSP, sp);
    put(uc, W_GREGS + REG_RBX, sp + link_idx * 8);
    for i in 0..argc {
        // One argument per call, exactly argc of them (the obligation of `liella_va_long`).
        let v = usize::from_ne_bytes(ffi::liella_va_long(ap).to_ne_bytes());
        match ARG_REGS.get(i) {
            Some(&r) => put(uc, W_GREGS + r, v),
            None => put(stack, at(i - 5), v),
        }
    }
}

/// `__longjmp_chk(env, val)`: aborts if the jump would land below the current stack pointer,
/// unless it leaves the alternate signal stack for the stack it interrupted (§21.1).
pub fn longjmp_chk(env: KResult<&[AtomicUsize]>, val: i32) -> ! {
    let Ok(env) = env else { crate::process::fortify_fail(b"longjmp causes uninitialized stack frame") };
    let guard = tb::current().pointer_guard.load(Relaxed);
    let target = get(env, 6).rotate_right(0x11) ^ guard;
    let here = core::ptr::from_ref(&guard).expose_provenance();
    if target < here {
        let leaving_altstack = CSys.sigaltstack_get().is_ok_and(|s| s.flags & SS_ONSTACK != 0 && (target < s.sp || target >= s.sp + s.size));
        if !leaving_altstack {
            crate::process::fortify_fail(b"longjmp causes uninitialized stack frame");
        }
    }
    ffi::liella_machine_siglongjmp(env.as_ptr().expose_provenance(), val)
}
