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

// UNSAFE(extern): the loader's machine primitives (R14-8), one declaration each.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// `csys/src/machine/entry.S` assembled with `-DLIELLA_LD`: the entry point of `ld.so`. The
    /// kernel enters it with `%rsp` at `argc`; it passes that address to `ld::liella_ld_start`.
    /// Never called from Rust: its address is compared with `AT_ENTRY` (run as a program or not).
    pub(crate) safe fn liella_machine_ld_entry();
}
