/* Class machine: gprof call-arc entry points (spec §28.1, §28.3; PROFILING-GMON, K2).
 *
 * gcc -pg calls mcount after the function's prologue (push %rbp; mov %rsp, %rbp): the callee's
 * pc is the return address at (%rsp) and its caller's pc is the function's own return address at
 * 8(%rbp). gcc -pg -mfentry calls __fentry__ before the prologue: callee pc at (%rsp), caller pc
 * at 8(%rsp) (glibc 2.39 records arcs for both, black-box probe). The instrumented function's
 * arguments are still live, so each entry saves every argument register (%rdi %rsi %rdx %rcx
 * %r8 %r9, %rax for varargs, %r10 static chain, %xmm0-7), calls liella_mcount_record(from, self)
 * in Rust (crates/boundary/src/posix/s28_profiling.rs), restores them and returns. No logic here.
 *
 * Frames: mcount is entered after the prologue's push, with %rsp 8 mod 16, and takes 200 bytes;
 * __fentry__ is entered before any prologue, with %rsp 0 mod 16, and takes 208 bytes. Both
 * leave %rsp 16-aligned at the call into Rust. In either frame the xmm registers are at
 * 0..127 and the gprs at 128..191 (liella_mcount_save / _restore are called, hence +8 inside
 * them); the entry return address is at 200(%rsp) or 208(%rsp), __fentry__'s caller at 216.
 *
 * The glibc names exist only in the products (LIELLA_EXPORT), so that tests never interpose
 * on the host C library. */
	.text
#ifdef LIELLA_EXPORT
	.globl mcount
	.type mcount, @function
	.globl _mcount
	.type _mcount, @function
	.globl __fentry__
	.type __fentry__, @function
#endif
	.globl liella_machine_mcount
	.hidden liella_machine_mcount
	.type liella_machine_mcount, @function
	.globl liella_machine_fentry
	.hidden liella_machine_fentry
	.type liella_machine_fentry, @function

	.p2align 6
#ifdef LIELLA_EXPORT
mcount:
_mcount:
#endif
liella_machine_mcount:
	.cfi_startproc
	subq $200, %rsp
	.cfi_adjust_cfa_offset 200
	call liella_mcount_save
	movq 8(%rbp), %rdi
	movq 200(%rsp), %rsi
	call liella_mcount_record
	call liella_mcount_restore
	addq $200, %rsp
	.cfi_adjust_cfa_offset -200
	ret
	.cfi_endproc
	.size liella_machine_mcount, .-liella_machine_mcount

	.p2align 6
#ifdef LIELLA_EXPORT
__fentry__:
#endif
liella_machine_fentry:
	.cfi_startproc
	subq $208, %rsp
	.cfi_adjust_cfa_offset 208
	call liella_mcount_save
	movq 216(%rsp), %rdi
	movq 208(%rsp), %rsi
	call liella_mcount_record
	call liella_mcount_restore
	addq $208, %rsp
	.cfi_adjust_cfa_offset -208
	ret
	.cfi_endproc
	.size liella_machine_fentry, .-liella_machine_fentry

/* Save the argument registers into the caller's 200-byte frame (offsets +8 for our own return
 * address). */
	.p2align 4
	.type liella_mcount_save, @function
liella_mcount_save:
	.cfi_startproc
	movdqu %xmm0, 8(%rsp)
	movdqu %xmm1, 24(%rsp)
	movdqu %xmm2, 40(%rsp)
	movdqu %xmm3, 56(%rsp)
	movdqu %xmm4, 72(%rsp)
	movdqu %xmm5, 88(%rsp)
	movdqu %xmm6, 104(%rsp)
	movdqu %xmm7, 120(%rsp)
	movq %rax, 136(%rsp)
	movq %rcx, 144(%rsp)
	movq %rdx, 152(%rsp)
	movq %rsi, 160(%rsp)
	movq %rdi, 168(%rsp)
	movq %r8, 176(%rsp)
	movq %r9, 184(%rsp)
	movq %r10, 192(%rsp)
	ret
	.cfi_endproc
	.size liella_mcount_save, .-liella_mcount_save

/* Restore the registers liella_mcount_save saved, from the caller's frame (offsets +8 for our
 * own return address). */
	.p2align 4
	.type liella_mcount_restore, @function
liella_mcount_restore:
	.cfi_startproc
	movdqu 8(%rsp), %xmm0
	movdqu 24(%rsp), %xmm1
	movdqu 40(%rsp), %xmm2
	movdqu 56(%rsp), %xmm3
	movdqu 72(%rsp), %xmm4
	movdqu 88(%rsp), %xmm5
	movdqu 104(%rsp), %xmm6
	movdqu 120(%rsp), %xmm7
	movq 136(%rsp), %rax
	movq 144(%rsp), %rcx
	movq 152(%rsp), %rdx
	movq 160(%rsp), %rsi
	movq 168(%rsp), %rdi
	movq 176(%rsp), %r8
	movq 184(%rsp), %r9
	movq 192(%rsp), %r10
	ret
	.cfi_endproc
	.size liella_mcount_restore, .-liella_mcount_restore

/* In-process test fixtures (hidden; the glibc names are not used). gcc -mfentry emits `call
 * __fentry__` before the prologue; gcc -pg emits `push %rbp; mov %rsp, %rbp; call mcount`.
 * probe_run calls one such site `n` times from a known caller, so the tests (which may not use
 * `unsafe` or inline assembly) can check the arcs the real entry points record. No logic. */
	.globl liella_machine_probe_addrs
	.hidden liella_machine_probe_addrs
	.type liella_machine_probe_addrs, @function
	.globl liella_machine_probe_run
	.hidden liella_machine_probe_run
	.type liella_machine_probe_run, @function

	.p2align 4
liella_probe_fentry_site:
	.cfi_startproc
	call liella_machine_fentry
	ret
	.cfi_endproc
	.size liella_probe_fentry_site, .-liella_probe_fentry_site

	.p2align 4
liella_probe_mcount_site:
	.cfi_startproc
	pushq %rbp
	.cfi_adjust_cfa_offset 8
	movq %rsp, %rbp
	call liella_machine_mcount
	popq %rbp
	.cfi_adjust_cfa_offset -8
	ret
	.cfi_endproc
	.size liella_probe_mcount_site, .-liella_probe_mcount_site

/* void probe_addrs(usize out[3]): the fentry site, the mcount site, the caller (probe_run). */
	.p2align 4
liella_machine_probe_addrs:
	.cfi_startproc
	leaq liella_probe_fentry_site(%rip), %rax
	movq %rax, (%rdi)
	leaq liella_probe_mcount_site(%rip), %rax
	movq %rax, 8(%rdi)
	leaq liella_machine_probe_run(%rip), %rax
	movq %rax, 16(%rdi)
	ret
	.cfi_endproc
	.size liella_machine_probe_addrs, .-liella_machine_probe_addrs

/* void probe_run(u32 mcount, u32 n): call the mcount site (mcount != 0) or the fentry site n times. */
	.p2align 4
liella_machine_probe_run:
	.cfi_startproc
	pushq %rbx
	.cfi_adjust_cfa_offset 8
	.cfi_offset %rbx, -16
	pushq %r12
	.cfi_adjust_cfa_offset 8
	.cfi_offset %r12, -24
	pushq %r13
	.cfi_adjust_cfa_offset 8
	.cfi_offset %r13, -32
	movl %esi, %ebx
	movl %edi, %r12d
1:	testl %ebx, %ebx
	jz 3f
	testl %r12d, %r12d
	jz 2f
	call liella_probe_mcount_site
	jmp 4f
2:	call liella_probe_fentry_site
4:	decl %ebx
	jmp 1b
3:	popq %r13
	.cfi_adjust_cfa_offset -8
	popq %r12
	.cfi_adjust_cfa_offset -8
	popq %rbx
	.cfi_adjust_cfa_offset -8
	ret
	.cfi_endproc
	.size liella_machine_probe_run, .-liella_machine_probe_run

	.section .note.GNU-stack,"",@progbits
