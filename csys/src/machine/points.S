/* Class machine: the direct cancellation-point entries (CANCEL-FAST-2, D-22.11 "Direct points";
 * plan S2 hot-kernel). In the products these ARE `read`, `write`, `pread`/`pread64`,
 * `pwrite`/`pwrite64` and `close`/`__close` (second labels under LIELLA_EXPORT, versions from the
 * Rust twins' `export! { asm ... }`): a call reaches the system call with no Rust hop. Test builds
 * define only the liella_machine_* names; the Rust twins (crates/boundary/src/posix/s11_lowio.rs)
 * call them, so the tests run this code.
 *
 * Why assembly: the Rust export spent ~40 instructions per call around the system call (the
 * slice check, the thread-pointer and descriptor loads, the argument shuffle into the shared
 * window, the result conversion); glibc's wrapper is ~21. Here the common case is the descriptor
 * load, three tests, the shared window (machine/thread_start.S) and one result test.
 *
 * Every entry takes the Rust path (`liella_*_rest`: the full former export, unchanged) for
 * whatever is not the common case: no descriptor yet (`%fs:0xa0` is 0: a thread the runtime did
 * not create and has not adopted), a count of 0 or above isize::MAX, a NULL buffer (the export's
 * slice rules, EFAULT without a system call). A kernel error result (-4095..-1, which includes the
 * window's -ECANCELED) tail-jumps to `liella_cancel_point_fail` with the raw result: it acts on a
 * due request (the cancellation word, checked there) and otherwise sets errno and returns -1.
 *
 * CONTRACT: the C contracts of the exports; nothing is dereferenced here except the thread block
 * word at %fs:0xa0 (`ThreadBlock::thread`, asserted in crates/rt/src/tls.rs) and, through the
 * window, the descriptor's cancellation word at offset 32 (`Thread::cancel`, asserted in
 * crates/rt/src/thread.rs). The stack pointer never moves: the default call-frame row describes
 * every instruction, so `backtrace` from a signal handler in the window unwinds through here.
 * No .macro (the affected map reads this file for the symbols it defines and uses). */

	.text
	.hidden liella_machine_cancel_window	/* machine/thread_start.S */
	.hidden liella_cancel_point_fail	/* crates/boundary/src/posix/s11_lowio.rs */
	.hidden liella_close_point_fail
	.hidden liella_read_rest
	.hidden liella_write_rest
	.hidden liella_pread_rest
	.hidden liella_pwrite_rest
	.hidden liella_close_rest
	.hidden liella_machine_aio_used

/* ssize_t liella_machine_read(int fd, void *buf, size_t n) */
	.globl liella_machine_read
	.hidden liella_machine_read
	.type liella_machine_read, @function
	.p2align 6
	.cfi_startproc
liella_machine_read:
#ifdef LIELLA_EXPORT
	.globl read
	.type read, @function
read:
#endif
	movq %fs:0xa0, %r11
	testq %r11, %r11
	jz liella_read_rest
	testq %rdx, %rdx
	jle liella_read_rest
	testq %rsi, %rsi
	jz liella_read_rest
	addq $32, %r11
	xorl %eax, %eax			/* SYS_read */
	call liella_machine_cancel_window
	cmpq $-4095, %rax
	jae 1f
	ret
1:	movq %rax, %rdi
	jmp liella_cancel_point_fail
	.cfi_endproc
	.size liella_machine_read, .-liella_machine_read

/* ssize_t liella_machine_write(int fd, const void *buf, size_t n) */
	.globl liella_machine_write
	.hidden liella_machine_write
	.type liella_machine_write, @function
	.p2align 6
	.cfi_startproc
liella_machine_write:
#ifdef LIELLA_EXPORT
	.globl write
	.type write, @function
write:
#endif
	movq %fs:0xa0, %r11
	testq %r11, %r11
	jz liella_write_rest
	testq %rdx, %rdx
	jle liella_write_rest
	testq %rsi, %rsi
	jz liella_write_rest
	addq $32, %r11
	movl $1, %eax			/* SYS_write */
	call liella_machine_cancel_window
	cmpq $-4095, %rax
	jae 1f
	ret
1:	movq %rax, %rdi
	jmp liella_cancel_point_fail
	.cfi_endproc
	.size liella_machine_write, .-liella_machine_write

/* ssize_t liella_machine_pread(int fd, void *buf, size_t n, off_t off) */
	.globl liella_machine_pread
	.hidden liella_machine_pread
	.type liella_machine_pread, @function
	.p2align 6
	.cfi_startproc
liella_machine_pread:
#ifdef LIELLA_EXPORT
	.globl pread
	.type pread, @function
pread:
	.globl pread64
	.type pread64, @function
pread64:
#endif
	movq %fs:0xa0, %r11
	testq %r11, %r11
	jz liella_pread_rest
	testq %rdx, %rdx
	jle liella_pread_rest
	testq %rsi, %rsi
	jz liella_pread_rest
	addq $32, %r11
	movq %rcx, %r10
	movl $17, %eax			/* SYS_pread64 */
	call liella_machine_cancel_window
	cmpq $-4095, %rax
	jae 1f
	ret
1:	movq %rax, %rdi
	jmp liella_cancel_point_fail
	.cfi_endproc
	.size liella_machine_pread, .-liella_machine_pread

/* ssize_t liella_machine_pwrite(int fd, const void *buf, size_t n, off_t off) */
	.globl liella_machine_pwrite
	.hidden liella_machine_pwrite
	.type liella_machine_pwrite, @function
	.p2align 6
	.cfi_startproc
liella_machine_pwrite:
#ifdef LIELLA_EXPORT
	.globl pwrite
	.type pwrite, @function
pwrite:
	.globl pwrite64
	.type pwrite64, @function
pwrite64:
#endif
	movq %fs:0xa0, %r11
	testq %r11, %r11
	jz liella_pwrite_rest
	testq %rdx, %rdx
	jle liella_pwrite_rest
	testq %rsi, %rsi
	jz liella_pwrite_rest
	addq $32, %r11
	movq %rcx, %r10
	movl $18, %eax			/* SYS_pwrite64 */
	call liella_machine_cancel_window
	cmpq $-4095, %rax
	jae 1f
	ret
1:	movq %rax, %rdi
	jmp liella_cancel_point_fail
	.cfi_endproc
	.size liella_machine_pwrite, .-liella_machine_pwrite

/* int liella_machine_close(int fd): the descriptor is closed regardless, outside the window
 * (§22.3), then a due request is acted on (in liella_close_point_fail, which also sets errno).
 * Any asynchronous I/O ever started (liella_machine_aio_used, set by crates/boundary/src/aio.rs
 * before its first request or internal descriptor) sends every close to the Rust path, which
 * cancels the descriptor's requests first. */
	.globl liella_machine_close
	.hidden liella_machine_close
	.type liella_machine_close, @function
	.p2align 6
	.cfi_startproc
liella_machine_close:
#ifdef LIELLA_EXPORT
	.globl close
	.type close, @function
close:
	.globl __close
	.type __close, @function
__close:
#endif
	cmpb $0, liella_machine_aio_used(%rip)
	jne liella_close_rest
	movl $3, %eax			/* SYS_close */
	syscall
	movq %fs:0xa0, %r11
	testq %r11, %r11
	jz 1f
	movl 32(%r11), %r11d
	andl $13, %r11d			/* DISABLED | REQUESTED | ACTING */
	cmpl $4, %r11d			/* == REQUESTED: due */
	je 2f
1:	cmpq $-4095, %rax
	jae 2f
	ret
2:	movq %rax, %rdi
	jmp liella_close_point_fail
	.cfi_endproc
	.size liella_machine_close, .-liella_machine_close

/* The asynchronous-I/O flag (read above; written only by crates/boundary/src/aio.rs). */
	.data
	.globl liella_machine_aio_used
	.type liella_machine_aio_used, @object
liella_machine_aio_used:
	.byte 0
	.size liella_machine_aio_used, 1
	.section .note.GNU-stack,"",@progbits
