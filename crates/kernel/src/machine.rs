//! The `Machine` trait: CPU primitives that need an instruction Rust cannot express safely
//! (spec Appendix A.2 "Thread pointer register" and "CPU instructions requiring OS support";
//! owner decision OD-C1). The C layer implements them in assembly (`csys/src/machine`).
//!
//! Every method is sound for every argument. Where an instruction would fault or revoke
//! access to live memory, the implementation must refuse or mask the value (documented per
//! method). Calls to arbitrary addresses are not here: the vDSO is called inside the `Sys`
//! implementation, and other call-at-address uses are the boundary's.

/// The x87 environment as `fnstenv` stores it (28 bytes, 32-bit protected-mode format).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct X87Env {
    /// Control word (low 16 bits).
    pub control: u32,
    /// Status word (low 16 bits).
    pub status: u32,
    /// Tag word (low 16 bits).
    pub tag: u32,
    /// Instruction and operand pointers (opaque).
    pub rest: [u32; 4],
}

/// MXCSR bits that are architecturally defined on every x86_64 CPU (flags, masks, rounding,
/// flush-to-zero). DAZ (bit 6) is added when `MXCSR_MASK` allows it.
pub const MXCSR_DEFINED: u32 = 0xffbf;

/// CPU primitives.
pub trait Machine {
    /// The thread pointer (FS base) as the thread's first word stores it (x86_64 TLS ABI: the
    /// word at `%fs:0` points to itself, ⚑ KERN/ABI).
    fn thread_pointer(&self) -> usize;
    /// `cpuid(leaf, subleaf)`: `[eax, ebx, ecx, edx]`.
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4];
    /// `xgetbv(0)` (XCR0), or `None` when the OS has not enabled `xgetbv` (`CPUID.1:ECX.OSXSAVE`).
    fn xcr0(&self) -> Option<u64>;
    /// Read MXCSR.
    fn mxcsr(&self) -> u32;
    /// Write MXCSR. Bits the CPU does not define are cleared (they would fault).
    fn set_mxcsr(&self, value: u32);
    /// Read the x87 control word.
    fn x87_control(&self) -> u16;
    /// Write the x87 control word.
    fn set_x87_control(&self, value: u16);
    /// Read the x87 status word.
    fn x87_status(&self) -> u16;
    /// Clear the x87 exception flags (`fnclex`).
    fn x87_clear_exceptions(&self);
    /// Store the x87 environment (`fnstenv`, then reload it: `fnstenv` masks exceptions).
    fn x87_store_env(&self) -> X87Env;
    /// Load an x87 environment (`fldenv`).
    fn x87_load_env(&self, env: &X87Env);
    /// Read PKRU, or `None` when the OS has not enabled protection keys (`CPUID.7:ECX.OSPKE`).
    fn pkru(&self) -> Option<u32>;
    /// Write PKRU. The bits for key 0 (the default key of all ordinary memory) are forced to
    /// "allow", so live references never lose access. `false` when unsupported.
    fn set_pkru(&self, value: u32) -> bool;
    /// An optimisation barrier: the compiler must assume `bytes` are read by an unknown party
    /// (`explicit_bzero`, spec §4).
    fn barrier(&self, bytes: &[u8]);
}
