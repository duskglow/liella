/* Class machine: process entry (_start), the content of liella-crt1.o (spec §1.1, IP-9).
 *
 * The kernel enters with %rsp at argc, followed by argv[], NULL, envp[], NULL and the auxiliary
 * vector (⚑ KERN); %rdx holds the dynamic linker's termination function (0 for a static
 * program). As glibc's crt1 does since 2.34, this calls
 *   __libc_start_main(main, argc, argv, init = 0, fini = 0, rtld_fini, stack_end)
 * with the stack aligned to 16 bytes, and never returns. `cargo xtask so` assembles this file
 * alone into the start object; crates/boundary/build.rs leaves it out of the C layer's archive,
 * so no other link ever sees a second _start.
 *
 * The start of the executable's .eh_frame (D-2.7). A static link has no .eh_frame_hdr (the
 * compiler driver asks the linker for one only in dynamic links), so `backtrace` finds a static
 * executable's call-frame information between two link-time labels: liella_eh_frame_start,
 * at the start of this object's .eh_frame, which the linker places first because the start
 * object comes first in a link, and the end label in adapter/runtime.c. _start's own row marks
 * it as the outermost frame (its return-address column is undefined).
 *
 * _start lives in .text.startup (HOT-ORDER): GNU ld's default script places that section, with a
 * -O2 main, right before the hot text group, so every program's entry shares the 64 KiB
 * fault-around window of the startup functions instead of costing a page fault of its own. */
	.section .eh_frame,"a",@unwind
	.globl liella_eh_frame_start
	.hidden liella_eh_frame_start
liella_eh_frame_start:
	.section .text.startup._start,"ax",@progbits
	.globl _start
	.type _start, @function
	.p2align 6
_start:
	.cfi_startproc
	.cfi_undefined %rip
	xorl %ebp, %ebp
	movq %rdx, %r9
	popq %rsi
	movq %rsp, %rdx
	andq $-16, %rsp
	pushq %rax
	pushq %rsp
	xorl %r8d, %r8d
	xorl %ecx, %ecx
	movq main@GOTPCREL(%rip), %rdi
	call *__libc_start_main@GOTPCREL(%rip)
	hlt
	.cfi_endproc
	.size _start, .-_start
	.section .note.GNU-stack,"",@progbits
