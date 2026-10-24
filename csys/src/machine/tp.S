/* Class machine: thread-pointer read (FS base). x86_64 TLS ABI: the word at %fs:0 holds the
 * thread pointer itself (spec Appendix A.2, KERN/ABI). */
	.text
	.globl liella_machine_tp
	.hidden liella_machine_tp
	.type liella_machine_tp, @function
	.p2align 6
liella_machine_tp:
	movq %fs:0, %rax
	ret
	.size liella_machine_tp, .-liella_machine_tp
	.section .note.GNU-stack,"",@progbits
