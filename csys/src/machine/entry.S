/* Class machine: process entry (_start), the content of liella-crt1.o (spec §1.1, IP-9).
 *
 * The kernel enters with %rsp at argc, followed by argv[], NULL, envp[], NULL and the auxiliary
 * vector (⚑ KERN); %rdx holds the dynamic linker's termination function (0 for a static
 * program). As glibc's crt1 does since 2.34, this calls
 *   __libc_start_main(main, argc, argv, init = 0, fini = 0, rtld_fini, stack_end)
 * with the stack aligned to 16 bytes, and never returns. `cargo xtask so` assembles this file
 * alone into the start object; crates/boundary/build.rs leaves it out of the C layer's archive,
 * so no other link ever sees a second _start.
 *
 * The start of the executable's .eh_frame (D-2.7). A static link has no .eh_frame_hdr (the
 * compiler driver asks the linker for one only in dynamic links), so `backtrace` finds a static
 * executable's call-frame information between two link-time labels: liella_eh_frame_start,
 * at the start of this object's .eh_frame, which the linker places first because the start
 * object comes first in a link, and the end label in adapter/runtime.c. _start's own row marks
 * it as the outermost frame (its return-address column is undefined).
 *
 * _start lives in .text.startup (HOT-ORDER): GNU ld's default script places that section, with a
 * -O2 main, right before the hot text group, so every program's entry shares the 64 KiB
 * fault-around window of the startup functions instead of costing a page fault of its own.
 *
 * Assembled with -DLIELLA_STATIC_PIE it is liella-rcrt1.o, the start object of a static PIE
 * (D-27.10): the same _start, calling liella_start_main_static_pie (same arguments), which
 * relocates the executable before anything reads an address from its data. The choice is made
 * at link time, so the start of every other program tests nothing. A static PIE runs this code
 * before its relocation: every reference here is PC-relative (no GOT slot, which would still
 * hold a link-time address); `cargo xtask so` refuses a GOT-typed relocation in the object. */
#ifdef LIELLA_LD
/* Assembled with -DLIELLA_LD it is the entry of ld.so (phase 14 DL-5, R14-8 "program entry", D-27.13),
 * liella_machine_ld_entry, the product's ELF entry point. Register convention: the kernel enters
 * with %rsp at argc (the initial stack as above); this clears %rbp (outermost frame), passes that
 * stack pointer in %rdi and the run-time address of ld.so's own ELF header (__ehdr_start, taken
 * PC-relatively) in %rsi, aligns %rsp to 16 bytes and calls liella_ld_start, which never returns.
 * It runs before ld.so is relocated: both references are PC-relative to hidden symbols (no GOT, no
 * PLT), so nothing reads a stored address before the relocation loop; `cargo xtask so` refuses an
 * ld.so whose code reads its GOT at all (L14-1). Declared once in boundary/src/arch/. */
	.section .text.liella_machine_ld_entry,"ax",@progbits
	.globl liella_machine_ld_entry
	.hidden liella_machine_ld_entry
	.hidden liella_ld_start
	.hidden __ehdr_start
	.type liella_machine_ld_entry, @function
	.p2align 4
liella_machine_ld_entry:
	.cfi_startproc
	.cfi_undefined %rip
	xorl %ebp, %ebp
	movq %rsp, %rdi
	leaq __ehdr_start(%rip), %rsi
	andq $-16, %rsp
	call liella_ld_start
	hlt
	.cfi_endproc
	.size liella_machine_ld_entry, .-liella_machine_ld_entry

/* liella_machine_ld_enter(sp, entry, fini) (phase 14 DL-6, R14-8 "program entry transfer", D-27.14,
 * DL-11 D-27.19): hand the process to a program's entry point. Register convention: %rdi = the stack
 * pointer the program gets (argc at it, then argv, envp and the auxiliary vector, laid out as the
 * kernel's initial stack), %rsi = the entry address, %rdx = the termination function the program
 * registers with atexit (0 = none), left in %rdx as x86-64 psABI §3.4.1 "Process Initialization"
 * places it. %rsp = %rdi, %rbp = 0 (outermost frame). Never returns; no logic (K2). Declared once in
 * boundary/src/arch/. */
	.section .text.liella_machine_ld_enter,"ax",@progbits
	.globl liella_machine_ld_enter
	.hidden liella_machine_ld_enter
	.type liella_machine_ld_enter, @function
	.p2align 4
liella_machine_ld_enter:
	.cfi_startproc
	.cfi_undefined %rip
	movq %rdi, %rsp
	xorl %ebp, %ebp
	jmp *%rsi
	.cfi_endproc
	.size liella_machine_ld_enter, .-liella_machine_ld_enter
#else
	/* The start-up data clusters (D-X.62, D-X.64, xtask/src/startup_ro.rs): this object comes first in
	 * a link, so its empty section comes first in each output section and gives the cluster its start
	 * alignment (the start-up relro objects stay beside `.got`: a head cluster took a window of its own).
	 * `cargo xtask so` passes each alignment: the smallest power of two (at least 4 KiB, at most
	 * 64 KiB) not below the cluster's size over the whole archive, so a cluster of at most 64 KiB
	 * never straddles a 64 KiB fault-around window, and the written cluster takes the fewest pages.
	 * The defaults are the largest values (a build that does not pass them). */
#ifndef LIELLA_RODATA1_ALIGN
#define LIELLA_RODATA1_ALIGN 65536
#endif
#ifndef LIELLA_DATA1_ALIGN
#define LIELLA_DATA1_ALIGN 65536
#endif
#ifndef LIELLA_RELRO_ALIGN
#define LIELLA_RELRO_ALIGN 65536
#endif
#ifdef LIELLA_STATIC_PIE
	/* LAYOUT-DATA-CLUSTERS-4 (D-X.68): a static PIE relocates itself with exactly one writable
	 * PT_LOAD (crates/ld/src/boot.rs `image`). The linker starts a new segment where the gap before
	 * a section reaches a whole page: a 16 KiB `.data1` after `.data` left a 3-page gap and a
	 * second writable segment, and the start refused the executable (exit 127). At 4 KiB the gap
	 * stays under a page, and the written cluster still takes ceil(size / 4 KiB) pages. (The relro
	 * alignment kept one segment in the same test, `.tdata` and `.init_array` ahead of it.) */
#undef LIELLA_DATA1_ALIGN
#define LIELLA_DATA1_ALIGN 4096
#endif
	.section .rodata1,"a",@progbits
	.balign LIELLA_RODATA1_ALIGN
	.section .data1,"aw",@progbits
	.balign LIELLA_DATA1_ALIGN
	/* LAYOUT-DATA-CLUSTERS-3 (D-X.67): the relro output section starts on the same kind of boundary
	 * (its size over the archive), so the relro objects start-up reads share one fault-around window
	 * wherever text and read-only data end (a 28 KiB relro across a 64 KiB boundary cost tool and
	 * setlocale a second window, release profile). */
	.section .data.rel.ro,"aw",@progbits
	.balign LIELLA_RELRO_ALIGN
	.section .eh_frame,"a",@unwind
	.globl liella_eh_frame_start
	.hidden liella_eh_frame_start
liella_eh_frame_start:
	.section .text.startup._start,"ax",@progbits
	.globl _start
	.type _start, @function
	.p2align 6
_start:
	.cfi_startproc
	.cfi_undefined %rip
	xorl %ebp, %ebp
	movq %rdx, %r9
	popq %rsi
	movq %rsp, %rdx
	andq $-16, %rsp
	pushq %rax
	pushq %rsp
	xorl %r8d, %r8d
	xorl %ecx, %ecx
#ifdef LIELLA_STATIC_PIE
	leaq main(%rip), %rdi
	call liella_start_main_static_pie
#else
	movq main@GOTPCREL(%rip), %rdi
	call *__libc_start_main@GOTPCREL(%rip)
#endif
	hlt
	.cfi_endproc
	.size _start, .-_start
#endif
	.section .note.GNU-stack,"",@progbits
