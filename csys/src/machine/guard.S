/* Class machine: a frame for library code that calls C callbacks (OD-6 revised, S11). Status: approved.
 *
 * long liella_machine_guard(long (*body)(void *), void *ctx, void (*undo)(void *)): runs
 * body(ctx) in this frame and returns its result. A C++ exception thrown by a callback that
 * body calls (or a forced unwind) passes this frame on its way to the caller's handler; the
 * frame's call-frame information names a personality routine (liella_guard_personality,
 * boundary/src/cancel.rs) that, during the cleanup phase, removes the frame's record and runs
 * undo(ctx), which puts the library's state back in order (a sort's elements back in the
 * array, its buffer freed, directory handles closed, the working directory restored).
 * As in once.S, no unwinder function is called: the personality finds undo and ctx through
 * the cancellation record liella_guard_begin pushed in this frame (so a cancellation acting
 * inside body runs undo too), and the library needs no libgcc_s. */
	.section .data.rel.ro.liella_guard,"aw",@progbits
	.p2align 3
.Lguard_personality:
	.quad liella_guard_personality
	.text
	.hidden liella_guard_begin
	.hidden liella_guard_end
	.hidden liella_guard_personality
	.globl liella_machine_guard
	.hidden liella_machine_guard
	.type liella_machine_guard, @function
	.p2align 4
liella_machine_guard:
	.cfi_startproc
	.cfi_personality 0x9b, .Lguard_personality	/* pcrel | sdata4 | indirect */
	pushq %rbx
	.cfi_adjust_cfa_offset 8
	.cfi_offset %rbx, -16
	pushq %r12
	.cfi_adjust_cfa_offset 8
	.cfi_offset %r12, -24
	subq $40, %rsp			/* the 4-word record; %rsp 16-aligned at the calls */
	.cfi_adjust_cfa_offset 40
	movq %rdi, %r12			/* body */
	movq %rsi, %rbx			/* ctx */
	movq %rsp, %rdi			/* record */
	movq %rdx, %rsi			/* undo */
	movq %rbx, %rdx			/* ctx */
	call liella_guard_begin
	movq %rbx, %rdi
	call *%r12			/* may unwind: the personality runs undo(ctx) */
	movq %rax, %rbx			/* the result (ctx is no longer needed) */
	movq %rsp, %rdi
	call liella_guard_end
	movq %rbx, %rax
	addq $40, %rsp
	.cfi_adjust_cfa_offset -40
	popq %r12
	.cfi_adjust_cfa_offset -8
	.cfi_restore %r12
	popq %rbx
	.cfi_adjust_cfa_offset -8
	.cfi_restore %rbx
	ret
	.cfi_endproc
	.size liella_machine_guard, .-liella_machine_guard
	.section .note.GNU-stack,"",@progbits
