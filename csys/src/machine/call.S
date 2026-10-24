/* Class machine: call code at an address (owner decision OD-C1): ELF constructors and entry points
 * in ld.so. liella_machine_callN(fn, args...) tail-calls fn with the arguments shifted down. */
	.text
	.globl liella_machine_call0
	.type liella_machine_call0, @function
	.p2align 6
liella_machine_call0:
	jmp *%rdi
	.size liella_machine_call0, .-liella_machine_call0
	.text
	.section .text.liella_machine_call3,"ax",@progbits
	.globl liella_machine_call3
	.type liella_machine_call3, @function
	.p2align 6
liella_machine_call3:
	movq %rdi, %rax
	movq %rsi, %rdi
	movq %rdx, %rsi
	movq %rcx, %rdx
	jmp *%rax
	.size liella_machine_call3, .-liella_machine_call3
	.text
	.text
	.globl liella_machine_call4
	.type liella_machine_call4, @function
	.p2align 6
liella_machine_call4:
	movq %rdi, %rax
	movq %rsi, %rdi
	movq %rdx, %rsi
	movq %rcx, %rdx
	movq %r8, %rcx
	jmp *%rax
	.size liella_machine_call4, .-liella_machine_call4
	.section .note.GNU-stack,"",@progbits
