//! The loader's architecture seam in the boundary (plan/ph14/design.md §0a, L14-13): the
//! thread-pointer write and the declarations of the loader's machine primitives. Feature `ld`.
//! Nothing else in the loader's boundary code names an architecture.

/// Make `tb` the calling thread's thread pointer (design §0a "Thread pointer"). The loader's
/// provisional block (L14-2) is installed with this before any `%fs`-relative access can happen.
///
/// # Errors
///
/// The kernel's error when it refuses the address.
pub(crate) fn set_thread_pointer(tb: usize) -> liella_kernel::errno::KResult<()> {
    // ARCH(x86_64): arch_prctl(ARCH_SET_FS); aarch64 writes TPIDR_EL0 through a primitive (A-4)
    crate::raw::set_fs(tb)
}

/// Hand the process to the program entry point `entry` with the initial stack `sp` (design §0a
/// "Machine primitives", program entry transfer). The caller has mapped and relocated the program
/// and laid out argc, argv, envp and the auxiliary vector from `sp` as the kernel does. `fini` is
/// the termination function the program registers with `atexit` (psABI process initialisation;
/// 0 = none): the loader's destructor pass (DL-11, D-27.19).
pub(crate) fn enter(sp: usize, entry: usize, fini: usize) -> ! {
    liella_machine_ld_enter(sp, entry, fini)
}

// UNSAFE(extern): the loader's machine primitives (R14-8), one declaration each.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// `csys/src/machine/entry.S` assembled with `-DLIELLA_LD`: the entry point of `ld.so`. The
    /// kernel enters it with `%rsp` at `argc`; it passes that address to `ld::liella_ld_start`.
    /// Never called from Rust: its address is compared with `AT_ENTRY` (run as a program or not).
    // ARCH(x86_64): the primitive's register convention is x86-64's (entry.S); aarch64 has its own (A-4).
    pub(crate) safe fn liella_machine_ld_entry();
    /// `csys/src/machine/entry.S` assembled with `-DLIELLA_LD`: hand the process to a program's
    /// entry point `entry` with the initial stack `sp` (argc at it) and the termination function
    /// `fini` (0 = none). Never returns.
    // ARCH(x86_64): x86-64 psABI process entry (%rsp, %rdx = fini); aarch64 has its own (A-4).
    pub(crate) safe fn liella_machine_ld_enter(sp: usize, entry: usize, fini: usize) -> !;
}
