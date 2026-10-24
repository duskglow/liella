/* The unwinder's two entry points for a panic=abort Rust program linked without an unwinder
 * (crates/native, D-X.10; compiled and linked by `cargo xtask native`).
 *
 * Rust's precompiled `core` and `alloc` are built for unwinding: their cleanup landing pads end
 * in a call to `_Unwind_Resume`, and their call-frame information names the personality routine
 * `rust_eh_personality`. A panic=abort program never unwinds, so neither is ever called, but
 * the link needs both. The unwinder that would define `_Unwind_Resume` (libgcc_eh) needs
 * `dl_iterate_phdr` and `_dl_find_object` from the C library, which arrive with dynamic linking
 * (phase 14), and Liella's libc.so.6 does not export a personality routine (libc.a defines one
 * for its own Rust code).
 *
 * Both are weak, so a real unwinder or Rust's `std` (which defines the personality) takes their
 * place, and both abort if they are ever reached. */
extern void abort(void) __attribute__((__noreturn__));

void _Unwind_Resume(void *exception) __attribute__((__weak__, __noreturn__));
void rust_eh_personality(void) __attribute__((__weak__));

void _Unwind_Resume(void *exception)
{
    (void)exception;
    abort();
}

void rust_eh_personality(void)
{
    abort();
}
