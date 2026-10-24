//! Translation for spec §21: non-local transfer and contexts (phase 6).
//!
//! `setjmp`, `_setjmp`, `__sigsetjmp`, `longjmp`, `_longjmp`, `siglongjmp`, `getcontext`,
//! `setcontext` and `swapcontext` are the C layer's own entry points (`machine/setjmp.S`,
//! `machine/context.S`, glibc names in the products only): a function that returns twice, or
//! that replaces the caller's registers, cannot be Rust. `makecontext` is a variadic adapter
//! (`adapter/variadic.c`) forwarding to `liella_makecontext4` here (D-21.2).

use liella_abi::ctypes::{c_int, c_void};
use liella_abi::layout::{jmp_buf, ucontext_t};

use crate::context;
use crate::conv;
use crate::export;

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_makecontext4(uc: *mut ucontext_t, func: usize, argc: c_int, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: uc is the caller's context, its uc_stack the caller's stack (§21.2).
    context::make(unsafe { conv::obj64(uc) }, unsafe { conv::ucontext_stack(uc) }, func, argc, conv::addr(ap))
}}

export! { "GLIBC_2.11";
pub extern "C" fn __longjmp_chk(env: *mut jmp_buf, val: c_int) -> ! {
    // UNSAFE(slice) SAFETY: env was filled by setjmp in a frame that has not returned (§21.1).
    context::longjmp_chk(unsafe { conv::obj64(env) }, val)
}}
