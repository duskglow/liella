/* Class machine: the architecture trap instruction `abort` executes if the process survives
 * SIGABRT (spec §1.2, abort clause 2). `hlt` is privileged, so in user mode it faults (SIGSEGV),
 * as glibc's ABORT_INSTRUCTION does on x86-64. If even that is survived, the instruction
 * returns and the caller exits with status 127. */
	.text
	.globl liella_machine_trap
	.type liella_machine_trap, @function
	.p2align 6
liella_machine_trap:
	hlt
	ret
	.size liella_machine_trap, .-liella_machine_trap
	.section .note.GNU-stack,"",@progbits
