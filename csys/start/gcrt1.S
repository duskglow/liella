/* Class machine: the profiling start object, the content of liella-gcrt1.o (spec §28.1,
 * GCRT1-START). A `-pg` program links it instead of liella-crt1.o: the same _start (entry.S),
 * plus __gmon_start__, which starts profiling over the executable's text and has gmon.out
 * written at exit, as the compiler driver's gcrt1.o does with glibc.
 *
 * Probe-settled against glibc 2.39 (GCRT1-START, 2026-10-23):
 * - the profiled range is the executable's text, __executable_start up to the end of text
 *   (_etext, rounded up to 4 bytes by monstartup);
 * - profiling starts before the program's own constructors (an arc out of a constructor is
 *   recorded), so the hook is in .preinit_array, which runs before every .init_array entry;
 * - gmon.out is written by _mcleanup, registered with atexit: on return from main and on
 *   exit, not on _exit or a fatal signal; a forked child that exits writes its own (the same
 *   name, or $GMON_OUT_PREFIX.<pid>, which secure mode ignores: _mcleanup reads it with
 *   secure_getenv);
 * - a -pg executable defines __gmon_start__ (global, text).
 *
 * __gmon_start__ runs its work once: an object that calls it again (a toolchain's crti.o
 * calls __gmon_start__ when defined) does not register _mcleanup twice. The calls go through
 * the GOT, as in entry.S, and the linker relaxes them in a static link. The reserved names
 * __monstartup and _etext keep a program's own monstartup or etext from being taken. */
/* Outside csys/src on purpose: crates/boundary/build.rs archives every source under csys/src,
 * and no archive may carry _start or this .preinit_array entry. */
#include "../src/machine/entry.S"

	.section .text.startup.__gmon_start__,"ax",@progbits
	.globl __gmon_start__
	.type __gmon_start__, @function
	.p2align 6
__gmon_start__:
	.cfi_startproc
	cmpb $0, liella_gmon_started(%rip)
	jne 1f
	movb $1, liella_gmon_started(%rip)
	/* Entered with %rsp 8 below a 16-byte boundary: align it for the calls. */
	subq $8, %rsp
	.cfi_adjust_cfa_offset 8
	leaq __executable_start(%rip), %rdi
	leaq _etext(%rip), %rsi
	call *__monstartup@GOTPCREL(%rip)
	movq _mcleanup@GOTPCREL(%rip), %rdi
	call *atexit@GOTPCREL(%rip)
	addq $8, %rsp
	.cfi_adjust_cfa_offset -8
1:	ret
	.cfi_endproc
	.size __gmon_start__, .-__gmon_start__

	.section .preinit_array,"aw",@preinit_array
	.p2align 3
	.quad __gmon_start__

	.local liella_gmon_started
	.comm liella_gmon_started, 1, 1
	.section .note.GNU-stack,"",@progbits
