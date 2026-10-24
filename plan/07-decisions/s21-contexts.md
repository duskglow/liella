<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

#### §21: Contexts

| ID | Spec | Ann. | Decision |
|---|---|---|---|
| D-21.1 | §21.1 | HIST, ARB | `setjmp`/`longjmp` are assembly in the C layer (`machine/setjmp.S`). The `jmp_buf` layout is ABI (OD-1), with pointer mangling and CET shadow-stack unwinding. Library functions that are async-signal-safe keep no drop-carrying Rust state across interruptible points, so a `siglongjmp` out of a handler does not skip destructors (F37). |
| D-21.2 | §21.2 | HIST | **Internal:** a safe `Fiber` coroutine API over `rt::context` and `machine/context.S`. **Boundary:** `makecontext`'s variadic `int` arguments go through a C variadic adapter (F11). |
| D-21.3 | §21.1–21.2 | KERN | **Shadow stacks** (refines D-21.1). Linux enables a user shadow stack only when the C library asks for it (`arch_prctl(ARCH_SHSTK_ENABLE)`, Linux 6.6+); glibc 2.39 does so only on request (tunable). Liella never asks, so no shadow stack exists in a Liella process and `longjmp`/`setcontext` have nothing to unwind. The `jmp_buf` and `ucontext_t` keep their shadow-stack fields (ABI) and Liella writes 0 there. Enabling shadow stacks is future work with its own decision. |
| D-21.4 | §21.1 | HIST | **The `setjmp` symbol does not save the signal mask**, as the spec states (§21.1: "`setjmp` on glibc does not save the mask either"); it is `_setjmp`. **Divergence found (phase 6, for the owner):** the host glibc 2.39 binary disagrees: its `setjmp@@GLIBC_2.2.5` loads 1 into `%esi` before `__sigsetjmp` (the BSD behaviour; `objdump -d` of `libc.so.6`). C code never sees the difference, because `<setjmp.h>` defines `setjmp` as `_setjmp`; only a call through the symbol itself (an address taken, another language's binding) does. **Owner ruling (2026-10-01): confirmed.** Liella follows the spec; the difference from the host glibc is documented in `docs/differences-from-glibc.md`. |
