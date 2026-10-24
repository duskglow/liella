/* Class machine: setjmp/longjmp register save and restore, pointer mangling (spec §21.1;
 * D-21.1, D-21.3, D-21.4). Status: approved.
 *
 * jmp_buf (spec/abi/layouts.tsv): rbx, rbp, r12, r13, r14, r15, rsp, pc at 0..56, the
 * stack pointer, frame pointer and program counter mangled with the thread's pointer guard
 * (%fs:0x30: xor, then rotate left 17, as glibc does); __mask_was_saved (int) at 64;
 * __saved_mask at 72 (the kernel's 8 bytes). No shadow stack exists in a Liella process
 * (D-21.3), so none is recorded or unwound.
 *
 * The glibc names exist only in the products (LIELLA_EXPORT), so that tests never interpose
 * on the host C library; the liella_machine_ forms are always present. */
	.text

/* int liella_machine_sigsetjmp(jmp_buf env, int savemask): save the caller's registers; with
 * savemask, the signal mask too. Returns 0 (and the value passed to longjmp, later). */
	.globl liella_machine_sigsetjmp
	.hidden liella_machine_sigsetjmp
	.type liella_machine_sigsetjmp, @function
	.p2align 6
liella_machine_sigsetjmp:
	movq %rbx, 0(%rdi)
	movq %rbp, %rax
	xorq %fs:0x30, %rax
	rolq $0x11, %rax
	movq %rax, 8(%rdi)
	movq %r12, 16(%rdi)
	movq %r13, 24(%rdi)
	movq %r14, 32(%rdi)
	movq %r15, 40(%rdi)
	leaq 8(%rsp), %rax		/* the caller's stack pointer once this returns */
	xorq %fs:0x30, %rax
	rolq $0x11, %rax
	movq %rax, 48(%rdi)
	movq (%rsp), %rax		/* the return address */
	xorq %fs:0x30, %rax
	rolq $0x11, %rax
	movq %rax, 56(%rdi)
	xorl %eax, %eax
	testl %esi, %esi
	setne %al
	movl %eax, 64(%rdi)
	jz 1f
	leaq 72(%rdi), %rdx		/* rt_sigprocmask(SIG_BLOCK, NULL, &env->__saved_mask, 8) */
	xorl %esi, %esi
	xorl %edi, %edi
	movl $8, %r10d
	movl $14, %eax
	syscall
1:	xorl %eax, %eax
	ret
	.size liella_machine_sigsetjmp, .-liella_machine_sigsetjmp

/* void liella_machine_siglongjmp(jmp_buf env, int val): restore the signal mask if the buffer
 * saved one, then the registers; setjmp returns val (1 if val is 0). Does not return. */
	.globl liella_machine_siglongjmp
	.hidden liella_machine_siglongjmp
	.type liella_machine_siglongjmp, @function
/* void liella_machine_longjmp_nomask(jmp_buf env, int val): the same without the mask
 * (_longjmp). */
	.globl liella_machine_longjmp_nomask
	.hidden liella_machine_longjmp_nomask
	.type liella_machine_longjmp_nomask, @function
	.p2align 6
liella_machine_siglongjmp:
	cmpl $0, 64(%rdi)
	je liella_machine_longjmp_nomask
	pushq %rdi
	pushq %rsi
	leaq 72(%rdi), %rsi		/* rt_sigprocmask(SIG_SETMASK, &env->__saved_mask, NULL, 8) */
	xorl %edx, %edx
	movl $2, %edi
	movl $8, %r10d
	movl $14, %eax
	syscall
	popq %rsi
	popq %rdi
liella_machine_longjmp_nomask:
	movq 48(%rdi), %r8
	movq 8(%rdi), %r9
	movq 56(%rdi), %rdx
	rorq $0x11, %r8
	xorq %fs:0x30, %r8
	rorq $0x11, %r9
	xorq %fs:0x30, %r9
	rorq $0x11, %rdx
	xorq %fs:0x30, %rdx
	movq 0(%rdi), %rbx
	movq 16(%rdi), %r12
	movq 24(%rdi), %r13
	movq 32(%rdi), %r14
	movq 40(%rdi), %r15
	movl %esi, %eax
	testl %eax, %eax
	jnz 2f
	movl $1, %eax
2:	movq %r8, %rsp
	movq %r9, %rbp
	jmpq *%rdx
	.size liella_machine_siglongjmp, .-liella_machine_siglongjmp
	.size liella_machine_longjmp_nomask, .-liella_machine_longjmp_nomask

#ifdef LIELLA_EXPORT
/* The exported entry points (versions from spec/abi/versions.tsv). `setjmp` does not save the
 * mask (spec §21.1; D-21.4); `_longjmp` never restores one. */
	.globl _setjmp
	.type _setjmp, @function
	.p2align 6
_setjmp:
	xorl %esi, %esi
	jmp liella_machine_sigsetjmp
	.size _setjmp, .-_setjmp

	.globl setjmp
	.type setjmp, @function
	.p2align 6
setjmp:
	xorl %esi, %esi
	jmp liella_machine_sigsetjmp
	.size setjmp, .-setjmp

	.globl __sigsetjmp
	.type __sigsetjmp, @function
	.p2align 6
__sigsetjmp:
	jmp liella_machine_sigsetjmp
	.size __sigsetjmp, .-__sigsetjmp

	.globl longjmp
	.type longjmp, @function
	.p2align 6
longjmp:
	jmp liella_machine_siglongjmp
	.size longjmp, .-longjmp

	.globl siglongjmp
	.type siglongjmp, @function
	.p2align 6
siglongjmp:
	jmp liella_machine_siglongjmp
	.size siglongjmp, .-siglongjmp

	.globl _longjmp
	.type _longjmp, @function
	.p2align 6
_longjmp:
	jmp liella_machine_longjmp_nomask
	.size _longjmp, .-_longjmp
#endif
	.section .note.GNU-stack,"",@progbits
