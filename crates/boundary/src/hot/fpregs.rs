//! Machine-register intrinsics (unsafe category *machine-register*, owner ruling 2026-11-07,
//! D-X.44): the libm envelope's two pieces of inline machine code, and nothing else.
//!
//! * [`mxcsr`]: one `stmxcsr` into a stack local, the MXCSR read of the fast entries
//!   (`mathret::elem_fast`, `mathret::sincos_fast`) in place of a call to the out-of-line
//!   `liella_machine_mxcsr_get`, so the entry keeps its operand in its register (a call clobbers
//!   every `xmm` register) and pays no call and return.
//! * [`Pin::pin`]: an empty (comment-only), register-only barrier on a floating-point value, in place of
//!   `core::hint::black_box` on the fast path: the arithmetic on the value cannot start before the
//!   barrier, and the barrier is not hoisted above the branch on the MXCSR value, as with
//!   `black_box`, but without its store to and load from the stack.
//!
//! RULES (checked by `cargo xtask static`, unsafe audit): inline assembly is permitted in this
//! file and in the one *tb-field* site of `tb.rs` only, and the only templates permitted here are
//! the MXCSR store (exactly once) and the barrier's assembler comment, which names its operand
//! (rustc refuses an operand the template does not use) and emits no instruction. Neither may
//! access memory other than
//! the one local, transfer control, touch the flags or the stack pointer, or write a register
//! other than its declared output. Everything else that needs the machine stays in `csys/`.

/// The calling thread's MXCSR, read with one `stmxcsr` into a local (no call).
///
/// MAY: store the 32-bit MXCSR into its own 4-byte stack local, and load it into the result.
/// MAY NOT: write MXCSR, access any other memory, branch, touch the flags or the stack pointer.
/// Like any MXCSR read it does not order the compiler's floating-point arithmetic: an entry that
/// must keep arithmetic behind its test of the value puts a [`Pin::pin`] on the operand there.
#[inline(always)]
pub(crate) fn mxcsr() -> u32 { // ARCH(x86_64): the SSE control and status word; aarch64 has FPCR/FPSR (A-4)
    let mut v: u32 = 0;
    // UNSAFE(machine-register): Rust has no stable safe MXCSR read (owner ruling 2026-11-07, D-X.44).
    #[allow(unsafe_code)]
    // UNSAFE(machine-register) SAFETY: `{p}` holds the address of `v`, a live, aligned, writable
    // 4-byte local; `stmxcsr` writes exactly those 4 bytes and nothing else (no other memory, no
    // flag, no stack adjustment, no control transfer). Not `pure`: two reads must not be merged.
    unsafe {
        core::arch::asm!("stmxcsr dword ptr [{p}]", p = in(reg) &raw mut v, options(nostack, preserves_flags)); // ARCH(x86_64): SSE MXCSR store; aarch64 reads FPCR/FPSR (A-4)
    }
    v
}

/// A value the compiler must treat as unknown from here on, held in its `xmm` register.
pub trait Pin: Sized {
    /// `self`, unchanged, through an empty register-only barrier.
    ///
    /// MAY: nothing; the template is an assembler comment naming the register (no instruction),
    /// the value stays in the same `xmm` register.
    /// MAY NOT: access memory, branch, touch the flags or the stack, change the value. Being
    /// `pure` and `nomem`, the barrier itself may be dropped when its result is unused, merged with
    /// an identical one, or moved later; the arithmetic on its result cannot run before it.
    #[must_use]
    fn pin(self) -> Self;
}

impl Pin for f64 {
    #[inline(always)]
    fn pin(self) -> f64 {
        let mut x = self;
        // UNSAFE(machine-register): the empty xmm barrier (owner ruling 2026-11-07, D-X.44).
        #[allow(unsafe_code)]
        // UNSAFE(machine-register) SAFETY: the template is only a comment: no instruction runs, so no
        // memory, flag, stack or control effect; the value's register is its own in and out.
        unsafe {
            core::arch::asm!("/* {x} */", x = inout(xmm_reg) x, options(pure, nomem, nostack, preserves_flags)); // ARCH(x86_64): xmm register class; another target pins its own FP register class (A-4)
        }
        x
    }
}

impl Pin for f32 {
    #[inline(always)]
    fn pin(self) -> f32 {
        let mut x = self;
        // UNSAFE(machine-register): the empty xmm barrier (owner ruling 2026-11-07, D-X.44).
        #[allow(unsafe_code)]
        // UNSAFE(machine-register) SAFETY: the template is only a comment: no instruction runs, so no
        // memory, flag, stack or control effect; the value's register is its own in and out.
        unsafe {
            core::arch::asm!("/* {x} */", x = inout(xmm_reg) x, options(pure, nomem, nostack, preserves_flags)); // ARCH(x86_64): xmm register class; another target pins its own FP register class (A-4)
        }
        x
    }
}

/// A two-operand entry (`pow`, `hypot`, `atan2`): each operand through its own barrier.
impl<A: Pin, B: Pin> Pin for (A, B) {
    #[inline(always)]
    fn pin(self) -> (A, B) {
        (self.0.pin(), self.1.pin())
    }
}
