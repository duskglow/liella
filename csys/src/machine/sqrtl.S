/* `sqrtl` and `sqrtf64x` (D-10.71): the operand stays in the caller's stack slot and on the x87
 * stack, never rebuilt from separate integer stores (a 10-byte load spanning two stores cannot
 * be forwarded and stalls; the generic forwarder's bit-struct round trip paid that twice).
 *
 * In place when the x87 control word is the default in some rounding direction (every
 * exception masked, 64-bit precision: `fpu::is_x87_default_but_rounding`) and `fxam` classes
 * the operand as a positive normal: `fsqrt` is then correctly rounded in the caller's direction
 * and raises only inexact, and no `errno` applies. Every other case (another control word,
 * zeros, subnormals and pseudo-denormals, infinities, NaNs, negative operands, the encodings
 * `fxam` reports unsupported) tail-calls the generated forwarder `liella_slow_sqrtl`
 * (csys/src/adapter/math_libm.c), which keeps the rules of D-10.20 and D-10.22. `fldt` of an
 * 80-bit operand and `fxam` raise nothing (Intel SDM, FLD and FXAM), so the forwarder sees the
 * caller's flags unchanged. */
#if defined(LIELLA_LIBM)
#define SQRTL sqrtl
#define SQRTF64X sqrtf64x
#define SLOW liella_slow_sqrtl
#else
#define SQRTL liella_fwd_sqrtl
#define SQRTF64X liella_fwd_sqrtf64x
#define SLOW liella_fwd_slow_sqrtl
#endif
	.section .text.liella_sqrtl,"ax",@progbits
	.globl SQRTL
	.type SQRTL, @function
	.globl SQRTF64X
	.type SQRTF64X, @function
	.p2align 4
SQRTL:
SQRTF64X:
liella_machine_sqrtl:	/* local: the anchor plants and audits name (docs/plants.md) */
	fnstcw -2(%rsp)
	movzwl -2(%rsp), %eax
	andl $0xf3ff, %eax
	cmpl $0x037f, %eax
	jne 2f
	fldt 8(%rsp)
	fxam
	fnstsw %ax
	andl $0x4700, %eax
	cmpl $0x0400, %eax
	jne 1f
	fsqrt
	ret
1:
	fstp %st(0)
2:
	jmp SLOW
	.size SQRTL, .-SQRTL
	.size SQRTF64X, .-SQRTF64X
	.section .note.GNU-stack,"",@progbits
