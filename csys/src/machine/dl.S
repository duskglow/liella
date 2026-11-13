/* Class machine: the dlopen entry (phase 14 DL-16a, plan/ph14/dl-16-dlopen.md §1, the caller-address
 * seam). dlopen(file, mode) tail-calls liella_dl_open(file, mode, caller) with caller = its own
 * return address, the address in the calling object that the search uses as its requester. The
 * entry is frame-free, so the compat stub's `jmp dlopen` (dlopen@GLIBC_2.2.5) keeps the caller's
 * return address on top of the stack too.
 *
 * The glibc name exists only in the products (LIELLA_EXPORT). */
#ifdef LIELLA_EXPORT
	.section .text.dlopen,"ax",@progbits
	.hidden liella_dl_open
	.globl dlopen
	.type dlopen, @function
	.p2align 6			/* exported entries are 64-byte aligned (D-4.33) */
dlopen:
	.cfi_startproc
	movq (%rsp), %rdx
	jmp liella_dl_open
	.cfi_endproc
	.size dlopen, .-dlopen
#endif
	.section .note.GNU-stack,"",@progbits
