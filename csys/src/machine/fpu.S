/* Class machine: MXCSR and x87 control/status/environment access (owner decision OD-C1).
 * Values pass in registers; memory operands use the red zone below %rsp (SysV ABI). */
	.text
	.globl liella_machine_mxcsr_get
	.type liella_machine_mxcsr_get, @function
	.p2align 6
liella_machine_mxcsr_get:
	stmxcsr -4(%rsp)
	movl -4(%rsp), %eax
	ret
	.size liella_machine_mxcsr_get, .-liella_machine_mxcsr_get
	.text
	.globl liella_machine_mxcsr_set
	.type liella_machine_mxcsr_set, @function
	.p2align 6
liella_machine_mxcsr_set:
	movl %edi, -4(%rsp)
	ldmxcsr -4(%rsp)
	ret
	.size liella_machine_mxcsr_set, .-liella_machine_mxcsr_set
	.text
	.globl liella_machine_x87_cw_get
	.type liella_machine_x87_cw_get, @function
	.p2align 6
liella_machine_x87_cw_get:
	fnstcw -2(%rsp)
	movzwl -2(%rsp), %eax
	ret
	.size liella_machine_x87_cw_get, .-liella_machine_x87_cw_get
	.text
	.globl liella_machine_x87_cw_set
	.type liella_machine_x87_cw_set, @function
	.p2align 6
liella_machine_x87_cw_set:
	movw %di, -2(%rsp)
	fldcw -2(%rsp)
	ret
	.size liella_machine_x87_cw_set, .-liella_machine_x87_cw_set
	.text
	.globl liella_machine_x87_sw_get
	.type liella_machine_x87_sw_get, @function
	.p2align 6
liella_machine_x87_sw_get:
	fnstsw %ax
	movzwl %ax, %eax
	ret
	.size liella_machine_x87_sw_get, .-liella_machine_x87_sw_get
	.text
	.globl liella_machine_x87_clex
	.type liella_machine_x87_clex, @function
	.p2align 6
liella_machine_x87_clex:
	fnclex
	ret
	.size liella_machine_x87_clex, .-liella_machine_x87_clex
	.text
	.globl liella_machine_x87_env_store
	.type liella_machine_x87_env_store, @function
	.p2align 6
liella_machine_x87_env_store:
	fnstenv (%rdi)
	fldenv (%rdi)
	ret
	.size liella_machine_x87_env_store, .-liella_machine_x87_env_store
	.text
	.globl liella_machine_x87_env_load
	.type liella_machine_x87_env_load, @function
	.p2align 6
liella_machine_x87_env_load:
	fldenv (%rdi)
	ret
	.size liella_machine_x87_env_load, .-liella_machine_x87_env_load
	.text
	.globl liella_machine_mxcsr_mask
	.type liella_machine_mxcsr_mask, @function
	.p2align 6
liella_machine_mxcsr_mask:
	subq $520, %rsp
	movq %rsp, %rax
	andq $-16, %rax
	fxsave (%rax)
	movl 28(%rax), %eax
	addq $520, %rsp
	ret
	.size liella_machine_mxcsr_mask, .-liella_machine_mxcsr_mask
/* The hardware square roots (M6-2): correctly rounded in the unit's current rounding direction,
 * raising their flags in that unit (IEEE 754 squareRoot; Intel SDM SQRTSS/SQRTSD/FSQRT). */
	.text
	.globl liella_machine_sqrtsd
	.type liella_machine_sqrtsd, @function
	.p2align 4
liella_machine_sqrtsd:
	sqrtsd %xmm0, %xmm0
	ret
	.size liella_machine_sqrtsd, .-liella_machine_sqrtsd
	.text
	.globl liella_machine_sqrtss
	.type liella_machine_sqrtss, @function
	.p2align 4
liella_machine_sqrtss:
	sqrtss %xmm0, %xmm0
	ret
	.size liella_machine_sqrtss, .-liella_machine_sqrtss
	.text
	.globl liella_machine_fsqrt
	.type liella_machine_fsqrt, @function
	.p2align 4
liella_machine_fsqrt:
	fldt (%rdi)
	fsqrt
	fstpt (%rsi)
	ret
	.size liella_machine_fsqrt, .-liella_machine_fsqrt
	.section .note.GNU-stack,"",@progbits
