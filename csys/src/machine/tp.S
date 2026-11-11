/* Class machine: thread-pointer read (FS base). x86_64 TLS ABI: the word at %fs:0 holds the
 * thread pointer itself (spec Appendix A.2, KERN/ABI). */
	.section .text.liella_machine_tp,"ax",@progbits
	.globl liella_machine_tp
	.hidden liella_machine_tp
	.type liella_machine_tp, @function
	.p2align 6
liella_machine_tp:
	movq %fs:0, %rax
	ret
	.size liella_machine_tp, .-liella_machine_tp

/* int *liella_machine_errno_location(void): `__errno_location` (spec §2.1, D-2.1, D-X.60). In the
 * products with the runtime this IS `__errno_location` (second label; the Rust twin in
 * crates/boundary/src/posix/s02_diagnostics.rs calls this code in the tests). The block's errno
 * word (thread pointer + 0x80, asserted in crates/rt/src/tls.rs) only in a process the runtime
 * started (`liella_machine_owned`, machine/points.S, D-X.58): elsewhere 0x80 of the block is
 * another library's word, and `liella_errno_rest` (Rust, cold) returns the thread's registry
 * word, stable for the thread's life. Why assembly: the owned test is one rip-relative `cmpb`
 * of the hidden byte here, where Rust reaches the assembler's symbol through the GOT. */
	.section .text.liella_machine_errno_location,"ax",@progbits
	.hidden liella_errno_rest	/* crates/boundary/src/posix/s02_diagnostics.rs */
	.globl liella_machine_errno_location
	.hidden liella_machine_errno_location
	.type liella_machine_errno_location, @function
	.p2align 6
	.cfi_startproc
liella_machine_errno_location:
#ifdef LIELLA_EXPORT
#ifdef LIELLA_RUNTIME
	.globl __errno_location
	.type __errno_location, @function
__errno_location:
#endif
#endif
	cmpb $0, liella_machine_owned(%rip)
	je liella_errno_rest
	movq %fs:0, %rax
	addq $0x80, %rax
	ret
	.cfi_endproc
	.size liella_machine_errno_location, .-liella_machine_errno_location
	.section .note.GNU-stack,"",@progbits
