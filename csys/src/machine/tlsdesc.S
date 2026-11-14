/* Class machine: the TLS descriptor functions of Liella's ld.so (phase 14 DL-15 `DL-TLS-THREADS`,
 * R14-8 "TLSDESC return and fallback", plan/ph14/design.md section 7). Declared once in
 * boundary/src/arch/.
 *
 * A TLS descriptor (R_X86_64_TLSDESC; A. Oliva, "Thread-Local Storage Descriptors for IA32 and
 * AMD64/EM64T", 2005) is two words in the object's GOT: [function, argument]. Code compiled with
 * -mtls-dialect=gnu2 loads the descriptor's address into %rax and calls *(%rax); the function
 * returns in %rax the variable's offset from the thread pointer, and the caller adds %fs:0.
 *
 * Register convention (both functions): on entry %rax = the descriptor's address; on return
 * %rax = the offset from the thread pointer. Every other general register, every vector register,
 * the x87 state and MXCSR hold their values on return. The flags are not preserved. The stack
 * pointer at entry may have any alignment: the call site may be anywhere in a function. Nothing
 * is written below the stack pointer of the caller other than this function's own frame.
 *
 * liella_machine_ld_tlsdesc_static: the argument word is the offset itself (the module's block
 * lies in the static area at a fixed offset from every thread's TP; the TPOFF arithmetic). One
 * load, no frame.
 *
 * liella_machine_ld_tlsdesc_dynamic: the fallback for a block that is not static. The argument
 * word points at a record whose first word is the resolver, an `extern "C" fn(record) -> address`
 * that returns the variable's absolute address in the calling thread; what follows its first word
 * is the resolver's own. The trampoline saves the caller-saved general registers (rcx, rdx, rsi,
 * rdi, r8-r11) and the x87/SSE state (FXSAVE: x87, MXCSR, xmm0-15) in a 64-byte aligned frame,
 * calls the resolver with the record, subtracts %fs:0 and restores; it has no branch. The
 * resolver is baseline x86-64 code (SSE2, no AVX instruction), so the upper halves of the ymm/zmm
 * registers, zmm16-31 and the mask registers pass through it unchanged; FXSAVE covers everything
 * such code can change. */

	.section .text.liella_machine_ld_tlsdesc_static,"ax",@progbits
	.globl liella_machine_ld_tlsdesc_static
	.hidden liella_machine_ld_tlsdesc_static
	.type liella_machine_ld_tlsdesc_static, @function
	.p2align 4
liella_machine_ld_tlsdesc_static:
	.cfi_startproc
	movq 8(%rax), %rax
	ret
	.cfi_endproc
	.size liella_machine_ld_tlsdesc_static, .-liella_machine_ld_tlsdesc_static

/* The frame: 8 saved general registers (64 bytes), then the 512-byte FXSAVE area (16-aligned). */
#define TLSDESC_FRAME (64 + 512)

	.section .text.liella_machine_ld_tlsdesc_dynamic,"ax",@progbits
	.globl liella_machine_ld_tlsdesc_dynamic
	.hidden liella_machine_ld_tlsdesc_dynamic
	.type liella_machine_ld_tlsdesc_dynamic, @function
	.p2align 4
liella_machine_ld_tlsdesc_dynamic:
	.cfi_startproc
	pushq %rbp
	.cfi_def_cfa_offset 16
	.cfi_offset %rbp, -16
	movq %rsp, %rbp
	.cfi_def_cfa_register %rbp
	subq $TLSDESC_FRAME, %rsp
	andq $-64, %rsp
	movq %rcx, 0(%rsp)
	movq %rdx, 8(%rsp)
	movq %rsi, 16(%rsp)
	movq %rdi, 24(%rsp)
	movq %r8, 32(%rsp)
	movq %r9, 40(%rsp)
	movq %r10, 48(%rsp)
	movq %r11, 56(%rsp)
	fxsave64 64(%rsp)
	movq 8(%rax), %rdi
	call *(%rdi)
	subq %fs:0, %rax
	fxrstor64 64(%rsp)
	movq 0(%rsp), %rcx
	movq 8(%rsp), %rdx
	movq 16(%rsp), %rsi
	movq 24(%rsp), %rdi
	movq 32(%rsp), %r8
	movq 40(%rsp), %r9
	movq 48(%rsp), %r10
	movq 56(%rsp), %r11
	movq %rbp, %rsp
	.cfi_def_cfa_register %rsp
	popq %rbp
	.cfi_def_cfa_offset 8
	ret
	.cfi_endproc
	.size liella_machine_ld_tlsdesc_dynamic, .-liella_machine_ld_tlsdesc_dynamic
	.section .note.GNU-stack,"",@progbits
