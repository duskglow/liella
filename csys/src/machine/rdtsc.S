/* Class machine: time-stamp counter read. rdtsc leaves the low 32 bits in %eax and the high 32 in
 * %edx (both zero-extended); the result is their combination. Not serialising: the caller uses
 * it only as a coarse elapsed-time gate (D-7.12, GETTEXT-6). */
	.text
	.globl liella_machine_rdtsc
	.hidden liella_machine_rdtsc
	.type liella_machine_rdtsc, @function
	.p2align 4
liella_machine_rdtsc:
	rdtsc
	shlq $32, %rdx
	orq %rdx, %rax
	ret
	.size liella_machine_rdtsc, .-liella_machine_rdtsc
	.section .note.GNU-stack,"",@progbits
