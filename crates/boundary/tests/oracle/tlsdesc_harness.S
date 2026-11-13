/* Test harness for csys/src/machine/tlsdesc.S (crates/boundary/tests/ld_tlsdesc.rs): calls a TLS
 * descriptor function the way -mtls-dialect=gnu2 code does, with every register under test loaded
 * from a known state, and records every register after the call.
 *
 * struct state (tlsdesc_abi.c): 0..103 the general registers rbx rcx rdx rsi rdi r8 r9 r10 r11 r12
 * r13 r14 r15 (13 words), 104 rax, 112..367 xmm0..xmm15, 368 MXCSR (4 bytes), 372 x87 control word.
 *
 * void tlsdesc_call(void *desc, const struct state *in, struct state *out, long misalign)
 *   %rdi = the descriptor (its address goes in %rax for the call), %rsi = in, %rdx = out,
 *   %rcx = bytes (0 or 8) by which %rsp is moved off 16-byte alignment at the call. */

#define G(i) (8 * (i))
#define RAX 104
#define XMM(i) (112 + 16 * (i))
#define MXCSR 368
#define FCW 372

	.text
	.globl tlsdesc_call
	.type tlsdesc_call, @function
	.p2align 4
tlsdesc_call:
	pushq %rbp
	movq %rsp, %rbp
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdx, -48(%rbp)
	movq %rdi, -56(%rbp)
	leaq -64(%rbp), %rsp
	subq %rcx, %rsp
	ldmxcsr MXCSR(%rsi)
	fldcw FCW(%rsi)
	movdqu XMM(0)(%rsi), %xmm0
	movdqu XMM(1)(%rsi), %xmm1
	movdqu XMM(2)(%rsi), %xmm2
	movdqu XMM(3)(%rsi), %xmm3
	movdqu XMM(4)(%rsi), %xmm4
	movdqu XMM(5)(%rsi), %xmm5
	movdqu XMM(6)(%rsi), %xmm6
	movdqu XMM(7)(%rsi), %xmm7
	movdqu XMM(8)(%rsi), %xmm8
	movdqu XMM(9)(%rsi), %xmm9
	movdqu XMM(10)(%rsi), %xmm10
	movdqu XMM(11)(%rsi), %xmm11
	movdqu XMM(12)(%rsi), %xmm12
	movdqu XMM(13)(%rsi), %xmm13
	movdqu XMM(14)(%rsi), %xmm14
	movdqu XMM(15)(%rsi), %xmm15
	movq G(0)(%rsi), %rbx
	movq G(1)(%rsi), %rcx
	movq G(2)(%rsi), %rdx
	movq G(4)(%rsi), %rdi
	movq G(5)(%rsi), %r8
	movq G(6)(%rsi), %r9
	movq G(7)(%rsi), %r10
	movq G(8)(%rsi), %r11
	movq G(9)(%rsi), %r12
	movq G(10)(%rsi), %r13
	movq G(11)(%rsi), %r14
	movq G(12)(%rsi), %r15
	movq G(3)(%rsi), %rsi
	movq -56(%rbp), %rax
	call *(%rax)
	pushq %rsi
	movq -48(%rbp), %rsi
	movq %rax, RAX(%rsi)
	movq %rbx, G(0)(%rsi)
	movq %rcx, G(1)(%rsi)
	movq %rdx, G(2)(%rsi)
	movq %rdi, G(4)(%rsi)
	movq %r8, G(5)(%rsi)
	movq %r9, G(6)(%rsi)
	movq %r10, G(7)(%rsi)
	movq %r11, G(8)(%rsi)
	movq %r12, G(9)(%rsi)
	movq %r13, G(10)(%rsi)
	movq %r14, G(11)(%rsi)
	movq %r15, G(12)(%rsi)
	popq %rax
	movq %rax, G(3)(%rsi)
	movdqu %xmm0, XMM(0)(%rsi)
	movdqu %xmm1, XMM(1)(%rsi)
	movdqu %xmm2, XMM(2)(%rsi)
	movdqu %xmm3, XMM(3)(%rsi)
	movdqu %xmm4, XMM(4)(%rsi)
	movdqu %xmm5, XMM(5)(%rsi)
	movdqu %xmm6, XMM(6)(%rsi)
	movdqu %xmm7, XMM(7)(%rsi)
	movdqu %xmm8, XMM(8)(%rsi)
	movdqu %xmm9, XMM(9)(%rsi)
	movdqu %xmm10, XMM(10)(%rsi)
	movdqu %xmm11, XMM(11)(%rsi)
	movdqu %xmm12, XMM(12)(%rsi)
	movdqu %xmm13, XMM(13)(%rsi)
	movdqu %xmm14, XMM(14)(%rsi)
	movdqu %xmm15, XMM(15)(%rsi)
	stmxcsr MXCSR(%rsi)
	fnstcw FCW(%rsi)
	/* The harness's own state back to the C defaults before returning to C. */
	pushq $0x1f80
	ldmxcsr (%rsp)
	movw $0x037f, (%rsp)
	fldcw (%rsp)
	leaq -40(%rbp), %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	popq %rbp
	ret
	.size tlsdesc_call, .-tlsdesc_call

/* A resolver that changes everything a C function may change: every caller-saved general
 * register, xmm0-15, MXCSR (rounding toward zero) and the x87 control word (single precision).
 * Returns record[1], the address the test chose. */
	.globl clobber_resolver
	.type clobber_resolver, @function
	.p2align 4
clobber_resolver:
	movq 8(%rdi), %rax
	movabsq $0x5a5a5a5a5a5a5a5a, %rcx
	movq %rcx, %rdx
	movq %rcx, %rsi
	movq %rcx, %rdi
	movq %rcx, %r8
	movq %rcx, %r9
	movq %rcx, %r10
	movq %rcx, %r11
	pcmpeqd %xmm0, %xmm0
	pcmpeqd %xmm1, %xmm1
	pcmpeqd %xmm2, %xmm2
	pcmpeqd %xmm3, %xmm3
	pcmpeqd %xmm4, %xmm4
	pcmpeqd %xmm5, %xmm5
	pcmpeqd %xmm6, %xmm6
	pcmpeqd %xmm7, %xmm7
	pcmpeqd %xmm8, %xmm8
	pcmpeqd %xmm9, %xmm9
	pcmpeqd %xmm10, %xmm10
	pcmpeqd %xmm11, %xmm11
	pcmpeqd %xmm12, %xmm12
	pcmpeqd %xmm13, %xmm13
	pcmpeqd %xmm14, %xmm14
	pcmpeqd %xmm15, %xmm15
	pushq $0x7f80
	ldmxcsr (%rsp)
	movw $0x007f, (%rsp)
	fldcw (%rsp)
	popq %rcx
	movq %rdx, %rcx
	ret
	.size clobber_resolver, .-clobber_resolver
	.section .note.GNU-stack,"",@progbits
