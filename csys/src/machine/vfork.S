/* Class machine: vfork (spec §18.2; D-18.1, owner ruling #410, VFORK-CLONE-VM). Status: approved.
 *
 * vfork() is SYS_vfork, which is clone(CLONE_VM | CLONE_VFORK | SIGCHLD): the child shares the
 * caller's memory (no page-table copy, the parent's pages stay writable) and the caller is
 * suspended until the child calls execve or _exit, as in glibc. The child runs on the caller's
 * stack, so this entry keeps nothing there across the syscall: the return address is popped
 * into a register (the child's return, and whatever it calls next, overwrite that slot) and
 * pushed back by each of the two returns. No signal is blocked (glibc blocks none). An error
 * (-4095..-1) goes to liella_vfork_fail (Rust, local), which sets errno and returns -1.
 *
 * The glibc names exist only in the products (LIELLA_EXPORT), so that tests never interpose
 * on the host C library. */
	.text
#ifdef LIELLA_EXPORT
	.globl __vfork
	.type __vfork, @function
	.globl vfork
	.type vfork, @function
	.p2align 6
__vfork:
vfork:
	popq %rdi			/* the return address, out of the shared stack */
	movl $58, %eax			/* SYS_vfork */
	syscall
	pushq %rdi
	cmpq $-4095, %rax
	jae 1f
	ret
1:	movq %rax, %rdi			/* the parent, no child: errno from the raw result */
	jmp liella_vfork_fail
	.size vfork, .-vfork
	.size __vfork, .-__vfork
#endif
	.section .note.GNU-stack,"",@progbits
