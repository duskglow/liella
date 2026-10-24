/* Class machine: PKRU read and write (owner decision OD-C1). Callers check CPUID.7:ECX.OSPKE. */
	.text
	.globl liella_machine_rdpkru
	.type liella_machine_rdpkru, @function
	.p2align 6
liella_machine_rdpkru:
	xorl %ecx, %ecx
	rdpkru
	ret
	.size liella_machine_rdpkru, .-liella_machine_rdpkru
	.text
	.globl liella_machine_wrpkru
	.type liella_machine_wrpkru, @function
	.p2align 6
liella_machine_wrpkru:
	movl %edi, %eax
	xorl %ecx, %ecx
	xorl %edx, %edx
	wrpkru
	ret
	.size liella_machine_wrpkru, .-liella_machine_wrpkru
	.section .note.GNU-stack,"",@progbits
