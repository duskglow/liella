/* Class machine: call a vDSO function by address (spec Appendix A.2 vDSO).
 * liella_machine_vcall2(fn, a, b) and liella_machine_vcall3(fn, a, b, c) tail-call fn. */
	.text
	.section .text.liella_machine_vcall2,"ax",@progbits
	.globl liella_machine_vcall2
	.type liella_machine_vcall2, @function
	.p2align 6
liella_machine_vcall2:
	movq %rdi, %rax
	movq %rsi, %rdi
	movq %rdx, %rsi
	jmp *%rax
	.size liella_machine_vcall2, .-liella_machine_vcall2
	.text
	.text
	.section .text.liella_machine_vcall3,"ax",@progbits
	.globl liella_machine_vcall3
	.type liella_machine_vcall3, @function
	.p2align 6
liella_machine_vcall3:
	movq %rdi, %rax
	movq %rsi, %rdi
	movq %rdx, %rsi
	movq %rcx, %rdx
	jmp *%rax
	.size liella_machine_vcall3, .-liella_machine_vcall3
	.text
	.section .note.GNU-stack,"",@progbits
