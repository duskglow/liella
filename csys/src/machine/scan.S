/* Class machine: terminator and byte scanners (OD-C2 standing approval, rule P7, D-4.14).
 *
 * Safe Rust may not read past a C string's terminator (F1), so it scans a byte at a time.
 * Assembly may read whole aligned vectors: an aligned load never crosses a page boundary, so
 * it touches no page the string does not already occupy. Every load below is aligned (32 bytes
 * on the AVX2 path, 16 on the SSE2 path), and the unrolled loops start at a 128- (AVX2) or
 * 64-byte (SSE2) boundary, so one group of loads never spans two pages either. Bytes before
 * the start are masked off; bytes after the terminator are read and ignored.
 *
 * Path selection: liella_machine_scan_level (0 = SSE2, the x86-64 baseline; 1 = AVX2), set
 * once by the runtime through liella_machine_set_scan_level after it has checked CPUID and
 * XCR0 (rt::cpu). The default, SSE2, is valid on every x86-64 CPU.
 *
 * Rust owns the contract: it passes only non-NULL pointers, and checks each result's
 * constant-time postconditions (the byte at the returned index is the one searched for;
 * strnlen's result is at most its bound). */

	.data
	.p2align 2
	.globl liella_machine_scan_level
	.hidden liella_machine_scan_level
	.type liella_machine_scan_level, @object
	.size liella_machine_scan_level, 4
liella_machine_scan_level:
	.long 0

/* SMALL-STR (D-4.45): the short-head page limit. The AVX2 heads of strlen, strnlen, rawmemchr,
 * strchr and strchrnul run when s's page offset is BELOW this value: 4033 on the AVX2 level (then
 * [s, s + 64) lies in s's page), 0 on the SSE2 level (no offset is below 0: the head never runs).
 * One compare against it is both the page test and the level test. Set with the level. */
	.p2align 2
	.globl liella_machine_short_lim
	.hidden liella_machine_short_lim
	.type liella_machine_short_lim, @object
	.size liella_machine_short_lim, 4
liella_machine_short_lim:
	.long 0

/* REGEX-WL (D-4.45 as amended): memrchr's end-page mask. 0xfc0 on the AVX2 level, 0 on SSE2.
 * (s + n - 1) & mask != 0 exactly when the AVX2 level runs and (s + n - 1) & 4095 >= 64, so
 * [s + n - 64, s + n) lies in the page of s + n - 1. One test is the page test and the level
 * test, with no negation of the end address (three instructions fewer than the limit compare).
 * STR-CHR-CMP-SHORT: the direct strchr / strchrnul / strncmp entries compare (s - 1) & 4095 below
 * it: below 0xfc0 exactly when s's page offset is 1..4032 ([s, s + 64) in s's page) on the AVX2
 * level; a NULL s (offset 4095 after the - 1) and every SSE2 call take the slow path, which holds
 * the S6 NULL test. Set with the level. */
	.p2align 2
	.globl liella_machine_rshort_mask
	.hidden liella_machine_rshort_mask
	.type liella_machine_rshort_mask, @object
	.size liella_machine_rshort_mask, 4
liella_machine_rshort_mask:
	.long 0

/* PERF-STRCHR-NONAVX2-3: strchr / strchrnul's slow-entry mask. All ones on the AVX2 level, 0 on SSE2.
 * The slow entry's `testq %rdi, mask` is then the NULL test on the AVX2 level (the same two
 * instructions as before the SSE2 head) and sends every SSE2 call (ZF always set) to the SSE2 head,
 * which re-tests the page and leaves NULL to the NULL test. Set with the level. */
	.p2align 3
	.globl liella_machine_schr_mask
	.hidden liella_machine_schr_mask
	.type liella_machine_schr_mask, @object
	.size liella_machine_schr_mask, 8
liella_machine_schr_mask:
	.quad 0

	.text

/* void liella_machine_set_scan_level(uint32_t level) */
	.section .text.liella_machine_set_scan_level,"ax",@progbits
	.globl liella_machine_set_scan_level
	.type liella_machine_set_scan_level, @function
	.p2align 6
liella_machine_set_scan_level:
	movl %edi, liella_machine_scan_level(%rip)
	/* level 1 (AVX2) -> 4033, level 0 -> 0 (D-4.45). */
	xorl %eax, %eax
	testl %edi, %edi
	movl $4033, %edx
	cmovnzl %edx, %eax
	movl %eax, liella_machine_short_lim(%rip)
	/* level 1 -> 0xfc0, level 0 -> 0 (REGEX-WL, memrchr's end-page mask). */
	andl $0xfc0, %eax
	movl %eax, liella_machine_rshort_mask(%rip)
	/* level 1 -> all ones, level 0 -> 0 (PERF-STRCHR-NONAVX2-3). */
	xorl %eax, %eax
	testl %edi, %edi
	setz %al
	decq %rax
	movq %rax, liella_machine_schr_mask(%rip)
	ret
	.size liella_machine_set_scan_level, .-liella_machine_set_scan_level
	.text

/* uint32_t liella_machine_scan_level_get(void): the current path, for the self-tests (SHORT-3). */
	.globl liella_machine_scan_level_get
	.type liella_machine_scan_level_get, @function
	.p2align 4
liella_machine_scan_level_get:
	movl liella_machine_scan_level(%rip), %eax
	ret
	.size liella_machine_scan_level_get, .-liella_machine_scan_level_get

/* SSE2 pattern "mask64 at base" (inlined): rdx = the 64-bit match mask of the aligned 64-byte group at base (bit i
 * set iff byte base + i equals the byte xmm0 holds in every lane). Clobbers xmm1-xmm4, r8-r10.
 * Used for the masked head near a page end (STR-K1). */

/* SSE2 scan patterns (STR-K1), written out inline where used (the affected-map reads this file
 * statically, so no .macro). xmm0 holds the byte searched for in every lane (zero for the
 * terminator scanners). Groups are 64-byte aligned unless a macro says otherwise.
 * Pattern "anyz64 at base": edx != 0 iff the group at base holds a zero byte (unsigned-minimum fold,
 * loads as memory operands). Clobbers xmm1, xmm2. */
/* Pattern "anyeq64 at base": edx != 0 iff the group at base holds a byte equal to xmm0's.
 * Clobbers xmm1-xmm4. */
/* Pattern "find64 at base" (ld = load): edx = the index of the first byte equal to xmm0's in the 64 bytes at
 * base, which must hold one (ld = movdqu when base is not aligned). Tests the 16-byte blocks
 * in order with a branch each: after a fold found the group this measured faster than a
 * branch-free 64-bit mask, whose pmovmskb-shift-or chain sits on the critical path.
 * Clobbers xmm1. */

/* size_t liella_machine_strlen(const char *s): bytes before the first NUL. */
	.section .text.liella_machine_strlen,"ax",@progbits
	.globl liella_machine_strlen
	.type liella_machine_strlen, @function
	.p2align 6
liella_machine_strlen:
	/* SMALL-STR head (D-4.45): on the AVX2 level with [s, s + 64) inside s's page, two unaligned
	 * 32-byte blocks [s, s + 32) and [s + 32, s + 64), one branch each, before any other setup
	 * (SAFETY: both loads lie in s's page, which holds s's first byte). A miss in both continues
	 * with the aligned block holding s + 64 (.Lstrlen_h64, STR-SCAN-SHORT), then the aligned body. */
	movl %edi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lstrlen_pre
	vpxor %xmm0, %xmm0, %xmm0
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsfl %eax, %eax
	jz .Lstrlen_h32
	vzeroupper
	ret
.Lstrlen_h32:
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %eax
	jz .Lstrlen_h64
	addl $32, %eax
	vzeroupper
	ret
.Lstrlen_h64:
	/* STR-SCAN-SHORT: the aligned 32-byte block a = (s + 64) & -32, which holds s + 64 (the
	 * next unchecked byte) and starts in (s + 32, s + 64]: a 64-byte string ends here with one
	 * branch, no 64-bit mask merge and no group setup. SAFETY: a is aligned (no page crossing)
	 * and its first byte is a checked non-NUL string byte or s + 64 itself, which the string
	 * holds (no NUL before it). A miss checks a + 32 the same way (it starts at a + 32, the next
	 * unchecked byte). Two misses enter the 128-byte loop at L = (a + 64) & -128: with
	 * G = (s & -64) + 64, a is G or G + 32, so L = (G + 64) & -128, in [G, G + 64]: after s, at
	 * or before a + 64, so [s, L) is checked (the old entry from s & -64: no extra group). */
	leaq 64(%rdi), %rax
	andq $-32, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrlen_h96
	subq %rdi, %rax
	addq %rdx, %rax
	vzeroupper
	ret
.Lstrlen_h96:
	vpcmpeqb 32(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrlen_h128
	subq %rdi, %rax
	leaq 32(%rax,%rdx), %rax
	vzeroupper
	ret
.Lstrlen_h128:
	addq $64, %rax
	andq $-128, %rax
	jmp .Lstrlen_avx_loop
.Lstrlen_pre:
	/* Short-string prologue (PJ-8): one unaligned SSE2 load of the first 16 bytes, with no
	 * AVX state and no vzeroupper. Page rule: taken only when [s, s + 16) lies inside the page
	 * holding s's first byte, so the load touches no page the string does not occupy. A NUL
	 * in it ends the scan. SHORT-3: then a second SSE2 load, [s + 16, s + 32), so 16-31 B
	 * strings stay on SSE2 as well (no AVX state, no vzeroupper); the prologue is taken only
	 * when [s, s + 32) lies in s's page (page offset <= 4064). Otherwise (or near a page end)
	 * the paths below run from s. SSE2-STRLEN: ecx = s's page offset, from the entry test. */
	cmpl $4064, %ecx
	ja .Lstrlen_main
	pxor %xmm0, %xmm0
	movdqu (%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz .Lstrlen_second
	ret
.Lstrlen_second:
	movdqu 16(%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz .Lstrlen_third
	addl $16, %eax
	ret
.Lstrlen_third:
	/* SSE2-STRLEN-2: ecx >= short_lim here, which is 4033 on the AVX2 level, so ecx <= 4032 means
	 * the SSE2 level AND [s, s + 64) inside s's page: one compare is both tests. Then
	 * [s + 32, s + 64) as one unsigned-minimum fold (SAFETY: both loads lie in s's page) and,
	 * on a miss, the aligned body from s & -64 ([s, (s & -64) + 64) is checked). */
	cmpl $4032, %ecx
	ja .Lstrlen_main
	movdqu 32(%rdi), %xmm1
	movdqu 48(%rdi), %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrlen_at32
	/* PERF-STRLEN-NONAVX2: the aligned 16-byte block b = (s + 64) & -16, which holds s + 64 (the
	 * next unchecked byte) and starts in (s + 48, s + 64]: a 64-79 B string ends here with one
	 * branch, no group fold and no locate step. SAFETY: b is aligned (no page crossing) and its
	 * first byte is a checked non-NUL string byte or s + 64 itself, which the string holds. A miss
	 * enters the aligned body from s & -64 (.Lstrlen_b_miss): its first group starts at or
	 * before s + 64, inside the checked [s, b + 16). */
	leaq 64(%rdi), %rax
	andq $-16, %rax
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jz .Lstrlen_b_miss
	subq %rdi, %rax
	addq %rdx, %rax
	ret
.Lstrlen_at32:
	/* A zero lane of the fold with none in [s + 32, s + 48) is a NUL in [s + 48, s + 64). */
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz 1f
	addl $32, %eax
	ret
1:
	bsfl %edx, %eax
	addl $48, %eax
	ret
.Lstrlen_main:
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lstrlen_sse2
	/* AVX2 (STR-K2), the SSE2 path's shape with 32-byte blocks. Head: [s + 16, s + 80) as two
	 * unaligned blocks when [s, s + 80) lies in s's page (page offset <= 4016; SHORT-2), else the
	 * aligned 64-byte group holding s, masked below s. Then the aligned group g1 = (s & -64) + 64, then 128-byte aligned groups from
	 * (g1 + 64) & -128 (g1 or g1 + 64), folded with vpminub, ONE branch per 128 bytes. Head and
	 * g1 build the 64-bit match mask of their two blocks directly (two pieces: no locate step).
	 * SAFETY (D-4.14 page rule): as the SSE2 path's below - every load lies in an aligned 64- or
	 * 128-byte group (never spans two pages) or in [s, s + 80) inside s's page, a group is loaded
	 * only when every byte from s to its first byte is a checked byte of the string, and no
	 * group starts below s except the masked head, which shares s's page. */
	/* ecx = s's page offset (the prologue computed it on both of its exits). SHORT-2: when
	 * [s, s + 80) lies in s's page (offset <= 4016) the prologue ran and found no NUL in
	 * [s, s + 16), so two unaligned 32-byte blocks [s + 16, s + 48) and [s + 48, s + 80) come
	 * next, one branch each (16-79 B strings: no mask merge, no aligned-group setup). Then the
	 * aligned groups from g1 = ((s + 16) & -64) + 64 <= s + 80: every byte from s to g1's
	 * first byte is checked. Near a page end: the aligned 64-byte group holding s, masked. */
	vpxor %xmm0, %xmm0, %xmm0
	cmpl $4016, %ecx
	ja .Lstrlen_avx_edge
	vpcmpeqb 16(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jnz .Lstrlen_avx_at16
	vpcmpeqb 48(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jnz .Lstrlen_avx_at48
	leaq 16(%rdi), %rax
	andq $-64, %rax
	jmp .Lstrlen_avx_body
.Lstrlen_avx_at16:
	bsfl %edx, %eax
	addl $16, %eax
	vzeroupper
	ret
.Lstrlen_avx_at48:
	bsfl %edx, %eax
	addl $48, %eax
	vzeroupper
	ret
.Lstrlen_avx_edge:
	movq %rdi, %rax
	andq $-64, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	/* The count is taken mod 64: s & 63. */
	shrq %cl, %rdx
	testq %rdx, %rdx
	jz .Lstrlen_avx_body
	bsfq %rdx, %rax
	vzeroupper
	ret
.Lstrlen_avx_body:
	/* rax = s & -64; [s, rax + 64) holds no NUL. */
	addq $64, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	jnz .Lstrlen_avx_found
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lstrlen_avx_loop:
	vmovdqa (%rax), %ymm1
	vpminub 32(%rax), %ymm1, %ymm1
	vmovdqa 64(%rax), %ymm2
	vpminub 96(%rax), %ymm2, %ymm2
	vpminub %ymm1, %ymm2, %ymm3
	vpcmpeqb %ymm0, %ymm3, %ymm3
	subq $-128, %rax
	vpmovmskb %ymm3, %edx
	testl %edx, %edx
	jz .Lstrlen_avx_loop
	addq $-128, %rax
	/* ymm1 = the minimum of the group's first 64 bytes. */
	vpcmpeqb %ymm0, %ymm1, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jnz .Lstrlen_avx_half
	addq $64, %rax
.Lstrlen_avx_half:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
.Lstrlen_avx_found:
	bsfq %rdx, %rdx
	subq %rdi, %rax
	addq %rdx, %rax
	vzeroupper
	ret

	/* SSE2 (STR-K1). Head: [s, s + 64) unaligned when it lies in s's page (page offset <=
	 * 4032), else the aligned 64-byte group holding s, masked below s. Then the aligned group
	 * g1 = (s & -64) + 64, then 128-byte aligned groups from (g1 + 64) & -128 (g1 or g1 + 64),
	 * one branch per 128 bytes. SAFETY (D-4.14 page rule): every load lies in an aligned
	 * 64- or 128-byte group (never spans two pages) or in [s, s + 64) inside s's page, and a
	 * group is loaded only when every byte from s to its first byte is a checked byte of the
	 * string: its first byte then belongs to the string (or is its terminator), so its page is
	 * mapped. No group starts below s except the masked head, which shares s's page. */
.Lstrlen_sse2:
	/* Reached only with ecx > 4032 (SSE2-STRLEN-2: .Lstrlen_third takes the rest): the masked
	 * head. ecx = s's page offset (entry test). */
	movq %rdi, %rax
	andq $-64, %rax
.Lstrlen_sse_masked:
	andl $63, %ecx
	pxor %xmm0, %xmm0
	/* mask64 %rax */
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm2
	movdqa 32(%rax), %xmm3
	movdqa 48(%rax), %xmm4
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r8d
	pmovmskb %xmm3, %r9d
	pmovmskb %xmm4, %r10d
	shll $16, %r8d
	orl %r8d, %edx
	shll $16, %r10d
	orl %r10d, %r9d
	shlq $32, %r9
	orq %r9, %rdx
	shrq %cl, %rdx
	testq %rdx, %rdx
	jnz .Lstrlen_sse_head
	/* The masked head falls through here with rax = s & -64 already: recomputing it costs two
	 * instructions on that (page-end) path and saves the miss above a jump. */
.Lstrlen_b_miss:
	movq %rdi, %rax
	andq $-64, %rax
.Lstrlen_sse_body:
	/* rax = s & -64; [s, rax + 64) holds no NUL. */
	addq $64, %rax
	/* SSE2-MID: anyz64 %rax keeping xmm1 = min(b0, b1) and xmm2 = min(b0..b3) (bk = block k) for
	 * the locate step (same instruction count as a plain fold). */
	movdqa (%rax), %xmm1
	pminub 16(%rax), %xmm1
	movdqa 32(%rax), %xmm2
	pminub 48(%rax), %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	testl %ecx, %ecx
	jnz .Lstrlen_sse_g_hit
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lstrlen_sse_loop:
	movdqa (%rax), %xmm1
	pminub 16(%rax), %xmm1
	movdqa 32(%rax), %xmm2
	pminub 48(%rax), %xmm2
	movdqa 64(%rax), %xmm3
	pminub 80(%rax), %xmm3
	movdqa 96(%rax), %xmm4
	pminub 112(%rax), %xmm4
	/* SSE2-MID: xmm1 = min01, xmm2 = min0123, xmm3 = min45, xmm4 = min of all eight. */
	pminub %xmm1, %xmm2
	pminub %xmm3, %xmm4
	pminub %xmm2, %xmm4
	pcmpeqb %xmm0, %xmm4
	subq $-128, %rax
	pmovmskb %xmm4, %edx
	testl %edx, %edx
	jz .Lstrlen_sse_loop
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	testl %ecx, %ecx
	jz .Lstrlen_sse_h2
	addq $-128, %rax
.Lstrlen_sse_g_hit:
	/* rax = a 64-byte group holding a NUL, xmm1 = min(b0, b1), ecx = the NUL mask of min(b0..b3).
	 * A NUL in b0|b1: b0 decides; else the min01 mask is b1's. Else b2 decides; else ecx is b3's. */
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r8d
	testl %r8d, %r8d
	jz 5f
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jnz 7f
	bsfl %r8d, %edx
	addl $16, %edx
	jmp 7f
5:
	movdqa 32(%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	leal 32(%rdx), %edx
	jnz 7f
	bsfl %ecx, %edx
	addl $48, %edx
7:
	subq %rdi, %rax
	addq %rdx, %rax
	ret
.Lstrlen_sse_h2:
	/* The second 64 bytes (the first are clean): xmm3 = min(b4, b5), edx = the NUL mask of the
	 * whole group's minimum, which is b7's when b4..b6 are clean. */
	addq $-64, %rax
	pcmpeqb %xmm0, %xmm3
	pmovmskb %xmm3, %r8d
	testl %r8d, %r8d
	jz 5f
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jnz 7f
	bsfl %r8d, %ecx
	addl $16, %ecx
	jmp 7f
5:
	movdqa 32(%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	leal 32(%rcx), %ecx
	jnz 7f
	bsfl %edx, %ecx
	addl $48, %ecx
7:
	subq %rdi, %rax
	addq %rcx, %rax
	ret
.Lstrlen_sse_head:
	bsfq %rdx, %rax
	ret
	.size liella_machine_strlen, .-liella_machine_strlen
	.text

/* The direct strcpy entry (STRCPY-SHORT-2, D-4.22): in the products this IS `strcpy` (second
 * label under LIELLA_EXPORT); the Rust export is its twin and calls it (ffi). One pass for a
 * string whose terminator lies in [s, s + 64) inside s's page: its len + 1 bytes go to d, then
 * the terminator is stored on its own, so d ends in NUL at the returned length even if a writer
 * changes s during the call (D-4.21). No NUL in those 64 bytes: the scan goes on in aligned
 * 16-byte blocks and len + 1 bytes go through liella_machine_memmove, then the terminator.
 * NULL d or s, or [s, s + 64) leaving s's page, tail-jump to the Rust rest entry with nothing
 * written: %rdi, %rsi unchanged, %rcx = 0 (leading bytes known to hold no NUL). NULL goes
 * there before any access (explicit S6 checks there; no folding). AVX2 level: STRCPY-AVX2 head.
 * SAFETY: the head loads lie in [s, s + 64), inside s's page; each later aligned block holds
 * a string byte not past the NUL, so it lies in a page the string occupies; every store lies
 * in [d, d + len + 1). Returns d. */
	.hidden liella_hot_strcpy_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.hidden liella_machine_memmove  /* fold.S */

	.globl liella_machine_strcpy
	.hidden liella_machine_strcpy
	.type liella_machine_strcpy, @function
	.p2align 6
liella_machine_strcpy:
#ifdef LIELLA_EXPORT
	.globl strcpy
	.type strcpy, @function
strcpy:
#endif
	testq %rdi, %rdi
	jz liella_hot_strcpy_rest
	testq %rsi, %rsi
	jz liella_hot_strcpy_rest
	movl %esi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lsc_sse
	/* AVX2 level (STRCPY-AVX2, D-4.22), [s, s + 64) inside s's page: the 16-byte head in
	 * VEX-128 (memory operand, no upper state: 0-15 B need no vzeroupper), a 16-byte test for
	 * 16-31 B, then ONE 32-byte test for 32-63 B with two overlapping 32-byte stores. */
	movq %rdi, %rax
	vpxor %xmm2, %xmm2, %xmm2
	vpcmpeqb (%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jnz .Lsc_cls
	vpcmpeqb 16(%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsc_a32
	vmovdqu (%rsi), %xmm0
	vmovdqu 1(%rsi,%rcx), %xmm1
	vmovdqu %xmm0, (%rdi)
	vmovdqu %xmm1, 1(%rdi,%rcx)
	movb $0, 16(%rdi,%rcx)
	ret
.Lsc_a32:
	vpcmpeqb 32(%rsi), %ymm2, %ymm0
	vpmovmskb %ymm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsc_afull
	vmovdqu (%rsi), %ymm0
	vmovdqu 1(%rsi,%rcx), %ymm1
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm1, 1(%rdi,%rcx)
	movb $0, 32(%rdi,%rcx)
	vzeroupper
	ret
.Lsc_afull:
	vzeroupper
	jmp .Lsc_full
.Lsc_sse:
	cmpl $4032, %ecx
	ja .Lsc_page
	movq %rdi, %rax
	pxor %xmm2, %xmm2
	movdqu (%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	/* bsf sets ZF for a zero mask (its destination is then unused). */
	bsfl %ecx, %ecx
	jz .Lsc_h16
.Lsc_cls:
	cmpl $8, %ecx
	jae .Lsc_8
	cmpl $3, %ecx
	jae .Lsc_4
	cmpl $1, %ecx
	jae .Lsc_2
	movb $0, (%rdi)
	ret
.Lsc_2:
	movzwl (%rsi), %r8d
	movw %r8w, (%rdi)
	movb $0, (%rdi,%rcx)
	ret
.Lsc_4:
	movl (%rsi), %r8d
	movl -3(%rsi,%rcx), %r9d
	movl %r8d, (%rdi)
	movl %r9d, -3(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsc_8:
	movq (%rsi), %r8
	movq -7(%rsi,%rcx), %r9
	movq %r8, (%rdi)
	movq %r9, -7(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsc_h16:
	/* len 16..31 (STRCPY-CLASSES): one block test; %ecx = len - 16; two overlapping 16-byte
	 * stores cover [d, d + len + 1). */
	movdqu 16(%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsc_h32
	movdqu (%rsi), %xmm0
	movdqu 1(%rsi,%rcx), %xmm1
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 1(%rdi,%rcx)
	movb $0, 16(%rdi,%rcx)
	ret
.Lsc_h32:
	/* len 32..63: one test of the two last head blocks; %ecx = len - 32; four overlapping
	 * 16-byte stores cover [d, d + len + 1). */
	movdqu 32(%rsi), %xmm0
	movdqu 48(%rsi), %xmm1
	pcmpeqb %xmm2, %xmm0
	pcmpeqb %xmm2, %xmm1
	pmovmskb %xmm0, %ecx
	pmovmskb %xmm1, %r8d
	shll $16, %r8d
	orl %r8d, %ecx
	jz .Lsc_full
	bsfl %ecx, %ecx
	movdqu (%rsi), %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu 1(%rsi,%rcx), %xmm2
	movdqu 17(%rsi,%rcx), %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, 1(%rdi,%rcx)
	movdqu %xmm3, 17(%rdi,%rcx)
	movb $0, 32(%rdi,%rcx)
	ret
.Lsc_page:
	xorl %ecx, %ecx
	jmp liella_hot_strcpy_rest
.Lsc_full:
	/* No NUL in [s, s + 64): aligned 16-byte blocks from the one holding s + 64 (each holds a
	 * byte of the string not past its NUL, so it lies in a page the string occupies); bytes
	 * before s + 64 are shifted out of the first mask. xmm2 is still zero here. */
	leaq 64(%rsi), %r9
	movl %r9d, %ecx
	andl $15, %ecx
	andq $-16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	shrl %cl, %r10d
	testl %r10d, %r10d
	jz .Lsc_loop
	bsfl %r10d, %r10d
	addq $64, %r10
	jmp .Lsc_copy
.Lsc_loop:
	addq $16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	testl %r10d, %r10d
	jz .Lsc_loop
	bsfl %r10d, %r10d
	addq %r9, %r10
	subq %rsi, %r10
.Lsc_copy:
	/* r10 = len (> 63): len + 1 bytes through the memmove kernel, then the terminator on its
	 * own (D-4.21). Three slots keep the stack 16-byte aligned at the call. */
	pushq %rdi
	pushq %r10
	subq $8, %rsp
	leaq 1(%r10), %rdx
	call liella_machine_memmove
	addq $8, %rsp
	popq %rcx
	popq %rax
	movb $0, (%rax,%rcx)
	ret
	.size liella_machine_strcpy, .-liella_machine_strcpy
#ifdef LIELLA_EXPORT
	.size strcpy, .-strcpy
#endif

/* The direct stpcpy / __stpcpy / __stpcpy_chk entries (STRCPY-FAMILY, D-4.22): the strcpy
 * entry's shape with a room limit in %rdx (SIZE_MAX for stpcpy and __stpcpy, set by their
 * entry, which falls through) and the end pointer d + len returned. The room is checked
 * (len < room) BEFORE any store (fortify): a string that does not fit, a NULL d or s, or
 * [s, s + 64) leaving s's page tail-jump to the Rust rest entry with nothing written and %rdi,
 * %rsi, %rdx unchanged (it reports NULLs (S6) and a failed check).
 * SAFETY: as liella_machine_strcpy; every store lies in [d, d + len + 1) with len < room. */
	.hidden liella_hot_stpcpy_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.hidden liella_hot_strcpy_chk_rest  /* crates/boundary/src/posix/s04_strings.rs */

	.globl liella_machine_stpcpy
	.hidden liella_machine_stpcpy
	.type liella_machine_stpcpy, @function
	.globl liella_machine_stpcpy_chk
	.hidden liella_machine_stpcpy_chk
	.type liella_machine_stpcpy_chk, @function
	.p2align 6
liella_machine_stpcpy:
#ifdef LIELLA_EXPORT
	.globl stpcpy
	.type stpcpy, @function
	.globl __stpcpy
	.type __stpcpy, @function
stpcpy:
__stpcpy:
#endif
	movq $-1, %rdx
	/* Every exported entry is 64-byte aligned (xtask so), so stpcpy jumps to the
	 * aligned __stpcpy_chk entry instead of falling through. */
	jmp liella_machine_stpcpy_chk
	.p2align 6
liella_machine_stpcpy_chk:
#ifdef LIELLA_EXPORT
	.globl __stpcpy_chk
	.type __stpcpy_chk, @function
__stpcpy_chk:
#endif
	testq %rdi, %rdi
	jz liella_hot_stpcpy_rest
	testq %rsi, %rsi
	jz liella_hot_stpcpy_rest
	movl %esi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lsp_sse
	/* AVX2 level (STRCPY-AVX2, D-4.22), [s, s + 64) inside s's page: the 16-byte head in
	 * VEX-128 (memory operand, no upper state: 0-15 B need no vzeroupper), a 16-byte test for
	 * 16-31 B, then ONE 32-byte test for 32-63 B with two overlapping 32-byte stores. */
	vpxor %xmm2, %xmm2, %xmm2
	vpcmpeqb (%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jnz .Lsp_cls
	vpcmpeqb 16(%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsp_a32
	addl $16, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_stpcpy_rest
	leaq (%rdi,%rcx), %rax
	vmovdqu (%rsi), %xmm0
	vmovdqu -15(%rsi,%rcx), %xmm1
	vmovdqu %xmm0, (%rdi)
	vmovdqu %xmm1, -15(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsp_a32:
	vpcmpeqb 32(%rsi), %ymm2, %ymm0
	vpmovmskb %ymm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsp_afull
	addl $32, %ecx
	cmpq %rcx, %rdx
	jbe .Lsp_arest
	leaq (%rdi,%rcx), %rax
	vmovdqu (%rsi), %ymm0
	vmovdqu -31(%rsi,%rcx), %ymm1
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm1, -31(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	vzeroupper
	ret
.Lsp_arest:
	vzeroupper
	jmp liella_hot_stpcpy_rest
.Lsp_afull:
	vzeroupper
	jmp .Lsp_full
.Lsp_sse:
	cmpl $4032, %ecx
	ja liella_hot_stpcpy_rest
	pxor %xmm2, %xmm2
	movdqu (%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsp_h16
.Lsp_cls:
	cmpq %rcx, %rdx
	jbe liella_hot_stpcpy_rest
	leaq (%rdi,%rcx), %rax
	cmpl $8, %ecx
	jae .Lsp_8
	cmpl $3, %ecx
	jae .Lsp_4
	cmpl $1, %ecx
	jae .Lsp_2
	movb $0, (%rdi)
	ret
.Lsp_2:
	movzwl (%rsi), %r8d
	movw %r8w, (%rdi)
	movb $0, (%rax)
	ret
.Lsp_4:
	movl (%rsi), %r8d
	movl -3(%rsi,%rcx), %r9d
	movl %r8d, (%rdi)
	movl %r9d, -3(%rdi,%rcx)
	movb $0, (%rax)
	ret
.Lsp_8:
	movq (%rsi), %r8
	movq -7(%rsi,%rcx), %r9
	movq %r8, (%rdi)
	movq %r9, -7(%rdi,%rcx)
	movb $0, (%rax)
	ret
.Lsp_h16:
	/* len 16..31 (STRCPY-CLASSES): one block test, as .Lsc_h16. */
	movdqu 16(%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsp_h32
	addl $16, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_stpcpy_rest
	leaq (%rdi,%rcx), %rax
	movdqu (%rsi), %xmm0
	movdqu -15(%rsi,%rcx), %xmm1
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, -15(%rdi,%rcx)
	movb $0, (%rax)
	ret
.Lsp_h32:
	/* len 32..63: one test of the two last head blocks, as .Lsc_h32. */
	movdqu 32(%rsi), %xmm0
	movdqu 48(%rsi), %xmm1
	pcmpeqb %xmm2, %xmm0
	pcmpeqb %xmm2, %xmm1
	pmovmskb %xmm0, %ecx
	pmovmskb %xmm1, %r8d
	shll $16, %r8d
	orl %r8d, %ecx
	jz .Lsp_full
	bsfl %ecx, %ecx
	addl $32, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_stpcpy_rest
	leaq (%rdi,%rcx), %rax
	movdqu (%rsi), %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu -31(%rsi,%rcx), %xmm2
	movdqu -15(%rsi,%rcx), %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, -31(%rdi,%rcx)
	movdqu %xmm3, -15(%rdi,%rcx)
	movb $0, (%rax)
	ret
.Lsp_full:
	/* No NUL in [s, s + 64): as .Lsc_full; the room check comes before the copy. */
	leaq 64(%rsi), %r9
	movl %r9d, %ecx
	andl $15, %ecx
	andq $-16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	shrl %cl, %r10d
	testl %r10d, %r10d
	jz .Lsp_loop
	bsfl %r10d, %r10d
	addq $64, %r10
	jmp .Lsp_copy
.Lsp_loop:
	addq $16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	testl %r10d, %r10d
	jz .Lsp_loop
	bsfl %r10d, %r10d
	addq %r9, %r10
	subq %rsi, %r10
.Lsp_copy:
	cmpq %r10, %rdx
	jbe liella_hot_stpcpy_rest
	pushq %rdi
	pushq %r10
	subq $8, %rsp
	leaq 1(%r10), %rdx
	call liella_machine_memmove
	addq $8, %rsp
	popq %rcx
	popq %rax
	addq %rcx, %rax
	movb $0, (%rax)
	ret
	.size liella_machine_stpcpy_chk, .-liella_machine_stpcpy_chk
	.size liella_machine_stpcpy, .-liella_machine_stpcpy
#ifdef LIELLA_EXPORT
	.size __stpcpy_chk, .-__stpcpy_chk
	.size stpcpy, .-stpcpy
	.size __stpcpy, .-__stpcpy
#endif

/* The direct __strcpy_chk entry (STRCPY-FAMILY, D-4.22): the stpcpy_chk shape returning d;
 * len < destlen (%rdx) is checked BEFORE any store; else, for a NULL, or for [s, s + 64)
 * leaving s's page, a tail jump to the Rust rest with nothing written and %rdi, %rsi, %rdx
 * unchanged. SAFETY: as liella_machine_stpcpy_chk. */
	.globl liella_machine_strcpy_chk
	.hidden liella_machine_strcpy_chk
	.type liella_machine_strcpy_chk, @function
	.p2align 6
liella_machine_strcpy_chk:
#ifdef LIELLA_EXPORT
	.globl __strcpy_chk
	.type __strcpy_chk, @function
__strcpy_chk:
#endif
	testq %rdi, %rdi
	jz liella_hot_strcpy_chk_rest
	testq %rsi, %rsi
	jz liella_hot_strcpy_chk_rest
	movl %esi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lsk_sse
	/* AVX2 level (STRCPY-AVX2, D-4.22), [s, s + 64) inside s's page: the 16-byte head in
	 * VEX-128 (memory operand, no upper state: 0-15 B need no vzeroupper), a 16-byte test for
	 * 16-31 B, then ONE 32-byte test for 32-63 B with two overlapping 32-byte stores. */
	movq %rdi, %rax
	vpxor %xmm2, %xmm2, %xmm2
	vpcmpeqb (%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jnz .Lsk_cls
	vpcmpeqb 16(%rsi), %xmm2, %xmm0
	vpmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsk_a32
	addl $16, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_strcpy_chk_rest
	vmovdqu (%rsi), %xmm0
	vmovdqu -15(%rsi,%rcx), %xmm1
	vmovdqu %xmm0, (%rdi)
	vmovdqu %xmm1, -15(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_a32:
	vpcmpeqb 32(%rsi), %ymm2, %ymm0
	vpmovmskb %ymm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsk_afull
	addl $32, %ecx
	cmpq %rcx, %rdx
	jbe .Lsk_arest
	vmovdqu (%rsi), %ymm0
	vmovdqu -31(%rsi,%rcx), %ymm1
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm1, -31(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	vzeroupper
	ret
.Lsk_arest:
	vzeroupper
	jmp liella_hot_strcpy_chk_rest
.Lsk_afull:
	vzeroupper
	jmp .Lsk_full
.Lsk_sse:
	cmpl $4032, %ecx
	ja liella_hot_strcpy_chk_rest
	movq %rdi, %rax
	pxor %xmm2, %xmm2
	movdqu (%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsk_h16
.Lsk_cls:
	cmpq %rcx, %rdx
	jbe liella_hot_strcpy_chk_rest
	cmpl $8, %ecx
	jae .Lsk_8
	cmpl $3, %ecx
	jae .Lsk_4
	cmpl $1, %ecx
	jae .Lsk_2
	movb $0, (%rdi)
	ret
.Lsk_2:
	movzwl (%rsi), %r8d
	movw %r8w, (%rdi)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_4:
	movl (%rsi), %r8d
	movl -3(%rsi,%rcx), %r9d
	movl %r8d, (%rdi)
	movl %r9d, -3(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_8:
	movq (%rsi), %r8
	movq -7(%rsi,%rcx), %r9
	movq %r8, (%rdi)
	movq %r9, -7(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_h16:
	/* len 16..31 (STRCPY-CLASSES): one block test, as .Lsc_h16. */
	movdqu 16(%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %ecx
	bsfl %ecx, %ecx
	jz .Lsk_h32
	addl $16, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_strcpy_chk_rest
	movdqu (%rsi), %xmm0
	movdqu -15(%rsi,%rcx), %xmm1
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, -15(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_h32:
	/* len 32..63: one test of the two last head blocks, as .Lsc_h32. */
	movdqu 32(%rsi), %xmm0
	movdqu 48(%rsi), %xmm1
	pcmpeqb %xmm2, %xmm0
	pcmpeqb %xmm2, %xmm1
	pmovmskb %xmm0, %ecx
	pmovmskb %xmm1, %r8d
	shll $16, %r8d
	orl %r8d, %ecx
	jz .Lsk_full
	bsfl %ecx, %ecx
	addl $32, %ecx
	cmpq %rcx, %rdx
	jbe liella_hot_strcpy_chk_rest
	movdqu (%rsi), %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu -31(%rsi,%rcx), %xmm2
	movdqu -15(%rsi,%rcx), %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, -31(%rdi,%rcx)
	movdqu %xmm3, -15(%rdi,%rcx)
	movb $0, (%rdi,%rcx)
	ret
.Lsk_full:
	/* No NUL in [s, s + 64): as .Lsc_full; the room check comes before the copy. */
	leaq 64(%rsi), %r9
	movl %r9d, %ecx
	andl $15, %ecx
	andq $-16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	shrl %cl, %r10d
	testl %r10d, %r10d
	jz .Lsk_loop
	bsfl %r10d, %r10d
	addq $64, %r10
	jmp .Lsk_copy
.Lsk_loop:
	addq $16, %r9
	movdqa (%r9), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %r10d
	testl %r10d, %r10d
	jz .Lsk_loop
	bsfl %r10d, %r10d
	addq %r9, %r10
	subq %rsi, %r10
.Lsk_copy:
	cmpq %r10, %rdx
	jbe liella_hot_strcpy_chk_rest
	pushq %rdi
	pushq %r10
	subq $8, %rsp
	leaq 1(%r10), %rdx
	call liella_machine_memmove
	addq $8, %rsp
	popq %rcx
	popq %rax
	movb $0, (%rax,%rcx)
	ret
	.size liella_machine_strcpy_chk, .-liella_machine_strcpy_chk
#ifdef LIELLA_EXPORT
	.size __strcpy_chk, .-__strcpy_chk
#endif

/* The direct strncpy entry (STRNCPY-SHORT, D-4.22): in the products this IS `strncpy` (second
 * label under LIELLA_EXPORT); the Rust export is its twin and calls it (ffi). One pass for
 * 1 <= n <= 64 when [s, s + 64) lies in s's page: the blocks of s that can hold the first NUL
 * before n are scanned, len = min(first NUL, n), and every store of the n-byte copy classes takes
 * the source bytes at its offset AND-ed with the keep mask at .Lsn_keep + 64 - len + offset
 * (0xff below len, 0 from len on): text and padding in the same stores, nothing past d + n.
 * NULL d or s, n == 0, n > 64 or [s, s + 64) leaving s's page: tail jump to the Rust rest entry
 * with the arguments unchanged and nothing written (S6 checks there). SSE2 only.
 * SAFETY: every load of s lies in [s, s + 64), inside s's page; every keep-mask load lies in
 * [.Lsn_keep, .Lsn_keep + 128); every store lies in [d, d + n). Returns d. */
	.hidden liella_hot_strncpy_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.section .rodata
	.p2align 6
.Lsn_keep:
	.fill 64, 1, 0xff
	.fill 64, 1, 0
	.text

/* The one-pass body (.Lsn_*) is written out here and again, returning d + min(first NUL, n),
 * in stpncpy (.Lspn_*): no .macro in .S files (the affected map scans them for symbols). */
	.globl liella_machine_strncpy
	.hidden liella_machine_strncpy
	.type liella_machine_strncpy, @function
	.p2align 6
liella_machine_strncpy:
#ifdef LIELLA_EXPORT
	.globl strncpy
	.type strncpy, @function
strncpy:
#endif
	testq %rdi, %rdi
	jz liella_hot_strncpy_rest
	testq %rsi, %rsi
	jz liella_hot_strncpy_rest
.Lsn_body:
	leaq -1(%rdx), %rax
	cmpq $63, %rax
	ja .Lsn_out
	movl %esi, %ecx
	andl $4095, %ecx
	cmpl $4032, %ecx
	ja liella_hot_strncpy_rest
	pxor %xmm2, %xmm2
	movdqu (%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %eax
	cmpl $16, %edx
	jbe .Lsn_len
	testl %eax, %eax
	jnz .Lsn_len
	movdqu 16(%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %eax
	shll $16, %eax
	cmpl $32, %edx
	jbe .Lsn_len
	testl %eax, %eax
	jnz .Lsn_len
	movdqu 32(%rsi), %xmm0
	movdqu 48(%rsi), %xmm1
	pcmpeqb %xmm2, %xmm0
	pcmpeqb %xmm2, %xmm1
	pmovmskb %xmm0, %eax
	pmovmskb %xmm1, %ecx
	shll $16, %ecx
	orl %ecx, %eax
	shlq $32, %rax
.Lsn_len:
	/* rax: the NUL bits of the scanned blocks at their byte offsets (an unscanned block had no
	 * NUL or starts at or past n). rcx = the first NUL (<= 63), or n without one; a NUL at or
	 * past n keeps every byte below n, as min(first NUL, n) would. r8 = .Lsn_keep + 64 - rcx. */
	bsfq %rax, %rcx
	cmovzq %rdx, %rcx
	leaq .Lsn_keep+64(%rip), %r8
	subq %rcx, %r8
	movq %rdi, %rax
	cmpl $16, %edx
	jae .Lsn_16
	cmpl $8, %edx
	jae .Lsn_8
	cmpl $4, %edx
	jae .Lsn_4
	cmpl $2, %edx
	jae .Lsn_2
	movzbl (%rsi), %ecx
	andb (%r8), %cl
	movb %cl, (%rdi)
	ret
.Lsn_2:
	movzwl (%rsi), %ecx
	andw (%r8), %cx
	movzwl -2(%rsi,%rdx), %r9d
	andw -2(%r8,%rdx), %r9w
	movw %cx, (%rdi)
	movw %r9w, -2(%rdi,%rdx)
	ret
.Lsn_4:
	movl (%rsi), %ecx
	andl (%r8), %ecx
	movl -4(%rsi,%rdx), %r9d
	andl -4(%r8,%rdx), %r9d
	movl %ecx, (%rdi)
	movl %r9d, -4(%rdi,%rdx)
	ret
.Lsn_8:
	movq (%rsi), %rcx
	andq (%r8), %rcx
	movq -8(%rsi,%rdx), %r9
	andq -8(%r8,%rdx), %r9
	movq %rcx, (%rdi)
	movq %r9, -8(%rdi,%rdx)
	ret
.Lsn_16:
	cmpl $32, %edx
	ja .Lsn_33
	movdqu (%rsi), %xmm0
	movdqu (%r8), %xmm1
	pand %xmm1, %xmm0
	movdqu -16(%rsi,%rdx), %xmm1
	movdqu -16(%r8,%rdx), %xmm3
	pand %xmm3, %xmm1
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, -16(%rdi,%rdx)
	ret
.Lsn_33:
	movdqu (%rsi), %xmm0
	movdqu (%r8), %xmm4
	pand %xmm4, %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu 16(%r8), %xmm4
	pand %xmm4, %xmm1
	movdqu -32(%rsi,%rdx), %xmm2
	movdqu -32(%r8,%rdx), %xmm4
	pand %xmm4, %xmm2
	movdqu -16(%rsi,%rdx), %xmm3
	movdqu -16(%r8,%rdx), %xmm4
	pand %xmm4, %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, -32(%rdi,%rdx)
	movdqu %xmm3, -16(%rdi,%rdx)
	ret
.Lsn_out:
	/* n == 0 (d, s non-NULL): nothing to read or write. n > 64: the general path. */
	testq %rdx, %rdx
	jnz liella_hot_strncpy_rest
	movq %rdi, %rax
	ret
	.size liella_machine_strncpy, .-liella_machine_strncpy
#ifdef LIELLA_EXPORT
	.size strncpy, .-strncpy
#endif

/* The direct __strncpy_chk entry (STRNCPY-FAMILY; `__strncpy_chk` in the products): the fortify
 * check n <= destlen comes before any load or store; n > destlen or a NULL d or s: tail jump to
 * the Rust rest entry with the arguments unchanged (it fails the check / reports S6). Else the
 * strncpy body, whose own general path (n > 64, page edge) then sees n <= destlen. */
	.hidden liella_hot_strncpy_chk_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_strncpy_chk
	.hidden liella_machine_strncpy_chk
	.type liella_machine_strncpy_chk, @function
	.p2align 6
liella_machine_strncpy_chk:
#ifdef LIELLA_EXPORT
	.globl __strncpy_chk
	.type __strncpy_chk, @function
__strncpy_chk:
#endif
	cmpq %rcx, %rdx
	ja liella_hot_strncpy_chk_rest
	testq %rdi, %rdi
	jz liella_hot_strncpy_chk_rest
	testq %rsi, %rsi
	jz liella_hot_strncpy_chk_rest
	jmp .Lsn_body
	.size liella_machine_strncpy_chk, .-liella_machine_strncpy_chk
#ifdef LIELLA_EXPORT
	.size __strncpy_chk, .-__strncpy_chk
#endif

/* The direct stpncpy entry (STRNCPY-FAMILY; `stpncpy` and, STPNCPY-ALIAS, `__stpncpy` in the
 * products): the strncpy body returning d + min(strlen(s), n); NULLs and the general path:
 * liella_hot_stpncpy_rest. */
	.hidden liella_hot_stpncpy_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_stpncpy
	.hidden liella_machine_stpncpy
	.type liella_machine_stpncpy, @function
	.p2align 6
liella_machine_stpncpy:
#ifdef LIELLA_EXPORT
	.globl stpncpy
	.type stpncpy, @function
stpncpy:
	.globl __stpncpy
	.type __stpncpy, @function
__stpncpy:
#endif
	testq %rdi, %rdi
	jz liella_hot_stpncpy_rest
	testq %rsi, %rsi
	jz liella_hot_stpncpy_rest
.Lspn_body:
	/* The strncpy body above (.Lsn_*) with .Lspn_* labels; only the return value differs. */
	leaq -1(%rdx), %rax
	cmpq $63, %rax
	ja .Lspn_out
	movl %esi, %ecx
	andl $4095, %ecx
	cmpl $4032, %ecx
	ja liella_hot_stpncpy_rest
	pxor %xmm2, %xmm2
	movdqu (%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %eax
	cmpl $16, %edx
	jbe .Lspn_len
	testl %eax, %eax
	jnz .Lspn_len
	movdqu 16(%rsi), %xmm0
	pcmpeqb %xmm2, %xmm0
	pmovmskb %xmm0, %eax
	shll $16, %eax
	cmpl $32, %edx
	jbe .Lspn_len
	testl %eax, %eax
	jnz .Lspn_len
	movdqu 32(%rsi), %xmm0
	movdqu 48(%rsi), %xmm1
	pcmpeqb %xmm2, %xmm0
	pcmpeqb %xmm2, %xmm1
	pmovmskb %xmm0, %eax
	pmovmskb %xmm1, %ecx
	shll $16, %ecx
	orl %ecx, %eax
	shlq $32, %rax
.Lspn_len:
	/* rax: the NUL bits of the scanned blocks at their byte offsets (an unscanned block had no
	 * NUL or starts at or past n). rcx = the first NUL (<= 63), or n without one; a NUL at or
	 * past n keeps every byte below n, as min(first NUL, n) would. r8 = .Lsn_keep + 64 - rcx. */
	bsfq %rax, %rcx
	cmovzq %rdx, %rcx
	/* stpncpy returns d + min(first NUL, n): clamp len to n first (the keep mask
	 * is the same for any len >= n). */
	cmpq %rdx, %rcx
	cmova %rdx, %rcx
	leaq .Lsn_keep+64(%rip), %r8
	subq %rcx, %r8
	leaq (%rdi,%rcx), %rax
	cmpl $16, %edx
	jae .Lspn_16
	cmpl $8, %edx
	jae .Lspn_8
	cmpl $4, %edx
	jae .Lspn_4
	cmpl $2, %edx
	jae .Lspn_2
	movzbl (%rsi), %ecx
	andb (%r8), %cl
	movb %cl, (%rdi)
	ret
.Lspn_2:
	movzwl (%rsi), %ecx
	andw (%r8), %cx
	movzwl -2(%rsi,%rdx), %r9d
	andw -2(%r8,%rdx), %r9w
	movw %cx, (%rdi)
	movw %r9w, -2(%rdi,%rdx)
	ret
.Lspn_4:
	movl (%rsi), %ecx
	andl (%r8), %ecx
	movl -4(%rsi,%rdx), %r9d
	andl -4(%r8,%rdx), %r9d
	movl %ecx, (%rdi)
	movl %r9d, -4(%rdi,%rdx)
	ret
.Lspn_8:
	movq (%rsi), %rcx
	andq (%r8), %rcx
	movq -8(%rsi,%rdx), %r9
	andq -8(%r8,%rdx), %r9
	movq %rcx, (%rdi)
	movq %r9, -8(%rdi,%rdx)
	ret
.Lspn_16:
	cmpl $32, %edx
	ja .Lspn_33
	movdqu (%rsi), %xmm0
	movdqu (%r8), %xmm1
	pand %xmm1, %xmm0
	movdqu -16(%rsi,%rdx), %xmm1
	movdqu -16(%r8,%rdx), %xmm3
	pand %xmm3, %xmm1
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, -16(%rdi,%rdx)
	ret
.Lspn_33:
	movdqu (%rsi), %xmm0
	movdqu (%r8), %xmm4
	pand %xmm4, %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu 16(%r8), %xmm4
	pand %xmm4, %xmm1
	movdqu -32(%rsi,%rdx), %xmm2
	movdqu -32(%r8,%rdx), %xmm4
	pand %xmm4, %xmm2
	movdqu -16(%rsi,%rdx), %xmm3
	movdqu -16(%r8,%rdx), %xmm4
	pand %xmm4, %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, -32(%rdi,%rdx)
	movdqu %xmm3, -16(%rdi,%rdx)
	ret
.Lspn_out:
	/* n == 0 (d, s non-NULL): nothing to read or write. n > 64: the general path. */
	testq %rdx, %rdx
	jnz liella_hot_stpncpy_rest
	movq %rdi, %rax
	ret
	.size liella_machine_stpncpy, .-liella_machine_stpncpy
#ifdef LIELLA_EXPORT
	.size stpncpy, .-stpncpy
	.size __stpncpy, .-__stpncpy
#endif

/* The direct __stpncpy_chk entry (STPNCPY-ALIAS; `__stpncpy_chk` in the products): the fortify
 * check n <= destlen comes before any load or store; n > destlen or a NULL d or s: tail jump to
 * the Rust rest entry with the arguments unchanged (it fails the check / reports S6). Else the
 * stpncpy body, whose own general path (n > 64, page edge) then sees n <= destlen. */
	.hidden liella_hot_stpncpy_chk_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_stpncpy_chk
	.hidden liella_machine_stpncpy_chk
	.type liella_machine_stpncpy_chk, @function
	.p2align 6
liella_machine_stpncpy_chk:
#ifdef LIELLA_EXPORT
	.globl __stpncpy_chk
	.type __stpncpy_chk, @function
__stpncpy_chk:
#endif
	cmpq %rcx, %rdx
	ja liella_hot_stpncpy_chk_rest
	testq %rdi, %rdi
	jz liella_hot_stpncpy_chk_rest
	testq %rsi, %rsi
	jz liella_hot_stpncpy_chk_rest
	jmp .Lspn_body
	.size liella_machine_stpncpy_chk, .-liella_machine_stpncpy_chk
#ifdef LIELLA_EXPORT
	.size __stpncpy_chk, .-__stpncpy_chk
#endif

/* size_t liella_machine_strnlen(const char *s, size_t max): bytes before the first NUL, at
 * most max. A block or group is loaded only if it starts before s + max: its page then holds a
 * byte the caller guarantees, and an aligned group never leaves that page. */
	.section .text.liella_machine_strnlen,"ax",@progbits
	.globl liella_machine_strnlen
	.type liella_machine_strnlen, @function
	.p2align 6
liella_machine_strnlen:
	/* PERF-STRNLEN-MID-2: max <= 64 (max = 0 included) takes .Lstrnlen_small, so the fast path
	 * below knows max > 64 without a further test. ecx = s's page offset on every path. */
	movl %edi, %ecx
	andl $4095, %ecx
	cmpq $64, %rsi
	jbe .Lstrnlen_small
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lstrnlen_pre
	/* SMALL-STR head (D-4.45), max > 64: on the AVX2 level with [s, s + 64) inside s's page,
	 * two unaligned 32-byte blocks [s, s + 32) and [s + 32, s + 64), one branch each (SAFETY:
	 * both loads lie in s's page, mapped because max >= 1). Answers are clamped to max. */
	vpxor %xmm0, %xmm0, %xmm0
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsfl %eax, %eax
	jz .Lstrnlen_f32
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_f32:
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrnlen_f64
	leaq 32(%rdx), %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_f64:
	/* PERF-STRNLEN-MID-2, the in-page chain: a = (s + 64) & -32 (holds s + 64, starts in
	 * (s + 32, s + 64]). With s's page offset <= 3871 the six aligned 32-byte blocks a, a + 32,
	 * ..., a + 160 all start at or below s + 224, inside s's page, so each is loaded with one
	 * branch and no bound test (SAFETY: s's page is mapped because max >= 1, and an aligned
	 * block never leaves its page; a NUL index at or past max is clamped to max). Six misses:
	 * [s, a + 192) holds no NUL; the bounded loop goes on at (a + 192) & -128, in
	 * (a + 64, a + 192]. Page offsets 3872-4032 take the bounded .Lstrnlen_h64 steps. */
	cmpl $3871, %ecx
	ja .Lstrnlen_h64
	leaq 64(%rdi), %rax
	andq $-32, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c0
	vpcmpeqb 32(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c1
	vpcmpeqb 64(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c2
	vpcmpeqb 96(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c3
	vpcmpeqb 128(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c4
	vpcmpeqb 160(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jnz .Lstrnlen_c5
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	addq $192, %rax
	andq $-128, %rax
	jmp .Lstrnlen_avx_loop
.Lstrnlen_c0:
	addq %rdx, %rax
	jmp .Lstrnlen_vz_clamp
.Lstrnlen_c1:
	leaq 32(%rax,%rdx), %rax
	subq %rdi, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_c2:
	leaq 64(%rax,%rdx), %rax
	subq %rdi, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_c3:
	leaq 96(%rax,%rdx), %rax
	subq %rdi, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_c4:
	leaq 128(%rax,%rdx), %rax
	subq %rdi, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_c5:
	leaq 160(%rax,%rdx), %rax
.Lstrnlen_vz_clamp:
	subq %rdi, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_small:
	/* max <= 64: the SMALL-STR head with a max test per block (max = 0 reads nothing). On the
	 * AVX2 level with [s, s + 64) inside s's page, [s, s + 32), then [s + 32, s + 64) if
	 * max > 32 (SAFETY: both loads lie in s's page, mapped because max >= 1); answers are
	 * clamped to max, and past them [s, s + max) is all checked (.Lstrnlen_h64: max). The
	 * .Lstrnlen_h64 steps also serve max > 64 at page offsets 3872-4032; r11 = s + max,
	 * saturated, is the body's bound. */
	testq %rsi, %rsi
	jz .Lstrnlen_zero
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lstrnlen_pre
	vpxor %xmm0, %xmm0, %xmm0
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsfl %eax, %eax
	jz .Lstrnlen_h32
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_h32:
	cmpq $32, %rsi
	jbe .Lstrnlen_hmax
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrnlen_h64
	leaq 32(%rdx), %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_hmax:
	movq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_h64:
	cmpq $64, %rsi
	jbe .Lstrnlen_hmax
	/* STR-SCAN-SHORT: as strlen's .Lstrlen_h64, the aligned 32-byte block a = (s + 64) & -32
	 * (holds s + 64, starts in (s + 32, s + 64]), one branch, the answer clamped to max.
	 * SAFETY: max > 64, so a lies below s + max after bytes with no NUL: the caller guarantees
	 * a's first byte, and the aligned block never leaves its page. A miss sets r11 = s + max
	 * (saturated) and checks a + 32 only if it starts below r11 (else [s, s + max) is all
	 * checked: max); two misses enter the bounded loop at (a + 64) & -128, as strlen's. */
	leaq 64(%rdi), %rax
	andq $-32, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrnlen_h96
	subq %rdi, %rax
	addq %rdx, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_h96:
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	addq $32, %rax
	cmpq %r11, %rax
	jae .Lstrnlen_avx_max
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lstrnlen_h128
	subq %rdi, %rax
	addq %rdx, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	vzeroupper
	ret
.Lstrnlen_h128:
	addq $32, %rax
	andq $-128, %rax
	jmp .Lstrnlen_avx_loop
.Lstrnlen_pre:
	/* Short-string prologue (PJ-8), as in strlen: max >= 1, so s's first byte is readable and
	 * its page mapped; the load is taken only when [s, s + 16) stays inside that page. eax =
	 * the first NUL's index, or 16 if none (the sentinel bit); at or past max the answer is
	 * max, below 16 it is the NUL; 16 below max goes on to the second block. SHORT-3: a second
	 * SSE2 load, [s + 16, s + 32), the same way (32 below max goes on to the paths below), so
	 * 16-31 B strings take no AVX state; the prologue is taken only when [s, s + 32) lies in
	 * s's page (page offset <= 4064). PERF-STRNLEN-RAWMEMCHR: ecx = s's page offset, from the
	 * entry test (no recompute). */
	cmpl $4064, %ecx
	ja .Lstrnlen_main
	pxor %xmm0, %xmm0
	movdqu (%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	orl $0x10000, %eax
	bsfl %eax, %eax
	cmpq %rsi, %rax
	jae .Lstrnlen_pro_max
	cmpl $16, %eax
	je .Lstrnlen_second
	ret
.Lstrnlen_second:
	movdqu 16(%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	orl $0x10000, %eax
	bsfl %eax, %eax
	addl $16, %eax
	cmpq %rsi, %rax
	jae .Lstrnlen_pro_max
	cmpl $32, %eax
	je .Lstrnlen_third
	ret
.Lstrnlen_pro_max:
	movq %rsi, %rax
	ret
.Lstrnlen_third:
	/* PERF-STRNLEN-RAWMEMCHR, as strlen's .Lstrlen_third: ecx >= short_lim here (4033 on the
	 * AVX2 level), so ecx <= 4032 means the SSE2 level AND [s, s + 64) inside s's page. Here
	 * max > 32 and [s, s + 32) holds no NUL. [s + 32, s + 64) as one unsigned-minimum fold
	 * (SAFETY: both loads lie in s's page, mapped because max >= 1; a NUL index at or past max
	 * is clamped). */
	cmpl $4032, %ecx
	ja .Lstrnlen_main
	movdqu 32(%rdi), %xmm1
	movdqu 48(%rdi), %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrnlen_at32
	/* The aligned 16-byte block b = (s + 64) & -16, which holds s + 64 (the next unchecked
	 * byte) and starts in (s + 48, s + 64]: a 64-79 B string ends here with one branch, no
	 * group fold and no locate step. If max <= 64, [s, s + max) is all checked: max.
	 * PERF-STRNLEN-MID-3: the test is max against 64 (it was b - s against max, with b - s
	 * kept in r8). SAFETY: otherwise max > 64, so b <= s + 64 lies below s + max and its
	 * first byte is a checked non-NUL byte or s + 64 itself, which the caller guarantees (no
	 * NUL before it, and below s + max); b is aligned, so it never leaves that byte's page.
	 * A miss enters the bounded aligned body from s & -64: its first group starts at or before
	 * s + 64 (inside the checked [s, b + 16)) and is loaded only below r11. */
	cmpq $64, %rsi
	jbe .Lstrnlen_pro_max
	leaq 64(%rdi), %rax
	andq $-16, %rax
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jz .Lstrnlen_b_miss
	subq %rdi, %rax
	addq %rdx, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	ret
.Lstrnlen_b_miss:
	/* PERF-STRNLEN-BODY-3: q = b + 16 (16-aligned, in (s + 64, s + 80]; [s, q) holds no NUL).
	 * With [s, s + 144) inside s's page (ecx <= 3952), the group [q, q + 64) as one fold,
	 * located from registers. PERF-STRNLEN-MID-3: loaded with no bound test (it was loaded
	 * only below r11): SAFETY: every load lies in s's page, mapped because max >= 1; a NUL
	 * index at or past max is clamped, and a clean group with q at or past s + max leaves
	 * the loop's first bound test to return max. r11 is set only on the way to the loop.
	 * The aligned body from s & -64 would re-check bytes already done
	 * and send 100-200 B strings to the 128-byte loop. A miss goes on at P = (q + 64) & -64
	 * ([s, P) is checked): a 128-aligned P is the loop's next group, else the body checks
	 * [P, P + 64) first (no group is checked twice). */
	cmpl $3952, %ecx
	ja .Lstrnlen_b_old
	addq $16, %rax
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm2
	movdqa 32(%rax), %xmm3
	movdqa 48(%rax), %xmm4
	pminub %xmm1, %xmm2
	pminub %xmm3, %xmm4
	pminub %xmm2, %xmm4
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm4, %edx
	testl %edx, %edx
	jnz .Lstrnlen_q_hit
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	addq $64, %rax
	andq $-64, %rax
	testb $64, %al
	jz .Lstrnlen_sse_loop
	addq $-64, %rax
	jmp .Lstrnlen_sse_body
.Lstrnlen_q_hit:
	/* xmm1 = [q, q + 16), xmm2 = the minimum of the first 32 bytes, xmm3 = [q + 32, q + 48),
	 * edx = the zero mask of the whole group's minimum: the first block holding a NUL is the
	 * first of them whose compare is nonzero (blocks before it are clean). */
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jnz 1f
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	bsfl %ecx, %ecx
	leal 16(%rcx), %ecx
	jnz 1f
	pcmpeqb %xmm0, %xmm3
	pmovmskb %xmm3, %ecx
	bsfl %ecx, %ecx
	leal 32(%rcx), %ecx
	jnz 1f
	bsfl %edx, %ecx
	addl $48, %ecx
1:
	subq %rdi, %rax
	addq %rcx, %rax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	ret
.Lstrnlen_b_old:
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	movq %rdi, %rax
	andq $-64, %rax
	jmp .Lstrnlen_sse_body
.Lstrnlen_at32:
	/* A zero lane of the fold with none in [s + 32, s + 48) is a NUL in [s + 48, s + 64). */
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz 1f
	addl $32, %eax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	ret
1:
	bsfl %edx, %eax
	addl $48, %eax
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	ret
.Lstrnlen_main:
	movq %rdi, %rax
	movl %edi, %ecx
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lstrnlen_sse2
	/* AVX2 (STR-K2): strlen's AVX2 shape, bounded as the SSE2 path: r11 = s + max, saturated;
	 * g1 and each 128-byte group are loaded only if they start below r11, so their first byte
	 * lies inside the caller's guaranteed min(max, strlen + 1) bytes (SAFETY: as strlen's; the
	 * head stays in s's page, mapped because max >= 1). Found indices are clamped to max. */
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	andq $-64, %rax
	andl $4095, %ecx
	vpxor %xmm0, %xmm0, %xmm0
	cmpl $4032, %ecx
	ja .Lstrnlen_avx_masked
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpcmpeqb 32(%rdi), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	jz .Lstrnlen_avx_body
	bsfq %rdx, %rax
	vzeroupper
	jmp .Lstrnlen_clamp
.Lstrnlen_avx_masked:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	/* The count is taken mod 64: s & 63. */
	shrq %cl, %rdx
	testq %rdx, %rdx
	jz .Lstrnlen_avx_body
	bsfq %rdx, %rax
	vzeroupper
	jmp .Lstrnlen_clamp
.Lstrnlen_avx_body:
	/* rax = s & -64; [s, rax + 64) holds no NUL. */
	addq $64, %rax
	cmpq %r11, %rax
	jae .Lstrnlen_avx_max
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	jnz .Lstrnlen_avx_found
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lstrnlen_avx_loop:
	cmpq %r11, %rax
	jae .Lstrnlen_avx_max
	vmovdqa (%rax), %ymm1
	vpminub 32(%rax), %ymm1, %ymm1
	vmovdqa 64(%rax), %ymm2
	vpminub 96(%rax), %ymm2, %ymm2
	vpminub %ymm1, %ymm2, %ymm3
	vpcmpeqb %ymm0, %ymm3, %ymm3
	subq $-128, %rax
	vpmovmskb %ymm3, %edx
	testl %edx, %edx
	jz .Lstrnlen_avx_loop
	addq $-128, %rax
	/* ymm1 = the minimum of the group's first 64 bytes. */
	vpcmpeqb %ymm0, %ymm1, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jnz .Lstrnlen_avx_half
	addq $64, %rax
.Lstrnlen_avx_half:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
.Lstrnlen_avx_found:
	bsfq %rdx, %rdx
	subq %rdi, %rax
	addq %rdx, %rax
	vzeroupper
	jmp .Lstrnlen_clamp
.Lstrnlen_avx_max:
	vzeroupper
	movq %rsi, %rax
	ret

.Lstrnlen_sse2:
	/* SSE2 (STR-K1), as strlen's, bounded: r11 = s + max, saturated; a group is loaded only
	 * if it starts below r11, so its first byte lies inside the caller's guaranteed
	 * min(max, strlen + 1) bytes (SAFETY: as strlen's). Found indices are clamped to max. */
	movq %rdi, %r11
	addq %rsi, %r11
	movq $-1, %rdx
	cmovcq %rdx, %r11
	andq $-64, %rax
	/* PERF-STRNLEN-RAWMEMCHR: reached only near a page end (page offset > 4032; the prologue's
	 * .Lstrnlen_third takes the rest on this level), so the masked aligned head always runs. */
.Lstrnlen_sse_masked:
	andl $63, %ecx
	pxor %xmm0, %xmm0
	/* mask64 %rax */
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm2
	movdqa 32(%rax), %xmm3
	movdqa 48(%rax), %xmm4
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r8d
	pmovmskb %xmm3, %r9d
	pmovmskb %xmm4, %r10d
	shll $16, %r8d
	orl %r8d, %edx
	shll $16, %r10d
	orl %r10d, %r9d
	shlq $32, %r9
	orq %r9, %rdx
	shrq %cl, %rdx
	testq %rdx, %rdx
	jnz .Lstrnlen_sse_head
.Lstrnlen_sse_body:
	addq $64, %rax
	cmpq %r11, %rax
	jae .Lstrnlen_max
	/* anyz64 %rax as a prefix fold (PERF-STRNLEN-BODY-3): xmm1 = the minimum of the group's
	 * first 32 bytes, ecx = the zero mask of the whole group's minimum; the hit is located from
	 * them by .Lstrnlen_sse_half with one block reload. */
	movdqa (%rax), %xmm1
	pminub 16(%rax), %xmm1
	movdqa 32(%rax), %xmm2
	pminub 48(%rax), %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	testl %ecx, %ecx
	jnz .Lstrnlen_sse_half
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lstrnlen_sse_loop:
	cmpq %r11, %rax
	jae .Lstrnlen_max
	movdqa (%rax), %xmm1
	pminub 16(%rax), %xmm1
	movdqa 32(%rax), %xmm2
	pminub 48(%rax), %xmm2
	movdqa 64(%rax), %xmm3
	pminub 80(%rax), %xmm3
	movdqa 96(%rax), %xmm4
	pminub 112(%rax), %xmm4
	/* Prefix fold (same pminub count as a plain fold): xmm1 = min of bytes 0-31, xmm2 = 0-63,
	 * xmm3 = 64-95, xmm4 = 0-127. */
	pminub %xmm1, %xmm2
	pminub %xmm3, %xmm4
	pminub %xmm2, %xmm4
	pcmpeqb %xmm0, %xmm4
	subq $-128, %rax
	pmovmskb %xmm4, %edx
	testl %edx, %edx
	jz .Lstrnlen_sse_loop
	/* PERF-STRNLEN-BODY-3: the exit locates from the fold registers (it used to re-load the
	 * first half, then reload up to four blocks). The first half holds the NUL iff xmm2's
	 * compare is nonzero; else edx (0-127) is the second half's mask and xmm3 its first 32. */
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	addq $-128, %rax
	testl %ecx, %ecx
	jnz .Lstrnlen_sse_half
	addq $64, %rax
	movdqa %xmm3, %xmm1
	movl %edx, %ecx
.Lstrnlen_sse_half:
	/* rax = a 64-byte group holding a NUL, xmm1 = the minimum of its first 32 bytes, ecx = the
	 * zero mask of its whole minimum (nonzero): with the first 32 bytes clean, ecx is the
	 * mask of the second 32. */
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	testl %edx, %edx
	jnz .Lstrnlen_sse_pair
	addq $32, %rax
	movl %ecx, %edx
.Lstrnlen_sse_pair:
	/* rax = 32 aligned bytes holding a NUL, edx = the zero mask of their minimum: with
	 * [rax, rax + 16) clean (the one reload), edx is [rax + 16, rax + 32)'s mask. */
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jnz 7f
	bsfl %edx, %ecx
	addl $16, %ecx
7:
	subq %rdi, %rax
	addq %rcx, %rax
	jmp .Lstrnlen_clamp
.Lstrnlen_sse_head:
	bsfq %rdx, %rax
.Lstrnlen_clamp:
	cmpq %rsi, %rax
	cmovaq %rsi, %rax
	ret
.Lstrnlen_max:
	movq %rsi, %rax
	ret
.Lstrnlen_zero:
	xorl %eax, %eax
	ret
	.size liella_machine_strnlen, .-liella_machine_strnlen
	.text

/* const void *liella_machine_rawmemchr(const void *s, int c): a POINTER to the first byte equal
 * to (unsigned char)c, which the caller asserts exists (STR-K2: a pointer, so the rawmemchr
 * export tail-calls it). Groups as in strlen, combined with OR. */
/* const void *liella_machine_rawmemchr_entry(const void *s, int c): the direct rawmemchr entry
 * (SCAN-TAIL; `rawmemchr` and `__rawmemchr` in the products). The S6 NULL test stays (rawmemchr
 * is not folded, D-4.48): a NULL s tail-jumps to `liella_hot_rawmemchr_rest` (strz::find: the
 * explicit abort); any other s falls through into the kernel below, saving the Rust entry's
 * byte conversion and jump. The kernel reads only c's low byte. Reads nothing itself. */
	.hidden liella_hot_rawmemchr_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_rawmemchr_entry
	.hidden liella_machine_rawmemchr_entry
	.type liella_machine_rawmemchr_entry, @function
	.p2align 6
liella_machine_rawmemchr_entry:
#ifdef LIELLA_EXPORT
	.globl rawmemchr
	.type rawmemchr, @function
rawmemchr:
	.globl __rawmemchr
	.type __rawmemchr, @function
__rawmemchr:
#endif
	testq %rdi, %rdi
	jz liella_hot_rawmemchr_rest
	/* Falls through: no alignment between the test and the kernel. */
	.globl liella_machine_rawmemchr
	.type liella_machine_rawmemchr, @function
liella_machine_rawmemchr:
	/* SMALL-STR head (D-4.45): on the AVX2 level with [s, s + 64) inside s's page, two unaligned
	 * 32-byte blocks [s, s + 32) and [s + 32, s + 64), one branch each, before any other setup
	 * (SAFETY: both loads lie in s's page, which holds s's first byte). A miss in both continues
	 * in the aligned AVX2 body with rax = s & -64: [s, rax + 64) is inside [s, s + 64), checked. */
	movl %edi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lraw_pre
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lraw_h32
	leaq (%rdi,%rdx), %rax
	vzeroupper
	ret
.Lraw_h32:
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lraw_h64
	leaq 32(%rdi,%rdx), %rax
	vzeroupper
	ret
.Lraw_h64:
	/* SCAN-RAW: the next two aligned 32-byte blocks, one branch each as the head (the 64-127
	 * byte strings skip the 64-byte mask merge). SAFETY: an aligned block starting at or before
	 * the asserted match (which is past s + 64) is readable. */
	/* STR-SCAN-SHORT: the blocks are a = (s + 64) & -32 and a + 32 (a in (s + 32, s + 64],
	 * so a holds s + 64: a match at s + 64 is found in the first block whatever s & 63 is;
	 * from s & -64 it took the second block for half the offsets). Same safety: both start at
	 * or before s + 96, past checked bytes, and before the asserted match. Two misses: [s, a + 64)
	 * is checked, and the loop entry (a + 64) & -128 is ((s & -64) + 128) & -128 (strlen's
	 * .Lstrlen_h64): in (s, a + 64]. */
	leaq 64(%rdi), %rax
	andq $-32, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lraw_h96
	addq %rdx, %rax
	vzeroupper
	ret
.Lraw_h96:
	vpcmpeqb 32(%rax), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lraw_h128
	leaq 32(%rax,%rdx), %rax
	vzeroupper
	ret
.Lraw_h128:
	/* PERF-STRNLEN-MID-2: the loop's first group, peeled as two 64-byte halves, so a match in
	 * the first half is located without the second half's compares and the second half ends
	 * in the loop's own branch (no jmp). The group at (a + 64) & -128 starts in (a - 64, a + 64]
	 * after bytes with no match (SAFETY: aligned groups never leave their page, as the loop's).
	 * The halves leave their compares in ymm1/ymm2 and ymm3/ymm4, as the loop's exit expects. */
	addq $64, %rax
	andq $-128, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpor %ymm1, %ymm2, %ymm5
	vptest %ymm5, %ymm5
	jnz .Lraw_avx_lo
	vpcmpeqb 64(%rax), %ymm0, %ymm3
	vpcmpeqb 96(%rax), %ymm0, %ymm4
	vpor %ymm3, %ymm4, %ymm6
	subq $-128, %rax
	vptest %ymm6, %ymm6
	jz .Lraw_avx_loop
	addq $-64, %rax
	jmp .Lraw_avx_hi
.Lraw_pre:
	/* ecx = s's page offset, from the entry test (SSE2-STRLEN). SSE2-STRLEN-2: here ecx >=
	 * short_lim, which is 4033 on the AVX2 level, so ecx <= 4032 means the SSE2 level AND
	 * [s, s + 64) inside s's page: one compare is both tests. */
	cmpl $4032, %ecx
	jbe .Lraw_sse_fast
	movq %rdi, %rax
	movd %esi, %xmm0
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lraw_sse2
	/* AVX2 (STR-K2): strlen's AVX2 shape with compares against the broadcast byte (vpor fold).
	 * SAFETY: as strlen's, with "no earlier byte matched" in place of "no earlier NUL": a loaded
	 * group's first byte is at or before the match the caller asserts, hence readable. */
	vpbroadcastb %xmm0, %ymm0
	andq $-64, %rax
	andl $4095, %ecx
	cmpl $4032, %ecx
	ja .Lraw_avx_masked
	/* SHORT-2: [s, s + 64) in s's page as two unaligned 32-byte blocks, one branch each
	 * (a hit in the first block costs no second load and no mask merge). */
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jnz .Lraw_avx_at0
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %edx
	testl %edx, %edx
	jz .Lraw_avx_body
	bsfl %edx, %edx
	leaq 32(%rdi,%rdx), %rax
	vzeroupper
	ret
.Lraw_avx_at0:
	bsfl %edx, %edx
	leaq (%rdi,%rdx), %rax
	vzeroupper
	ret
.Lraw_avx_masked:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	/* The count is taken mod 64: s & 63. */
	shrq %cl, %rdx
	testq %rdx, %rdx
	jz .Lraw_avx_body
	bsfq %rdx, %rax
	addq %rdi, %rax
	vzeroupper
	ret
.Lraw_avx_body:
	/* rax = s & -64; [s, rax + 64) holds no match. */
	addq $64, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	jnz .Lraw_avx_found
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lraw_avx_loop:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpcmpeqb 64(%rax), %ymm0, %ymm3
	vpcmpeqb 96(%rax), %ymm0, %ymm4
	vpor %ymm1, %ymm2, %ymm5
	vpor %ymm3, %ymm4, %ymm6
	vpor %ymm5, %ymm6, %ymm6
	subq $-128, %rax
	vpmovmskb %ymm6, %edx
	testl %edx, %edx
	jz .Lraw_avx_loop
	addq $-128, %rax
	/* The compares are still in ymm1-ymm4: the mask of the first or the second 64 bytes. */
.Lraw_avx_lo:
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %r8d
	shlq $32, %r8
	orq %r8, %rdx
	jnz .Lraw_avx_found
	addq $64, %rax
.Lraw_avx_hi:
	vpmovmskb %ymm3, %edx
	vpmovmskb %ymm4, %r8d
	shlq $32, %r8
	orq %r8, %rdx
.Lraw_avx_found:
	bsfq %rdx, %rdx
	addq %rdx, %rax
	vzeroupper
	ret

.Lraw_sse2:
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	/* SSE2 (STR-K1), as strlen's with compares against the broadcast byte. STR-K2 measured the
	 * alternatives for 64-byte strings on nico (cycles/call, best of 5): the old aligned
	 * 16-byte-step head 30-32, one unaligned 64-byte fold 27, this 16 + 48 head 27 (glibc 15);
	 * most of the gap is the fixed cost (17 vs 9 at length 0), hence the pointer return.
	 * SAFETY: as strlen's, with "no earlier byte matched" in place of "no earlier NUL": a loaded
	 * group's first byte is at or before the match the caller asserts, hence readable. */
	/* Reached only near a page end (ecx > 4032; the fast path below takes the rest). */
	andq $-64, %rax
	jmp .Lraw_sse_masked
.Lraw_sse_fast:
	/* SSE2-STRLEN-2: SSE2 level, [s, s + 64) inside s's page (SAFETY: every unaligned load
	 * below lies in it). Broadcast, then [s, s + 16) and [s + 16, s + 32) one branch each
	 * (bsf sets ZF on an empty mask: no separate test), then [s + 32, s + 64) folded. */
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	movdqu (%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jz 1f
	leaq (%rdi,%rdx), %rax
	ret
1:
	movdqu 16(%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jz 2f
	leaq 16(%rdi,%rdx), %rax
	ret
2:
	movdqu 32(%rdi), %xmm1
	movdqu 48(%rdi), %xmm2
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	por %xmm1, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz 3f
	/* PERF-STRNLEN-RAWMEMCHR: the aligned 16-byte block b = (s + 64) & -16, which holds s + 64
	 * (the next unchecked byte) and starts in (s + 48, s + 64]: a match at s + 64..79 ends here
	 * with one branch, no group fold and no locate step. SAFETY: b is aligned (no page crossing)
	 * and its first byte is a checked non-matching byte or s + 64 itself, which is at or before
	 * the match the caller asserts (none in [s, s + 64)). A miss enters the aligned body from
	 * s & -64: its first group starts at or before s + 64, inside the checked [s, b + 16). */
	leaq 64(%rdi), %rax
	andq $-16, %rax
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jz .Lraw_b_miss
	addq %rdx, %rax
	ret
.Lraw_b_miss:
	/* PERF-STRNLEN-BODY-3: q = b + 16 (16-aligned, in (s + 64, s + 80]; [s, q) is checked). With
	 * [s, s + 144) inside s's page (ecx = s's page offset <= 3952; SAFETY: the q group
	 * [q, q + 64) lies in that page, whose first checked bytes precede the asserted match), the
	 * group [q, q + 64) as the body's prefix fold, located by .Lraw_sse_g_hit from registers: the
	 * aligned body from s & -64 would re-check bytes already done and send 100-200 B strings to
	 * the 128-byte loop. A miss goes on at P = (q + 64) & -64 ([s, P) is checked): a
	 * 128-aligned P is the loop's next group, else the body checks [P, P + 64) first. */
	cmpl $3952, %ecx
	ja .Lraw_b_old
	addq $16, %rax
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	por %xmm1, %xmm2
	por %xmm3, %xmm4
	por %xmm2, %xmm4
	pmovmskb %xmm4, %ecx
	testl %ecx, %ecx
	jnz .Lraw_sse_g_hit
	addq $64, %rax
	andq $-64, %rax
	testb $64, %al
	jz .Lraw_sse_loop
	addq $-64, %rax
	jmp .Lraw_sse_body
.Lraw_b_old:
	movq %rdi, %rax
	andq $-64, %rax
	jmp .Lraw_sse_body
3:
	/* xmm1 = the [s + 32, s + 48) compare; a miss there puts the hit in [s + 48, s + 64). */
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jz 4f
	leaq 32(%rdi,%rcx), %rax
	ret
4:
	bsfl %edx, %edx
	leaq 48(%rdi,%rdx), %rax
	ret
.Lraw_sse_masked:
	andl $63, %ecx
	/* mask64 %rax */
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm2
	movdqa 32(%rax), %xmm3
	movdqa 48(%rax), %xmm4
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r8d
	pmovmskb %xmm3, %r9d
	pmovmskb %xmm4, %r10d
	shll $16, %r8d
	orl %r8d, %edx
	shll $16, %r10d
	orl %r10d, %r9d
	shlq $32, %r9
	orq %r9, %rdx
	shrq %cl, %rdx
	testq %rdx, %rdx
	jnz .Lraw_sse_head
.Lraw_sse_body:
	addq $64, %rax
	/* SSE2-MID: anyeq64 %rax as a prefix fold that keeps what the locate step needs: xmm1 = c0,
	 * xmm2 = c0|c1, xmm3 = c2, xmm4 = c0|c1|c2|c3 (ck = the compare of block k). Blocks before
	 * the first matching one are all zero, so the first nonzero of xmm1, xmm2, xmm3, xmm4's masks
	 * is that block's own mask: the hit is located from registers, with no reload (same por count
	 * as a plain fold). */
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	por %xmm1, %xmm2
	por %xmm3, %xmm4
	por %xmm2, %xmm4
	pmovmskb %xmm4, %ecx
	testl %ecx, %ecx
	jnz .Lraw_sse_g_hit
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lraw_sse_loop:
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	movdqa %xmm0, %xmm5
	pcmpeqb 64(%rax), %xmm5
	movdqa %xmm0, %xmm6
	pcmpeqb 80(%rax), %xmm6
	movdqa %xmm0, %xmm7
	pcmpeqb 96(%rax), %xmm7
	movdqa %xmm0, %xmm8
	pcmpeqb 112(%rax), %xmm8
	/* SSE2-MID prefix fold (as the body's): xmm2 = c0|c1, xmm4 = c0..c3, xmm6 = c4|c5,
	 * xmm8 = c0..c7; xmm1, xmm3, xmm5, xmm7 = c0, c2, c4, c6. */
	por %xmm1, %xmm2
	por %xmm3, %xmm4
	por %xmm5, %xmm6
	por %xmm7, %xmm8
	por %xmm2, %xmm4
	por %xmm6, %xmm8
	por %xmm4, %xmm8
	subq $-128, %rax
	pmovmskb %xmm8, %edx
	testl %edx, %edx
	jz .Lraw_sse_loop
	/* rax = the group's end. A hit in the first 64 bytes: xmm4's mask is nonzero. */
	pmovmskb %xmm4, %ecx
	testl %ecx, %ecx
	jz .Lraw_sse_h2
	addq $-128, %rax
.Lraw_sse_g_hit:
	/* rax = a 64-byte group, xmm1 = c0, xmm2 = c0|c1, xmm3 = c2, ecx = mask of c0..c3 (nonzero). */
	pmovmskb %xmm1, %edx
	bsfl %edx, %edx
	jnz 7f
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jnz 6f
	pmovmskb %xmm3, %edx
	bsfl %edx, %edx
	jnz 5f
	bsfl %ecx, %edx
	leaq 48(%rax,%rdx), %rax
	ret
5:
	leaq 32(%rax,%rdx), %rax
	ret
6:
	leaq 16(%rax,%rdx), %rax
	ret
7:
	addq %rdx, %rax
	ret
.Lraw_sse_h2:
	/* The second 64 bytes (the first are clean): xmm5 = c4, xmm6 = c4|c5, xmm7 = c6, edx = the
	 * mask of c0..c7, which is c7's when c0..c6 are clean. */
	pmovmskb %xmm5, %ecx
	bsfl %ecx, %ecx
	jnz 7f
	pmovmskb %xmm6, %ecx
	bsfl %ecx, %ecx
	jnz 6f
	pmovmskb %xmm7, %ecx
	bsfl %ecx, %ecx
	jnz 5f
	bsfl %edx, %edx
	leaq -16(%rax,%rdx), %rax
	ret
5:
	leaq -32(%rax,%rcx), %rax
	ret
6:
	leaq -48(%rax,%rcx), %rax
	ret
7:
	leaq -64(%rax,%rcx), %rax
	ret
.Lraw_sse_head:
	bsfq %rdx, %rax
	addq %rdi, %rax
	ret
	.size liella_machine_rawmemchr, .-liella_machine_rawmemchr
	.size liella_machine_rawmemchr_entry, .-liella_machine_rawmemchr_entry
#ifdef LIELLA_EXPORT
	.size rawmemchr, .-rawmemchr
	.size __rawmemchr, .-__rawmemchr
#endif

/* Bounded scanners (OD-C2 revised: memchr and memcmp; D-4.14's treatment). memcmp knows both
 * regions are readable whole, so every load lies inside [a, a + n) and [b, b + n): unaligned
 * vectors, in unrolled groups, and a last vector that overlaps the previous one and ends exactly
 * at the end. memchr may stop early (see below), so it reads as the terminator scanners do.
 * memcmp returns an index: the first difference, or n when there is none; Rust checks it (at
 * most n; the bytes there differ) and derives the sign itself. memchr returns a pointer or NULL
 * (STR-K3), checked in the selftest build. */

/* const void *liella_machine_memchr(const void *s, int c, size_t n): a POINTER to the first byte
 * of [s, s + n) equal to (unsigned char)c, or NULL (STR-K3: the export tail-calls it, as
 * rawmemchr; Rust checks the answer in the selftest build only, RESULT CHECKS ruling). ISO C
 * (C17 7.24.5.1p2) has memchr behave as if it read the bytes in order and stopped at the first
 * match, so a caller may pass an n that reaches past the end of the readable memory as long as
 * the byte is found before it. The scan is therefore the terminator scanners' (STR-K2: strlen's
 * shape, bounded as strnlen's). The caller (Rust, strz::find_n) answers n == 0 and rejects NULL
 * itself: here s != NULL and n >= 1 (STR-K3, HOT KERNELS ruling: the caller validates once).
 * AVX2 with [s, s + 64) inside s's page (page offset <= 4032): the head is [s, s + 64) unaligned,
 * read as two 32-byte halves whatever n is (MEMCHR-2); SSE2 the same as block 0 alone, then
 * [s + 16, s + 64) when n > 16 (MEMCHR-SSE2). Otherwise n <= 32 (AVX2) or 16 (SSE2) with that
 * vector inside s's page is one unaligned vector (STR-K3); else the head is the aligned
 * 64-byte group holding s, masked below s; then the aligned group
 * g1 = (s & -64) + 64 and 128-byte aligned groups from (g1 + 64) & -128, each loaded only if it
 * starts below r8 = s + n (saturated). SAFETY (D-4.14 page rule): n >= 1 makes s's page
 * readable, and the head never leaves it; an aligned group never spans two pages, and its first
 * byte lies in [s, s + n) after bytes that did not match, so the caller guarantees it. A match
 * found at or beyond s + n gives NULL. */

/* const void *liella_machine_memchr_entry(const void *s, int c, size_t n): the direct memchr entry
 * (MEMCHR-FOLD, D-4.48 amendment; owner ruling 2026-10-11 extending the S6 NULL-folding exception
 * to memchr under the same startup guard; `memchr` in the products). One gate: n - 1 below
 * `liella_machine_chr_limit` (machine/fold.S: isize::MAX while NULL folding is on, 0 otherwise)
 * falls through into the kernel below with no NULL test. A NULL s then faults on page zero: every
 * path's first load lies in [s & -64, s + 64), which for s = NULL is [0, 64), below the guard's
 * 64 KiB, whatever n is (no later load is reached). n == 0 (wraps to the maximum), n > isize::MAX
 * and folding off (limit 0: every n fails the gate) tail-jump to `liella_hot_memchr_rest`, the
 * checked entry (explicit S6 abort, the lazy folding decision). Reads nothing itself. */
	.hidden liella_hot_memchr_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_memchr_entry
	.hidden liella_machine_memchr_entry
	.type liella_machine_memchr_entry, @function
	.p2align 6
liella_machine_memchr_entry:
#ifdef LIELLA_EXPORT
	.globl memchr
	.type memchr, @function
memchr:
#endif
	leaq -1(%rdx), %r9
	cmpq liella_machine_chr_limit(%rip), %r9
	jae liella_hot_memchr_rest
	/* Falls through: no alignment between the gate and the kernel. */
	.globl liella_machine_memchr
	.type liella_machine_memchr, @function
liella_machine_memchr:
	/* SMALL-STR-2 (D-4.45): the short limit is the level test and the page test ([s, s + 64)
	 * inside s's page) in one compare; then, for every n, the 64-byte head (WL-GREP, D-4.48;
	 * MEMCHR-2 splits it in two 32-byte halves): [s, s + 64) unaligned, whose first match is
	 * the answer when it lies below n; a miss with n <= 64 is NULL, else the aligned groups
	 * follow. A match settled in the head pays no setup (grep's memchr calls mostly end in
	 * their first 64 bytes). */
	movl %edi, %ecx
	andl $4095, %ecx
	cmpl liella_machine_short_lim(%rip), %ecx
	jae .Lmemchr_entry
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	/* MEMCHR-2: [s, s + 64) lies in s's page (the short limit), so both of its 32-byte halves
	 * may be read whatever n is. The first half goes first and alone: a match there settles
	 * with one compare against n and no branch on n's class (most memchr calls end in their
	 * first 32 bytes). A match at or past n is outside the region, and then so is every later
	 * one, so the first match decides (bsf's undefined target for a zero mask is never used:
	 * ZF is then set). */
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsfl %eax, %eax
	jz .Lmemchr_head32
	vzeroupper
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_head32:
	/* [s, s + 32) holds no match; [s + 32, s + 64), read also when n <= 32 (it is in s's page;
	 * its match is then at or past n). */
	vpcmpeqb 32(%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsfl %eax, %eax
	jz .Lmemchr_head_miss
	vzeroupper
	addl $32, %eax
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_head_miss:
	/* [s, s + 64) holds no match: NULL when n <= 64 (the head covered [s, s + n)). Else the
	 * aligned group g1 = (s & -64) + 64 starts in (s, s + 64], below s + n, after bytes that did
	 * not match: loaded with no end test (D-4.14 page rule), and a match there is checked as an
	 * index from s (no end pointer to set up). */
	cmpq $64, %rdx
	jbe .Lmemchr_avx_none
	movq %rdi, %rax
	andq $-64, %rax
	vpcmpeqb 64(%rax), %ymm0, %ymm1
	vpcmpeqb 96(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %r9d
	vpmovmskb %ymm2, %r10d
	shlq $32, %r10
	orq %r10, %r9
	jz .Lmemchr_g1_miss
	bsfq %r9, %r9
	vzeroupper
	leaq 64(%rax,%r9), %rax
	movq %rax, %r9
	subq %rdi, %r9
	cmpq %rdx, %r9
	jae .Lmemchr_none
	ret
.Lmemchr_g1_miss:
	/* As .Lmemchr_avx_body after its first group: rax = g1, r8 = s + n (saturated). */
	addq $64, %rax
	movq %rdi, %r8
	addq %rdx, %r8
	movq $-1, %r9
	cmovcq %r9, %r8
	jmp .Lmemchr_avx_g2
	/* STR-K3: 1 <= n <= 32 and [s, s + 32) inside s's page: one vector; a match counts only
	 * below n (bsf's undefined target for a zero mask is never used: ZF is then set). */
.Lmemchr_n32:
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	vzeroupper
	bsfl %eax, %eax
	jz .Lmemchr_none
	cmpl %edx, %eax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_entry:
	movd %esi, %xmm0
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lmemchr_sse2
	vpbroadcastb %xmm0, %ymm0
	cmpq $32, %rdx
	ja .Lmemchr_avx_setup
	cmpl $4064, %ecx
	ja .Lmemchr_avx_setup
	jmp .Lmemchr_n32
.Lmemchr_avx_setup:
	movq %rdi, %rax
	andq $-64, %rax
	movq %rdi, %r8
	addq %rdx, %r8
	movq $-1, %r9
	cmovcq %r9, %r8
	cmpl $4032, %ecx
	ja .Lmemchr_avx_masked
	vpcmpeqb (%rdi), %ymm0, %ymm1
	vpcmpeqb 32(%rdi), %ymm0, %ymm2
	vpmovmskb %ymm1, %r9d
	vpmovmskb %ymm2, %r10d
	shlq $32, %r10
	orq %r10, %r9
	jz .Lmemchr_avx_body
	bsfq %r9, %rax
	vzeroupper
	jmp .Lmemchr_clamp
.Lmemchr_avx_masked:
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %r9d
	vpmovmskb %ymm2, %r10d
	shlq $32, %r10
	orq %r10, %r9
	/* The count is taken mod 64: s & 63. */
	shrq %cl, %r9
	testq %r9, %r9
	jz .Lmemchr_avx_body
	bsfq %r9, %rax
	vzeroupper
	jmp .Lmemchr_clamp
.Lmemchr_avx_body:
	/* rax = s & -64; [s, rax + 64) holds no match. */
	addq $64, %rax
	cmpq %r8, %rax
	jae .Lmemchr_avx_none
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpmovmskb %ymm1, %r9d
	vpmovmskb %ymm2, %r10d
	shlq $32, %r10
	orq %r10, %r9
	jnz .Lmemchr_avx_found
.Lmemchr_avx_g2:
	/* rax = g1; [s, g1 + 64) holds no match. */
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lmemchr_avx_loop:
	cmpq %r8, %rax
	jae .Lmemchr_avx_none
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpcmpeqb 64(%rax), %ymm0, %ymm3
	vpcmpeqb 96(%rax), %ymm0, %ymm4
	vpor %ymm1, %ymm2, %ymm5
	vpor %ymm3, %ymm4, %ymm6
	vpor %ymm5, %ymm6, %ymm6
	subq $-128, %rax
	vpmovmskb %ymm6, %r9d
	testl %r9d, %r9d
	jz .Lmemchr_avx_loop
	addq $-128, %rax
	/* The compares are still in ymm1-ymm4: the mask of the first or the second 64 bytes. */
	vpmovmskb %ymm1, %r9d
	vpmovmskb %ymm2, %r10d
	shlq $32, %r10
	orq %r10, %r9
	jnz .Lmemchr_avx_found
	addq $64, %rax
	vpmovmskb %ymm3, %r9d
	vpmovmskb %ymm4, %r10d
	shlq $32, %r10
	orq %r10, %r9
.Lmemchr_avx_found:
	/* A pointer from here: at or past r8 = s + n (saturated) is outside the region. */
	bsfq %r9, %r9
	addq %r9, %rax
	vzeroupper
	cmpq %r8, %rax
	jae .Lmemchr_none
	ret
.Lmemchr_avx_none:
	vzeroupper
	xorl %eax, %eax
	ret

	/* SSE2: the same with 16-byte blocks; the head and the groups fold with por, and a hit is
	 * located per 16-byte block with a branch each (STR-K1's measurement). */
.Lmemchr_sse2:
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	cmpl $4032, %ecx
	ja .Lmemchr_sse_edge
	/* MEMCHR-SSE2 (the AVX2 head's order, MEMCHR-2): [s, s + 64) lies in s's page, so all four
	 * of its 16-byte blocks may be read whatever n is. Block 0 goes first and alone: a match
	 * there settles with one compare against n and no branch on n's class. A match at or past n
	 * is outside the region, and then so is every later one, so the first match decides (bsf's
	 * undefined target for a zero mask is never used: ZF is then set). */
	movdqu (%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz .Lmemchr_sse_head16
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_head16:
	/* [s, s + 16) holds no match. Blocks 1-3 ([s + 16, s + 64), inside s's page) fold with por
	 * whatever n is: with n <= 16 a hit there is at or past n and a miss ends at head_miss's
	 * n <= 64 test, so no n <= 16 test is spent on the common long calls (SSE2-GAP-4). The fold
	 * goes into block 3's compare (no copy), so its mask is block 3's own when blocks 1-2 hold
	 * no match. A hit is resolved per block (SSE2-GAP-2 measured a combined 48-bit mask: 2 more
	 * instructions per miss). A match at or past n is outside the region, and then so is every
	 * later one, so the lowest match decides. */
	movdqu 16(%rdi), %xmm1
	movdqu 32(%rdi), %xmm2
	movdqu 48(%rdi), %xmm3
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	por %xmm1, %xmm3
	por %xmm2, %xmm3
	pmovmskb %xmm3, %r9d
	testl %r9d, %r9d
	jz .Lmemchr_sse_head_miss
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jnz .Lmemchr_sse_h16_hit
	pmovmskb %xmm2, %eax
	bsfl %eax, %eax
	jnz .Lmemchr_sse_h32_hit
	bsfl %r9d, %eax
	addl $48, %eax
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_h32_hit:
	addl $32, %eax
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_h16_hit:
	addl $16, %eax
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_head_miss:
	/* [s, s + 64) holds no match: NULL when n <= 64 (the head covered [s, s + n)). Else the
	 * aligned group g1 = (s & -64) + 64 starts in (s, s + 64], below s + n, after bytes that did
	 * not match (D-4.14 page rule): it is loaded with no end test, as the AVX2 head_miss
	 * (SSE2-GAP-3). The fold goes into block 3's compare, so blocks 0-2 keep theirs and a hit is
	 * resolved from registers (no reload), checked as an index from s (no end pointer); the
	 * saturated end pointer r8 is set up only when g1 misses, for the loop. */
	cmpq $64, %rdx
	jbe .Lmemchr_none
	movq %rdi, %rax
	andq $-64, %rax
	addq $64, %rax
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	por %xmm1, %xmm4
	por %xmm2, %xmm4
	por %xmm3, %xmm4
	pmovmskb %xmm4, %r9d
	testl %r9d, %r9d
	jnz .Lmemchr_sse_g1_hit
	movq %rdi, %r8
	addq %rdx, %r8
	movq $-1, %r9
	cmovcq %r9, %r8
	addq $64, %rax
	andq $-128, %rax
	jmp .Lmemchr_sse_loop
.Lmemchr_sse_g1_hit:
	/* r9d = the fold's mask: when blocks 0-2 hold no match it is block 3's own. */
	subq %rdi, %rax
	pmovmskb %xmm1, %r10d
	bsfl %r10d, %r10d
	jnz 7f
	pmovmskb %xmm2, %r10d
	bsfl %r10d, %r10d
	leal 16(%r10), %r10d
	jnz 7f
	pmovmskb %xmm3, %r10d
	bsfl %r10d, %r10d
	leal 32(%r10), %r10d
	jnz 7f
	bsfl %r9d, %r10d
	addl $48, %r10d
7:
	/* rax = g1 - s in (0, 64], r10 < 64: the match's index from s, no overflow. */
	addq %r10, %rax
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_edge:
	/* [s, s + 64) leaves s's page. */
	cmpq $16, %rdx
	ja .Lmemchr_sse_setup
	cmpl $4080, %ecx
	ja .Lmemchr_sse_setup
	/* STR-K3: 1 <= n <= 16 and [s, s + 16) inside s's page: one vector, as the AVX2 path. */
	movdqu (%rdi), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	bsfl %eax, %eax
	jz .Lmemchr_none
	cmpl %edx, %eax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_sse_setup:
	/* Page offset > 4032: the aligned 64-byte group holding s, masked below s. */
	movq %rdi, %rax
	andq $-64, %rax
	movq %rdi, %r8
	addq %rdx, %r8
	movq $-1, %r9
	cmovcq %r9, %r8
	/* mask64 %rax (r9 = the mask; esi is free after the broadcast) */
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm2
	movdqa 32(%rax), %xmm3
	movdqa 48(%rax), %xmm4
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm1, %r9d
	pmovmskb %xmm2, %r10d
	pmovmskb %xmm3, %r11d
	pmovmskb %xmm4, %esi
	shll $16, %r10d
	orl %r10d, %r9d
	shll $16, %esi
	orl %esi, %r11d
	shlq $32, %r11
	orq %r11, %r9
	/* The count is taken mod 64: s & 63. */
	shrq %cl, %r9
	testq %r9, %r9
	jz .Lmemchr_sse_body
	bsfq %r9, %rax
	jmp .Lmemchr_clamp
.Lmemchr_sse_body:
	/* rax = s & -64; [s, rax + 64) holds no match. */
	addq $64, %rax
	cmpq %r8, %rax
	jae .Lmemchr_none
.Lmemchr_sse_g1:
	/* anyeq64 %rax: rax = g1 */
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	por %xmm2, %xmm1
	por %xmm4, %xmm3
	por %xmm3, %xmm1
	pmovmskb %xmm1, %r9d
	testl %r9d, %r9d
	jnz .Lmemchr_sse_hit
	addq $64, %rax
	andq $-128, %rax
	.p2align 4
.Lmemchr_sse_loop:
	cmpq %r8, %rax
	jae .Lmemchr_none
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	movdqa %xmm0, %xmm5
	pcmpeqb 64(%rax), %xmm5
	movdqa %xmm0, %xmm6
	pcmpeqb 80(%rax), %xmm6
	movdqa %xmm0, %xmm7
	pcmpeqb 96(%rax), %xmm7
	movdqa %xmm0, %xmm8
	pcmpeqb 112(%rax), %xmm8
	por %xmm2, %xmm1
	por %xmm4, %xmm3
	por %xmm6, %xmm5
	por %xmm8, %xmm7
	por %xmm3, %xmm1
	por %xmm7, %xmm5
	/* xmm1 keeps the first 64 bytes' fold for the exit. */
	por %xmm1, %xmm5
	subq $-128, %rax
	pmovmskb %xmm5, %r9d
	testl %r9d, %r9d
	jz .Lmemchr_sse_loop
	addq $-128, %rax
	pmovmskb %xmm1, %r9d
	testl %r9d, %r9d
	jnz .Lmemchr_sse_hit
	addq $64, %rax
.Lmemchr_sse_hit:
	/* find64 %rax (r9 = the index) */
	movdqa (%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r9d
	bsfl %r9d, %r9d
	jnz 7f
	movdqa 16(%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r9d
	bsfl %r9d, %r9d
	leal 16(%r9), %r9d
	jnz 7f
	movdqa 32(%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r9d
	bsfl %r9d, %r9d
	leal 32(%r9), %r9d
	jnz 7f
	movdqa 48(%rax), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r9d
	bsfl %r9d, %r9d
	leal 48(%r9), %r9d
7:
	addq %r9, %rax
	cmpq %r8, %rax
	jae .Lmemchr_none
	ret
.Lmemchr_clamp:
	/* rax = an index from s: inside the region, s + rax; else NULL. */
	cmpq %rdx, %rax
	jae .Lmemchr_none
	addq %rdi, %rax
	ret
.Lmemchr_none:
	xorl %eax, %eax
	ret
	.size liella_machine_memchr, .-liella_machine_memchr
#ifdef LIELLA_EXPORT
	.size memchr, .-memchr
#endif
	.size liella_machine_memchr_entry, .-liella_machine_memchr_entry

/* size_t liella_machine_memcmp(const void *a, const void *b, size_t n): the index of the first
 * byte where [a, a + n) and [b, b + n) differ, or n. */
	.globl liella_machine_memcmp
	.type liella_machine_memcmp, @function
	.p2align 6
liella_machine_memcmp:
	cmpq $16, %rdx
	jb .Lmemcmp_small
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lmemcmp_sse2
	cmpq $32, %rdx
	jb .Lmemcmp_sse2
	xorl %eax, %eax
	movq %rdx, %r8
	subq $128, %r8
	.p2align 4
.Lmemcmp_avx_loop:
	cmpq %r8, %rax
	jg .Lmemcmp_avx_tail
	vmovdqu (%rdi,%rax), %ymm1
	vmovdqu 32(%rdi,%rax), %ymm2
	vmovdqu 64(%rdi,%rax), %ymm3
	vmovdqu 96(%rdi,%rax), %ymm4
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm1
	vpcmpeqb 32(%rsi,%rax), %ymm2, %ymm2
	vpcmpeqb 64(%rsi,%rax), %ymm3, %ymm3
	vpcmpeqb 96(%rsi,%rax), %ymm4, %ymm4
	vpand %ymm1, %ymm2, %ymm5
	vpand %ymm3, %ymm4, %ymm6
	vpand %ymm5, %ymm6, %ymm5
	vpmovmskb %ymm5, %ecx
	cmpl $-1, %ecx
	jne .Lmemcmp_avx_group
	subq $-128, %rax
	jmp .Lmemcmp_avx_loop
.Lmemcmp_avx_group:
	vpmovmskb %ymm1, %ecx
	notl %ecx
	testl %ecx, %ecx
	jnz .Lmemcmp_avx_found
	addq $32, %rax
	vpmovmskb %ymm2, %ecx
	notl %ecx
	testl %ecx, %ecx
	jnz .Lmemcmp_avx_found
	addq $32, %rax
	vpmovmskb %ymm3, %ecx
	notl %ecx
	testl %ecx, %ecx
	jnz .Lmemcmp_avx_found
	addq $32, %rax
	vpmovmskb %ymm4, %ecx
	notl %ecx
.Lmemcmp_avx_found:
	bsfl %ecx, %ecx
	addq %rcx, %rax
	vzeroupper
	ret
.Lmemcmp_avx_tail:
	movq %rdx, %r8
	subq $32, %r8
.Lmemcmp_avx_tail_loop:
	cmpq %r8, %rax
	jg .Lmemcmp_avx_last
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm1
	vpmovmskb %ymm1, %ecx
	notl %ecx
	testl %ecx, %ecx
	jnz .Lmemcmp_avx_found
	addq $32, %rax
	jmp .Lmemcmp_avx_tail_loop
.Lmemcmp_avx_last:
	cmpq %rdx, %rax
	jae .Lmemcmp_avx_none
	movq %r8, %rax
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm1
	vpmovmskb %ymm1, %ecx
	notl %ecx
	testl %ecx, %ecx
	jnz .Lmemcmp_avx_found
.Lmemcmp_avx_none:
	movq %rdx, %rax
	vzeroupper
	ret

.Lmemcmp_sse2:
	xorl %eax, %eax
	movq %rdx, %r8
	subq $64, %r8
	.p2align 4
.Lmemcmp_sse_loop:
	cmpq %r8, %rax
	jg .Lmemcmp_sse_tail
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm1
	movdqu 16(%rdi,%rax), %xmm2
	movdqu 16(%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm2
	movdqu 32(%rdi,%rax), %xmm3
	movdqu 32(%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm3
	movdqu 48(%rdi,%rax), %xmm4
	movdqu 48(%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm4
	movdqa %xmm1, %xmm5
	pand %xmm2, %xmm5
	movdqa %xmm3, %xmm6
	pand %xmm4, %xmm6
	pand %xmm6, %xmm5
	pmovmskb %xmm5, %ecx
	cmpl $0xffff, %ecx
	jne .Lmemcmp_sse_group
	addq $64, %rax
	jmp .Lmemcmp_sse_loop
.Lmemcmp_sse_group:
	pmovmskb %xmm1, %ecx
	xorl $0xffff, %ecx
	jnz .Lmemcmp_sse_found
	addq $16, %rax
	pmovmskb %xmm2, %ecx
	xorl $0xffff, %ecx
	jnz .Lmemcmp_sse_found
	addq $16, %rax
	pmovmskb %xmm3, %ecx
	xorl $0xffff, %ecx
	jnz .Lmemcmp_sse_found
	addq $16, %rax
	pmovmskb %xmm4, %ecx
	xorl $0xffff, %ecx
.Lmemcmp_sse_found:
	bsfl %ecx, %ecx
	addq %rcx, %rax
	ret
.Lmemcmp_sse_tail:
	movq %rdx, %r8
	subq $16, %r8
.Lmemcmp_sse_tail_loop:
	cmpq %r8, %rax
	jg .Lmemcmp_sse_last
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm1
	pmovmskb %xmm1, %ecx
	xorl $0xffff, %ecx
	jnz .Lmemcmp_sse_found
	addq $16, %rax
	jmp .Lmemcmp_sse_tail_loop
.Lmemcmp_sse_last:
	cmpq %rdx, %rax
	jae .Lmemcmp_sse_none
	movq %r8, %rax
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm5
	pcmpeqb %xmm5, %xmm1
	pmovmskb %xmm1, %ecx
	xorl $0xffff, %ecx
	jnz .Lmemcmp_sse_found
.Lmemcmp_sse_none:
	movq %rdx, %rax
	ret

/* Fewer than 16 bytes: two overlapping 8-byte words from 8 bytes on, else byte by byte. */
.Lmemcmp_small:
	cmpq $8, %rdx
	jb .Lmemcmp_bytes
	movq (%rdi), %rax
	xorq (%rsi), %rax
	jnz .Lmemcmp_word0
	movq -8(%rdi,%rdx), %rcx
	xorq -8(%rsi,%rdx), %rcx
	jnz .Lmemcmp_word1
	movq %rdx, %rax
	ret
.Lmemcmp_word0:
	bsfq %rax, %rax
	shrq $3, %rax
	ret
.Lmemcmp_word1:
	bsfq %rcx, %rcx
	shrq $3, %rcx
	leaq -8(%rdx,%rcx), %rax
	ret
.Lmemcmp_bytes:
	xorl %eax, %eax
.Lmemcmp_byte_loop:
	cmpq %rdx, %rax
	jae .Lmemcmp_byte_done
	movzbl (%rdi,%rax), %ecx
	cmpb (%rsi,%rax), %cl
	jne .Lmemcmp_byte_done
	incq %rax
	jmp .Lmemcmp_byte_loop
.Lmemcmp_byte_done:
	ret
	.size liella_machine_memcmp, .-liella_machine_memcmp

/* int liella_machine_strcmp(const char *a, const char *b) (PJ-8; STRCOLL-1, D-4.29): the order
 * of the strings as strcmp defines it (unsigned bytes): -1, 0 or 1. One pass over both strings
 * (no separate measuring). SSE2 at scan level 0; at level 1 the same steps with 32-byte AVX2
 * blocks (two per 64-byte step), VEX-encoded, vzeroupper before returning from them.
 *
 * Page rule: the two strings have unrelated alignments, so the loads are unaligned, and each
 * is taken only when it stays inside the page holding its first byte, which belongs to a
 * string not yet ended (no terminator before it). At .Lstrcmp_top, ecx = the larger page
 * offset of a + i and b + i: above 4032 one 16-byte block (AVX2: 32), stepped back to end at
 * that page's end when it would cross it (single bytes only while i is below the step);
 * otherwise four 16-byte blocks, each tested (short strings end in the block that finds
 * them), then (4032 - ecx) / 64 groups of 64 bytes, all inside both pages, with one test per
 * group (the four blocks' masks folded by pminub), before the offsets are taken again. In a block,
 * lane k is zero in min(a, a == b ? 0xFF : 0) iff the bytes differ or a's byte is the
 * terminator.
 *
 * Settling: the order comes from the two bytes at the first such index, read once more. If a
 * writer made them equal and not NUL since the vector read, the scan goes on after them
 * (D-4.21's rescan, here in the kernel; counted in liella_machine_strcmp_rescans for the
 * selftest), so the answer always comes from bytes this call read. */
/* int liella_machine_strcmp_entry(const char *a, const char *b): the direct strcmp entry
 * (STRCMP-ENTRY; `strcmp` in the products): the kernel below itself, with no wrapper, call or
 * jump. The S6 NULL tests (§4.3: NULL aborts) live in the kernel's slow path (STRCMP-HEADNULL):
 * the head's page test takes its window from one byte before each string, so a NULL argument
 * (window 0xFFFFFFFF..63, or ..127 in the AVX2 head) always fails it and reaches them before any read; they jump to
 * `liella_hot_strcmp_null`, the checked entry, which aborts. Page-aligned starts take the slow
 * path too (one start in 4096). */
	.hidden liella_hot_strcmp_null  /* crates/boundary/src/posix/s04_strings.rs */
	.section .text.liella_machine_strcmp,"ax",@progbits
	.globl liella_machine_strcmp_entry
	.hidden liella_machine_strcmp_entry
	.type liella_machine_strcmp_entry, @function
	.p2align 6
liella_machine_strcmp_entry:
#ifdef LIELLA_EXPORT
	.globl strcmp
	.type strcmp, @function
strcmp:
#endif
	/* Falls through: the NULL tests are in the kernel's slow path (STRCMP-HEADNULL). */
	.globl liella_machine_strcmp
	.type liella_machine_strcmp, @function
liella_machine_strcmp:
	cmpl $0, liella_machine_scan_level(%rip)
	jne .Lstrcmp_avx2
	/* Head (STRCMP-3, STRCMP-HEAD, D-4.38): the AVX2 head below in SSE2, as four 16-byte blocks
	 * (both loads movdqu; the zero vector from the aligned .rodata constant). Each later block's
	 * stop is settled with its offset in the displacement, sharing the head's returns.
	 * Page test per string (PERF-STRCMP-LOOP): the 65 bytes from one before the start stay in
	 * one page iff that byte's page offset is at most 4031 (8 instructions; the bit-12 XOR of
	 * both ends for both strings took 9). NULL (offset 4095) and page-aligned starts fail it. */
	leal -1(%rdi), %ecx
	andl $4095, %ecx
	cmpl $4031, %ecx
	ja .Lstrcmp_start
	leal -1(%rsi), %edx
	andl $4095, %edx
	cmpl $4031, %edx
	ja .Lstrcmp_start
	movdqu (%rdi), %xmm1
	movdqu (%rsi), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb .Lstrcmp_zero16(%rip), %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_head16
	bsfl %edx, %edx
	movzbl (%rdi,%rdx), %eax
	cmpb (%rsi,%rdx), %al
	jne .Lstrcmp_differ
	testl %eax, %eax
	jnz .Lstrcmp_head_rescan
.Lstrcmp_ret_head_equal:
	ret
.Lstrcmp_head_rescan:
	pxor %xmm0, %xmm0
	movl %edx, %eax
	jmp .Lstrcmp_rescan
.Lstrcmp_head16:
	movdqu 16(%rdi), %xmm1
	movdqu 16(%rsi), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb .Lstrcmp_zero16(%rip), %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_head32
	bsfl %edx, %edx
	movzbl 16(%rdi,%rdx), %eax
	cmpb 16(%rsi,%rdx), %al
	jne .Lstrcmp_differ
	testl %eax, %eax
	jz .Lstrcmp_ret_head_equal
	pxor %xmm0, %xmm0
	leal 16(%rdx), %eax
	jmp .Lstrcmp_rescan
.Lstrcmp_head32:
	movdqu 32(%rdi), %xmm1
	movdqu 32(%rsi), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb .Lstrcmp_zero16(%rip), %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_head48
	bsfl %edx, %edx
	movzbl 32(%rdi,%rdx), %eax
	cmpb 32(%rsi,%rdx), %al
	jne .Lstrcmp_differ
	testl %eax, %eax
	jz .Lstrcmp_ret_head_equal
	pxor %xmm0, %xmm0
	leal 32(%rdx), %eax
	jmp .Lstrcmp_rescan
.Lstrcmp_head48:
	movdqu 48(%rdi), %xmm1
	movdqu 48(%rsi), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb .Lstrcmp_zero16(%rip), %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_head_miss
	bsfl %edx, %edx
	movzbl 48(%rdi,%rdx), %eax
	cmpb 48(%rsi,%rdx), %al
	jne .Lstrcmp_differ
	testl %eax, %eax
	jz .Lstrcmp_ret_head_equal
	pxor %xmm0, %xmm0
	leal 48(%rdx), %eax
	jmp .Lstrcmp_rescan
.Lstrcmp_head_miss:
	/* Bytes 0-63 equal and not NUL: the general steps go on at 64 (their page test first).
	 * Going straight to the groups here instead was measured (PERF-STRCMP-NONAVX2): -9
	 * instructions at a stop near 256 but +16 at a stop in bytes 64-127, so the four tested
	 * blocks stay. */
	pxor %xmm0, %xmm0
	movl $64, %eax
	jmp .Lstrcmp_top
.Lstrcmp_gtop:
	/* After a run of groups: as .Lstrcmp_top, but 64 bytes that fit go straight to the groups,
	 * 1 + (4032 - ecx) / 64 of them (the strings are known to be longer than a block). */
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	orl %edx, %ecx
	andl $4095, %ecx
	subl $4032, %ecx
	ja .Lstrcmp_exact
	negl %ecx
	shrl $6, %ecx
	incl %ecx
	jmp .Lstrcmp_group
.Lstrcmp_start:
	/* S6 NULL tests (STRCMP-HEADNULL): the head's page window starts one byte before each
	 * string, so NULL and page-aligned starts always come here first. */
	testq %rdi, %rdi
	je liella_hot_strcmp_null
	testq %rsi, %rsi
	je liella_hot_strcmp_null
	xorl %eax, %eax
	pxor %xmm0, %xmm0
	/* First pass: one test for both pages. The OR of the two offsets is at least the larger one,
	 * so OR <= 4032 proves both are; above it the exact steps at .Lstrcmp_top decide. */
	movl %edi, %ecx
	orl %esi, %ecx
	andl $4095, %ecx
	subl $4032, %ecx
	ja .Lstrcmp_exact
.Lstrcmp_blocks:
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found
	/* Blocks 1-3 carry their offset in the displacement and settle through .Lstrcmp_found16/
	 * 32/48 (PERF-STRCMP-LOOP): one add per pass instead of four, never more per stop. */
	movdqu 16(%rdi,%rax), %xmm1
	movdqu 16(%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found16
	movdqu 32(%rdi,%rax), %xmm1
	movdqu 32(%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found32
	movdqu 48(%rdi,%rax), %xmm1
	movdqu 48(%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found48
	addq $64, %rax
	negl %ecx
	shrl $6, %ecx
	jz .Lstrcmp_top
.Lstrcmp_group:
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	movdqu 16(%rdi,%rax), %xmm3
	movdqu 16(%rsi,%rax), %xmm4
	pcmpeqb %xmm3, %xmm4
	pminub %xmm3, %xmm4
	movdqu 32(%rdi,%rax), %xmm5
	movdqu 32(%rsi,%rax), %xmm6
	pcmpeqb %xmm5, %xmm6
	pminub %xmm5, %xmm6
	movdqu 48(%rdi,%rax), %xmm7
	movdqu 48(%rsi,%rax), %xmm8
	pcmpeqb %xmm7, %xmm8
	pminub %xmm7, %xmm8
	/* The fold is a tree in place (3 instructions per group): blocks 1 and 3 keep their
	 * minima (xmm4, xmm8), and a's blocks 0 and 2 stay in xmm1 and xmm5 for .Lstrcmp_locate.
	 * A copy that kept all four (PERF-STRCMP-NONAVX2 run 1) cost one instruction per group. */
	pminub %xmm4, %xmm2
	pminub %xmm8, %xmm6
	pminub %xmm6, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_locate
	addq $64, %rax
	decl %ecx
	jnz .Lstrcmp_group
	jmp .Lstrcmp_gtop
.Lstrcmp_locate:
	/* The group holds the index: blocks in order, first stop wins. Blocks 1 and 3 from the
	 * kept minima; blocks 0 and 2 from a's kept bytes and b's block read once more (the fold
	 * overwrote them). PERF-STRCMP-NONAVX2-3: 9 instructions to the settle for a stop in
	 * block 0, 14 / 22 / 27 in blocks 1 / 2 / 3 (the old four-block re-read loop: 11, 22,
	 * 33, 44; the 64-bit mask of run 1: 17 everywhere); PERF-STRCMP-LOOP moved the offsets
	 * into the displacement and .Lstrcmp_found16/32/48: 9 / 14 / 21 / 25. If a writer removed
	 * it meanwhile, go on after the group; the settle rescans a stop a writer moved. */
	movdqu (%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found
	pcmpeqb %xmm0, %xmm4
	pmovmskb %xmm4, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found16
	movdqu 32(%rsi,%rax), %xmm6
	pcmpeqb %xmm5, %xmm6
	pminub %xmm5, %xmm6
	pcmpeqb %xmm0, %xmm6
	pmovmskb %xmm6, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found32
	pcmpeqb %xmm0, %xmm8
	pmovmskb %xmm8, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found48
	addq $64, %rax
	jmp .Lstrcmp_top
.Lstrcmp_top:
	/* Offsets again at index rax: ecx = max(offset a+i, offset b+i) - 4032. The OR bound
	 * (exact when one offset is the larger and has no bit the other lacks) is tried first;
	 * above it the exact maximum decides: up to 4032 the blocks, up to 4080 one 16-byte
	 * block, above that one byte. */
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	orl %edx, %ecx
	andl $4095, %ecx
	subl $4032, %ecx
	jbe .Lstrcmp_blocks
.Lstrcmp_exact:
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	andl $4095, %ecx
	andl $4095, %edx
	cmpl %ecx, %edx
	cmova %edx, %ecx
	subl $4032, %ecx
	jbe .Lstrcmp_blocks
	/* Above offset 4080 the 16-byte block steps back to end at the page end (as in the AVX2
	 * steps below, STRCMP-2); single bytes only when i is below the step. */
	subl $48, %ecx
	jbe .Lstrcmp_one
	cmpq %rcx, %rax
	jb .Lstrcmp_byte
	subq %rcx, %rax
.Lstrcmp_one:
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_found
	addq $16, %rax
	jmp .Lstrcmp_top
.Lstrcmp_byte:
	movzbl (%rdi,%rax), %ecx
	cmpb (%rsi,%rax), %cl
	jne .Lstrcmp_settle
	testl %ecx, %ecx
	jz .Lstrcmp_settle
	incq %rax
	jmp .Lstrcmp_top
.Lstrcmp_found16:
	bsfl %edx, %edx
	leaq 16(%rax,%rdx), %rax
	jmp .Lstrcmp_settle
.Lstrcmp_found32:
	bsfl %edx, %edx
	leaq 32(%rax,%rdx), %rax
	jmp .Lstrcmp_settle
.Lstrcmp_found48:
	bsfl %edx, %edx
	leaq 48(%rax,%rdx), %rax
	jmp .Lstrcmp_settle
.Lstrcmp_found:
	bsfl %edx, %edx
	addq %rdx, %rax
.Lstrcmp_settle:
	movzbl (%rdi,%rax), %ecx
	cmpb (%rsi,%rax), %cl
	jne .Lstrcmp_differ
	testl %ecx, %ecx
	jnz .Lstrcmp_rescan
.Lstrcmp_ret_equal:
	xorl %eax, %eax
	ret
.Lstrcmp_differ:
	/* CF = a's byte < b's byte: -1, else 1. */
.Lstrcmp_ret_differ:
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lstrcmp_rescan:
	lock incq liella_machine_strcmp_rescans(%rip)
	incq %rax
	jmp .Lstrcmp_top
.Lstrcmp_avx2:
	/* AVX2 (scan level 1): the same steps with 32-byte blocks and VEX encoding throughout
	 * (no legacy-SSE transition); vzeroupper before each return that ran a 256-bit instruction.
	 * Every return is labelled .Lstrcmp_v_ret_<path> (SSE2: .Lstrcmp_ret_<path>): the ymm test
	 * in machine_scanners.rs reads these labels and needs a case for each.
	 *
	 * Head (STRCMP-2, STRCMP-HEAD; D-4.38): 16-byte VEX.128 blocks at the strings' starts and at
	 * 16, with no index register, settle most calls (equal strings and differences in bytes
	 * 0-31) and return without vzeroupper (a 32-byte first block cost ~5 cycles more per short
	 * call on Zen 1: 256-bit uops plus vzeroupper). Then 32-byte blocks at 32, 64 and 96, returning
	 * through the 256-bit returns below. Later blocks settle their stop with the offset in the
	 * displacement. One page test, exact for the first 128 bytes (STRCMP-MIX), over the 128 bytes and
	 * the byte before them (NULL and page-aligned starts fail it and meet the NULL tests,
	 * STRCMP-HEADNULL): a range crosses a page iff bit 12
	 * differs between its first and last byte (the OR of the two offsets misjudged about 1 in 5
	 * random pairs, an unpredictable branch). The zero vector comes from memory; the equal
	 * return gives a's byte (0) as the answer. A miss goes on at index 128 through the general
	 * page test: the blocks keep their positions relative to the strings' starts (32-byte
	 * blocks at 16 + 32k split cache lines for aligned strings, measured slower on keke), and
	 * the 16-byte head that restarted at 0 cost 14 instructions more for every stop past 15.
	 * The test is per string (PERF-STRCMP-LOOP): the 129 bytes from one before the start stay
	 * in one page iff that byte's page offset is at most 3967 (8 instructions, the XOR form 9). */
	leal -1(%rdi), %ecx
	andl $4095, %ecx
	cmpl $3967, %ecx
	ja .Lstrcmp_v_start
	leal -1(%rsi), %edx
	andl $4095, %edx
	cmpl $3967, %edx
	ja .Lstrcmp_v_start
	vmovdqu (%rdi), %xmm1
	vpcmpeqb (%rsi), %xmm1, %xmm2
	vpminub %xmm1, %xmm2, %xmm2
	vpcmpeqb .Lstrcmp_zero32(%rip), %xmm2, %xmm2
	vpmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_v_head16
	bsfl %edx, %edx
	movzbl (%rdi,%rdx), %eax
	cmpb (%rsi,%rdx), %al
	jne .Lstrcmp_v_ret_head_differ
	testl %eax, %eax
	jnz .Lstrcmp_v_head_rescan
	/* The first block's returns ran only VEX.128 instructions, which zero the upper halves of
	 * what they write: no vzeroupper (STRCMP-3, D-4.38; a caller that came in clean leaves clean). */
.Lstrcmp_v_ret_head_equal:
	ret
.Lstrcmp_v_ret_head_differ:
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lstrcmp_v_head_rescan:
	vpxor %xmm0, %xmm0, %xmm0
	movl %edx, %eax
	jmp .Lstrcmp_v_rescan
.Lstrcmp_v_head16:
	vmovdqu 16(%rdi), %xmm1
	vpcmpeqb 16(%rsi), %xmm1, %xmm2
	vpminub %xmm1, %xmm2, %xmm2
	vpcmpeqb .Lstrcmp_zero32(%rip), %xmm2, %xmm2
	vpmovmskb %xmm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_v_head32
	bsfl %edx, %edx
	movzbl 16(%rdi,%rdx), %eax
	cmpb 16(%rsi,%rdx), %al
	jne .Lstrcmp_v_ret_head_differ
	testl %eax, %eax
	jz .Lstrcmp_v_ret_head_equal
	vpxor %xmm0, %xmm0, %xmm0
	leal 16(%rdx), %eax
	jmp .Lstrcmp_v_rescan
.Lstrcmp_v_head32:
	vmovdqu 32(%rdi), %ymm1
	vpcmpeqb 32(%rsi), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb .Lstrcmp_zero32(%rip), %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_v_head64
	bsfl %edx, %edx
	movzbl 32(%rdi,%rdx), %eax
	cmpb 32(%rsi,%rdx), %al
	jne .Lstrcmp_v_differ
	testl %eax, %eax
	jz .Lstrcmp_v_ret_equal
	vpxor %xmm0, %xmm0, %xmm0
	leal 32(%rdx), %eax
	jmp .Lstrcmp_v_rescan
.Lstrcmp_v_head64:
	/* STRCMP-MIX (D-4.38): the head's page test covers 128 bytes, so the blocks at 64 and 96 need
	 * no page test of their own (was: the general steps' offset test at 64, which misjudged about
	 * 1 in 5 random pairs - an unpredictable branch for every string of 64-127 bytes). */
	vmovdqu 64(%rdi), %ymm1
	vpcmpeqb 64(%rsi), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb .Lstrcmp_zero32(%rip), %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_v_head96
	bsfl %edx, %edx
	movzbl 64(%rdi,%rdx), %eax
	cmpb 64(%rsi,%rdx), %al
	jne .Lstrcmp_v_differ
	testl %eax, %eax
	jz .Lstrcmp_v_ret_equal
	vpxor %xmm0, %xmm0, %xmm0
	leal 64(%rdx), %eax
	jmp .Lstrcmp_v_rescan
.Lstrcmp_v_head96:
	vmovdqu 96(%rdi), %ymm1
	vpcmpeqb 96(%rsi), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb .Lstrcmp_zero32(%rip), %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jz .Lstrcmp_v_head_miss
	bsfl %edx, %edx
	movzbl 96(%rdi,%rdx), %eax
	cmpb 96(%rsi,%rdx), %al
	jne .Lstrcmp_v_differ
	testl %eax, %eax
	jz .Lstrcmp_v_ret_equal
	vpxor %xmm0, %xmm0, %xmm0
	leal 96(%rdx), %eax
	jmp .Lstrcmp_v_rescan
.Lstrcmp_v_head_miss:
	/* Bytes 0-127 equal and not NUL: the general steps go on at 128 (their page test first). */
	vpxor %xmm0, %xmm0, %xmm0
	movl $128, %eax
	jmp .Lstrcmp_v_top
.Lstrcmp_v_start:
	/* S6 NULL tests (STRCMP-HEADNULL): the head's page window starts one byte before each
	 * string, so NULL and page-aligned starts always come here first. */
	testq %rdi, %rdi
	je liella_hot_strcmp_null
	testq %rsi, %rsi
	je liella_hot_strcmp_null
	xorl %eax, %eax
	vpxor %xmm0, %xmm0, %xmm0
	movl %edi, %ecx
	orl %esi, %ecx
	andl $4095, %ecx
	subl $4032, %ecx
	ja .Lstrcmp_v_exact
.Lstrcmp_v_blocks:
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_v_found
	addq $32, %rax
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_v_found
	addq $32, %rax
	negl %ecx
	shrl $6, %ecx
	jz .Lstrcmp_v_top
.Lstrcmp_v_group:
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vmovdqu 32(%rdi,%rax), %ymm3
	vpcmpeqb 32(%rsi,%rax), %ymm3, %ymm4
	vpminub %ymm3, %ymm4, %ymm4
	vpminub %ymm4, %ymm2, %ymm5
	vpcmpeqb %ymm0, %ymm5, %ymm5
	vpmovmskb %ymm5, %edx
	testl %edx, %edx
	jnz .Lstrcmp_v_locate
	addq $64, %rax
	decl %ecx
	jnz .Lstrcmp_v_group
	jmp .Lstrcmp_v_top
.Lstrcmp_v_locate:
	/* The group holds the index: the two blocks' min vectors are still in ymm2 and ymm4, so
	 * their masks form one 64-bit mask (no second read, no branch on the half; STRCMP-2). */
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm4, %ymm4
	vpmovmskb %ymm2, %edx
	vpmovmskb %ymm4, %ecx
	shlq $32, %rcx
	orq %rcx, %rdx
	bsfq %rdx, %rdx
	addq %rdx, %rax
	jmp .Lstrcmp_v_settle
.Lstrcmp_v_top:
	/* As .Lstrcmp_top. */
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	orl %edx, %ecx
	andl $4095, %ecx
	subl $4032, %ecx
	jbe .Lstrcmp_v_blocks
.Lstrcmp_v_exact:
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	andl $4095, %ecx
	andl $4095, %edx
	cmpl %ecx, %edx
	cmova %edx, %ecx
	subl $4032, %ecx
	jbe .Lstrcmp_v_blocks
	/* Near a page end (STRCMP-2): one 32-byte block. Up to offset 4064 it starts at i; above,
	 * it steps back by k = offset - 4064 so that it ends exactly at that page's end. Bytes
	 * i-k..i-1 were already found equal and not NUL, so the first stop in the block is still
	 * the first at or after i (a writer's change there is a stop this call read); the other
	 * string's block ends k bytes before its own page offset + 32, inside its page, and its
	 * bytes before i are string bytes. Only when i < k (the strings start there) do single
	 * bytes remain. */
	subl $32, %ecx
	jbe .Lstrcmp_v_one
	cmpq %rcx, %rax
	jb .Lstrcmp_v_byte
	subq %rcx, %rax
.Lstrcmp_v_one:
	vmovdqu (%rdi,%rax), %ymm1
	vpcmpeqb (%rsi,%rax), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpmovmskb %ymm2, %edx
	testl %edx, %edx
	jnz .Lstrcmp_v_found
	addq $32, %rax
	jmp .Lstrcmp_v_top
.Lstrcmp_v_byte:
	movzbl (%rdi,%rax), %ecx
	cmpb (%rsi,%rax), %cl
	jne .Lstrcmp_v_settle
	testl %ecx, %ecx
	jz .Lstrcmp_v_settle
	incq %rax
	jmp .Lstrcmp_v_top
.Lstrcmp_v_found:
	bsfl %edx, %edx
	addq %rdx, %rax
.Lstrcmp_v_settle:
	movzbl (%rdi,%rax), %ecx
	cmpb (%rsi,%rax), %cl
	jne .Lstrcmp_v_differ
	testl %ecx, %ecx
	jnz .Lstrcmp_v_rescan
.Lstrcmp_v_ret_equal:
	vzeroupper
	xorl %eax, %eax
	ret
.Lstrcmp_v_differ:
	/* vzeroupper leaves the flags: CF still = a's byte < b's byte. */
.Lstrcmp_v_ret_differ:
	vzeroupper
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lstrcmp_v_rescan:
	lock incq liella_machine_strcmp_rescans(%rip)
	incq %rax
	jmp .Lstrcmp_v_top
	.size liella_machine_strcmp, .-liella_machine_strcmp
	.text

/* uint64_t liella_machine_strcmp_rescan_count(void): how often liella_machine_strcmp found the
 * bytes at its index changed by a writer and scanned on (the selftest reads it). */
	.globl liella_machine_strcmp_rescan_count
	.type liella_machine_strcmp_rescan_count, @function
	.p2align 4
liella_machine_strcmp_rescan_count:
	movq liella_machine_strcmp_rescans(%rip), %rax
	ret
	.size liella_machine_strcmp_rescan_count, .-liella_machine_strcmp_rescan_count

/* struct liella_strdiff liella_machine_strdiff(const char *a, const char *b) (STRCOLL-2,
 * STRCOLL-4; D-4.37): in rax the first index i at which a[i] != b[i] or a[i] is the terminator
 * (so the strings share the i bytes before it, none of them a terminator); in rdx a value that
 * is nonzero iff one of a[0..=i] has bit 7 set (strcoll's "prefix is ASCII" test, from the
 * blocks already loaded). SSE2 only (every x86-64; strcoll's prefixes are short).
 *
 * High bits: r8d collects the pmovmskb of a's whole shared blocks and bit 7 of each byte
 * compared one at a time; in the block holding i, the lanes 0..=k (k = i's lane) are
 * c ^ (c - 1) of the nonzero lane mask c.
 *
 * Page rule (D-4.14, as liella_machine_strcmp): a 16-byte block at index i is loaded from a + i
 * and b + i only while the larger page offset of the two is at most 4080, so both loads stay in
 * the pages holding a[i] and b[i], which are readable (every byte before i is shared and not a
 * terminator). Above 4080 one byte is compared, then the offsets are taken again. At
 * .Lstrdiff_top, edx = the number of whole blocks before either page ends. The first block is
 * entered with a cheaper sufficient test (STRCOLL-4): (a | b) & 4095 is at least each page
 * offset, so at most 4080 means both are; otherwise .Lstrdiff_top decides. In a block, lane k
 * is zero in min(a, a == b ? 0xFF : 0) iff the bytes differ or a's byte is the terminator.
 *
 * The caller reads the bytes at the index itself (the answer is never taken from this scan). */
	.globl liella_machine_strdiff
	.type liella_machine_strdiff, @function
	.p2align 5
liella_machine_strdiff:
	xorl %eax, %eax
	xorl %r8d, %r8d
	pxor %xmm0, %xmm0
	movl %edi, %ecx
	orl %esi, %ecx
	andl $4095, %ecx
	movl $1, %edx
	cmpl $4080, %ecx
	ja .Lstrdiff_top
.Lstrdiff_block:
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pmovmskb %xmm2, %ecx
	testl %ecx, %ecx
	jnz .Lstrdiff_found
	pmovmskb %xmm1, %ecx
	orl %ecx, %r8d
	addq $16, %rax
	decl %edx
	jnz .Lstrdiff_block
.Lstrdiff_top:
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %edx
	andl $4095, %ecx
	andl $4095, %edx
	cmpl %edx, %ecx
	cmovbl %edx, %ecx
	movl $4080, %edx
	subl %ecx, %edx
	jb .Lstrdiff_byte
	shrl $4, %edx
	incl %edx
	jmp .Lstrdiff_block
.Lstrdiff_byte:
	movzbl (%rdi,%rax), %ecx
	movl %ecx, %edx
	shrl $7, %edx
	orl %edx, %r8d
	cmpb (%rsi,%rax), %cl
	jne .Lstrdiff_done
	testl %ecx, %ecx
	jz .Lstrdiff_done
	incq %rax
	jmp .Lstrdiff_top
.Lstrdiff_found:
	bsfl %ecx, %r9d
	leal -1(%rcx), %edx
	xorl %edx, %ecx
	pmovmskb %xmm1, %edx
	andl %ecx, %edx
	orl %edx, %r8d
	addq %r9, %rax
.Lstrdiff_done:
	movl %r8d, %edx
	ret
	.size liella_machine_strdiff, .-liella_machine_strdiff

/* uint64_t liella_machine_clean_upper_call(int (*f)(const char *, const char *), const char *a,
 * const char *b, int *result) (STRCMP-3 selftest, D-4.38): vzeroupper, f(a, b) with no other code
 * in between, its result stored at *result, and the OR of the upper halves of ymm0..ymm15 returned:
 * 0 = f left no upper state behind (a caller that came in clean leaves clean). AVX2 only.
 * liella_machine_ymm15_dirty_cmp is its positive control: it sets ymm15 to all ones, returns 0. */
	.globl liella_machine_clean_upper_call
	.type liella_machine_clean_upper_call, @function
	.p2align 4
liella_machine_clean_upper_call:
	pushq %rcx
	movq %rdi, %r11
	movq %rsi, %rdi
	movq %rdx, %rsi
	vzeroupper
	call *%r11
	popq %rcx
	movl %eax, (%rcx)
	xorl %eax, %eax
	vextracti128 $1, %ymm0, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm1, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm2, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm3, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm4, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm5, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm6, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm7, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm8, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm9, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm10, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm11, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm12, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm13, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm14, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	vextracti128 $1, %ymm15, -16(%rsp)
	orq -16(%rsp), %rax
	orq -8(%rsp), %rax
	ret
	.size liella_machine_clean_upper_call, .-liella_machine_clean_upper_call

	.globl liella_machine_ymm15_dirty_cmp
	.type liella_machine_ymm15_dirty_cmp, @function
	.p2align 4
liella_machine_ymm15_dirty_cmp:
	vpcmpeqd %ymm15, %ymm15, %ymm15
	xorl %eax, %eax
	ret
	.size liella_machine_ymm15_dirty_cmp, .-liella_machine_ymm15_dirty_cmp

	.bss
	.p2align 3
	.globl liella_machine_strcmp_rescans
	.hidden liella_machine_strcmp_rescans
	.type liella_machine_strcmp_rescans, @object
	.size liella_machine_strcmp_rescans, 8
liella_machine_strcmp_rescans:
	.zero 8
	.text

/* size_t liella_machine_env_find(const char *const *env, const char *name, size_t len)
 * (PJ-8): the address of the first entry of the NULL-terminated array env (0 = no array)
 * that is name's len bytes followed by '=', or 0. One pass over the array: an entry whose
 * first byte differs from name's (from '=' when len is 0) is passed over after that byte.
 *
 * Byte loop, never past a terminator: entry byte i is read only after bytes 0..i matched
 * name and were not NUL, so the entry has not ended before i; a NUL in the entry stops the
 * compare (it can match no byte of a name). Contract: name's len bytes hold no NUL (the
 * callers pass a measured C string or a slice checked for one).
 *
 * Word compare (TINY-HOT): once byte 0 matched, the entry's first 8 bytes are compared as one
 * masked word with "name=" (its first min(len + 1, 8) bytes, precomputed in r10/r11 at entry),
 * when the 8-byte load stays inside the entry's page (an aligned page is mapped whole; bytes
 * past the entry's NUL are masked off or differ from the name, which holds no NUL in its len
 * bytes and ends in '='). Entries that share a prefix with the name (BENCHVARxx vs BENCHNOPE)
 * now cost one load instead of a byte loop. A page-crossing entry takes the byte loop. The
 * name's word is loaded the same way, or built byte by byte when it would cross its page. */
	.section .text.liella_machine_env_find,"ax",@progbits
	.globl liella_machine_env_find
	.type liella_machine_env_find, @function
	.p2align 6
liella_machine_env_find:
	testq %rdi, %rdi
	jz .Lenv_none
	movl $0x3d, %r8d
	testq %rdx, %rdx
	jz .Lenv_top
	movzbl (%rsi), %r8d
	testl %r8d, %r8d
	jz .Lenv_none
	/* r10 = the first min(len + 1, 8) bytes of "name=", r11 = their mask (len >= 1 here) */
	movl %esi, %eax
	andl $4095, %eax
	cmpl $4088, %eax
	ja .Lenv_name_bytes
	movq (%rsi), %r10
.Lenv_name_word:
	movq $-1, %r11
	cmpq $8, %rdx
	jae .Lenv_top
	leal (,%rdx,8), %ecx
	movl $1, %r11d
	shlq %cl, %r11
	decq %r11
	andq %r11, %r10
	movl $0x3d, %eax
	shlq %cl, %rax
	orq %rax, %r10
	shlq $8, %r11
	orq $0xff, %r11
.Lenv_top:
	/* Four slots per pass (ENV-SCAN): each slot is loaded once, tested against NULL before
	 * anything is read through it, and its first byte compared; the next slot is read only
	 * after this one was not the terminator. 5.5 instructions per entry that differs at
	 * byte 0. */
	movq (%rdi), %rcx
	testq %rcx, %rcx
	jz .Lenv_none
	cmpb %r8b, (%rcx)
	je .Lenv_c0
	movq 8(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lenv_none
	cmpb %r8b, (%rcx)
	je .Lenv_c1
	movq 16(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lenv_none
	cmpb %r8b, (%rcx)
	je .Lenv_c2
	movq 24(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lenv_none
	cmpb %r8b, (%rcx)
	je .Lenv_c3
	addq $32, %rdi
	jmp .Lenv_top
.Lenv_c3:
	addq $8, %rdi
.Lenv_c2:
	addq $8, %rdi
.Lenv_c1:
	addq $8, %rdi
.Lenv_c0:
	/* rdi: the slot, rcx: its entry, whose byte 0 matched */
	testq %rdx, %rdx
	jz .Lenv_hit
	movl %ecx, %eax
	andl $4095, %eax
	cmpl $4088, %eax
	ja .Lenv_bytes
	movq (%rcx), %rax
	andq %r11, %rax
	cmpq %r10, %rax
	jne .Lenv_adv
	cmpq $8, %rdx
	jb .Lenv_hit
	movl $8, %r9d
	jmp .Lenv_cmp
.Lenv_bytes:
	movl $1, %r9d
.Lenv_cmp:
	cmpq %rdx, %r9
	jae .Lenv_eq
	movzbl (%rcx,%r9), %eax
	testl %eax, %eax
	jz .Lenv_adv
	cmpb (%rsi,%r9), %al
	jne .Lenv_adv
	incq %r9
	jmp .Lenv_cmp
.Lenv_eq:
	cmpb $0x3d, (%rcx,%rdx)
	jne .Lenv_adv
.Lenv_hit:
	movq %rcx, %rax
	ret
.Lenv_none:
	xorl %eax, %eax
	ret
.Lenv_adv:
	addq $8, %rdi
	jmp .Lenv_top
.Lenv_name_bytes:
	/* the name's first min(len, 8) bytes, one by one (its 8-byte load would cross a page) */
	movq %rdx, %r9
	cmpq $8, %r9
	jbe .Lenv_name_n
	movl $8, %r9d
.Lenv_name_n:
	xorl %r10d, %r10d
.Lenv_name_loop:
	shlq $8, %r10
	movzbl -1(%rsi,%r9), %eax
	orq %rax, %r10
	decq %r9
	jnz .Lenv_name_loop
	jmp .Lenv_name_word
	.size liella_machine_env_find, .-liella_machine_env_find
	.text

/* struct liella_env_slot liella_machine_env_slot(const char *const *env, const char *name, size_t len)
 * (ENVIRON-SMALL-CYCLE): liella_machine_env_find's scan, answering also where it stopped.
 * rax = the entry found (0: none, as env_find); rdx = the address of its slot, or on a miss
 * the address of the array's NULL slot (0 when env is 0). setenv/putenv/unsetenv need the
 * slot index (to store into it) and, on a miss, the entry count (to append): both used to cost
 * a second pass over the pointers. A separate copy (.Lens_*) so env_find (getenv's scanner)
 * keeps its exact instructions; the loop, the word compare and the byte loop are the same,
 * with the same contract (name's len bytes hold no NUL). The miss exits note which of the four
 * slots of the pass held the NULL. */
	.globl liella_machine_env_slot
	.type liella_machine_env_slot, @function
	.p2align 6
liella_machine_env_slot:
	testq %rdi, %rdi
	jz .Lens_n0
	movl $0x3d, %r8d
	testq %rdx, %rdx
	jz .Lens_top
	movzbl (%rsi), %r8d
	testl %r8d, %r8d
	jz .Lens_name_nul
	movl %esi, %eax
	andl $4095, %eax
	cmpl $4088, %eax
	ja .Lens_name_bytes
	movq (%rsi), %r10
.Lens_name_word:
	movq $-1, %r11
	cmpq $8, %rdx
	jae .Lens_top
	leal (,%rdx,8), %ecx
	movl $1, %r11d
	shlq %cl, %r11
	decq %r11
	andq %r11, %r10
	movl $0x3d, %eax
	shlq %cl, %rax
	orq %rax, %r10
	shlq $8, %r11
	orq $0xff, %r11
.Lens_top:
	movq (%rdi), %rcx
	testq %rcx, %rcx
	jz .Lens_n0
	cmpb %r8b, (%rcx)
	je .Lens_c0
	movq 8(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lens_n1
	cmpb %r8b, (%rcx)
	je .Lens_c1
	movq 16(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lens_n2
	cmpb %r8b, (%rcx)
	je .Lens_c2
	movq 24(%rdi), %rcx
	testq %rcx, %rcx
	jz .Lens_n3
	cmpb %r8b, (%rcx)
	je .Lens_c3
	addq $32, %rdi
	jmp .Lens_top
.Lens_c3:
	addq $8, %rdi
.Lens_c2:
	addq $8, %rdi
.Lens_c1:
	addq $8, %rdi
.Lens_c0:
	/* rdi: the slot, rcx: its entry, whose byte 0 matched */
	testq %rdx, %rdx
	jz .Lens_hit
	movl %ecx, %eax
	andl $4095, %eax
	cmpl $4088, %eax
	ja .Lens_bytes
	movq (%rcx), %rax
	andq %r11, %rax
	cmpq %r10, %rax
	jne .Lens_adv
	cmpq $8, %rdx
	jb .Lens_hit
	movl $8, %r9d
	jmp .Lens_cmp
.Lens_bytes:
	movl $1, %r9d
.Lens_cmp:
	cmpq %rdx, %r9
	jae .Lens_eq
	movzbl (%rcx,%r9), %eax
	testl %eax, %eax
	jz .Lens_adv
	cmpb (%rsi,%r9), %al
	jne .Lens_adv
	incq %r9
	jmp .Lens_cmp
.Lens_eq:
	cmpb $0x3d, (%rcx,%rdx)
	jne .Lens_adv
.Lens_hit:
	movq %rcx, %rax
	movq %rdi, %rdx
	ret
.Lens_adv:
	addq $8, %rdi
	jmp .Lens_top
.Lens_name_nul:
	/* a NUL first name byte (outside the contract) matches nothing: walk to the NULL slot */
	cmpq $0, (%rdi)
	je .Lens_n0
	addq $8, %rdi
	jmp .Lens_name_nul
.Lens_n3:
	addq $8, %rdi
.Lens_n2:
	addq $8, %rdi
.Lens_n1:
	addq $8, %rdi
.Lens_n0:
	/* rdi: the NULL slot (0 when env is 0) */
	xorl %eax, %eax
	movq %rdi, %rdx
	ret
.Lens_name_bytes:
	/* the name's first min(len, 8) bytes, one by one (its 8-byte load would cross a page) */
	movq %rdx, %r9
	cmpq $8, %r9
	jbe .Lens_name_n
	movl $8, %r9d
.Lens_name_n:
	xorl %r10d, %r10d
.Lens_name_loop:
	shlq $8, %r10
	movzbl -1(%rsi,%r9), %eax
	orq %rax, %r10
	decq %r9
	jnz .Lens_name_loop
	jmp .Lens_name_word
	.size liella_machine_env_slot, .-liella_machine_env_slot

/* ERMS copy and fill (OD-C2 b): `rep movsb` / `rep stosb`. Rust calls them only for disjoint,
 * validated regions above the measured threshold, and only when CPUID reports ERMS. */

/* void *liella_machine_copy_erms(void *dst, const void *src, size_t n): returns dst */
	.globl liella_machine_copy_erms
	.type liella_machine_copy_erms, @function
	.p2align 6
liella_machine_copy_erms:
	movq %rdi, %rax
	movq %rdx, %rcx
	rep movsb
	ret
	.size liella_machine_copy_erms, .-liella_machine_copy_erms

/* void *liella_machine_copy_erms_a64(void *dst, const void *src, size_t n): n >= 64, disjoint;
 * returns dst. The first 64 bytes go with four unaligned SSE2 moves, then `rep movsb` copies the
 * rest from the next 64-byte-aligned destination (h = -dst & 63 bytes in; h = 0 when aligned,
 * the head store is then rewritten). MEMCPY-POLICY-2: rep movsb from a misaligned destination
 * lost 1.8x on Broadwell, aligned it won (D-4.14). */
	.globl liella_machine_copy_erms_a64
	.type liella_machine_copy_erms_a64, @function
	.p2align 6
liella_machine_copy_erms_a64:
	movq %rdi, %rax
	movdqu (%rsi), %xmm0
	movdqu 16(%rsi), %xmm1
	movdqu 32(%rsi), %xmm2
	movdqu 48(%rsi), %xmm3
	movdqu %xmm0, (%rdi)
	movdqu %xmm1, 16(%rdi)
	movdqu %xmm2, 32(%rdi)
	movdqu %xmm3, 48(%rdi)
	movl %edi, %ecx
	negl %ecx
	andl $63, %ecx
	addq %rcx, %rdi
	addq %rcx, %rsi
	subq %rcx, %rdx
	movq %rdx, %rcx
	rep movsb
	ret
	.size liella_machine_copy_erms_a64, .-liella_machine_copy_erms_a64

/* void liella_machine_fill_erms(void *dst, int c, size_t n) */
	.globl liella_machine_fill_erms
	.type liella_machine_fill_erms, @function
	.p2align 6
liella_machine_fill_erms:
	movl %esi, %eax
	movq %rdx, %rcx
	rep stosb
	ret
	.size liella_machine_fill_erms, .-liella_machine_fill_erms

/* AVX2 copy and move (owner ruling 2026-10-05, D-4.14, PJ-7; overlap MEM-BULK). Rust calls it only
 * when the CPU has AVX2 and the OS saves the YMM state (`boundary::cpu`), for validated regions of
 * at least 64 bytes; they may overlap (memmove).
 *
 * The first and last 32 bytes are loaded before anything is stored and written as unaligned
 * vectors after everything else; between them, whole 32-byte vectors go to 32-byte-aligned
 * destination addresses (four per step, then one at a time; the last one may overlap the tail),
 * each step loading all its vectors before storing any. Overlapping regions fix the direction:
 * from the end when the destination starts inside the source (dst - src < n), from the start when
 * the source starts inside the destination (src - dst < n); every load then reads bytes no
 * earlier store has written, and the held head and tail are the original bytes. Disjoint regions
 * take the direction from d = (dst - src) mod 4 KiB: for 0 < d < 2048 from the end, otherwise
 * from the start. A load whose address matches an earlier, still-buffered store in its low 12
 * bits waits for it (4 KiB aliasing, Intel optimization manual); in the forward order the store to
 * dst + i shares those bits with the load of src + i + d, issued d bytes later, and in the
 * backward order with the load of src + i - (4096 - d). Choosing the order whose distance is at
 * least 2 KiB keeps each such load further back than the store buffer reaches. No non-temporal
 * stores; `vzeroupper` before returning.
 *
 * void liella_machine_copy_avx2(void *dst, const void *src, size_t n), n >= 64; the same code as
 * void *liella_machine_move_avx2(void *dst, const void *src, size_t n), which returns dst */
	.section .text.liella_machine_copy_avx2,"ax",@progbits
	.globl liella_machine_copy_avx2
	.type liella_machine_copy_avx2, @function
	.globl liella_machine_move_avx2
	.type liella_machine_move_avx2, @function
	.p2align 6
liella_machine_copy_avx2:
liella_machine_move_avx2:
	vmovdqu (%rsi), %ymm4			/* the first 32 bytes */
	vmovdqu -32(%rsi,%rdx), %ymm5		/* the last 32 bytes */
	movq %rdi, %r11				/* where the first 32 bytes go */
	leaq -32(%rdi,%rdx), %r8		/* where the last 32 bytes go */
	movq %rdi, %rax
	subq %rsi, %rax				/* dst - src */
	cmpq %rdx, %rax
	jb .Lcpy_back				/* dst inside [src, src + n): from the end */
	movq %rsi, %rcx
	subq %rdi, %rcx
	cmpq %rdx, %rcx
	jb .Lcpy_fwd				/* src inside (dst, dst + n): from the start */
	andl $4095, %eax
	leal -1(%rax), %eax			/* d - 1: below 2047 exactly when 0 < d < 2048 */
	cmpl $2047, %eax
	jb .Lcpy_back

	/* Forward. One induction pointer: rdi walks the destination, the loads are at rdi + (src -
	 * dst), so every store has a base-only address (Haswell's store-address port takes no index). */
.Lcpy_fwd:
	movl %edi, %ecx
	andl $31, %ecx
	subq $32, %rcx
	negq %rcx				/* 32 - (dst mod 32): 1..32, covered by the head */
	addq %rcx, %rdi
	subq %rcx, %rdx				/* bytes left from the aligned dst; the tail covers the last 32 */
	subq %r11, %rsi				/* src - dst */
	leaq (%rdi,%rdx), %r9			/* the end */
	leaq -128(%r9), %r10
	cmpq %r10, %rdi
	jae .Lcpy_f32
	.p2align 4
.Lcpy_f128:					/* while more than 128 bytes are left */
	vmovdqu (%rsi,%rdi), %ymm0
	vmovdqu 32(%rsi,%rdi), %ymm1
	vmovdqu 64(%rsi,%rdi), %ymm2
	vmovdqu 96(%rsi,%rdi), %ymm3
	vmovdqa %ymm0, (%rdi)
	vmovdqa %ymm1, 32(%rdi)
	vmovdqa %ymm2, 64(%rdi)
	vmovdqa %ymm3, 96(%rdi)
	subq $-128, %rdi
	cmpq %r10, %rdi
	jb .Lcpy_f128
.Lcpy_f32:
	leaq -32(%r9), %r10
.Lcpy_f32_next:					/* while more than 32 bytes are left */
	cmpq %r10, %rdi
	jae .Lcpy_ends
	vmovdqu (%rsi,%rdi), %ymm0
	vmovdqa %ymm0, (%rdi)
	addq $32, %rdi
	jmp .Lcpy_f32_next

	/* Backward: r10 walks the destination down from its last 32-byte boundary, the loads at
	 * r10 + (src - dst). */
.Lcpy_back:
	leaq (%rdi,%rdx), %r10
	leal -1(%r10), %ecx
	andl $31, %ecx
	addq $1, %rcx				/* ((end - 1) mod 32) + 1: 1..32, covered by the tail */
	subq %rcx, %r10
	subq %rdi, %rsi				/* src - dst */
	leaq 128(%rdi), %r9
	cmpq %r9, %r10
	jbe .Lcpy_b32
	.p2align 4
.Lcpy_b128:					/* while more than 128 bytes are left below r10 */
	addq $-128, %r10
	vmovdqu 96(%rsi,%r10), %ymm0
	vmovdqu 64(%rsi,%r10), %ymm1
	vmovdqu 32(%rsi,%r10), %ymm2
	vmovdqu (%rsi,%r10), %ymm3
	vmovdqa %ymm0, 96(%r10)
	vmovdqa %ymm1, 64(%r10)
	vmovdqa %ymm2, 32(%r10)
	vmovdqa %ymm3, (%r10)
	cmpq %r9, %r10
	ja .Lcpy_b128
.Lcpy_b32:
	leaq 32(%rdi), %r9
.Lcpy_b32_next:					/* while more than 32 bytes are left; the head covers the first 32 */
	cmpq %r9, %r10
	jbe .Lcpy_ends
	subq $32, %r10
	vmovdqu (%rsi,%r10), %ymm0
	vmovdqa %ymm0, (%r10)
	jmp .Lcpy_b32_next

	/* Both directions: the held first and last 32 bytes, stored last. */
.Lcpy_ends:
	vmovdqu %ymm5, (%r8)
	vmovdqu %ymm4, (%r11)
	movq %r11, %rax				/* dst */
	vzeroupper
	ret
	.size liella_machine_copy_avx2, .-liella_machine_copy_avx2
	.size liella_machine_move_avx2, .-liella_machine_move_avx2
	.text

/* SSE2 move (MEM-FALLBACK, D-4.14 Amendment MEM-FALLBACK): the AVX2 copy above in 16-byte vectors,
 * for CPUs without AVX2 (SSE2 is the x86-64 baseline). The same rules: the first and last 16
 * bytes are loaded before anything is stored and written last; between them, whole 16-byte
 * vectors go to 16-byte-aligned destination addresses (eight per step, then one at a time), each
 * step loading all its vectors before storing any; overlap fixes the direction (dst - src < n:
 * from the end; src - dst < n: from the start); disjoint regions take it from d = (dst - src) mod
 * 4 KiB as above. Rust calls it for validated regions of at least 64 bytes, which may overlap.
 *
 * void *liella_machine_move_sse2(void *dst, const void *src, size_t n), n >= 64; returns dst */
	.globl liella_machine_move_sse2
	.type liella_machine_move_sse2, @function
	.p2align 6
liella_machine_move_sse2:
	movdqu (%rsi), %xmm8			/* the first 16 bytes */
	movdqu -16(%rsi,%rdx), %xmm9		/* the last 16 bytes */
	movq %rdi, %r11				/* where the first 16 bytes go */
	leaq -16(%rdi,%rdx), %r8		/* where the last 16 bytes go */
	movq %rdi, %rax
	subq %rsi, %rax				/* dst - src */
	cmpq %rdx, %rax
	jb .Lmv2_back				/* dst inside [src, src + n): from the end */
	movq %rsi, %rcx
	subq %rdi, %rcx
	cmpq %rdx, %rcx
	jb .Lmv2_fwd				/* src inside (dst, dst + n): from the start */
	andl $4095, %eax
	leal -1(%rax), %eax			/* d - 1: below 2047 exactly when 0 < d < 2048 */
	cmpl $2047, %eax
	jb .Lmv2_back

	/* Forward: rdi walks the destination, the loads at rdi + (src - dst). */
.Lmv2_fwd:
	movl %edi, %ecx
	andl $15, %ecx
	subq $16, %rcx
	negq %rcx				/* 16 - (dst mod 16): 1..16, covered by the head */
	addq %rcx, %rdi
	subq %rcx, %rdx				/* bytes left from the aligned dst; the tail covers the last 16 */
	subq %r11, %rsi				/* src - dst */
	leaq (%rdi,%rdx), %r9			/* the end */
	leaq -128(%r9), %r10
	cmpq %r10, %rdi
	jae .Lmv2_f16
	.p2align 4
.Lmv2_f128:					/* while more than 128 bytes are left */
	movdqu (%rsi,%rdi), %xmm0
	movdqu 16(%rsi,%rdi), %xmm1
	movdqu 32(%rsi,%rdi), %xmm2
	movdqu 48(%rsi,%rdi), %xmm3
	movdqu 64(%rsi,%rdi), %xmm4
	movdqu 80(%rsi,%rdi), %xmm5
	movdqu 96(%rsi,%rdi), %xmm6
	movdqu 112(%rsi,%rdi), %xmm7
	movdqa %xmm0, (%rdi)
	movdqa %xmm1, 16(%rdi)
	movdqa %xmm2, 32(%rdi)
	movdqa %xmm3, 48(%rdi)
	movdqa %xmm4, 64(%rdi)
	movdqa %xmm5, 80(%rdi)
	movdqa %xmm6, 96(%rdi)
	movdqa %xmm7, 112(%rdi)
	subq $-128, %rdi
	cmpq %r10, %rdi
	jb .Lmv2_f128
.Lmv2_f16:
	leaq -16(%r9), %r10
.Lmv2_f16_next:					/* while more than 16 bytes are left */
	cmpq %r10, %rdi
	jae .Lmv2_ends
	movdqu (%rsi,%rdi), %xmm0
	movdqa %xmm0, (%rdi)
	addq $16, %rdi
	jmp .Lmv2_f16_next

	/* Backward: r10 walks the destination down from its last 16-byte boundary, the loads at
	 * r10 + (src - dst). */
.Lmv2_back:
	leaq (%rdi,%rdx), %r10
	leal -1(%r10), %ecx
	andl $15, %ecx
	addq $1, %rcx				/* ((end - 1) mod 16) + 1: 1..16, covered by the tail */
	subq %rcx, %r10
	subq %rdi, %rsi				/* src - dst */
	leaq 128(%rdi), %r9
	cmpq %r9, %r10
	jbe .Lmv2_b16
	.p2align 4
.Lmv2_b128:					/* while more than 128 bytes are left below r10 */
	addq $-128, %r10
	movdqu 112(%rsi,%r10), %xmm0
	movdqu 96(%rsi,%r10), %xmm1
	movdqu 80(%rsi,%r10), %xmm2
	movdqu 64(%rsi,%r10), %xmm3
	movdqu 48(%rsi,%r10), %xmm4
	movdqu 32(%rsi,%r10), %xmm5
	movdqu 16(%rsi,%r10), %xmm6
	movdqu (%rsi,%r10), %xmm7
	movdqa %xmm0, 112(%r10)
	movdqa %xmm1, 96(%r10)
	movdqa %xmm2, 80(%r10)
	movdqa %xmm3, 64(%r10)
	movdqa %xmm4, 48(%r10)
	movdqa %xmm5, 32(%r10)
	movdqa %xmm6, 16(%r10)
	movdqa %xmm7, (%r10)
	cmpq %r9, %r10
	ja .Lmv2_b128
.Lmv2_b16:
	leaq 16(%rdi), %r9
.Lmv2_b16_next:					/* while more than 16 bytes are left; the head covers the first 16 */
	cmpq %r9, %r10
	jbe .Lmv2_ends
	subq $16, %r10
	movdqu (%rsi,%r10), %xmm0
	movdqa %xmm0, (%r10)
	jmp .Lmv2_b16_next

	/* Both directions: the held first and last 16 bytes, stored last. */
.Lmv2_ends:
	movdqu %xmm9, (%r8)
	movdqu %xmm8, (%r11)
	movq %r11, %rax				/* dst */
	ret
	.size liella_machine_move_sse2, .-liella_machine_move_sse2

/* AVX2 fill (MEM-FALLBACK): for AVX2 CPUs where `rep stosb` is not chosen (no ERMS, e.g. Zen 1, or
 * below the threshold). The first 32 bytes as one unaligned vector, then four 32-byte-aligned
 * vectors per step from the first 32-byte boundary above dst while a whole step fits, then the
 * last 128 bytes as four unaligned vectors ending at the end (they may overlap stored bytes: every
 * store writes the same value, so order and overlap do not matter). Rust calls it only when the
 * CPU has AVX2 and the OS saves the YMM state, for a validated region of at least 128 bytes.
 * `vzeroupper` before returning.
 *
 * void *liella_machine_fill_avx2(void *dst, int c, size_t n), n >= 128; returns dst */
	.section .text.liella_machine_fill_avx2,"ax",@progbits
	.globl liella_machine_fill_avx2
	.type liella_machine_fill_avx2, @function
	.p2align 6
liella_machine_fill_avx2:
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	movq %rdi, %rax				/* dst */
	leaq (%rdi,%rdx), %r9			/* the end */
	vmovdqu %ymm0, (%rdi)			/* the first 32 bytes */
	leaq 32(%rdi), %rcx
	andq $-32, %rcx				/* the first 32-byte boundary above dst: dst < rcx <= dst + 32 */
	leaq -128(%r9), %r10			/* the last 128 bytes start here (n >= 128: not below dst) */
	cmpq %r10, %rcx
	ja .Lfill_ends
	.p2align 4
.Lfill_128:					/* while a whole step ends at or before the end */
	vmovdqa %ymm0, (%rcx)
	vmovdqa %ymm0, 32(%rcx)
	vmovdqa %ymm0, 64(%rcx)
	vmovdqa %ymm0, 96(%rcx)
	subq $-128, %rcx
	cmpq %r10, %rcx
	jbe .Lfill_128
.Lfill_ends:					/* fewer than 128 bytes left from rcx: the last 128 */
	vmovdqu %ymm0, (%r10)
	vmovdqu %ymm0, 32(%r10)
	vmovdqu %ymm0, 64(%r10)
	vmovdqu %ymm0, 96(%r10)
	vzeroupper
	ret
	.size liella_machine_fill_avx2, .-liella_machine_fill_avx2
	.text

/* size_t liella_machine_casecmp_avx2(const char *a, const char *b, size_t n) (CASECMP-K1): the
 * first index i < n where a[i] and b[i] differ after folding the ASCII letters A-Z to a-z, or
 * where a[i] is NUL; n if there is none. AVX2 only (the caller runs it when the CPU and OS have
 * AVX2); 32 bytes of each string for the first step (short strings), then 64 per step. Bytes of 0x80 and above are not folded, so two such
 * bytes pass only when equal and the caller decides every other case (locale-dependent fold).
 *
 * Page rule (as liella_machine_strcmp): the strings have unrelated alignments, so the loads are
 * unaligned, and a 32-byte load is taken only when it stays inside the page holding its first
 * byte, which belongs to a string not yet ended and is below n (no terminator before it, no
 * bound reached). Each step checks the larger page offset of a + i and b + i: <= 4032 reads 64
 * bytes, <= 4064 reads 32, otherwise one byte. A load may run past n inside that page; a result past n is cut to n.
 *
 * Fold: t = x + 0x3f is below -102 (signed) exactly for x in 0x41..0x5a, so the lanes to lower
 * are (-102 > t); they get 0x20 or-ed in. Lane k of the final mask is zero iff the folded
 * bytes differ or a's folded byte is NUL (the fold never maps to or from 0). */
	.section .rodata
	.p2align 5
.Lstrcmp_zero16:
.Lstrcmp_zero32:
	.fill 32, 1, 0
	.p2align 5
.Lcasecmp_k3f:
	.fill 32, 1, 0x3f
.Lcasecmp_km102:
	.fill 32, 1, 0x9a
.Lcasecmp_k20:
	.fill 32, 1, 0x20
.Lcasecmp_km103:
	.fill 32, 1, 0x99
	.text
	.globl liella_machine_casecmp_avx2
	.type liella_machine_casecmp_avx2, @function
	.p2align 6
liella_machine_casecmp_avx2:
	xorl %eax, %eax
	vmovdqa .Lcasecmp_k3f(%rip), %ymm5
	vmovdqa .Lcasecmp_km102(%rip), %ymm6
	vmovdqa .Lcasecmp_k20(%rip), %ymm7
	vpxor %xmm0, %xmm0, %xmm0
.Lcasecmp_step32:
	cmpq %rdx, %rax
	jae .Lcasecmp_limit
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %r8d
	andl $4095, %ecx
	andl $4095, %r8d
	cmpl %ecx, %r8d
	cmova %r8d, %ecx
	cmpl $4064, %ecx
	ja .Lcasecmp_byte
	vmovdqu (%rdi,%rax), %ymm1
	vmovdqu (%rsi,%rax), %ymm2
	vpaddb %ymm5, %ymm1, %ymm3
	vpaddb %ymm5, %ymm2, %ymm4
	vpcmpgtb %ymm3, %ymm6, %ymm3
	vpcmpgtb %ymm4, %ymm6, %ymm4
	vpand %ymm7, %ymm3, %ymm3
	vpand %ymm7, %ymm4, %ymm4
	vpor %ymm3, %ymm1, %ymm1
	vpor %ymm4, %ymm2, %ymm2
	vpcmpeqb %ymm1, %ymm2, %ymm3
	vpminub %ymm1, %ymm3, %ymm3
	vpcmpeqb %ymm0, %ymm3, %ymm3
	vpmovmskb %ymm3, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp_found
	addq $32, %rax
.Lcasecmp_top64:
	cmpq %rdx, %rax
	jae .Lcasecmp_limit
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %r8d
	andl $4095, %ecx
	andl $4095, %r8d
	cmpl %ecx, %r8d
	cmova %r8d, %ecx
	cmpl $4032, %ecx
	ja .Lcasecmp_step32
	vmovdqu (%rdi,%rax), %ymm1
	vmovdqu (%rsi,%rax), %ymm2
	vmovdqu 32(%rdi,%rax), %ymm8
	vmovdqu 32(%rsi,%rax), %ymm9
	vpaddb %ymm5, %ymm1, %ymm3
	vpaddb %ymm5, %ymm2, %ymm4
	vpaddb %ymm5, %ymm8, %ymm10
	vpaddb %ymm5, %ymm9, %ymm11
	vpcmpgtb %ymm3, %ymm6, %ymm3
	vpcmpgtb %ymm4, %ymm6, %ymm4
	vpcmpgtb %ymm10, %ymm6, %ymm10
	vpcmpgtb %ymm11, %ymm6, %ymm11
	vpand %ymm7, %ymm3, %ymm3
	vpand %ymm7, %ymm4, %ymm4
	vpand %ymm7, %ymm10, %ymm10
	vpand %ymm7, %ymm11, %ymm11
	vpor %ymm3, %ymm1, %ymm1
	vpor %ymm4, %ymm2, %ymm2
	vpor %ymm10, %ymm8, %ymm8
	vpor %ymm11, %ymm9, %ymm9
	vpcmpeqb %ymm1, %ymm2, %ymm3
	vpcmpeqb %ymm8, %ymm9, %ymm10
	vpminub %ymm1, %ymm3, %ymm3
	vpminub %ymm8, %ymm10, %ymm10
	vpminub %ymm3, %ymm10, %ymm4
	vpcmpeqb %ymm0, %ymm4, %ymm4
	vpmovmskb %ymm4, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp_found64
	addq $64, %rax
	jmp .Lcasecmp_top64
.Lcasecmp_found64:
	vpcmpeqb %ymm0, %ymm3, %ymm3
	vpmovmskb %ymm3, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp_found
	vpcmpeqb %ymm0, %ymm10, %ymm10
	vpmovmskb %ymm10, %ecx
	addq $32, %rax
	jmp .Lcasecmp_found
.Lcasecmp_byte:
	movzbl (%rdi,%rax), %ecx
	movzbl (%rsi,%rax), %r8d
	leal -65(%rcx), %r9d
	cmpl $25, %r9d
	ja 1f
	orl $32, %ecx
1:
	leal -65(%r8), %r9d
	cmpl $25, %r9d
	ja 2f
	orl $32, %r8d
2:
	cmpl %r8d, %ecx
	jne .Lcasecmp_done
	testl %ecx, %ecx
	jz .Lcasecmp_done
	incq %rax
	jmp .Lcasecmp_top64
.Lcasecmp_found:
	bsfl %ecx, %ecx
	addq %rcx, %rax
	cmpq %rdx, %rax
	cmova %rdx, %rax
.Lcasecmp_done:
	vzeroupper
	ret
.Lcasecmp_limit:
	movq %rdx, %rax
	vzeroupper
	ret
	.size liella_machine_casecmp_avx2, .-liella_machine_casecmp_avx2

/* size_t liella_machine_casecmp_sse2(const char *a, const char *b, size_t n) (CASECMP): the result
 * of liella_machine_casecmp_avx2 with SSE2 only (every x86-64 CPU; the caller runs it where AVX2 is
 * absent): 16 bytes of each string for the first step (short strings), then 64 (four 16-byte blocks)
 * per step. Page rule as there, with these load sizes: the larger page offset of a + i and b + i
 * <= 4032 reads 64 bytes, <= 4080 reads 16, otherwise one byte. A load may run past n inside that
 * page; a result past n is cut to n.
 *
 * Fold: t = x + 0x25 is above 0x65 (signed) exactly for x in 0x41..0x5a (0x66..0x7f; every other
 * x lands in 0x80..0xff or 0x00..0x65), so those lanes get 0x20 or-ed in. Lane k of a block's mask
 * is zero iff the folded bytes differ or a's folded byte is NUL (the fold never maps to or from 0).
 * Registers: xmm12 = 0x25, xmm13 = 0x65, xmm14 = 0x20, xmm15 = 0; block results in xmm1..xmm4. */
	.section .rodata
	.p2align 4
.Lcasecmp_k25:
	.fill 16, 1, 0x25
.Lcasecmp_k65:
	.fill 16, 1, 0x65
	.text
	.globl liella_machine_casecmp_sse2
	.type liella_machine_casecmp_sse2, @function
	.p2align 6
liella_machine_casecmp_sse2:
	xorl %eax, %eax
	movdqa .Lcasecmp_k25(%rip), %xmm12
	movdqa .Lcasecmp_k65(%rip), %xmm13
	movdqa .Lcasecmp_k20(%rip), %xmm14
	pxor %xmm15, %xmm15
.Lcasecmp2_step16:
	cmpq %rdx, %rax
	jae .Lcasecmp2_limit
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %r8d
	andl $4095, %ecx
	andl $4095, %r8d
	cmpl %ecx, %r8d
	cmova %r8d, %ecx
	cmpl $4080, %ecx
	ja .Lcasecmp2_byte
	movdqu (%rdi,%rax), %xmm5
	movdqu (%rsi,%rax), %xmm1
	movdqa %xmm5, %xmm6
	movdqa %xmm1, %xmm7
	paddb %xmm12, %xmm6
	paddb %xmm12, %xmm7
	pcmpgtb %xmm13, %xmm6
	pcmpgtb %xmm13, %xmm7
	pand %xmm14, %xmm6
	pand %xmm14, %xmm7
	por %xmm6, %xmm5
	por %xmm7, %xmm1
	pcmpeqb %xmm5, %xmm1
	pminub %xmm5, %xmm1
	pcmpeqb %xmm15, %xmm1
	pmovmskb %xmm1, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp2_found
	addq $16, %rax
.Lcasecmp2_top64:
	cmpq %rdx, %rax
	jae .Lcasecmp2_limit
	leal (%rdi,%rax), %ecx
	leal (%rsi,%rax), %r8d
	andl $4095, %ecx
	andl $4095, %r8d
	cmpl %ecx, %r8d
	cmova %r8d, %ecx
	cmpl $4032, %ecx
	ja .Lcasecmp2_step16
	movdqu (%rdi,%rax), %xmm5
	movdqu (%rsi,%rax), %xmm1
	movdqa %xmm5, %xmm6
	movdqa %xmm1, %xmm7
	paddb %xmm12, %xmm6
	paddb %xmm12, %xmm7
	pcmpgtb %xmm13, %xmm6
	pcmpgtb %xmm13, %xmm7
	pand %xmm14, %xmm6
	pand %xmm14, %xmm7
	por %xmm6, %xmm5
	por %xmm7, %xmm1
	pcmpeqb %xmm5, %xmm1
	pminub %xmm5, %xmm1
	movdqu 16(%rdi,%rax), %xmm8
	movdqu 16(%rsi,%rax), %xmm2
	movdqa %xmm8, %xmm9
	movdqa %xmm2, %xmm10
	paddb %xmm12, %xmm9
	paddb %xmm12, %xmm10
	pcmpgtb %xmm13, %xmm9
	pcmpgtb %xmm13, %xmm10
	pand %xmm14, %xmm9
	pand %xmm14, %xmm10
	por %xmm9, %xmm8
	por %xmm10, %xmm2
	pcmpeqb %xmm8, %xmm2
	pminub %xmm8, %xmm2
	movdqu 32(%rdi,%rax), %xmm5
	movdqu 32(%rsi,%rax), %xmm3
	movdqa %xmm5, %xmm6
	movdqa %xmm3, %xmm7
	paddb %xmm12, %xmm6
	paddb %xmm12, %xmm7
	pcmpgtb %xmm13, %xmm6
	pcmpgtb %xmm13, %xmm7
	pand %xmm14, %xmm6
	pand %xmm14, %xmm7
	por %xmm6, %xmm5
	por %xmm7, %xmm3
	pcmpeqb %xmm5, %xmm3
	pminub %xmm5, %xmm3
	movdqu 48(%rdi,%rax), %xmm8
	movdqu 48(%rsi,%rax), %xmm4
	movdqa %xmm8, %xmm9
	movdqa %xmm4, %xmm10
	paddb %xmm12, %xmm9
	paddb %xmm12, %xmm10
	pcmpgtb %xmm13, %xmm9
	pcmpgtb %xmm13, %xmm10
	pand %xmm14, %xmm9
	pand %xmm14, %xmm10
	por %xmm9, %xmm8
	por %xmm10, %xmm4
	pcmpeqb %xmm8, %xmm4
	pminub %xmm8, %xmm4
	movdqa %xmm1, %xmm9
	pminub %xmm2, %xmm9
	movdqa %xmm3, %xmm10
	pminub %xmm4, %xmm10
	pminub %xmm10, %xmm9
	pcmpeqb %xmm15, %xmm9
	pmovmskb %xmm9, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp2_found64
	addq $64, %rax
	jmp .Lcasecmp2_top64
.Lcasecmp2_found64:
	pcmpeqb %xmm15, %xmm1
	pmovmskb %xmm1, %ecx
	testl %ecx, %ecx
	jnz .Lcasecmp2_found
	pcmpeqb %xmm15, %xmm2
	pmovmskb %xmm2, %ecx
	addq $16, %rax
	testl %ecx, %ecx
	jnz .Lcasecmp2_found
	pcmpeqb %xmm15, %xmm3
	pmovmskb %xmm3, %ecx
	addq $16, %rax
	testl %ecx, %ecx
	jnz .Lcasecmp2_found
	pcmpeqb %xmm15, %xmm4
	pmovmskb %xmm4, %ecx
	addq $16, %rax
	jmp .Lcasecmp2_found
.Lcasecmp2_byte:
	movzbl (%rdi,%rax), %ecx
	movzbl (%rsi,%rax), %r8d
	leal -65(%rcx), %r9d
	cmpl $25, %r9d
	ja 1f
	orl $32, %ecx
1:
	leal -65(%r8), %r9d
	cmpl $25, %r9d
	ja 2f
	orl $32, %r8d
2:
	cmpl %r8d, %ecx
	jne .Lcasecmp2_done
	testl %ecx, %ecx
	jz .Lcasecmp2_done
	incq %rax
	jmp .Lcasecmp2_top64
.Lcasecmp2_found:
	bsfl %ecx, %ecx
	addq %rcx, %rax
	cmpq %rdx, %rax
	cmova %rdx, %rax
.Lcasecmp2_done:
	ret
.Lcasecmp2_limit:
	movq %rdx, %rax
	ret
	.size liella_machine_casecmp_sse2, .-liella_machine_casecmp_sse2

/* int liella_machine_casecmp_ord_avx2(const char *a, const char *b, size_t n, const void *loc) and
 * int liella_machine_casecmp_ord_sse2(const char *a, const char *b, size_t n, const void *loc)
 * (STRCASECMP-SHORT, STRCASECMP-SHORT-2): the answer of strcasecmp/strncasecmp (-1, 0 or 1) for
 * a locale loc whose case map is A-Z -> a-z below 0x80. Same proof obligation as the index
 * kernels (non-NULL, readable up to a NUL or n bytes, n != 0); loc is the caller's &Locale,
 * passed through untouched. The order is the order of the first n bytes after folding A-Z.
 * When the bytes that decide hold a byte of 0x80 or more (the locale decides its fold), or the
 * re-read at the stop finds two equal non-NUL bytes (a writer changed the text), the call
 * tail-jumps to liella_hot_casecmp_rest(a, b, n, loc, code) with rdi, rsi, rdx, rcx unchanged
 * and r8 = 2; r8 = 3 there is an index above n (never happens; the rest aborts).
 *
 * The AVX2 entry is one kernel (STRCASECMP-SHORT-2): the loop of liella_machine_casecmp_avx2
 * (same steps, same page rule, same fold; after a 32-byte step, a bound at most 32 bytes on
 * takes another 32-byte step, not a 64-byte one); at the stop the two deciding bytes are read again with scalar loads and ordered in the same
 * call (no second call level); equal non-NUL bytes there mean a writer changed the text (open).
 * Scratch: rax, r8-r11, ymm0-ymm11. */
	.hidden liella_hot_casecmp_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.text
	.globl liella_machine_casecmp_ord_avx2
	.type liella_machine_casecmp_ord_avx2, @function
	.p2align 6
liella_machine_casecmp_ord_avx2:
	/* Head (PERF-STRCASECMP-AVX2): the first 32 bytes, with the fold's constants as memory
	 * operands (no register set-up) and no bound test before a stop. The or of the two page
	 * offsets bounds both from above: at most 4064 -> both blocks lie inside their pages; else
	 * the exact test. The fold is the loop's: t = x + 0x3f is A-Z iff not (t > -103).
	 * A NULL a or b goes to the rest entry (the measured path answers it), so the caller's
	 * kernel path tests neither. */
	testq %rdi, %rdi
	jz .Lcord_open
	testq %rsi, %rsi
	jz .Lcord_open
	movl %edi, %r8d
	orl %esi, %r8d
	andl $4095, %r8d
	cmpl $4064, %r8d
	ja .Lcord_head_exact
.Lcord_head_load:
	vmovdqu (%rdi), %ymm1
	vmovdqu (%rsi), %ymm2
	vpaddb .Lcasecmp_k3f(%rip), %ymm1, %ymm3
	vpaddb .Lcasecmp_k3f(%rip), %ymm2, %ymm4
	vpcmpgtb .Lcasecmp_km103(%rip), %ymm3, %ymm3
	vpcmpgtb .Lcasecmp_km103(%rip), %ymm4, %ymm4
	vpandn .Lcasecmp_k20(%rip), %ymm3, %ymm3
	vpandn .Lcasecmp_k20(%rip), %ymm4, %ymm4
	vpor %ymm3, %ymm1, %ymm1
	vpor %ymm4, %ymm2, %ymm2
	vpcmpeqb %ymm1, %ymm2, %ymm3
	vpminub %ymm1, %ymm3, %ymm4
	vpcmpeqb .Lstrcmp_zero32(%rip), %ymm4, %ymm4
	vpmovmskb %ymm4, %r8d
	testl %r8d, %r8d
	jz .Lcord_head_next
	/* A stop in the head. Where the folded bytes are equal there, a's is NUL and so is b's (the
	 * fold never maps to or from 0): both end, the answer is 0 from this one read of the text.
	 * Otherwise the stop path (bound test, re-read, order) with rax = the index. */
	vpmovmskb %ymm3, %r9d
	vzeroupper
	bsfl %r8d, %r8d
	btl %r8d, %r9d
	jnc .Lcord_head_differ
	xorl %eax, %eax
	ret
.Lcord_head_differ:
	movl %r8d, %eax
	jmp .Lcord_found_idx
.Lcord_head_exact:
	movl %edi, %r8d
	movl %esi, %r9d
	andl $4095, %r8d
	andl $4095, %r9d
	cmpl $4064, %r8d
	ja .Lcord_generic
	cmpl $4064, %r9d
	jbe .Lcord_head_load
	jmp .Lcord_generic
.Lcord_head_next:
	/* No stop in the first 32 bytes: the loop from offset 32, as after its own 32-byte step. */
	xorl %eax, %eax
	vmovdqa .Lcasecmp_k3f(%rip), %ymm5
	vmovdqa .Lcasecmp_km102(%rip), %ymm6
	vmovdqa .Lcasecmp_k20(%rip), %ymm7
	vpxor %xmm0, %xmm0, %xmm0
	jmp .Lcord_after32
.Lcord_generic:
	/* A head block would cross a page: the loop from offset 0 (byte steps near the end). */
	xorl %eax, %eax
	vmovdqa .Lcasecmp_k3f(%rip), %ymm5
	vmovdqa .Lcasecmp_km102(%rip), %ymm6
	vmovdqa .Lcasecmp_k20(%rip), %ymm7
	vpxor %xmm0, %xmm0, %xmm0
.Lcord_step32:
	cmpq %rdx, %rax
	jae .Lcord_zero
	leal (%rdi,%rax), %r8d
	leal (%rsi,%rax), %r9d
	andl $4095, %r8d
	andl $4095, %r9d
	cmpl %r8d, %r9d
	cmova %r9d, %r8d
	cmpl $4064, %r8d
	ja .Lcord_byte
	vmovdqu (%rdi,%rax), %ymm1
	vmovdqu (%rsi,%rax), %ymm2
	vpaddb %ymm5, %ymm1, %ymm3
	vpaddb %ymm5, %ymm2, %ymm4
	vpcmpgtb %ymm3, %ymm6, %ymm3
	vpcmpgtb %ymm4, %ymm6, %ymm4
	vpand %ymm7, %ymm3, %ymm3
	vpand %ymm7, %ymm4, %ymm4
	vpor %ymm3, %ymm1, %ymm1
	vpor %ymm4, %ymm2, %ymm2
	vpcmpeqb %ymm1, %ymm2, %ymm3
	vpminub %ymm1, %ymm3, %ymm3
	vpcmpeqb %ymm0, %ymm3, %ymm3
	vpmovmskb %ymm3, %r8d
	testl %r8d, %r8d
	jnz .Lcord_found
.Lcord_after32:
	addq $32, %rax
	leaq 32(%rax), %r9
	cmpq %rdx, %r9
	jae .Lcord_step32
.Lcord_top64:
	cmpq %rdx, %rax
	jae .Lcord_zero
	leal (%rdi,%rax), %r8d
	leal (%rsi,%rax), %r9d
	andl $4095, %r8d
	andl $4095, %r9d
	cmpl %r8d, %r9d
	cmova %r9d, %r8d
	cmpl $4032, %r8d
	ja .Lcord_step32
	vmovdqu (%rdi,%rax), %ymm1
	vmovdqu (%rsi,%rax), %ymm2
	vmovdqu 32(%rdi,%rax), %ymm8
	vmovdqu 32(%rsi,%rax), %ymm9
	vpaddb %ymm5, %ymm1, %ymm3
	vpaddb %ymm5, %ymm2, %ymm4
	vpaddb %ymm5, %ymm8, %ymm10
	vpaddb %ymm5, %ymm9, %ymm11
	vpcmpgtb %ymm3, %ymm6, %ymm3
	vpcmpgtb %ymm4, %ymm6, %ymm4
	vpcmpgtb %ymm10, %ymm6, %ymm10
	vpcmpgtb %ymm11, %ymm6, %ymm11
	vpand %ymm7, %ymm3, %ymm3
	vpand %ymm7, %ymm4, %ymm4
	vpand %ymm7, %ymm10, %ymm10
	vpand %ymm7, %ymm11, %ymm11
	vpor %ymm3, %ymm1, %ymm1
	vpor %ymm4, %ymm2, %ymm2
	vpor %ymm10, %ymm8, %ymm8
	vpor %ymm11, %ymm9, %ymm9
	vpcmpeqb %ymm1, %ymm2, %ymm3
	vpcmpeqb %ymm8, %ymm9, %ymm10
	vpminub %ymm1, %ymm3, %ymm3
	vpminub %ymm8, %ymm10, %ymm10
	vpminub %ymm3, %ymm10, %ymm4
	vpcmpeqb %ymm0, %ymm4, %ymm4
	vpmovmskb %ymm4, %r8d
	testl %r8d, %r8d
	jnz .Lcord_found64
	addq $64, %rax
	jmp .Lcord_top64
.Lcord_found64:
	vpcmpeqb %ymm0, %ymm3, %ymm3
	vpmovmskb %ymm3, %r8d
	testl %r8d, %r8d
	jnz .Lcord_found
	vpcmpeqb %ymm0, %ymm10, %ymm10
	vpmovmskb %ymm10, %r8d
	vmovdqa %ymm8, %ymm1
	vmovdqa %ymm9, %ymm2
	addq $32, %rax
.Lcord_found:
	/* r8d: stop mask (non-zero) of the block at rax. The two deciding bytes are read again (scalar,
	 * off the vector pipes) and ordered here: they lie in the block just loaded, inside its page. */
	vzeroupper
	bsfl %r8d, %r8d
	addq %r8, %rax
.Lcord_found_idx:
	cmpq %rdx, %rax
	jae .Lcord_zero_clean
	movzbl (%rdi,%rax), %r8d
	movzbl (%rsi,%rax), %r9d
	movl %r8d, %r10d
	orl %r9d, %r10d
	testb $0x80, %r10b
	jnz .Lcord_open
	leal -65(%r8), %r10d
	leal 32(%r8), %r11d
	cmpl $25, %r10d
	cmovbe %r11d, %r8d
	leal -65(%r9), %r10d
	leal 32(%r9), %r11d
	cmpl $25, %r10d
	cmovbe %r11d, %r9d
	cmpl %r9d, %r8d
	je .Lcord_same
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcord_same:
	/* Equal after folding: both NUL (the answer is 0), or a writer changed the text since the
	 * vector step (open: the rest entry measures it again). */
	xorl %eax, %eax
	testl %r8d, %r8d
	jnz .Lcord_open
	ret
.Lcord_byte:
	movzbl (%rdi,%rax), %r8d
	movzbl (%rsi,%rax), %r9d
	leal -65(%r8), %r10d
	leal 32(%r8), %r11d
	cmpl $25, %r10d
	cmovbe %r11d, %r8d
	leal -65(%r9), %r10d
	leal 32(%r9), %r11d
	cmpl $25, %r10d
	cmovbe %r11d, %r9d
	cmpl %r9d, %r8d
	jne .Lcord_byte_differ
	testl %r8d, %r8d
	jz .Lcord_zero
	incq %rax
	jmp .Lcord_top64
.Lcord_byte_differ:
	vzeroupper
	movl %r8d, %r10d
	orl %r9d, %r10d
	testb $0x80, %r10b
	jnz .Lcord_open
	cmpl %r9d, %r8d
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcord_zero:
	vzeroupper
.Lcord_zero_clean:
	xorl %eax, %eax
	ret
.Lcord_open:
	movl $2, %r8d
	jmp liella_hot_casecmp_rest
	.size liella_machine_casecmp_ord_avx2, .-liella_machine_casecmp_ord_avx2

/* The SSE2 entry is one kernel too (PERF-STRCASECMP-SSE2; it was a wrapper calling
 * liella_machine_casecmp_sse2, then re-reading): the steps of the AVX2 entry with each 32 bytes
 * as two 16-byte blocks (the fold of liella_machine_casecmp_sse2: xmm12 = 0x25, xmm13 = 0x65,
 * xmm14 = 0x20, xmm15 = 0); the first step is 32 bytes, not 16 then 64, so a string under 32
 * bytes takes two blocks. Same page rule (the larger page offset <= 4064 reads 32 bytes, <= 4032
 * reads 64, otherwise one byte), same stop: the two deciding bytes read again and ordered here.
 * Scratch: rax, r8-r10, xmm1-xmm15; rcx (loc) is kept for the rest entry. */
	.globl liella_machine_casecmp_ord_sse2
	.type liella_machine_casecmp_ord_sse2, @function
	.p2align 6
liella_machine_casecmp_ord_sse2:
	xorl %eax, %eax
	movdqa .Lcasecmp_k25(%rip), %xmm12
	movdqa .Lcasecmp_k65(%rip), %xmm13
	movdqa .Lcasecmp_k20(%rip), %xmm14
	pxor %xmm15, %xmm15
.Lcord2_step32:
	cmpq %rdx, %rax
	jae .Lcord2_zero
	leal (%rdi,%rax), %r8d
	leal (%rsi,%rax), %r9d
	andl $4095, %r8d
	andl $4095, %r9d
	cmpl %r8d, %r9d
	cmova %r9d, %r8d
	cmpl $4064, %r8d
	ja .Lcord2_byte
	/* The loop's 9-instruction fold (see .Lcord2_loop64) on two blocks: result in xmm5/xmm6, zero iff
	 * the A-Z-folded bytes are equal; a stop is a lane where the result is non-zero or a is NUL. */
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm5
	movdqa %xmm1, %xmm9
	pxor %xmm5, %xmm9
	pand %xmm1, %xmm5
	paddb %xmm12, %xmm5
	pcmpgtb %xmm13, %xmm5
	pand %xmm14, %xmm5
	pandn %xmm9, %xmm5
	movdqu 16(%rdi,%rax), %xmm2
	movdqu 16(%rsi,%rax), %xmm6
	movdqa %xmm2, %xmm9
	pxor %xmm6, %xmm9
	pand %xmm2, %xmm6
	paddb %xmm12, %xmm6
	pcmpgtb %xmm13, %xmm6
	pand %xmm14, %xmm6
	pandn %xmm9, %xmm6
	pcmpeqb %xmm15, %xmm5
	pcmpeqb %xmm15, %xmm6
	pminub %xmm1, %xmm5
	pminub %xmm2, %xmm6
	pcmpeqb %xmm15, %xmm5
	pcmpeqb %xmm15, %xmm6
	pmovmskb %xmm5, %r8d
	pmovmskb %xmm6, %r9d
	shll $16, %r9d
	orl %r9d, %r8d
	jnz .Lcord2_found
	addq $32, %rax
	leaq 32(%rax), %r9
	cmpq %rdx, %r9
	jae .Lcord2_step32
.Lcord2_top64:
	cmpq %rdx, %rax
	jae .Lcord2_zero
	leal (%rdi,%rax), %r8d
	leal (%rsi,%rax), %r9d
	andl $4095, %r8d
	andl $4095, %r9d
	cmpl %r8d, %r9d
	cmova %r9d, %r8d
	cmpl $4032, %r8d
	ja .Lcord2_step32
	/* r10 = the 64-byte blocks to run before the next page-rule check: the whole blocks left in
	 * the nearer page, at most one more than the blocks that cover n (a block past n is read,
	 * and a stop in it is cut to n at .Lcord2_found, as before). */
	movl $4096, %r10d
	subl %r8d, %r10d
	shrl $6, %r10d
	movq %rdx, %r9
	subq %rax, %r9
	shrq $6, %r9
	incq %r9
	cmpq %r10, %r9
	cmovb %r9, %r10
	/* Fold of a 64-byte block pair, 9 instructions a block, a in xmm1..4, result in xmm5..8:
	 * t = a ^ b; c = a & b; M = 0x20 in the lanes where c is A-Z (c + 0x25 above 0x65, signed, as the fold);
	 * result = t & ~M, zero iff the A-Z-folded bytes are equal (t is 0, or 0x20 with c a letter).
	 * A lane of a that is NUL stops the loop through the minimum of the four a blocks. */
.Lcord2_loop64:
	movdqu (%rdi,%rax), %xmm1
	movdqu (%rsi,%rax), %xmm5
	movdqa %xmm1, %xmm9
	pxor %xmm5, %xmm9
	pand %xmm1, %xmm5
	paddb %xmm12, %xmm5
	pcmpgtb %xmm13, %xmm5
	pand %xmm14, %xmm5
	pandn %xmm9, %xmm5
	movdqu 16(%rdi,%rax), %xmm2
	movdqu 16(%rsi,%rax), %xmm6
	movdqa %xmm2, %xmm9
	pxor %xmm6, %xmm9
	pand %xmm2, %xmm6
	paddb %xmm12, %xmm6
	pcmpgtb %xmm13, %xmm6
	pand %xmm14, %xmm6
	pandn %xmm9, %xmm6
	movdqu 32(%rdi,%rax), %xmm3
	movdqu 32(%rsi,%rax), %xmm7
	movdqa %xmm3, %xmm9
	pxor %xmm7, %xmm9
	pand %xmm3, %xmm7
	paddb %xmm12, %xmm7
	pcmpgtb %xmm13, %xmm7
	pand %xmm14, %xmm7
	pandn %xmm9, %xmm7
	movdqu 48(%rdi,%rax), %xmm4
	movdqu 48(%rsi,%rax), %xmm8
	movdqa %xmm4, %xmm9
	pxor %xmm8, %xmm9
	pand %xmm4, %xmm8
	paddb %xmm12, %xmm8
	pcmpgtb %xmm13, %xmm8
	pand %xmm14, %xmm8
	pandn %xmm9, %xmm8
	movdqa %xmm5, %xmm9
	movdqa %xmm1, %xmm10
	por %xmm6, %xmm9
	pminub %xmm2, %xmm10
	por %xmm7, %xmm9
	pminub %xmm3, %xmm10
	por %xmm8, %xmm9
	pminub %xmm4, %xmm10
	pcmpeqb %xmm15, %xmm9
	pminub %xmm10, %xmm9
	pcmpeqb %xmm15, %xmm9
	pmovmskb %xmm9, %r8d
	testl %r8d, %r8d
	jnz .Lcord2_found64
	addq $64, %rax
	decl %r10d
	jnz .Lcord2_loop64
	jmp .Lcord2_top64
.Lcord2_found64:
	pcmpeqb %xmm15, %xmm5
	pcmpeqb %xmm15, %xmm6
	pminub %xmm1, %xmm5
	pminub %xmm2, %xmm6
	pcmpeqb %xmm15, %xmm5
	pcmpeqb %xmm15, %xmm6
	pmovmskb %xmm5, %r8d
	pmovmskb %xmm6, %r9d
	shll $16, %r9d
	orl %r9d, %r8d
	jnz .Lcord2_found
	pcmpeqb %xmm15, %xmm7
	pcmpeqb %xmm15, %xmm8
	pminub %xmm3, %xmm7
	pminub %xmm4, %xmm8
	pcmpeqb %xmm15, %xmm7
	pcmpeqb %xmm15, %xmm8
	pmovmskb %xmm7, %r8d
	pmovmskb %xmm8, %r9d
	shll $16, %r9d
	orl %r9d, %r8d
	addq $32, %rax
.Lcord2_found:
	/* r8d: stop mask (non-zero) of the 32 bytes at rax; the deciding bytes lie in them, inside
	 * their page. A stop at or past n means the first n bytes are equal. */
	bsfl %r8d, %r8d
	addq %r8, %rax
	cmpq %rdx, %rax
	jae .Lcord2_zero
	movzbl (%rdi,%rax), %r8d
	movzbl (%rsi,%rax), %r9d
	movl %r8d, %r10d
	orl %r9d, %r10d
	testb $0x80, %r10b
	jnz .Lcord2_open
	leal -65(%r8), %r10d
	cmpl $25, %r10d
	leal 32(%r8), %r10d
	cmovbe %r10d, %r8d
	leal -65(%r9), %r10d
	cmpl $25, %r10d
	leal 32(%r9), %r10d
	cmovbe %r10d, %r9d
	cmpl %r9d, %r8d
	je .Lcord2_same
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcord2_same:
	/* Equal after folding: both NUL (the answer is 0), or a writer changed the text since the
	 * vector step (open: the rest entry measures it again). */
	xorl %eax, %eax
	testl %r8d, %r8d
	jnz .Lcord2_open
	ret
.Lcord2_byte:
	movzbl (%rdi,%rax), %r8d
	movzbl (%rsi,%rax), %r9d
	leal -65(%r8), %r10d
	cmpl $25, %r10d
	leal 32(%r8), %r10d
	cmovbe %r10d, %r8d
	leal -65(%r9), %r10d
	cmpl $25, %r10d
	leal 32(%r9), %r10d
	cmovbe %r10d, %r9d
	cmpl %r9d, %r8d
	jne .Lcord2_byte_differ
	testl %r8d, %r8d
	jz .Lcord2_zero
	incq %rax
	jmp .Lcord2_top64
.Lcord2_byte_differ:
	movl %r8d, %r10d
	orl %r9d, %r10d
	testb $0x80, %r10b
	jnz .Lcord2_open
	cmpl %r9d, %r8d
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcord2_zero:
	xorl %eax, %eax
	ret
.Lcord2_open:
	movl $2, %r8d
	jmp liella_hot_casecmp_rest
	.size liella_machine_casecmp_ord_sse2, .-liella_machine_casecmp_ord_sse2

/* size_t liella_machine_pairscan(const void *h, size_t count, size_t off, int b0, int b1): the
 * first index i < count with h[i] == (unsigned char)b0 and h[i + off] == (unsigned char)b1, or
 * count (FIND-SEQ-AVX2: the candidate scan of a substring search, first and last byte of the
 * needle, off = needle length - 1). Contract (the Rust caller `conv::find_slice` enforces it):
 * count >= 16 and [h, h + count + off) is readable. Every load lies inside that range: whole
 * blocks (32 bytes on AVX2 when count >= 32, else 16) at h + k and h + off + k with k + block <=
 * count, and a last block that ends exactly at count, overlapping the previous one (positions
 * the previous blocks cleared have no candidate, so the overlap cannot change the first hit).
 * Rust verifies the whole needle at the answer, so a too-small answer is harmless and a
 * too-large one is caught by the tests. */
	.globl liella_machine_pairscan
	.type liella_machine_pairscan, @function
	.p2align 4
liella_machine_pairscan:
	leaq (%rdi,%rdx), %r10
	xorl %eax, %eax
	cmpq $32, %rsi
	jb .Lpair_sse2
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lpair_sse2
	vmovd %ecx, %xmm0
	vmovd %r8d, %xmm1
	vpbroadcastb %xmm0, %ymm0
	vpbroadcastb %xmm1, %ymm1
	leaq -32(%rsi), %r9
	.p2align 4
.Lpair_avx_loop:
	vpcmpeqb (%rdi,%rax), %ymm0, %ymm2
	vpcmpeqb (%r10,%rax), %ymm1, %ymm3
	vpand %ymm2, %ymm3, %ymm2
	vpmovmskb %ymm2, %r11d
	testl %r11d, %r11d
	jnz .Lpair_avx_found
	addq $32, %rax
	cmpq %r9, %rax
	jbe .Lpair_avx_loop
	cmpq %rsi, %rax
	jae .Lpair_avx_none
	movq %r9, %rax
	jmp .Lpair_avx_loop
.Lpair_avx_found:
	bsfl %r11d, %r11d
	addq %r11, %rax
	vzeroupper
	ret
.Lpair_avx_none:
	vzeroupper
	movq %rsi, %rax
	ret
.Lpair_sse2:
	movd %ecx, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	movd %r8d, %xmm1
	punpcklbw %xmm1, %xmm1
	punpcklwd %xmm1, %xmm1
	pshufd $0, %xmm1, %xmm1
	leaq -16(%rsi), %r9
	.p2align 4
.Lpair_sse_loop:
	movdqu (%rdi,%rax), %xmm2
	movdqu (%r10,%rax), %xmm3
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm1, %xmm3
	pand %xmm3, %xmm2
	pmovmskb %xmm2, %r11d
	testl %r11d, %r11d
	jnz .Lpair_sse_found
	addq $16, %rax
	cmpq %r9, %rax
	jbe .Lpair_sse_loop
	cmpq %rsi, %rax
	jae .Lpair_sse_none
	movq %r9, %rax
	jmp .Lpair_sse_loop
.Lpair_sse_found:
	bsfl %r11d, %r11d
	addq %r11, %rax
	ret
.Lpair_sse_none:
	movq %rsi, %rax
	ret
	.size liella_machine_pairscan, .-liella_machine_pairscan

/* size_t liella_machine_pairscan_ci(const void *h, size_t count, size_t off, int b0, int b1,
 * unsigned fl): `liella_machine_pairscan` with an ASCII case fold (STRSTR-FIND: the candidate
 * scan of strcasestr). Bit 0 of fl set: the haystack byte at i is OR-ed with 0x20 before the
 * compare with b0; bit 1: the same for the byte at i + off and b1. The caller passes b0/b1
 * already lowered and sets the bit only for an ASCII letter (so the OR-ed byte equals b exactly
 * for the upper and lower case of that letter) and only when the locale's byte fold is plain
 * ASCII. Contract and load pattern are those of `liella_machine_pairscan` (count >= 16,
 * [h, h + count + off) readable); Rust verifies the whole needle at the answer. */
	.globl liella_machine_pairscan_ci
	.type liella_machine_pairscan_ci, @function
	.p2align 4
liella_machine_pairscan_ci:
	leaq (%rdi,%rdx), %r10
	xorl %eax, %eax
	movl %r9d, %edx
	andl $1, %edx
	shll $5, %edx
	shrl $1, %r9d
	andl $1, %r9d
	shll $5, %r9d
	cmpq $32, %rsi
	jb .Lpairci_sse2
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lpairci_sse2
	vmovd %ecx, %xmm0
	vmovd %r8d, %xmm1
	vmovd %edx, %xmm4
	vmovd %r9d, %xmm5
	vpbroadcastb %xmm0, %ymm0
	vpbroadcastb %xmm1, %ymm1
	vpbroadcastb %xmm4, %ymm4
	vpbroadcastb %xmm5, %ymm5
	leaq -32(%rsi), %r9
	.p2align 4
.Lpairci_avx_loop:
	vpor (%rdi,%rax), %ymm4, %ymm2
	vpor (%r10,%rax), %ymm5, %ymm3
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpcmpeqb %ymm1, %ymm3, %ymm3
	vpand %ymm2, %ymm3, %ymm2
	vpmovmskb %ymm2, %r11d
	testl %r11d, %r11d
	jnz .Lpairci_avx_found
	addq $32, %rax
	cmpq %r9, %rax
	jbe .Lpairci_avx_loop
	cmpq %rsi, %rax
	jae .Lpairci_avx_none
	movq %r9, %rax
	jmp .Lpairci_avx_loop
.Lpairci_avx_found:
	bsfl %r11d, %r11d
	addq %r11, %rax
	vzeroupper
	ret
.Lpairci_avx_none:
	vzeroupper
	movq %rsi, %rax
	ret
.Lpairci_sse2:
	movd %ecx, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	movd %r8d, %xmm1
	punpcklbw %xmm1, %xmm1
	punpcklwd %xmm1, %xmm1
	pshufd $0, %xmm1, %xmm1
	movd %edx, %xmm4
	punpcklbw %xmm4, %xmm4
	punpcklwd %xmm4, %xmm4
	pshufd $0, %xmm4, %xmm4
	movd %r9d, %xmm5
	punpcklbw %xmm5, %xmm5
	punpcklwd %xmm5, %xmm5
	pshufd $0, %xmm5, %xmm5
	leaq -16(%rsi), %r9
	.p2align 4
.Lpairci_sse_loop:
	movdqu (%rdi,%rax), %xmm2
	movdqu (%r10,%rax), %xmm3
	por %xmm4, %xmm2
	por %xmm5, %xmm3
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm1, %xmm3
	pand %xmm3, %xmm2
	pmovmskb %xmm2, %r11d
	testl %r11d, %r11d
	jnz .Lpairci_sse_found
	addq $16, %rax
	cmpq %r9, %rax
	jbe .Lpairci_sse_loop
	cmpq %rsi, %rax
	jae .Lpairci_sse_none
	movq %r9, %rax
	jmp .Lpairci_sse_loop
.Lpairci_sse_found:
	bsfl %r11d, %r11d
	addq %r11, %rax
	ret
.Lpairci_sse_none:
	movq %rsi, %rax
	ret
	.size liella_machine_pairscan_ci, .-liella_machine_pairscan_ci

/* const char *liella_machine_strchr(const char *s, int c) and
 * const char *liella_machine_strchrnul(const char *s, int c) (SHORT-1, D-4.35): one pass for the
 * first byte equal to (unsigned char)c or NUL (before SHORT-1 the exports measured the string,
 * then searched it: two passes). strchrnul returns that byte's address; strchr returns it if the
 * byte is c and NULL if it is the terminator (c == 0 finds the terminator). The caller (Rust,
 * strz::chr / chr_or_end) rejects NULL: here s != NULL. One test per 16 bytes: x ^ splat(c) is
 * zero where x == c, so min(x ^ splat(c), x) is zero exactly where x == c or x == 0.
 * Head: the aligned 16-byte block holding s (ALIGNED BLOCKS), its mask shifted right by s & 15;
 * then aligned 16-byte blocks up to a 64-byte boundary, then aligned 64-byte groups folded with
 * pminub (one branch per 64 bytes); a group with a hit is searched again block by block.
 * SAFETY (D-4.14 page rule): every load is an aligned 16-byte block (or a 64-byte aligned
 * group) whose first byte is s's block or follows bytes that were neither c nor NUL, so the
 * string reaches it; an aligned block or group never spans two pages. SSE2 only (the baseline):
 * no AVX state, no scan-level test. strchr settles from the byte at the answer: c returns it,
 * NUL gives NULL, anything else means a writer changed it after the scan, and the scan resumes
 * after it (so every answer comes from bytes the call read, D-4.21).
 * DIRECT ENTRIES (STR-CHR-CMP-SHORT): `strchrnul`, `strchr` and `index` in the products are these
 * kernels, with no Rust wrapper. The head's page test takes (s - 1) & 4095 below
 * liella_machine_rshort_mask, which a NULL s never passes; the slow path tests s for NULL and
 * tail-jumps to `liella_hot_strchr_null` (the S6 abort) before any read. */
	.hidden liella_hot_strchr_null  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_strchrnul
	.type liella_machine_strchrnul, @function
	.p2align 6
liella_machine_strchrnul:
#ifdef LIELLA_EXPORT
	.globl strchrnul
	.type strchrnul, @function
strchrnul:
#endif
	/* SMALL-STR head (D-4.45): on the AVX2 level with [s, s + 64) inside s's page, two unaligned
	 * 32-byte blocks, each loaded ONCE and compared with c and with zero from that register (the
	 * answer comes from the bytes this call read, D-4.21). SAFETY: both loads lie in s's page. A
	 * miss in both continues in the SSE2 scan from s + 64 (every byte before it was neither c nor
	 * NUL). */
	leal -1(%rdi), %ecx
	andl $4095, %ecx
	cmpl liella_machine_rshort_mask(%rip), %ecx
	jae .Lschrnul_slow
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	vpxor %xmm3, %xmm3, %xmm3
	vmovdqu (%rdi), %ymm1
	vpcmpeqb %ymm1, %ymm0, %ymm2
	vpcmpeqb %ymm1, %ymm3, %ymm1
	vpor %ymm1, %ymm2, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lschrnul_h32
	leaq (%rdi,%rdx), %rax
	vzeroupper
	ret
.Lschrnul_h32:
	vmovdqu 32(%rdi), %ymm1
	vpcmpeqb %ymm1, %ymm0, %ymm2
	vpcmpeqb %ymm1, %ymm3, %ymm1
	vpor %ymm1, %ymm2, %ymm1
	vpmovmskb %ymm1, %edx
	bsfl %edx, %edx
	jz .Lschrnul_h64
	leaq 32(%rdi,%rdx), %rax
	vzeroupper
	ret
.Lschrnul_h64:
	vzeroupper
	addq $64, %rdi
	xorl %r11d, %r11d
	jmp .Lschr_body
.Lschrnul_pre:
	/* PERF-STRCHR-NONAVX2: ecx = (s - 1) & 4095 >= rshort_mask here (4032 on the AVX2 level), so
	 * ecx < 4032 means the SSE2 level AND [s, s + 64) inside s's page (page offset 1..4032; a NULL
	 * s gives 4095): one compare is both tests. Then [s, s + 64) as four unaligned 16-byte blocks,
	 * one exit branch each, with no group fold, no locate step and no strchr flag. SAFETY: all four
	 * loads lie in s's page. A miss in all four enters the aligned body at .Lschr_next with
	 * rax = ((s + 64) & -16) - 16: [s, rax + 16) is inside the checked [s, s + 64). */
	cmpl $4032, %ecx
	jae .Lschrnul_snull
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	pxor %xmm3, %xmm3
	movdqu (%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jz .Lschrnul_s16
	leaq (%rdi,%rdx), %rax
	ret
.Lschrnul_s16:
	movdqu 16(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jz .Lschrnul_s32
	leaq 16(%rdi,%rdx), %rax
	ret
.Lschrnul_s32:
	movdqu 32(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jz .Lschrnul_s48
	leaq 32(%rdi,%rdx), %rax
	ret
.Lschrnul_s48:
	movdqu 48(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jz .Lschrnul_s64
	leaq 48(%rdi,%rdx), %rax
	ret
.Lschrnul_s64:
	leaq 64(%rdi), %rax
	andq $-16, %rax
	subq $16, %rax
	xorl %r11d, %r11d
	jmp .Lschr_next
.Lschrnul_slow:
	/* AVX2 level: the NULL test (mask all ones); SSE2 level: always to the SSE2 head. */
	testq %rdi, liella_machine_schr_mask(%rip)
	jz .Lschrnul_pre
	xorl %r11d, %r11d
	jmp .Lschr_body
.Lschrnul_snull:
	testq %rdi, %rdi
	jz liella_hot_strchr_null
	xorl %r11d, %r11d
	jmp .Lschr_body
	.size liella_machine_strchrnul, .-liella_machine_strchrnul

	.globl liella_machine_strchr
	.type liella_machine_strchr, @function
	.p2align 6
liella_machine_strchr:
#ifdef LIELLA_EXPORT
	.globl strchr
	.type strchr, @function
strchr:
	.globl index
	.type index, @function
index:
#endif
	/* SMALL-STR head (D-4.45): on the AVX2 level with [s, s + 64) inside s's page, two unaligned
	 * 32-byte blocks, each loaded ONCE and compared with c and with zero from that register (the
	 * answer comes from the bytes this call read, D-4.21). SAFETY: both loads lie in s's page. A
	 * miss in both continues in the SSE2 scan from s + 64 (every byte before it was neither c nor
	 * NUL). */
	/* strchr (STRCHR-SHORT): one mask of c-or-NUL; its first bit is the deciding byte (none: the
	 * next block). That byte is c (c's own mask has the bit: return it; c == 0 finds the
	 * terminator) or the NUL (NULL). A miss costs what a hit costs. */
	leal -1(%rdi), %ecx
	andl $4095, %ecx
	cmpl liella_machine_rshort_mask(%rip), %ecx
	jae .Lschr_slow
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	vpxor %xmm3, %xmm3, %xmm3
	vmovdqu (%rdi), %ymm1
	vpcmpeqb %ymm1, %ymm0, %ymm2
	vpcmpeqb %ymm1, %ymm3, %ymm1
	vpor %ymm1, %ymm2, %ymm1
	vpmovmskb %ymm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_h32
	vpmovmskb %ymm2, %edx
	btl %ecx, %edx
	jnc .Lschr_hnull
	leaq (%rdi,%rcx), %rax
	vzeroupper
	ret
.Lschr_h32:
	vmovdqu 32(%rdi), %ymm1
	vpcmpeqb %ymm1, %ymm0, %ymm2
	vpcmpeqb %ymm1, %ymm3, %ymm1
	vpor %ymm1, %ymm2, %ymm1
	vpmovmskb %ymm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_h64
	vpmovmskb %ymm2, %edx
	btl %ecx, %edx
	jnc .Lschr_hnull
	leaq 32(%rdi,%rcx), %rax
	vzeroupper
	ret
.Lschr_h64:
	vzeroupper
	addq $64, %rdi
	jmp .Lschr_pre1
.Lschr_hnull:
	xorl %eax, %eax
	vzeroupper
	ret
.Lschr_pre:
	/* PERF-STRCHR-NONAVX2: strchrnul's SSE2 head (one compare: SSE2 level and [s, s + 64) in s's
	 * page; four unaligned blocks, SAFETY as there). Each block's c lanes and NUL lanes are two
	 * masks of the one loaded register: the first set bit of their OR is the deciding byte, and
	 * c's mask having it means c (c == 0: both masks are the NUL's), else NULL. The answer comes
	 * from the bytes this call read (D-4.21), so no re-read of the byte. A miss in all four enters
	 * the aligned body as strchrnul's does, with the strchr flag set. */
	cmpl $4032, %ecx
	jae .Lschr_snullchk
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	pxor %xmm3, %xmm3
	movdqu (%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm3, %xmm1
	por %xmm2, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_s16
	pmovmskb %xmm2, %edx
	btl %ecx, %edx
	jnc .Lschr_snull
	leaq (%rdi,%rcx), %rax
	ret
.Lschr_s16:
	movdqu 16(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm3, %xmm1
	por %xmm2, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_s32
	pmovmskb %xmm2, %edx
	btl %ecx, %edx
	jnc .Lschr_snull
	leaq 16(%rdi,%rcx), %rax
	ret
.Lschr_s32:
	movdqu 32(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm3, %xmm1
	por %xmm2, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_s48
	pmovmskb %xmm2, %edx
	btl %ecx, %edx
	jnc .Lschr_snull
	leaq 32(%rdi,%rcx), %rax
	ret
.Lschr_s48:
	movdqu 48(%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm3, %xmm1
	por %xmm2, %xmm1
	pmovmskb %xmm1, %ecx
	bsfl %ecx, %ecx
	jz .Lschr_s64
	pmovmskb %xmm2, %edx
	btl %ecx, %edx
	jnc .Lschr_snull
	leaq 48(%rdi,%rcx), %rax
	ret
.Lschr_snull:
	xorl %eax, %eax
	ret
.Lschr_s64:
	leaq 64(%rdi), %rax
	andq $-16, %rax
	subq $16, %rax
	movl $1, %r11d
	jmp .Lschr_next
.Lschr_snullchk:
	testq %rdi, %rdi
	jz liella_hot_strchr_null
	jmp .Lschr_pre1
.Lschr_slow:
	/* AVX2 level: the NULL test (mask all ones); SSE2 level: always to the SSE2 head. */
	testq %rdi, liella_machine_schr_mask(%rip)
	jz .Lschr_pre
.Lschr_pre1:
	movl $1, %r11d
.Lschr_body:
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	pxor %xmm3, %xmm3
	movq %rdi, %rax
	andq $-16, %rax
	movl %edi, %ecx
	andl $4095, %ecx
	cmpl $4080, %ecx
	ja .Lschr_masked
	/* [s, s + 16) inside s's page: one unaligned vector, no shift (measured on keke: the
	 * masked head's variable shift cost ~40 cycles per call when the count was 0). */
	movdqu (%rdi), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	bsfl %edx, %edx
	jz .Lschr_next
	leaq (%rdi,%rdx), %rax
	testl %r11d, %r11d
	jnz .Lschr_check
	ret
.Lschr_masked:
	/* Near the page end: the aligned block holding s, masked below s. */
	andl $15, %ecx
	movdqa (%rax), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	shrl %cl, %edx
	bsfl %edx, %edx
	jz .Lschr_next
	leaq (%rdi,%rdx), %rax
	testl %r11d, %r11d
	jnz .Lschr_check
	ret
.Lschr_next:
	/* rax = s & -16: [s, rax + 16) holds neither c nor NUL. */
	addq $16, %rax
	testb $63, %al
	jz .Lschr_groups
.Lschr_align:
	movdqa (%rax), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lschr_hit
	addq $16, %rax
	testb $63, %al
	jnz .Lschr_align
	.p2align 4
.Lschr_groups:
	movdqa (%rax), %xmm1
	movdqa 16(%rax), %xmm4
	movdqa 32(%rax), %xmm5
	movdqa 48(%rax), %xmm6
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	movdqa %xmm4, %xmm7
	pxor %xmm0, %xmm7
	pminub %xmm4, %xmm7
	pminub %xmm7, %xmm2
	movdqa %xmm5, %xmm7
	pxor %xmm0, %xmm7
	pminub %xmm5, %xmm7
	pminub %xmm7, %xmm2
	movdqa %xmm6, %xmm7
	pxor %xmm0, %xmm7
	pminub %xmm6, %xmm7
	pminub %xmm7, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lschr_find
	addq $64, %rax
	jmp .Lschr_groups
.Lschr_find:
	/* The group at rax holds a hit: the first block with one (a block is loaded only after
	 * one without c or NUL, so even a writer's change keeps every load inside the string). */
	movdqa (%rax), %xmm1
	movdqa %xmm1, %xmm2
	pxor %xmm0, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %edx
	testl %edx, %edx
	jnz .Lschr_hit
	addq $16, %rax
	jmp .Lschr_find
.Lschr_hit:
	bsfl %edx, %edx
	addq %rdx, %rax
	testl %r11d, %r11d
	jnz .Lschr_check
	ret
.Lschr_check:
	movzbl (%rax), %edx
	cmpb %sil, %dl
	je .Lschr_ret
	testb %dl, %dl
	jz .Lschr_null
	/* Neither c nor NUL now: a writer changed the byte after the scan read it. */
	leaq 1(%rax), %rdi
	jmp .Lschr_body
.Lschr_null:
	xorl %eax, %eax
.Lschr_ret:
	ret
	.size liella_machine_strchr, .-liella_machine_strchr

/* const void *liella_machine_memrchr(const void *s, int c, size_t n) (SHORT-1, D-4.35): the
 * address of the last byte of [s, s + n) equal to (unsigned char)c, or NULL. The caller (Rust,
 * strz::rfind_n) answers n == 0 and rejects NULL and n > isize::MAX: here s != NULL and
 * 1 <= n. Scans from the end (most hits lie within 64 bytes of it): the aligned 64-byte group
 * holding s + n - 1 (four unaligned vectors ending at s + n when they stay in that byte's page), its 64-bit mask shifted left by 63 - ((s + n - 1) & 63) so bits past the end
 * fall out (the highest set bit then gives s + n - 64 + bit); then aligned 128-byte blocks
 * lying wholly in [s, s + n) (both levels; PERF-MEMRCHR-NONAVX2 for SSE2); then aligned 64-byte
 * groups down to s. A match
 * below s gives NULL. SAFETY: memrchr's contract makes all of [s, s + n) readable; every load
 * is an aligned 16/32-byte vector of an aligned 64-byte group that holds a byte of [s, s + n),
 * and an aligned group never spans two pages. */
/* const void *liella_machine_memrchr_entry(const void *s, int c, size_t n): the direct memrchr
 * entry (SCAN-RAW; `memrchr` in the products). n == 0, n > isize::MAX (one signed test) and a
 * NULL s (memrchr is not folded, D-4.48: the S6 abort stays) tail-jump to
 * `liella_hot_memrchr_rest` (strz::rfind_n: NULL for n == 0, else the explicit abort); any other
 * call falls through into the kernel below, saving the Rust entry's tests and jump. Reads
 * nothing itself. */
	.hidden liella_hot_memrchr_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_memrchr_entry
	.hidden liella_machine_memrchr_entry
	.type liella_machine_memrchr_entry, @function
	.p2align 6
liella_machine_memrchr_entry:
#ifdef LIELLA_EXPORT
	.globl memrchr
	.type memrchr, @function
memrchr:
#endif
	testq %rdx, %rdx
	jle liella_hot_memrchr_rest
	testq %rdi, %rdi
	jz liella_hot_memrchr_rest
	/* Falls through: no alignment between the tests and the kernel. */
	.globl liella_machine_memrchr
	.type liella_machine_memrchr, @function
liella_machine_memrchr:
	/* SMALL-STR-2 head (D-4.45; REGEX-WL end-page mask): the head runs on the AVX2 level when
	 * [s + n - 64, s + n) lies in the page of s + n - 1 and that byte is not among the page's
	 * first 64 (SAFETY: that page holds a byte of [s, s + n); bytes below s are read but never
	 * reported: a highest match below s gives NULL). Two unaligned 32-byte blocks from the end,
	 * one branch each; n > 64 with no match continues at .Lmrc_more with rax = (s + n - 1) & -64
	 * (>= s + n - 64, so [max(s, rax), s + n) is checked) and c in xmm0's low lanes. */
	/* REGEX-WL: r8 = s + n - 1 here; the end-page mask (0 on SSE2) passes exactly when
	 * (s + n - 1) & 4095 >= 64, so [s + n - 64, s + n) = [r8 - 63, r8] lies in r8's page. */
	leaq -1(%rdi,%rdx), %r8
	testl liella_machine_rshort_mask(%rip), %r8d
	jz .Lmrc_entry
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	vpcmpeqb -31(%r8), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsrl %eax, %eax
	jz .Lmrc_h32
	vzeroupper
	leaq -31(%r8,%rax), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_h32:
	cmpq $32, %rdx
	jbe .Lmrc_hnull
	vpcmpeqb -63(%r8), %ymm0, %ymm1
	vpmovmskb %ymm1, %eax
	bsrl %eax, %eax
	jz .Lmrc_h64
	vzeroupper
	leaq -63(%r8,%rax), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_h64:
	cmpq $64, %rdx
	jbe .Lmrc_hnull
	movq %r8, %rax
	andq $-64, %rax
	/* SCAN-TAIL: ymm0 holds c broadcast already: straight into the 128-byte blocks. */
	jmp .Lmrc_avx_go
.Lmrc_hnull:
	vzeroupper
	xorl %eax, %eax
	ret
.Lmrc_entry:
	/* The SSE2 level, and the AVX2 level when s + n - 1 lies in its page's first 64 bytes (that
	 * level then always takes the masked head below). r8 = s + n - 1 from the entry. */
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	pshufd $0, %xmm0, %xmm0
	/* PERF-MEMRCHR-NONAVX2: (s + n - 1) & 0xfc0 == 0 exactly when (s + n - 1) & 4095 < 64; else
	 * [s + n - 64, s + n) = [r8 - 63, r8] lies in r8's page (two instructions; the rax group
	 * is formed only on the paths that use it). */
	testl $0xfc0, %r8d
	jz .Lmrc_masked
	/* SSE2-GAP: the last 16 bytes first (most hits lie there); n <= 16 is then settled. A
	 * highest match below s gives NULL (every other match is lower still). */
	movdqu -15(%r8), %xmm1
	pcmpeqb %xmm0, %xmm1
	pmovmskb %xmm1, %r9d
	bsrl %r9d, %r9d
	jz .Lmrc_sse_h16
	leaq -15(%r8,%r9), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_sse_h16:
	cmpq $16, %rdx
	jbe .Lmrc_null
	/* [s + n - 64, s + n) inside the page of s + n - 1: its last 16 bytes held no match, so
	 * three unaligned vectors [s + n - 64, s + n - 16), no shift (as strchr's head); bit i of
	 * the 48-bit mask is byte s + n - 64 + i. Bytes below s are dropped by the bound test. */
	movdqu -63(%r8), %xmm1
	movdqu -47(%r8), %xmm2
	movdqu -31(%r8), %xmm3
	pcmpeqb %xmm0, %xmm1
	pcmpeqb %xmm0, %xmm2
	pcmpeqb %xmm0, %xmm3
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r9d
	pmovmskb %xmm3, %r10d
	shll $16, %r9d
	orl %r9d, %edx
	shlq $32, %r10
	orq %r10, %rdx
	bsrq %rdx, %rdx
	jz .Lmrc_sse_more
	leaq -63(%r8,%rdx), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_sse_more:
	/* Only the SSE2 level gets here (see above): straight into its loop with rax =
	 * (s + n - 1) & -64 >= s + n - 64, so [max(s, rax), s + n) holds no c. */
	movq %r8, %rax
	andq $-64, %rax
	jmp .Lmrc_sse_go
.Lmrc_masked:
	movq %r8, %rax
	andq $-64, %rax
	/* cl = ~(s + n - 1): the shift count is taken mod 64, 63 - ((s + n - 1) & 63). */
	movl %r8d, %ecx
	notl %ecx
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r9d
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm4, %esi
	shll $16, %r9d
	orl %r9d, %edx
	shlq $32, %r10
	shlq $48, %rsi
	orq %r10, %rdx
	orq %rsi, %rdx
	/* The highest set bit b after the shift is byte rax + b - cl = r8 - 63 + b. */
	shlq %cl, %rdx
	bsrq %rdx, %rdx
	jz .Lmrc_more
	leaq -63(%r8,%rdx), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_more:
	/* rax = (s + n - 1) & -64: [max(s, rax), s + n) holds no c. */
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lmrc_sse_go
	vpbroadcastb %xmm0, %ymm0
.Lmrc_avx_go:
	leaq 128(%rdi), %r9
	.p2align 4
.Lmrc_avx:
	/* Only blocks wholly inside [s, s + n): rax - 128 >= s. */
	cmpq %r9, %rax
	jb .Lmrc_avx_out
	subq $128, %rax
	vpcmpeqb (%rax), %ymm0, %ymm1
	vpcmpeqb 32(%rax), %ymm0, %ymm2
	vpcmpeqb 64(%rax), %ymm0, %ymm3
	vpcmpeqb 96(%rax), %ymm0, %ymm4
	vpor %ymm1, %ymm2, %ymm5
	vpor %ymm3, %ymm4, %ymm6
	vpor %ymm5, %ymm6, %ymm5
	vpmovmskb %ymm5, %edx
	testl %edx, %edx
	jz .Lmrc_avx
	vpmovmskb %ymm3, %edx
	vpmovmskb %ymm4, %esi
	shlq $32, %rsi
	orq %rsi, %rdx
	jz .Lmrc_avx_low
	bsrq %rdx, %rdx
	leaq 64(%rax,%rdx), %rax
	vzeroupper
	ret
.Lmrc_avx_low:
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %esi
	shlq $32, %rsi
	orq %rsi, %rdx
	bsrq %rdx, %rdx
	addq %rdx, %rax
	vzeroupper
	ret
.Lmrc_avx_out:
	/* SCAN-TAIL: rax < s + 128 and [max(s, rax), s + n) holds no c: at most two aligned 64-byte
	 * groups below rax remain, each as two 32-byte vectors with one vpor test (the SSE2 loop's
	 * four 16-byte vectors cost twice the instructions). A group starting at or above s is
	 * skipped when rax <= s; a match below s gives NULL. SAFETY: each group holds a byte of
	 * [s, rax), and an aligned group never spans two pages. */
	cmpq %rdi, %rax
	jbe .Lmrc_avx_null
	vpcmpeqb -64(%rax), %ymm0, %ymm1
	vpcmpeqb -32(%rax), %ymm0, %ymm2
	vpor %ymm1, %ymm2, %ymm5
	vpmovmskb %ymm5, %edx
	testl %edx, %edx
	jnz .Lmrc_avx_tail_hit
	subq $64, %rax
	cmpq %rdi, %rax
	jbe .Lmrc_avx_null
	vpcmpeqb -64(%rax), %ymm0, %ymm1
	vpcmpeqb -32(%rax), %ymm0, %ymm2
	vpor %ymm1, %ymm2, %ymm5
	vpmovmskb %ymm5, %edx
	testl %edx, %edx
	jnz .Lmrc_avx_tail_hit
.Lmrc_avx_null:
	vzeroupper
	xorl %eax, %eax
	ret
.Lmrc_avx_tail_hit:
	vpmovmskb %ymm1, %edx
	vpmovmskb %ymm2, %esi
	vzeroupper
	shlq $32, %rsi
	orq %rsi, %rdx
	bsrq %rdx, %rdx
	leaq -64(%rax,%rdx), %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_sse_go:
	/* PERF-MEMRCHR-NONAVX2: the SSE2 level's main loop, aligned 128-byte blocks lying wholly in
	 * [s, s + n) (rax - 128 >= s), eight compares and one or-tree per block: 29 instructions
	 * per 128 bytes where the 64-byte groups below cost 36. rax < s + 128 then leaves at most
	 * two groups to .Lmrc_loop. A block with a match: rax moves to its upper group when that
	 * group holds one (the higher match wins) and .Lmrc_loc finds the highest byte there.
	 * SAFETY: every load is an aligned 16-byte vector inside [s, s + n). */
	leaq 128(%rdi), %r9
	.p2align 4
.Lmrc_sse:
	cmpq %r9, %rax
	jb .Lmrc_loop
	subq $128, %rax
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	movdqa %xmm0, %xmm5
	pcmpeqb 64(%rax), %xmm5
	movdqa %xmm0, %xmm6
	pcmpeqb 80(%rax), %xmm6
	movdqa %xmm0, %xmm7
	pcmpeqb 96(%rax), %xmm7
	movdqa %xmm0, %xmm8
	pcmpeqb 112(%rax), %xmm8
	por %xmm2, %xmm1
	por %xmm4, %xmm3
	por %xmm6, %xmm5
	por %xmm8, %xmm7
	por %xmm3, %xmm1
	por %xmm7, %xmm5
	por %xmm5, %xmm1
	pmovmskb %xmm1, %edx
	testl %edx, %edx
	jz .Lmrc_sse
	/* xmm5 still holds the upper group's or. */
	pmovmskb %xmm5, %edx
	leaq 64(%rax), %rsi
	testl %edx, %edx
	cmovnzq %rsi, %rax
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	jmp .Lmrc_loc
	.p2align 4
.Lmrc_loop:
	cmpq %rdi, %rax
	jbe .Lmrc_null
	subq $64, %rax
	movdqa %xmm0, %xmm1
	pcmpeqb (%rax), %xmm1
	movdqa %xmm0, %xmm2
	pcmpeqb 16(%rax), %xmm2
	movdqa %xmm0, %xmm3
	pcmpeqb 32(%rax), %xmm3
	movdqa %xmm0, %xmm4
	pcmpeqb 48(%rax), %xmm4
	movdqa %xmm1, %xmm5
	por %xmm2, %xmm5
	por %xmm3, %xmm5
	por %xmm4, %xmm5
	pmovmskb %xmm5, %edx
	testl %edx, %edx
	jz .Lmrc_loop
.Lmrc_loc:
	/* xmm1..xmm4: the compares of the group [rax, rax + 64), which holds a match. */
	pmovmskb %xmm1, %edx
	pmovmskb %xmm2, %r9d
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm4, %esi
	shll $16, %r9d
	orl %r9d, %edx
	shlq $32, %r10
	shlq $48, %rsi
	orq %r10, %rdx
	orq %rsi, %rdx
	bsrq %rdx, %rdx
	addq %rdx, %rax
	cmpq %rdi, %rax
	jb .Lmrc_null
	ret
.Lmrc_null:
	xorl %eax, %eax
	ret
	.size liella_machine_memrchr, .-liella_machine_memrchr
	.size liella_machine_memrchr_entry, .-liella_machine_memrchr_entry
#ifdef LIELLA_EXPORT
	.size memrchr, .-memrchr
#endif

/* int liella_machine_strncmp(const char *a, const char *b, size_t n) (SHORT-1, D-4.35): the
 * order of the first n bytes of a and b as strncmp defines it (unsigned bytes, stopping at a
 * NUL), -1, 0 or 1, in one pass (before SHORT-1 the export measured both texts, then compared
 * them: three passes). The caller (Rust, strz::order_n) answers n == 0 and rejects NULL: here
 * a, b != NULL and n >= 1. Steps of 16 bytes at i: one unaligned vector from each side when
 * neither crosses its page (page offset <= 4080), else one byte; four such vectors under one
 * page test when 64 bytes fit in both pages (offset <= 4032). min(x, (x == y)) is zero
 * exactly where the bytes differ or x is NUL; a hit at or past n is no hit. The deciding bytes
 * are read again: equal and NUL gives 0, different gives the order, equal and not NUL means a
 * writer changed them after the scan, and the scan resumes after them (D-4.21).
 * SAFETY: a step's first bytes a[i], b[i] lie before n and after bytes that were equal and not
 * NUL, so the caller's contract makes them readable; a vector never leaves their page.
 * DIRECT ENTRY (STR-CHR-CMP-SHORT): `strncmp` in the products is this kernel, with no Rust
 * wrapper: n == 0 returns 0 before any read; the head's page test takes (a - 1) | (b - 1) & 4095
 * below liella_machine_rshort_mask, which a NULL a or b never passes; .Lsnc_pt tests both for
 * NULL and tail-jumps to `liella_hot_strncmp_null` (the S6 abort) before any read. */
	.hidden liella_hot_strncmp_null  /* crates/boundary/src/posix/s04_strings.rs */
	.globl liella_machine_strncmp
	.type liella_machine_strncmp, @function
	.p2align 6
liella_machine_strncmp:
#ifdef LIELLA_EXPORT
	.globl strncmp
	.type strncmp, @function
strncmp:
#endif
	/* SMALL-STR-2 head (D-4.45): on the AVX2 level with [a, a + 64) and [b, b + 64) inside their
	 * pages, two unaligned 32-byte blocks of both strings, one branch each. The page test is one
	 * compare of (a | b) & 4095 against the short limit: the OR is at least either offset, so
	 * passing it passes both; a rejected pair re-tests exactly at .Lsnc_pt (SAFETY: every load
	 * then lies in a's or b's first page, which hold a[0] and b[0], readable as n >= 1). A hit
	 * (bytes differ or a's is NUL) at index i < n decides at .Lsnc_byte, which re-reads both
	 * bytes (D-4.21); a hit at or past n, or no hit within min(n, 64), gives 0; n > 64 with no
	 * hit resumes the SSE2 steps at i = 64. */
	testq %rdx, %rdx
	jz .Lsnc_zero
	leal -1(%rdi), %eax
	leal -1(%rsi), %ecx
	orl %ecx, %eax
	andl $4095, %eax
	cmpl liella_machine_rshort_mask(%rip), %eax
	jae .Lsnc_pt
.Lsnc_head:
	vpxor %xmm0, %xmm0, %xmm0
	vmovdqu (%rdi), %ymm1
	vpcmpeqb (%rsi), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpmovmskb %ymm2, %eax
	bsfl %eax, %eax
	jz .Lsnc_h32
.Lsnc_hhit:
	vzeroupper
	cmpq %rdx, %rax
	jae .Lsnc_zero
	movq %rax, %rcx
	/* .Lsnc_byte inline; re-read equal and not NUL (a writer) resumes there. */
	movzbl (%rdi,%rax), %eax
	movzbl (%rsi,%rcx), %r8d
	cmpl %r8d, %eax
	jne .Lsnc_order
	testl %eax, %eax
	jz .Lsnc_ret
	jmp .Lsnc_byte
.Lsnc_h32:
	cmpq $32, %rdx
	jbe .Lsnc_hzero
	vmovdqu 32(%rdi), %ymm1
	vpcmpeqb 32(%rsi), %ymm1, %ymm2
	vpminub %ymm1, %ymm2, %ymm2
	vpcmpeqb %ymm0, %ymm2, %ymm2
	vpmovmskb %ymm2, %eax
	bsfl %eax, %eax
	jz .Lsnc_h64
	addl $32, %eax
	jmp .Lsnc_hhit
.Lsnc_h64:
	vzeroupper
	cmpq $64, %rdx
	jbe .Lsnc_zero
	pxor %xmm3, %xmm3
	movl $64, %ecx
	jmp .Lsnc_step
.Lsnc_hzero:
	vzeroupper
	xorl %eax, %eax
	ret
.Lsnc_pt:
	/* The S6 NULL tests, then the exact page test for a pair the OR test rejected (or the SSE2
	 * level: limit 0). */
	testq %rdi, %rdi
	jz liella_hot_strncmp_null
	testq %rsi, %rsi
	jz liella_hot_strncmp_null
	movl %edi, %eax
	andl $4095, %eax
	cmpl liella_machine_short_lim(%rip), %eax
	jae .Lsnc_old
	movl %esi, %eax
	andl $4095, %eax
	cmpl liella_machine_short_lim(%rip), %eax
	jb .Lsnc_head
.Lsnc_old:
	pxor %xmm3, %xmm3
	xorl %ecx, %ecx
.Lsnc_step:
	leaq (%rdi,%rcx), %r8
	leaq (%rsi,%rcx), %r9
	movl %r8d, %eax
	andl $4095, %eax
	cmpl $4032, %eax
	ja .Lsnc_step16
	movl %r9d, %eax
	andl $4095, %eax
	cmpl $4032, %eax
	ja .Lsnc_step16
	/* 64 bytes from i inside both pages: four vectors, one page test (rcx = each block's i). */
	movdqu (%r8), %xmm1
	movdqu (%r9), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %eax
	testl %eax, %eax
	jnz .Lsnc_hit
	addq $16, %rcx
	cmpq %rdx, %rcx
	jae .Lsnc_zero
	movdqu 16(%r8), %xmm1
	movdqu 16(%r9), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %eax
	testl %eax, %eax
	jnz .Lsnc_hit
	addq $16, %rcx
	cmpq %rdx, %rcx
	jae .Lsnc_zero
	movdqu 32(%r8), %xmm1
	movdqu 32(%r9), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %eax
	testl %eax, %eax
	jnz .Lsnc_hit
	addq $16, %rcx
	cmpq %rdx, %rcx
	jae .Lsnc_zero
	movdqu 48(%r8), %xmm1
	movdqu 48(%r9), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %eax
	testl %eax, %eax
	jnz .Lsnc_hit
	addq $16, %rcx
	cmpq %rdx, %rcx
	jb .Lsnc_step
.Lsnc_zero:
	xorl %eax, %eax
	ret
.Lsnc_hit:
	bsfl %eax, %eax
	addq %rax, %rcx
	cmpq %rdx, %rcx
	jb .Lsnc_byte
	xorl %eax, %eax
	ret
.Lsnc_step16:
	movl %r8d, %eax
	andl $4095, %eax
	cmpl $4080, %eax
	ja .Lsnc_byte
	movl %r9d, %eax
	andl $4095, %eax
	cmpl $4080, %eax
	ja .Lsnc_byte
	movdqu (%r8), %xmm1
	movdqu (%r9), %xmm2
	pcmpeqb %xmm1, %xmm2
	pminub %xmm1, %xmm2
	pcmpeqb %xmm3, %xmm2
	pmovmskb %xmm2, %eax
	bsfl %eax, %eax
	jz .Lsnc_none
	addq %rax, %rcx
	cmpq %rdx, %rcx
	jb .Lsnc_byte
	xorl %eax, %eax
	ret
.Lsnc_none:
	addq $16, %rcx
	cmpq %rdx, %rcx
	jb .Lsnc_step
	xorl %eax, %eax
	ret
.Lsnc_byte:
	/* One byte at i < n: near a page end, or the deciding byte of a vector step. */
	movzbl (%rdi,%rcx), %eax
	movzbl (%rsi,%rcx), %r8d
	cmpl %r8d, %eax
	jne .Lsnc_order
	testl %eax, %eax
	jz .Lsnc_ret
	incq %rcx
	cmpq %rdx, %rcx
	jb .Lsnc_step
	xorl %eax, %eax
.Lsnc_ret:
	ret
.Lsnc_order:
	/* CF = a's byte below b's: -1, else 1. */
	sbbl %eax, %eax
	orl $1, %eax
	ret
	.size liella_machine_strncmp, .-liella_machine_strncmp


/* size_t liella_machine_strpair(const char *p, size_t off, int c0, int c1) and
 * size_t liella_machine_wcspair(const int *p, size_t off, int c0, int c1) (STRSTR-VEC): the fused
 * candidate and terminator scan of strstr / wcsstr. A position q >= p (in units: bytes for
 * strpair, 4-byte units for wcspair) is a CANDIDATE when unit q is c1 and unit q - off is c0.
 * Returns 2*k when q = p + k is the first candidate and no unit in [p, q] is 0; returns 2*k + 1
 * when p + k is the first 0 unit at or after p and no candidate lies before it. c0/c1 are the
 * unit values (strpair: 0..255); c1 is never 0 in the callers, so the terminator is no candidate.
 *
 * Contract (the Rust caller `strz::find_str` / `find_wcs` keeps it): the off units before p are
 * units of the string before its terminator (readable, none 0), and p lies in the string (at or
 * before its terminator); wcspair's p is 4-byte aligned. Loads: when p's page has 32 bytes from
 * p on (page offset <= 4064), one unaligned head block at p and one at p - off, both inside
 * [p - off, p + 32) (the known units, then p's page). Otherwise units one by one up to the next
 * 32-byte boundary (stopping at a 0 unit or a candidate). Then aligned blocks A (32 bytes on
 * AVX2, 16 on SSE2), each with the block at A - off, which lies inside [p - off, A + block):
 * units already seen not to be 0, then A's own block; an aligned block never leaves its page.
 * AVX2 from a 64-byte boundary on: 64-byte aligned pairs of blocks (one page each), one test
 * for "any candidate or 0 unit" per pair; a pair that has one is resolved block by block.
 * Units past the terminator are read only inside the terminator's page and are masked off
 * (the bits from the first 0 unit on). Rust verifies the needle at every candidate. */
	.text
	.globl liella_machine_strpair
	.type liella_machine_strpair, @function
	.p2align 4
liella_machine_strpair:
	movq %rdi, %rax
	xorl %r8d, %r8d
	movl %edi, %r9d
	andl $4095, %r9d
	cmpl $4064, %r9d
	jbe .Lliella_machine_strpair_vec
	/* p in the last 32 bytes of its page: unit by unit to the 32-byte boundary. */
.Lliella_machine_strpair_units:
	movzbl (%rax), %r10d
	testl %r10d, %r10d
	jz .Lliella_machine_strpair_unit_nul
	cmpl %ecx, %r10d
	jne 1f
	movq %rax, %r9
	subq %rsi, %r9
	movzbl (%r9), %r10d
	cmpl %edx, %r10d
	je .Lliella_machine_strpair_unit_cand
1:
	addq $1, %rax
	testl $31, %eax
	jnz .Lliella_machine_strpair_units
	movl $1, %r8d
.Lliella_machine_strpair_vec:
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lliella_machine_strpair_sse2
	vmovd %edx, %xmm0
	vmovd %ecx, %xmm1
	vpbroadcastb %xmm0, %ymm0
	vpbroadcastb %xmm1, %ymm1
	vpxor %xmm2, %xmm2, %xmm2
	testl %r8d, %r8d
	jnz .Lliella_machine_strpair_avx_loop
	vmovdqu (%rax), %ymm3
	movq %rax, %r9
	subq %rsi, %r9
	vpcmpeqb (%r9), %ymm0, %ymm4
	vpcmpeqb %ymm3, %ymm2, %ymm5
	vpcmpeqb %ymm3, %ymm1, %ymm3
	vpand %ymm4, %ymm3, %ymm3
	vpmovmskb %ymm3, %r10d
	vpmovmskb %ymm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_strpair_avx_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_strpair_avx_nul
	andq $-32, %rax
	addq $32, %rax
	.p2align 4
.Lliella_machine_strpair_avx_loop:
	vmovdqa (%rax), %ymm3
	movq %rax, %r9
	subq %rsi, %r9
	vpcmpeqb (%r9), %ymm0, %ymm4
	vpcmpeqb %ymm3, %ymm2, %ymm5
	vpcmpeqb %ymm3, %ymm1, %ymm3
	vpand %ymm4, %ymm3, %ymm3
	vpmovmskb %ymm3, %r10d
	vpmovmskb %ymm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_strpair_avx_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_strpair_avx_nul
	addq $32, %rax
	testl $63, %eax
	jnz .Lliella_machine_strpair_avx_loop
	movq %rsi, %r9
	negq %r9
	.p2align 4
.Lliella_machine_strpair_avx_loop64:
	/* Unit q is a candidate or 0 exactly when min((Y ^ c1) | (L ^ c0), Y) is 0. */
	vmovdqa (%rax), %ymm3
	vmovdqa 32(%rax), %ymm6
	vpxor (%rax,%r9), %ymm0, %ymm4
	vpxor 32(%rax,%r9), %ymm0, %ymm7
	vpxor %ymm3, %ymm1, %ymm5
	vpxor %ymm6, %ymm1, %ymm8
	vpor %ymm4, %ymm5, %ymm4
	vpor %ymm7, %ymm8, %ymm7
	vpminub %ymm3, %ymm4, %ymm4
	vpminub %ymm6, %ymm7, %ymm7
	vpminub %ymm4, %ymm7, %ymm4
	vpcmpeqb %ymm4, %ymm2, %ymm4
	vpmovmskb %ymm4, %r10d
	testl %r10d, %r10d
	jnz .Lliella_machine_strpair_avx_loop
	addq $64, %rax
	jmp .Lliella_machine_strpair_avx_loop64
.Lliella_machine_strpair_avx_cand:
	vzeroupper
.Lliella_machine_strpair_sse_cand:
	bsfl %r10d, %r10d
	addq %r10, %rax
.Lliella_machine_strpair_unit_cand:
	subq %rdi, %rax
	addq %rax, %rax
	ret
.Lliella_machine_strpair_avx_nul:
	vzeroupper
.Lliella_machine_strpair_sse_nul:
	bsfl %r11d, %r11d
	addq %r11, %rax
.Lliella_machine_strpair_unit_nul:
	subq %rdi, %rax
	leaq 1(%rax,%rax), %rax
	ret
.Lliella_machine_strpair_sse2:
	movd %edx, %xmm0
	movd %ecx, %xmm1
	punpcklbw %xmm0, %xmm0
	punpcklwd %xmm0, %xmm0
	punpcklbw %xmm1, %xmm1
	punpcklwd %xmm1, %xmm1
	pshufd $0, %xmm0, %xmm0
	pshufd $0, %xmm1, %xmm1
	pxor %xmm2, %xmm2
	testl %r8d, %r8d
	jnz .Lliella_machine_strpair_sse_loop
	movdqu (%rax), %xmm3
	movq %rax, %r9
	subq %rsi, %r9
	movdqu (%r9), %xmm4
	pcmpeqb %xmm0, %xmm4
	movdqa %xmm3, %xmm5
	pcmpeqb %xmm2, %xmm5
	pcmpeqb %xmm1, %xmm3
	pand %xmm4, %xmm3
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_strpair_sse_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_strpair_sse_nul
	andq $-16, %rax
	addq $16, %rax
	.p2align 4
.Lliella_machine_strpair_sse_loop:
	movdqa (%rax), %xmm3
	movq %rax, %r9
	subq %rsi, %r9
	movdqu (%r9), %xmm4
	pcmpeqb %xmm0, %xmm4
	movdqa %xmm3, %xmm5
	pcmpeqb %xmm2, %xmm5
	pcmpeqb %xmm1, %xmm3
	pand %xmm4, %xmm3
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_strpair_sse_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_strpair_sse_nul
	addq $16, %rax
	jmp .Lliella_machine_strpair_sse_loop
	.size liella_machine_strpair, .-liella_machine_strpair

	.globl liella_machine_wcspair
	.type liella_machine_wcspair, @function
	.p2align 4
liella_machine_wcspair:
	shlq $2, %rsi
	movq %rdi, %rax
	xorl %r8d, %r8d
	movl %edi, %r9d
	andl $4095, %r9d
	cmpl $4064, %r9d
	jbe .Lliella_machine_wcspair_vec
	/* p in the last 32 bytes of its page: unit by unit to the 32-byte boundary. */
.Lliella_machine_wcspair_units:
	movl (%rax), %r10d
	testl %r10d, %r10d
	jz .Lliella_machine_wcspair_unit_nul
	cmpl %ecx, %r10d
	jne 1f
	movq %rax, %r9
	subq %rsi, %r9
	movl (%r9), %r10d
	cmpl %edx, %r10d
	je .Lliella_machine_wcspair_unit_cand
1:
	addq $4, %rax
	testl $31, %eax
	jnz .Lliella_machine_wcspair_units
	movl $1, %r8d
.Lliella_machine_wcspair_vec:
	cmpl $0, liella_machine_scan_level(%rip)
	je .Lliella_machine_wcspair_sse2
	vmovd %edx, %xmm0
	vmovd %ecx, %xmm1
	vpbroadcastd %xmm0, %ymm0
	vpbroadcastd %xmm1, %ymm1
	vpxor %xmm2, %xmm2, %xmm2
	testl %r8d, %r8d
	jnz .Lliella_machine_wcspair_avx_loop
	vmovdqu (%rax), %ymm3
	movq %rax, %r9
	subq %rsi, %r9
	vpcmpeqd (%r9), %ymm0, %ymm4
	vpcmpeqd %ymm3, %ymm2, %ymm5
	vpcmpeqd %ymm3, %ymm1, %ymm3
	vpand %ymm4, %ymm3, %ymm3
	vpmovmskb %ymm3, %r10d
	vpmovmskb %ymm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_wcspair_avx_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_wcspair_avx_nul
	andq $-32, %rax
	addq $32, %rax
	.p2align 4
.Lliella_machine_wcspair_avx_loop:
	vmovdqa (%rax), %ymm3
	movq %rax, %r9
	subq %rsi, %r9
	vpcmpeqd (%r9), %ymm0, %ymm4
	vpcmpeqd %ymm3, %ymm2, %ymm5
	vpcmpeqd %ymm3, %ymm1, %ymm3
	vpand %ymm4, %ymm3, %ymm3
	vpmovmskb %ymm3, %r10d
	vpmovmskb %ymm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_wcspair_avx_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_wcspair_avx_nul
	addq $32, %rax
	testl $63, %eax
	jnz .Lliella_machine_wcspair_avx_loop
	movq %rsi, %r9
	negq %r9
	.p2align 4
.Lliella_machine_wcspair_avx_loop64:
	/* Unit q is a candidate or 0 exactly when min((Y ^ c1) | (L ^ c0), Y) is 0. */
	vmovdqa (%rax), %ymm3
	vmovdqa 32(%rax), %ymm6
	vpxor (%rax,%r9), %ymm0, %ymm4
	vpxor 32(%rax,%r9), %ymm0, %ymm7
	vpxor %ymm3, %ymm1, %ymm5
	vpxor %ymm6, %ymm1, %ymm8
	vpor %ymm4, %ymm5, %ymm4
	vpor %ymm7, %ymm8, %ymm7
	vpminud %ymm3, %ymm4, %ymm4
	vpminud %ymm6, %ymm7, %ymm7
	vpminud %ymm4, %ymm7, %ymm4
	vpcmpeqd %ymm4, %ymm2, %ymm4
	vpmovmskb %ymm4, %r10d
	testl %r10d, %r10d
	jnz .Lliella_machine_wcspair_avx_loop
	addq $64, %rax
	jmp .Lliella_machine_wcspair_avx_loop64
.Lliella_machine_wcspair_avx_cand:
	vzeroupper
.Lliella_machine_wcspair_sse_cand:
	bsfl %r10d, %r10d
	addq %r10, %rax
.Lliella_machine_wcspair_unit_cand:
	subq %rdi, %rax
	shrq $2, %rax
	addq %rax, %rax
	ret
.Lliella_machine_wcspair_avx_nul:
	vzeroupper
.Lliella_machine_wcspair_sse_nul:
	bsfl %r11d, %r11d
	addq %r11, %rax
.Lliella_machine_wcspair_unit_nul:
	subq %rdi, %rax
	shrq $2, %rax
	leaq 1(%rax,%rax), %rax
	ret
.Lliella_machine_wcspair_sse2:
	movd %edx, %xmm0
	movd %ecx, %xmm1
	pshufd $0, %xmm0, %xmm0
	pshufd $0, %xmm1, %xmm1
	pxor %xmm2, %xmm2
	testl %r8d, %r8d
	jnz .Lliella_machine_wcspair_sse_loop
	movdqu (%rax), %xmm3
	movq %rax, %r9
	subq %rsi, %r9
	movdqu (%r9), %xmm4
	pcmpeqd %xmm0, %xmm4
	movdqa %xmm3, %xmm5
	pcmpeqd %xmm2, %xmm5
	pcmpeqd %xmm1, %xmm3
	pand %xmm4, %xmm3
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_wcspair_sse_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_wcspair_sse_nul
	andq $-16, %rax
	addq $16, %rax
	.p2align 4
.Lliella_machine_wcspair_sse_loop:
	movdqa (%rax), %xmm3
	movq %rax, %r9
	subq %rsi, %r9
	movdqu (%r9), %xmm4
	pcmpeqd %xmm0, %xmm4
	movdqa %xmm3, %xmm5
	pcmpeqd %xmm2, %xmm5
	pcmpeqd %xmm1, %xmm3
	pand %xmm4, %xmm3
	pmovmskb %xmm3, %r10d
	pmovmskb %xmm5, %r11d
	leal -1(%r11), %r9d
	xorl %r11d, %r9d
	andl %r9d, %r10d
	jnz .Lliella_machine_wcspair_sse_cand
	testl %r11d, %r11d
	jnz .Lliella_machine_wcspair_sse_nul
	addq $16, %rax
	jmp .Lliella_machine_wcspair_sse_loop
	.size liella_machine_wcspair, .-liella_machine_wcspair

	.section .note.GNU-stack,"",@progbits
