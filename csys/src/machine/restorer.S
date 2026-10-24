/* Class machine: signal restorer trampoline (KERN: SA_RESTORER). The kernel returns from a
 * handler to this address, which invokes rt_sigreturn (15) on the signal frame. The leading
 * nop follows the convention that unwinders look one byte before a return address.
 *
 * Call-frame information (D-2.7; DWARF 5 §6.4, the LSB's .eh_frame): when the handler returns
 * here, %rsp points at the ucontext_t the kernel saved (⚑ KERN; the handler's return address
 * was the word below it). The interrupted registers are in its uc_mcontext.gregs (at 40;
 * spec/abi/layouts.tsv; R8 .. RIP below, eight bytes each), so the row says: CFA = %rsp, and
 * each register is saved at CFA + its slot. The `S` augmentation (.cfi_signal_frame) marks a
 * signal frame: the frame it returns to is the interrupted one, whose address is the next
 * instruction to run rather than a return address. The FDE starts at the nop, where an
 * unwinder looks this trampoline up. */
	.text
	.globl liella_machine_restorer
	.type liella_machine_restorer, @function
	.p2align 6
	.cfi_startproc
	.cfi_signal_frame
	.cfi_def_cfa %rsp, 0
	.cfi_offset %r8, 40
	.cfi_offset %r9, 48
	.cfi_offset %r10, 56
	.cfi_offset %r11, 64
	.cfi_offset %r12, 72
	.cfi_offset %r13, 80
	.cfi_offset %r14, 88
	.cfi_offset %r15, 96
	.cfi_offset %rdi, 104
	.cfi_offset %rsi, 112
	.cfi_offset %rbp, 120
	.cfi_offset %rbx, 128
	.cfi_offset %rdx, 136
	.cfi_offset %rax, 144
	.cfi_offset %rcx, 152
	.cfi_offset %rsp, 160
	.cfi_offset %rip, 168
	nop
liella_machine_restorer:
	movq $15, %rax
	syscall
	hlt
	.cfi_endproc
	.size liella_machine_restorer, .-liella_machine_restorer
	.section .note.GNU-stack,"",@progbits
