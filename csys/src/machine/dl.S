/* Class machine: the dlopen, dlsym and dlvsym entries (phase 14 DL-16a and DL-16b,
 * plan/ph14/dl-16-dlopen.md §1 and §2, the caller-address seam). dlopen(file, mode) tail-calls
 * liella_dl_open(file, mode, caller), dlsym(handle, name) tail-calls
 * liella_dl_sym(handle, name, NULL, caller) and dlvsym(handle, name, version) tail-calls
 * liella_dl_sym(handle, name, version, caller), with caller = the entry's own return address: the
 * address in the calling object that dlopen's search uses as its requester and that RTLD_NEXT and
 * RTLD_DEFAULT start from. The entries are frame-free, so a compat stub's `jmp` (dlopen@GLIBC_2.2.5,
 * dlsym@GLIBC_2.2.5, dlvsym@GLIBC_2.2.5) keeps the caller's return address on top of the stack too.
 *
 * The glibc names exist only in the products (LIELLA_EXPORT). */
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

	.section .text.dlsym,"ax",@progbits
	.hidden liella_dl_sym
	.globl dlsym
	.type dlsym, @function
	.p2align 6
dlsym:
	.cfi_startproc
	movq (%rsp), %rcx
	xorl %edx, %edx
	jmp liella_dl_sym
	.cfi_endproc
	.size dlsym, .-dlsym

	.section .text.dlvsym,"ax",@progbits
	.globl dlvsym
	.type dlvsym, @function
	.p2align 6
dlvsym:
	.cfi_startproc
	movq (%rsp), %rcx
	jmp liella_dl_sym
	.cfi_endproc
	.size dlvsym, .-dlvsym
#endif
	.section .note.GNU-stack,"",@progbits
