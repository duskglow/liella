/* COMPARE-HEAD-RETHINK prototype (scheme H: head-first, ISA-late). Clean-room: designed from the
 * measured rows, the Intel optimization manual's instruction costs and the psABI; no libc source.
 * rdi = a, rsi = b, rdx = n.  proto_memcmp: ordered (sign of first differing byte, unsigned).
 * proto_memeq: equality, 0 iff equal else exactly 1 (bcmp / __memcmpeq contract of D-4.41/D-4.43).
 * Entry for n >= 16: the 16-byte block at 0 is compared BEFORE any size-class or ISA dispatch
 * (legacy SSE2 encoding: valid on every x86-64). The only ISA test is at the >128 long class.
 * n == 0 touches no memory; for n >= 1 the first access is at offset 0 (a NULL pointer faults). */
	.text

	.globl	proto_use_avx2
	.data
proto_use_avx2:	.byte 0
	.text

	.globl	proto_null
	.type	proto_null,@function
	.p2align 4
proto_null:
	xorl	%eax, %eax
	ret
	.size	proto_null, .-proto_null

/* 4 x 16-byte blocks at disp(reg) .. disp+63(reg); x0,x2,x4,x6 = per-block pcmpeqb, x1 = AND,
 * eax = mask - 0xffff (0 iff the 64 bytes are equal), ZF set accordingly. */
.macro GROUP4D disp, reg
	movdqu	\disp(%rdi,\reg), %xmm0
	movdqu	\disp(%rsi,\reg), %xmm1
	movdqu	(\disp+16)(%rdi,\reg), %xmm2
	movdqu	(\disp+16)(%rsi,\reg), %xmm3
	movdqu	(\disp+32)(%rdi,\reg), %xmm4
	movdqu	(\disp+32)(%rsi,\reg), %xmm5
	movdqu	(\disp+48)(%rdi,\reg), %xmm6
	movdqu	(\disp+48)(%rsi,\reg), %xmm7
	pcmpeqb	%xmm1, %xmm0
	pcmpeqb	%xmm3, %xmm2
	pcmpeqb	%xmm5, %xmm4
	pcmpeqb	%xmm7, %xmm6
	movdqa	%xmm0, %xmm1
	pand	%xmm2, %xmm1
	movdqa	%xmm4, %xmm3
	pand	%xmm6, %xmm3
	pand	%xmm3, %xmm1
	pmovmskb %xmm1, %eax
	subl	$0xffff, %eax
.endm

/* blocks 16, 32, 48 (bytes 16..63): x0, x2, x4 per block, x1 = AND, eax/ZF as above */
.macro GROUP3A
	movdqu	16(%rdi), %xmm0
	movdqu	16(%rsi), %xmm1
	movdqu	32(%rdi), %xmm2
	movdqu	32(%rsi), %xmm3
	movdqu	48(%rdi), %xmm4
	movdqu	48(%rsi), %xmm5
	pcmpeqb	%xmm1, %xmm0
	pcmpeqb	%xmm3, %xmm2
	pcmpeqb	%xmm5, %xmm4
	movdqa	%xmm0, %xmm1
	pand	%xmm2, %xmm1
	pand	%xmm4, %xmm1
	pmovmskb %xmm1, %eax
	subl	$0xffff, %eax
.endm

/* ---------------------------------------------------------------- ordered compare */
	.globl	proto_memcmp
	.type	proto_memcmp,@function
	.p2align 5
proto_memcmp:
	cmpq	$16, %rdx
	jb	.Lm_small
	movdqu	(%rdi), %xmm0
	movdqu	(%rsi), %xmm1
	pcmpeqb	%xmm1, %xmm0
	pmovmskb %xmm0, %eax
	subl	$0xffff, %eax
	jnz	.Lm_diff_0
	cmpq	$32, %rdx
	ja	.Lm_gt32
	movdqu	-16(%rdi,%rdx), %xmm0
	movdqu	-16(%rsi,%rdx), %xmm1
	pcmpeqb	%xmm1, %xmm0
	pmovmskb %xmm0, %eax
	subl	$0xffff, %eax
	jnz	.Lm_diff_e16
	ret
.Lm_diff_0:
	bsfl	%eax, %ecx
	movzbl	(%rdi,%rcx), %eax
	movzbl	(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_diff_e16:
	bsfl	%eax, %ecx
	addq	%rdx, %rcx
	movzbl	-16(%rdi,%rcx), %eax
	movzbl	-16(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_diff_e32:
	bsfl	%eax, %ecx
	addq	%rdx, %rcx
	movzbl	-32(%rdi,%rcx), %eax
	movzbl	-32(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_diff_16:
	bsfl	%eax, %ecx
	movzbl	16(%rdi,%rcx), %eax
	movzbl	16(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_gt32:			/* 33..64: blocks 16, n-32, n-16 */
	cmpq	$64, %rdx
	ja	.Lm_gt64
	movdqu	16(%rdi), %xmm0
	movdqu	16(%rsi), %xmm1
	movdqu	-32(%rdi,%rdx), %xmm2
	movdqu	-32(%rsi,%rdx), %xmm3
	movdqu	-16(%rdi,%rdx), %xmm4
	movdqu	-16(%rsi,%rdx), %xmm5
	pcmpeqb	%xmm1, %xmm0
	pcmpeqb	%xmm3, %xmm2
	pcmpeqb	%xmm5, %xmm4
	movdqa	%xmm0, %xmm1
	pand	%xmm2, %xmm1
	pand	%xmm4, %xmm1
	pmovmskb %xmm1, %eax
	subl	$0xffff, %eax
	jnz	.Lm_loc3
	ret
.Lm_loc3:
	pmovmskb %xmm0, %eax
	subl	$0xffff, %eax
	jnz	.Lm_diff_16
	pmovmskb %xmm2, %eax
	subl	$0xffff, %eax
	jnz	.Lm_diff_e32
	pmovmskb %xmm4, %eax
	subl	$0xffff, %eax
	jmp	.Lm_diff_e16
.Lm_gt64:			/* 65..128: bytes 16..63, then n-64..n-1 */
	cmpq	$128, %rdx
	ja	.Lm_long
	GROUP3A
	jnz	.Lm_locA
	GROUP4D -64, %rdx
	jnz	.Lm_locB
	ret
.Lm_locA:
	movl	$16, %ecx
	jmp	.Lm_locL
.Lm_locB:
	leaq	-64(%rdx), %rcx
.Lm_locL:			/* first mismatching block among x0,x2,x4,x6 at rcx, +16, +32, +48 */
	pmovmskb %xmm0, %eax
	subl	$0xffff, %eax
	jnz	1f
	addq	$16, %rcx
	pmovmskb %xmm2, %eax
	subl	$0xffff, %eax
	jnz	1f
	addq	$16, %rcx
	pmovmskb %xmm4, %eax
	subl	$0xffff, %eax
	jnz	1f
	addq	$16, %rcx
	pmovmskb %xmm6, %eax
	subl	$0xffff, %eax
1:	bsfl	%eax, %eax
	addq	%rax, %rcx
	movzbl	(%rdi,%rcx), %eax
	movzbl	(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_long:			/* n > 128: the only ISA test */
	cmpb	$0, proto_use_avx2(%rip)
	jne	.Lm_long_avx2
	GROUP3A
	jnz	.Lm_locA
	leaq	-64(%rdx), %r8
	movl	$64, %ecx
.Lm_loop:
	GROUP4D 0, %rcx
	jnz	.Lm_locL
	addq	$64, %rcx
	cmpq	%r8, %rcx
	jb	.Lm_loop
	movq	%r8, %rcx
	GROUP4D 0, %rcx
	jnz	.Lm_locL
	ret
.Lm_long_avx2:
	leaq	-64(%rdx), %r8
	movl	$16, %ecx
.Lm_avx_loop:
	vmovdqu	(%rdi,%rcx), %ymm0
	vmovdqu	32(%rdi,%rcx), %ymm1
	vpcmpeqb (%rsi,%rcx), %ymm0, %ymm0
	vpcmpeqb 32(%rsi,%rcx), %ymm1, %ymm1
	vpand	%ymm1, %ymm0, %ymm2
	vpmovmskb %ymm2, %eax
	incl	%eax
	jnz	.Lm_avx_loc
	addq	$64, %rcx
	cmpq	%r8, %rcx
	jb	.Lm_avx_loop
	movq	%r8, %rcx
	vmovdqu	(%rdi,%rcx), %ymm0
	vmovdqu	32(%rdi,%rcx), %ymm1
	vpcmpeqb (%rsi,%rcx), %ymm0, %ymm0
	vpcmpeqb 32(%rsi,%rcx), %ymm1, %ymm1
	vpand	%ymm1, %ymm0, %ymm2
	vpmovmskb %ymm2, %eax
	incl	%eax
	jnz	.Lm_avx_loc
	vzeroupper
	ret
.Lm_avx_loc:
	vpmovmskb %ymm0, %eax
	incl	%eax
	jnz	1f
	vpmovmskb %ymm1, %eax
	incl	%eax
	addq	$32, %rcx
1:	vzeroupper
	bsfl	%eax, %eax
	addq	%rax, %rcx
	movzbl	(%rdi,%rcx), %eax
	movzbl	(%rsi,%rcx), %edx
	subl	%edx, %eax
	ret
.Lm_small:			/* 0..15 */
	cmpl	$8, %edx
	jb	.Lm_lt8
	movq	(%rdi), %rax
	movq	(%rsi), %rcx
	cmpq	%rcx, %rax
	jne	.Lm_qdiff
	movq	-8(%rdi,%rdx), %rax
	movq	-8(%rsi,%rdx), %rcx
	cmpq	%rcx, %rax
	jne	.Lm_qdiff
	xorl	%eax, %eax
	ret
.Lm_qdiff:
	bswapq	%rax
	bswapq	%rcx
	cmpq	%rcx, %rax
	sbbl	%eax, %eax
	orl	$1, %eax
	ret
.Lm_lt8:
	cmpl	$4, %edx
	jb	.Lm_lt4
	movl	(%rdi), %eax
	movl	(%rsi), %ecx
	movl	-4(%rdi,%rdx), %r8d
	movl	-4(%rsi,%rdx), %r9d
	shlq	$32, %r8
	shlq	$32, %r9
	orq	%r8, %rax
	orq	%r9, %rcx
	cmpq	%rcx, %rax
	jne	.Lm_qdiff
	xorl	%eax, %eax
	ret
.Lm_lt4:
	testl	%edx, %edx
	jz	.Lm_ret0
	movzbl	(%rdi), %eax
	movzbl	(%rsi), %ecx
	shll	$16, %eax
	shll	$16, %ecx
	movl	%edx, %r8d
	shrl	$1, %r8d
	movzbl	(%rdi,%r8), %r9d
	movzbl	(%rsi,%r8), %r10d
	shll	$8, %r9d
	shll	$8, %r10d
	orl	%r9d, %eax
	orl	%r10d, %ecx
	movzbl	-1(%rdi,%rdx), %r9d
	movzbl	-1(%rsi,%rdx), %r10d
	orl	%r9d, %eax
	orl	%r10d, %ecx
	subl	%ecx, %eax
	ret
.Lm_ret0:
	xorl	%eax, %eax
	ret
	.size	proto_memcmp, .-proto_memcmp

/* ---------------------------------------------------------------- equality */
	.globl	proto_memeq
	.type	proto_memeq,@function
	.p2align 5
proto_memeq:
	cmpq	$16, %rdx
	jb	.Le_small
	movdqu	(%rdi), %xmm0
	movdqu	(%rsi), %xmm1
	pcmpeqb	%xmm1, %xmm0
	pmovmskb %xmm0, %eax
	subl	$0xffff, %eax
	jnz	.Le_ret1
	cmpq	$32, %rdx
	ja	.Le_gt32
	movdqu	-16(%rdi,%rdx), %xmm0
	movdqu	-16(%rsi,%rdx), %xmm1
	pcmpeqb	%xmm1, %xmm0
	pmovmskb %xmm0, %ecx
	xorl	%eax, %eax
	cmpl	$0xffff, %ecx
	setne	%al
	ret
.Le_ret1:
	movl	$1, %eax
	ret
.Le_gt32:			/* 33..64 */
	cmpq	$64, %rdx
	ja	.Le_gt64
	movdqu	16(%rdi), %xmm0
	movdqu	16(%rsi), %xmm1
	movdqu	-32(%rdi,%rdx), %xmm2
	movdqu	-32(%rsi,%rdx), %xmm3
	movdqu	-16(%rdi,%rdx), %xmm4
	movdqu	-16(%rsi,%rdx), %xmm5
	pcmpeqb	%xmm1, %xmm0
	pcmpeqb	%xmm3, %xmm2
	pcmpeqb	%xmm5, %xmm4
	pand	%xmm2, %xmm0
	pand	%xmm4, %xmm0
	pmovmskb %xmm0, %ecx
	xorl	%eax, %eax
	cmpl	$0xffff, %ecx
	setne	%al
	ret
.Le_gt64:			/* 65..128: 7 blocks, one test */
	cmpq	$128, %rdx
	ja	.Le_long
	movdqu	16(%rdi), %xmm0
	movdqu	16(%rsi), %xmm1
	movdqu	32(%rdi), %xmm2
	movdqu	32(%rsi), %xmm3
	movdqu	48(%rdi), %xmm4
	movdqu	48(%rsi), %xmm5
	movdqu	-64(%rdi,%rdx), %xmm6
	movdqu	-64(%rsi,%rdx), %xmm7
	movdqu	-48(%rdi,%rdx), %xmm8
	movdqu	-48(%rsi,%rdx), %xmm9
	movdqu	-32(%rdi,%rdx), %xmm10
	movdqu	-32(%rsi,%rdx), %xmm11
	movdqu	-16(%rdi,%rdx), %xmm12
	movdqu	-16(%rsi,%rdx), %xmm13
	pcmpeqb	%xmm1, %xmm0
	pcmpeqb	%xmm3, %xmm2
	pcmpeqb	%xmm5, %xmm4
	pcmpeqb	%xmm7, %xmm6
	pcmpeqb	%xmm9, %xmm8
	pcmpeqb	%xmm11, %xmm10
	pcmpeqb	%xmm13, %xmm12
	pand	%xmm2, %xmm0
	pand	%xmm6, %xmm4
	pand	%xmm10, %xmm8
	pand	%xmm4, %xmm0
	pand	%xmm12, %xmm8
	pand	%xmm8, %xmm0
	pmovmskb %xmm0, %ecx
	xorl	%eax, %eax
	cmpl	$0xffff, %ecx
	setne	%al
	ret
.Le_long:			/* n > 128: the only ISA test */
	cmpb	$0, proto_use_avx2(%rip)
	jne	.Le_long_avx2
	leaq	-64(%rdx), %r8
	movl	$16, %ecx
.Le_loop:
	GROUP4D 0, %rcx
	jnz	.Le_ret1
	addq	$64, %rcx
	cmpq	%r8, %rcx
	jb	.Le_loop
	GROUP4D -64, %rdx
	setne	%al
	movzbl	%al, %eax
	ret
.Le_long_avx2:
	leaq	-64(%rdx), %r8
	movl	$16, %ecx
.Le_avx_loop:
	vmovdqu	(%rdi,%rcx), %ymm0
	vmovdqu	32(%rdi,%rcx), %ymm1
	vpcmpeqb (%rsi,%rcx), %ymm0, %ymm0
	vpcmpeqb 32(%rsi,%rcx), %ymm1, %ymm1
	vpand	%ymm1, %ymm0, %ymm0
	vpmovmskb %ymm0, %eax
	incl	%eax
	jnz	.Le_avx_ret1
	addq	$64, %rcx
	cmpq	%r8, %rcx
	jb	.Le_avx_loop
	vmovdqu	-64(%rdi,%rdx), %ymm0
	vmovdqu	-32(%rdi,%rdx), %ymm1
	vpcmpeqb -64(%rsi,%rdx), %ymm0, %ymm0
	vpcmpeqb -32(%rsi,%rdx), %ymm1, %ymm1
	vpand	%ymm1, %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	vzeroupper
	xorl	%eax, %eax
	incl	%ecx
	setne	%al
	ret
.Le_avx_ret1:
	vzeroupper
	movl	$1, %eax
	ret
.Le_small:			/* 0..15 */
	cmpl	$8, %edx
	jb	.Le_lt8
	movq	(%rdi), %rax
	movq	-8(%rdi,%rdx), %rcx
	xorq	(%rsi), %rax
	xorq	-8(%rsi,%rdx), %rcx
	orq	%rcx, %rax
	setne	%al
	movzbl	%al, %eax
	ret
.Le_lt8:
	cmpl	$4, %edx
	jb	.Le_lt4
	movl	(%rdi), %eax
	movl	-4(%rdi,%rdx), %ecx
	xorl	(%rsi), %eax
	xorl	-4(%rsi,%rdx), %ecx
	orl	%ecx, %eax
	setne	%al
	movzbl	%al, %eax
	ret
.Le_lt4:
	testl	%edx, %edx
	jz	.Le_ret0
	cmpl	$2, %edx
	jb	.Le_one
	movzwl	(%rdi), %eax
	movzwl	-2(%rdi,%rdx), %ecx
	movzwl	(%rsi), %r8d
	movzwl	-2(%rsi,%rdx), %r9d
	xorl	%r8d, %eax
	xorl	%r9d, %ecx
	orl	%ecx, %eax
	setne	%al
	movzbl	%al, %eax
	ret
.Le_one:
	movzbl	(%rdi), %eax
	movzbl	(%rsi), %ecx
	xorl	%ecx, %eax
	setne	%al
	movzbl	%al, %eax
	ret
.Le_ret0:
	xorl	%eax, %eax
	ret
	.size	proto_memeq, .-proto_memeq

	.section .note.GNU-stack,"",@progbits
