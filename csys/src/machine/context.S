/* Class machine: user-context switch (spec §21.2; D-21.2, D-21.3). Status: approved.
 *
 * ucontext_t offsets (spec/abi/layouts.tsv): uc_link 8, gregs at 40 (R8 0, R9 1, R12 4,
 * R13 5, R14 6, R15 7, RDI 8, RSI 9, RBP 10, RBX 11, RDX 12, RAX 13, RCX 14, RSP 15, RIP 16,
 * eight bytes each), fpregs (a pointer) at 224, uc_sigmask at 296, __fpregs_mem at 424 (its
 * mxcsr at 448). As glibc, getcontext records the x87 environment and MXCSR (not the
 * registers' contents, which the ABI does not preserve across calls) and points fpregs at
 * __fpregs_mem. No shadow stack exists (D-21.3).
 *
 * setcontext and swapcontext never block the library's internal signal (32, D-20.1, OD-8):
 * they install a copy of the context's mask with its bit cleared.
 *
 * makecontext's argument placement is Rust's (liella_boundary::context); it points the new
 * context's return address at liella_machine_start_context. The glibc names exist only in the
 * products (LIELLA_EXPORT). */
	.text

/* int liella_machine_getcontext(ucontext_t *uc): returns 0. */
	.globl liella_machine_getcontext
	.hidden liella_machine_getcontext
	.type liella_machine_getcontext, @function
	.p2align 6
liella_machine_getcontext:
	movq %rbx, 0x80(%rdi)
	movq %rbp, 0x78(%rdi)
	movq %r12, 0x48(%rdi)
	movq %r13, 0x50(%rdi)
	movq %r14, 0x58(%rdi)
	movq %r15, 0x60(%rdi)
	movq %rdi, 0x68(%rdi)
	movq %rsi, 0x70(%rdi)
	movq %rdx, 0x88(%rdi)
	movq %rcx, 0x98(%rdi)
	movq %r8, 0x28(%rdi)
	movq %r9, 0x30(%rdi)
	movq (%rsp), %rcx
	movq %rcx, 0xa8(%rdi)		/* RIP: the return address */
	leaq 8(%rsp), %rcx
	movq %rcx, 0xa0(%rdi)		/* RSP: the caller's, once this returns */
	leaq 0x1a8(%rdi), %rcx
	movq %rcx, 0xe0(%rdi)		/* fpregs = &__fpregs_mem */
	fnstenv (%rcx)
	fldenv (%rcx)			/* fnstenv masks the x87 exceptions: put them back */
	stmxcsr 0x1c0(%rdi)
	leaq 0x128(%rdi), %rdx		/* rt_sigprocmask(SIG_BLOCK, NULL, &uc->uc_sigmask, 8) */
	xorl %esi, %esi
	xorl %edi, %edi
	movl $8, %r10d
	movl $14, %eax
	syscall
	xorl %eax, %eax
	ret
	.size liella_machine_getcontext, .-liella_machine_getcontext

/* int liella_machine_setcontext(const ucontext_t *uc): resumes uc (its mask, x87 environment,
 * MXCSR and registers). Does not return. */
	.globl liella_machine_setcontext
	.hidden liella_machine_setcontext
	.type liella_machine_setcontext, @function
	.p2align 6
liella_machine_setcontext:
	pushq %rdi
	movq 0x128(%rdi), %rax		/* a copy of the mask without the library's internal */
	btrq $31, %rax			/* signal 32, which is never blocked (D-20.1, OD-8) */
	pushq %rax
	movq %rsp, %rsi			/* rt_sigprocmask(SIG_SETMASK, &copy, NULL, 8) */
	xorl %edx, %edx
	movl $2, %edi
	movl $8, %r10d
	movl $14, %eax
	syscall
	popq %rax
	popq %rdx
	jmp 1f
/* The register half of setcontext and swapcontext: uc in %rdx. */
1:	movq 0xe0(%rdx), %rcx
	fldenv (%rcx)
	ldmxcsr 0x1c0(%rdx)
	movq 0xa0(%rdx), %rsp
	movq 0x80(%rdx), %rbx
	movq 0x78(%rdx), %rbp
	movq 0x48(%rdx), %r12
	movq 0x50(%rdx), %r13
	movq 0x58(%rdx), %r14
	movq 0x60(%rdx), %r15
	movq 0xa8(%rdx), %rcx
	pushq %rcx			/* resume at RIP */
	movq 0x68(%rdx), %rdi
	movq 0x70(%rdx), %rsi
	movq 0x98(%rdx), %rcx
	movq 0x28(%rdx), %r8
	movq 0x30(%rdx), %r9
	movq 0x88(%rdx), %rdx
	xorl %eax, %eax
	ret
	.size liella_machine_setcontext, .-liella_machine_setcontext

/* int liella_machine_swapcontext(ucontext_t *save, const ucontext_t *to): saves the caller's
 * context into save (it resumes as this call returning 0) and resumes to, switching the
 * signal mask in one system call. */
	.globl liella_machine_swapcontext
	.hidden liella_machine_swapcontext
	.type liella_machine_swapcontext, @function
	.p2align 6
liella_machine_swapcontext:
	movq %rbx, 0x80(%rdi)
	movq %rbp, 0x78(%rdi)
	movq %r12, 0x48(%rdi)
	movq %r13, 0x50(%rdi)
	movq %r14, 0x58(%rdi)
	movq %r15, 0x60(%rdi)
	movq %rdi, 0x68(%rdi)
	movq %rsi, 0x70(%rdi)
	movq %rdx, 0x88(%rdi)
	movq %rcx, 0x98(%rdi)
	movq %r8, 0x28(%rdi)
	movq %r9, 0x30(%rdi)
	movq (%rsp), %rcx
	movq %rcx, 0xa8(%rdi)
	leaq 8(%rsp), %rcx
	movq %rcx, 0xa0(%rdi)
	movq $0, 0x90(%rdi)		/* RAX: the saved context returns 0 */
	leaq 0x1a8(%rdi), %rcx
	movq %rcx, 0xe0(%rdi)
	fnstenv (%rcx)
	fldenv (%rcx)
	stmxcsr 0x1c0(%rdi)
	pushq %rsi
	movq 0x128(%rsi), %rax		/* to's mask without signal 32 (as setcontext) */
	btrq $31, %rax
	pushq %rax
	movq %rsp, %rsi			/* rt_sigprocmask(SIG_SETMASK, &copy, &save->uc_sigmask, 8) */
	leaq 0x128(%rdi), %rdx
	movl $2, %edi
	movl $8, %r10d
	movl $14, %eax
	syscall
	popq %rax
	popq %rdx
	jmp 1b
	.size liella_machine_swapcontext, .-liella_machine_swapcontext

/* Where a makecontext function returns to. %rbx (callee-saved, so the function preserved it)
 * points at two words makecontext wrote: uc_link, and the runtime's thread-exit function. Resume
 * uc_link, or end the thread (spec §21.2: "exits the thread if that is NULL"). */
	.globl liella_machine_start_context
	.hidden liella_machine_start_context
	.type liella_machine_start_context, @function
	.p2align 6
liella_machine_start_context:
	movq %rbx, %rsp
	movq (%rsp), %rdi
	testq %rdi, %rdi
	jz 2f
	jmp liella_machine_setcontext
2:	movq 8(%rsp), %rax
	andq $-16, %rsp
	callq *%rax
	hlt
	.size liella_machine_start_context, .-liella_machine_start_context

#ifdef LIELLA_EXPORT
	.globl getcontext
	.type getcontext, @function
	.p2align 6
getcontext:
	jmp liella_machine_getcontext
	.size getcontext, .-getcontext

	.globl setcontext
	.type setcontext, @function
	.p2align 6
setcontext:
	jmp liella_machine_setcontext
	.size setcontext, .-setcontext

	.globl swapcontext
	.type swapcontext, @function
	.p2align 6
swapcontext:
	jmp liella_machine_swapcontext
	.size swapcontext, .-swapcontext
#endif
	.section .note.GNU-stack,"",@progbits
