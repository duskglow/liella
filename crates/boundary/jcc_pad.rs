// JCC-erratum padding of the hot machine assembly (D-4.67, MACHINE-JCC-PAD). One list, read with `include!` by
// crates/boundary/build.rs (the products) and by xtask (`cargo xtask static` assembles these files with these flags,
// `cargo xtask so` checks the built libc.a; xtask/src/jcc.rs).
//
// Specification: Intel, "Mitigations for Jump Conditional Code Erratum" (2019). On the cores that have the erratum
// (Skylake family) the microcode update keeps out of the decoded-uop cache any 32-byte code window in which a jump
// (conditional, macro-fused compare-and-jump, unconditional, indirect, call or return) crosses a 32-byte boundary
// or ends on one; that code then runs from the legacy decoders (IDQ.MITE_UOPS). The GNU assembler options below
// move every such jump (or fused pair) off the boundary, with segment prefixes on earlier instructions where they
// fit and NOPs otherwise. Cores without the erratum see only the added bytes, except that more than 3 prefixes on
// one instruction slow the decode of cores that decode every pass (no decoded-uop cache: the N100's Gracemont
// E-cores ran strcasecmp's 64-byte loop at 1.55x glibc with 4 `cs` prefixes on its head instructions), so prefix
// padding stops at 3 bytes per instruction and NOPs take the rest (D-4.70, STRCASECMP-N100-REGRESS).

/// The `csys/src/machine/` files assembled with [`JCC_PAD_FLAGS`]: the string and memory kernels.
pub const JCC_PAD_FILES: &[&str] = &["fold.S", "scan.S"];

/// The assembler options (through the compiler driver): 32-byte boundary, every jump kind the erratum names,
/// prefix padding up to 3 bytes per instruction (D-4.70).
pub const JCC_PAD_FLAGS: &[&str] = &[
    "-Wa,-malign-branch-boundary=32",
    "-Wa,-malign-branch=jcc+fused+jmp+call+ret+indirect",
    "-Wa,-malign-branch-prefix-size=3",
];
