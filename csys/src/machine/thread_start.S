/* Class machine: thread start on a new stack (D-22.9), and the cancellation window (D-22.11).
 * Status: approved (thread start); the window is the system-call form D-22.11 describes. */
	.text

/* long liella_machine_clone3_run(const void *args, long size, void (*entry)(uintptr_t), uintptr_t arg)
 * clone3(2) for a spawn child that shares the caller's memory (CLONE_VM | CLONE_VFORK, D-18.3):
 * the parent gets the child's PID or -errno and nothing else of the child; the child starts on
 * the stack `args` names (16-aligned top, so the call below enters `entry` as the ABI wants),
 * calls entry(arg) with a zero frame pointer, and ends with exit_group(127) if it ever returns
 * (it must not). `entry` and `arg` wait in r8 and r9, which the system call preserves. */
	.globl liella_machine_clone3_run
	.hidden liella_machine_clone3_run
	.type liella_machine_clone3_run, @function
	.p2align 4
liella_machine_clone3_run:
	movq %rdx, %r8
	movq %rcx, %r9
	movl $435, %eax			/* SYS_clone3: args in rdi, size in rsi */
	syscall
	testq %rax, %rax
	jz 1f
	ret
1:	xorl %ebp, %ebp
	movq %r9, %rdi
	callq *%r8
	movl $127, %edi
	movl $231, %eax			/* SYS_exit_group */
	syscall
	hlt
	.size liella_machine_clone3_run, .-liella_machine_clone3_run

/* long liella_machine_clone_thread(unsigned long flags, uintptr_t stack, int *ptid, int *ctid,
 *                                  uintptr_t tls)
 * clone(2) for a thread. `stack` is 16-aligned and holds two words the caller wrote: the
 * entry function and its argument. The parent gets the new TID or -errno. The child pops the
 * two words, calls entry(arg) with a zero frame pointer, and makes the thread-only exit system
 * call with entry's result if it ever returns (the runtime's entry never does). */
	.globl liella_machine_clone_thread
	.hidden liella_machine_clone_thread
	.type liella_machine_clone_thread, @function
/* long liella_machine_clone_raw(...): the same entry, for clone(3), whose TID pointers and
 * thread pointer are the C caller's values (any of them may be 0). */
	.globl liella_machine_clone_raw
	.hidden liella_machine_clone_raw
	.type liella_machine_clone_raw, @function
	.p2align 6
liella_machine_clone_raw:
liella_machine_clone_thread:
	movq %rcx, %r10
	movl $56, %eax			/* SYS_clone: flags, newsp, parent_tid, child_tid, tls */
	syscall
	testq %rax, %rax
	jz 1f
	ret
1:	xorl %ebp, %ebp
	popq %rax			/* entry */
	popq %rdi			/* arg */
	callq *%rax
	movl %eax, %edi
	movl $60, %eax			/* SYS_exit: this thread only */
	syscall
	hlt
	.size liella_machine_clone_thread, .-liella_machine_clone_thread
	.size liella_machine_clone_raw, .-liella_machine_clone_raw

/* long liella_machine_syscall_cancel(const uint32_t *word, long nr, long a, long b, long c,
 *                                    long d, long e, long f)
 * A cancellation point's blocking system call (D-22.11). The window runs from
 * liella_machine_cancel_window to liella_machine_cancel_window_end: the load of the thread's
 * cancellation word through the `syscall` instruction. If the word says "requested and
 * enabled" (its bits DISABLED | REQUESTED | ACTING, 1 | 4 | 8, equal REQUESTED;
 * liella_rt::cancel), or if the internal signal's handler finds the interrupted program counter
 * inside the window and moves it to liella_machine_cancel_exit, the call returns -ECANCELED
 * and the system call had no effect (it was not made, or the kernel's restart is abandoned).
 * Otherwise it returns the kernel's result. The stack pointer never moves, so the call-frame
 * information is the default row (the return address at the stack pointer): `backtrace`
 * (D-2.7) then unwinds out of a blocking call a signal handler interrupted.
 *
 * long liella_machine_syscall_cancel4(const uint32_t *word, long nr, long a, long b, long c, long d)
 * The same for system calls of at most four arguments (CANCEL-FAST, D-22.11): every argument
 * arrives in a register, so the caller needs no stack arguments. Both entries move the word's
 * address to %r11 (clobbered by `syscall` anyway) and the arguments to the kernel's registers
 * BEFORE the window: the window itself, shared by both, is only the load of the word, the test
 * and the `syscall` instruction. A request arriving before the load is seen by the load; one
 * arriving between the load and the `syscall` instruction (or while the kernel restarts it) is
 * redirected by the handler; both as before the split. */
	.globl liella_machine_syscall_cancel
	.hidden liella_machine_syscall_cancel
	.type liella_machine_syscall_cancel, @function
	.globl liella_machine_syscall_cancel4
	.hidden liella_machine_syscall_cancel4
	.type liella_machine_syscall_cancel4, @function
/* The window itself, entered by `call` from the direct points (machine/points.S, CANCEL-FAST-2)
 * with the word's address in %r11 and the system call already in the kernel's registers. */
	.globl liella_machine_cancel_window
	.hidden liella_machine_cancel_window
	.p2align 6
	.cfi_startproc
liella_machine_syscall_cancel:
	movq %rdi, %r11
	movq %rsi, %rax
	movq %rdx, %rdi
	movq %rcx, %rsi
	movq %r8, %rdx
	movq %r9, %r10
	movq 8(%rsp), %r8
	movq 16(%rsp), %r9
	jmp liella_machine_cancel_window
liella_machine_syscall_cancel4:
	movq %rdi, %r11
	movq %rsi, %rax
	movq %rdx, %rdi
	movq %rcx, %rsi
	movq %r8, %rdx
	movq %r9, %r10
liella_machine_cancel_window:
	movl (%r11), %r11d
	andl $13, %r11d
	cmpl $4, %r11d
	je liella_machine_cancel_exit
	syscall
liella_machine_cancel_window_end:
	ret
liella_machine_cancel_exit:
	movq $-125, %rax		/* -ECANCELED */
	ret
	.cfi_endproc
	.size liella_machine_syscall_cancel, .-liella_machine_syscall_cancel
	.size liella_machine_syscall_cancel4, .-liella_machine_syscall_cancel4

/* void liella_machine_cancel_bounds(uintptr_t out[3]): the window's start and end and the
 * cancelled exit, as addresses (the internal signal's handler compares the interrupted
 * program counter with them). */
	.globl liella_machine_cancel_bounds
	.hidden liella_machine_cancel_bounds
	.type liella_machine_cancel_bounds, @function
	.p2align 6
liella_machine_cancel_bounds:
	leaq liella_machine_cancel_window(%rip), %rax
	movq %rax, (%rdi)
	leaq liella_machine_cancel_window_end(%rip), %rax
	movq %rax, 8(%rdi)
	leaq liella_machine_cancel_exit(%rip), %rax
	movq %rax, 16(%rdi)
	ret
	.size liella_machine_cancel_bounds, .-liella_machine_cancel_bounds
	.section .note.GNU-stack,"",@progbits
