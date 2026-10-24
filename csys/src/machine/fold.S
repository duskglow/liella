/* Class machine: the direct copy and fill entries (D-4.22 amendments MEM-K4b and COPY-FILL; owner
 * rulings 2026-10-08 "NULL FOLDING" and "ASSEMBLY WHERE RUST CANNOT EXPRESS IT"; plan S2 hot-kernel,
 * P7).
 *
 * In the products these ARE `memset`, `memcpy` and `memmove` (second labels under LIELLA_EXPORT,
 * versions from spec/abi/versions.tsv): a call reaches the kernel with no Rust hop in between
 * (COPY-FILL: the Rust entry's load, two compares and tail jump were 7 of memset(16)'s 17
 * instructions). Test builds define only the liella_machine_* names; the Rust twins
 * (crates/boundary/src/hot.rs `move_bytes`, `set_bytes`) call them, so the tests run this code.
 *
 * Why assembly: with NULL folding on, the 1-64 byte classes let a NULL pointer fault on page zero
 * instead of testing it. In Rust a NULL access is undefined behaviour; here it is an ordinary
 * load or store that faults. `liella_machine_fold_limit` (defined below; written by crates/boundary/src/dispatch.rs) is
 * 64 while folding is on and 0 otherwise, so `n <= limit` is the folded classes' one gate (n == 0
 * always passes it: the classes return at once for 0 without touching memory). Every other size
 * tail-jumps to Rust: `liella_hot_move_rest` / `liella_hot_fill_rest` (the folding decision, the
 * explicit S6 NULL checks, the large sizes). The 65-256 copy and fill classes are here, after an explicit NULL test (owner ruling 2026-10-08, option B: 65 and up keeps
 * the explicit checks); a NULL goes to Rust, which aborts with EFAULT (S6).
 *
 * HOT-KERNEL CONTRACT (what the callers rely on): the C contract only - d valid for writing n
 * bytes and s valid for reading n bytes (or NULL, handled as above). Each class reads only
 * [s, s + n), writes only [d, d + n), and loads every source byte into registers before its first
 * store, so overlapping regions move correctly (memcpy is memmove, D-4.2). Returns d. SSE2 (the
 * x86-64 baseline), except the 65-256 copy's and the 129-256 fill's AVX2 builds behind `liella_machine_avx2` (D-4.25). Every entry is .p2align 6 (MEM-K5b: a fixed line start keeps the short
 * paths in one line whatever code is added elsewhere). No .macro (the affected map reads this file
 * for the symbols it defines and uses). */

	.text
	.hidden liella_hot_move_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.hidden liella_hot_fill_rest

/* void *liella_machine_memmove(void *d, const void *s, size_t n): 129-256 (not folded, see
 * .Lmove_big) four 32-byte blocks from each end (AVX2) or eight 16-byte ones (SSE2), 65-128 two
 * 32-byte blocks from each end (AVX2) or four 16-byte ones (SSE2), 33-64 two
 * 16-byte blocks from each end, 16-32 one from each end, 8-15 two 8-byte words, 4-7 two 4-byte words, 2-3 two 2-byte
 * words, 1 one byte, 0 nothing; other sizes in Rust. */
	.globl liella_machine_memmove
	.hidden liella_machine_memmove
	.type liella_machine_memmove, @function
	.p2align 6
liella_machine_memmove:
#ifdef LIELLA_EXPORT
	.globl memmove
	.type memmove, @function
memmove:
	.globl memcpy
	.type memcpy, @function
memcpy:
#endif
	movq %rdi, %rax
	cmpq liella_machine_fold_limit(%rip), %rdx
	ja .Lmove_big
/* The folded classes: they never write %rax, so liella_machine_mempcpy enters here with d + n. */
.Lmove_small:
	cmpq $32, %rdx
	ja .Lmove_33
	cmpq $16, %rdx
	jb .Lmove_lt16
	movups (%rsi), %xmm0
	movups -16(%rsi,%rdx), %xmm1
	movups %xmm0, (%rdi)
	movups %xmm1, -16(%rdi,%rdx)
	ret
.Lmove_33:
	movups (%rsi), %xmm0
	movups 16(%rsi), %xmm1
	movups -32(%rsi,%rdx), %xmm2
	movups -16(%rsi,%rdx), %xmm3
	movups %xmm0, (%rdi)
	movups %xmm1, 16(%rdi)
	movups %xmm2, -32(%rdi,%rdx)
	movups %xmm3, -16(%rdi,%rdx)
	ret
.Lmove_lt16:
	cmpq $8, %rdx
	jb .Lmove_lt8
	movq (%rsi), %rcx
	movq -8(%rsi,%rdx), %r8
	movq %rcx, (%rdi)
	movq %r8, -8(%rdi,%rdx)
	ret
.Lmove_lt8:
	cmpq $4, %rdx
	jb .Lmove_lt4
	movl (%rsi), %ecx
	movl -4(%rsi,%rdx), %r8d
	movl %ecx, (%rdi)
	movl %r8d, -4(%rdi,%rdx)
	ret
.Lmove_lt4:
	cmpq $2, %rdx
	jb .Lmove_one
	movzwl (%rsi), %ecx
	movzwl -2(%rsi,%rdx), %r8d
	movw %cx, (%rdi)
	movw %r8w, -2(%rdi,%rdx)
	ret
.Lmove_one:
	testq %rdx, %rdx
	jz .Lmove_none
	movzbl (%rsi), %ecx
	movb %cl, (%rdi)
.Lmove_none:
	ret
/* Not folded: 65-256 here after the explicit NULL tests of d and s (option B; one unsigned compare
 * per class: n - 65 <= 63 for 65-128, then <= 191 for 129-256), with AVX2 (the flag byte below, set
 * by crates/boundary/src/dispatch.rs only when the CPU has AVX2 and the OS saves the YMM state) two
 * (65-128) or four (129-256) 32-byte blocks from each end, without it four or eight 16-byte ones;
 * a NULL, 1-64 without folding and every other size in Rust. The 65-128 class falls through
 * (no compare added to it by COPY-256). */
.Lmove_big:
	leaq -65(%rdx), %rcx
	cmpq $63, %rcx
	ja .Lmove_129
	testq %rdi, %rdi
	jz liella_hot_move_rest
	testq %rsi, %rsi
	jz liella_hot_move_rest
.Lmove_65:
	cmpb $0, liella_machine_avx2(%rip)
	je .Lmove_65_sse2
	vmovdqu (%rsi), %ymm0
	vmovdqu 32(%rsi), %ymm1
	vmovdqu -64(%rsi,%rdx), %ymm2
	vmovdqu -32(%rsi,%rdx), %ymm3
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm1, 32(%rdi)
	vmovdqu %ymm2, -64(%rdi,%rdx)
	vmovdqu %ymm3, -32(%rdi,%rdx)
	vzeroupper
	ret
.Lmove_65_sse2:
	movups (%rsi), %xmm0
	movups 16(%rsi), %xmm1
	movups 32(%rsi), %xmm2
	movups 48(%rsi), %xmm3
	movups -64(%rsi,%rdx), %xmm4
	movups -48(%rsi,%rdx), %xmm5
	movups -32(%rsi,%rdx), %xmm6
	movups -16(%rsi,%rdx), %xmm7
	movups %xmm0, (%rdi)
	movups %xmm1, 16(%rdi)
	movups %xmm2, 32(%rdi)
	movups %xmm3, 48(%rdi)
	movups %xmm4, -64(%rdi,%rdx)
	movups %xmm5, -48(%rdi,%rdx)
	movups %xmm6, -32(%rdi,%rdx)
	movups %xmm7, -16(%rdi,%rdx)
	ret
.Lmove_129:
	cmpq $191, %rcx
	ja liella_hot_move_rest
	testq %rdi, %rdi
	jz liella_hot_move_rest
	testq %rsi, %rsi
	jz liella_hot_move_rest
.Lmove_129_go:
	cmpb $0, liella_machine_avx2(%rip)
	je .Lmove_129_sse2
	vmovdqu (%rsi), %ymm0
	vmovdqu 32(%rsi), %ymm1
	vmovdqu 64(%rsi), %ymm2
	vmovdqu 96(%rsi), %ymm3
	vmovdqu -128(%rsi,%rdx), %ymm4
	vmovdqu -96(%rsi,%rdx), %ymm5
	vmovdqu -64(%rsi,%rdx), %ymm6
	vmovdqu -32(%rsi,%rdx), %ymm7
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm1, 32(%rdi)
	vmovdqu %ymm2, 64(%rdi)
	vmovdqu %ymm3, 96(%rdi)
	vmovdqu %ymm4, -128(%rdi,%rdx)
	vmovdqu %ymm5, -96(%rdi,%rdx)
	vmovdqu %ymm6, -64(%rdi,%rdx)
	vmovdqu %ymm7, -32(%rdi,%rdx)
	vzeroupper
	ret
.Lmove_129_sse2:
	movups (%rsi), %xmm0
	movups 16(%rsi), %xmm1
	movups 32(%rsi), %xmm2
	movups 48(%rsi), %xmm3
	movups 64(%rsi), %xmm4
	movups 80(%rsi), %xmm5
	movups 96(%rsi), %xmm6
	movups 112(%rsi), %xmm7
	movups -128(%rsi,%rdx), %xmm8
	movups -112(%rsi,%rdx), %xmm9
	movups -96(%rsi,%rdx), %xmm10
	movups -80(%rsi,%rdx), %xmm11
	movups -64(%rsi,%rdx), %xmm12
	movups -48(%rsi,%rdx), %xmm13
	movups -32(%rsi,%rdx), %xmm14
	movups -16(%rsi,%rdx), %xmm15
	movups %xmm0, (%rdi)
	movups %xmm1, 16(%rdi)
	movups %xmm2, 32(%rdi)
	movups %xmm3, 48(%rdi)
	movups %xmm4, 64(%rdi)
	movups %xmm5, 80(%rdi)
	movups %xmm6, 96(%rdi)
	movups %xmm7, 112(%rdi)
	movups %xmm8, -128(%rdi,%rdx)
	movups %xmm9, -112(%rdi,%rdx)
	movups %xmm10, -96(%rdi,%rdx)
	movups %xmm11, -80(%rdi,%rdx)
	movups %xmm12, -64(%rdi,%rdx)
	movups %xmm13, -48(%rdi,%rdx)
	movups %xmm14, -32(%rdi,%rdx)
	movups %xmm15, -16(%rdi,%rdx)
	ret
/* void liella_machine_move_mid(void *d, const void *s, size_t n): the library's internal copy
 * (crates/boundary/src/hot.rs `internal_move`, D-X.15) for 65 <= n <= 256 with d and s non-NULL
 * (its contract; no NULL tests, no range test beyond the class split): the classes above. Returns
 * nothing (rax is not set). */
	.globl liella_machine_move_mid
	.hidden liella_machine_move_mid
	.type liella_machine_move_mid, @function
liella_machine_move_mid:
	cmpq $128, %rdx
	jbe .Lmove_65
	jmp .Lmove_129_go
	.size liella_machine_move_mid, .-liella_machine_move_mid
#ifdef LIELLA_EXPORT
	.size memcpy, .-memcpy
	.size memmove, .-memmove
#endif
	.size liella_machine_memmove, .-liella_machine_memmove

/* void *liella_machine_mempcpy(void *d, const void *s, size_t n): memmove's copy, returning d + n
 * (WL-HOT-SEQ2: `mempcpy` and `__mempcpy` in the products; the Rust entry's frame, call and add were
 * 6 of mempcpy(8)'s 20 instructions, seq calls it once per line). The folded classes are entered
 * directly with %rax = d + n (they never write %rax); every other size calls the copy entry, which
 * returns d, and adds n (saved on the stack across the call; the push also aligns the stack). */
	.globl liella_machine_mempcpy
	.hidden liella_machine_mempcpy
	.type liella_machine_mempcpy, @function
	.p2align 4
liella_machine_mempcpy:
#ifdef LIELLA_EXPORT
	.globl mempcpy
	.type mempcpy, @function
mempcpy:
	.globl __mempcpy
	.type __mempcpy, @function
__mempcpy:
#endif
	leaq (%rdi,%rdx), %rax
	cmpq liella_machine_fold_limit(%rip), %rdx
	jbe .Lmove_small
	pushq %rdx
	call liella_machine_memmove
	popq %rdx
	addq %rdx, %rax
	ret
#ifdef LIELLA_EXPORT
	.size mempcpy, .-mempcpy
	.size __mempcpy, .-__mempcpy
#endif
	.size liella_machine_mempcpy, .-liella_machine_mempcpy

/* void *liella_machine_memset(void *d, int c, size_t n), (unsigned char)c: 16 and more bytes take
 * (unsigned char)c in all 16 bytes of xmm0 (movd, punpcklbw, pshuflw, punpcklqdq: the low byte
 * only, whatever the int's other bytes) and store 16-byte blocks from each end (1, 2, 4 or 8 per end
 * for 16-32, 33-64, 65-128, 129-256; with AVX2 129-256 four 32-byte blocks per end instead); 8-15 two 4-byte words from each end, 4-7 one 4-byte word from each
 * end, 2-3 one 2-byte word from each end, 1 one byte, 0 nothing; other sizes in Rust. */
	.globl liella_machine_memset
	.hidden liella_machine_memset
	.type liella_machine_memset, @function
	.p2align 6
liella_machine_memset:
#ifdef LIELLA_EXPORT
	.globl memset
	.type memset, @function
memset:
#endif
	movq %rdi, %rax
	cmpq liella_machine_fold_limit(%rip), %rdx
	ja .Lset_big
	cmpq $32, %rdx
	ja .Lset_33
	cmpq $16, %rdx
	jb .Lset_lt16
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	pshuflw $0, %xmm0, %xmm0
	punpcklqdq %xmm0, %xmm0
	movups %xmm0, (%rdi)
	movups %xmm0, -16(%rdi,%rdx)
	ret
.Lset_33:
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	pshuflw $0, %xmm0, %xmm0
	punpcklqdq %xmm0, %xmm0
	movups %xmm0, (%rdi)
	movups %xmm0, 16(%rdi)
	movups %xmm0, -32(%rdi,%rdx)
	movups %xmm0, -16(%rdi,%rdx)
	ret
.Lset_lt16:
	movzbl %sil, %ecx
	imull $0x01010101, %ecx, %ecx
	cmpq $8, %rdx
	jb .Lset_lt8
	movl %ecx, (%rdi)
	movl %ecx, 4(%rdi)
	movl %ecx, -8(%rdi,%rdx)
	movl %ecx, -4(%rdi,%rdx)
	ret
.Lset_lt8:
	cmpq $4, %rdx
	jb .Lset_lt4
	movl %ecx, (%rdi)
	movl %ecx, -4(%rdi,%rdx)
	ret
.Lset_lt4:
	cmpq $2, %rdx
	jb .Lset_one
	movw %cx, (%rdi)
	movw %cx, -2(%rdi,%rdx)
	ret
.Lset_one:
	testq %rdx, %rdx
	jz .Lset_none
	movb %cl, (%rdi)
.Lset_none:
	ret
/* Not folded: 65-256 here after the explicit NULL test (one unsigned compare per class: n - 65
 * <= 63 for 65-128, then <= 191 for 129-256; MEMSET-MID: 129-256 straight-line, no loop, as the
 * copy's class); a NULL d, 1-64 without folding and every other size in Rust. The 65-128 class
 * falls through (no compare added to it). */
.Lset_big:
	leaq -65(%rdx), %rcx
	cmpq $63, %rcx
	ja .Lset_129
	testq %rdi, %rdi
	jz liella_hot_fill_rest
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	pshuflw $0, %xmm0, %xmm0
	punpcklqdq %xmm0, %xmm0
	movups %xmm0, (%rdi)
	movups %xmm0, 16(%rdi)
	movups %xmm0, 32(%rdi)
	movups %xmm0, 48(%rdi)
	movups %xmm0, -64(%rdi,%rdx)
	movups %xmm0, -48(%rdi,%rdx)
	movups %xmm0, -32(%rdi,%rdx)
	movups %xmm0, -16(%rdi,%rdx)
	ret
.Lset_129:
	cmpq $191, %rcx
	ja liella_hot_fill_rest
	testq %rdi, %rdi
	jz liella_hot_fill_rest
	cmpb $0, liella_machine_avx2(%rip)
	je .Lset_129_sse2
	vmovd %esi, %xmm0
	vpbroadcastb %xmm0, %ymm0
	vmovdqu %ymm0, (%rdi)
	vmovdqu %ymm0, 32(%rdi)
	vmovdqu %ymm0, 64(%rdi)
	vmovdqu %ymm0, 96(%rdi)
	vmovdqu %ymm0, -128(%rdi,%rdx)
	vmovdqu %ymm0, -96(%rdi,%rdx)
	vmovdqu %ymm0, -64(%rdi,%rdx)
	vmovdqu %ymm0, -32(%rdi,%rdx)
	vzeroupper
	ret
.Lset_129_sse2:
	movd %esi, %xmm0
	punpcklbw %xmm0, %xmm0
	pshuflw $0, %xmm0, %xmm0
	punpcklqdq %xmm0, %xmm0
	movups %xmm0, (%rdi)
	movups %xmm0, 16(%rdi)
	movups %xmm0, 32(%rdi)
	movups %xmm0, 48(%rdi)
	movups %xmm0, 64(%rdi)
	movups %xmm0, 80(%rdi)
	movups %xmm0, 96(%rdi)
	movups %xmm0, 112(%rdi)
	movups %xmm0, -128(%rdi,%rdx)
	movups %xmm0, -112(%rdi,%rdx)
	movups %xmm0, -96(%rdi,%rdx)
	movups %xmm0, -80(%rdi,%rdx)
	movups %xmm0, -64(%rdi,%rdx)
	movups %xmm0, -48(%rdi,%rdx)
	movups %xmm0, -32(%rdi,%rdx)
	movups %xmm0, -16(%rdi,%rdx)
	ret
#ifdef LIELLA_EXPORT
	.size memset, .-memset
#endif
	.size liella_machine_memset, .-liella_machine_memset

/* int liella_machine_compare(const void *a, const void *b, size_t n): the direct compare entry
 * (MEMCMP-FOLD, D-4.23 amendment; owner ruling 2026-10-10 extending the S6 NULL-folding exception to
 * the compares for 8 <= n <= 64; `memcmp` in the products). With folding on (limit 64), 8 to 64
 * bytes take the head probe here: the first 8 bytes of each region in one load each, so a NULL a or
 * b faults on page zero (SIGSEGV) instead of an explicit test. Different heads decide the order
 * here (bswap both, compare as big-endian numbers: -1 or 1, D-4.23). Equal heads continue here
 * (MEMCMP-EQ64): 8-16 bytes the last word of each (bswap order as for the head), 17-32 one 16-byte
 * block from each end (SSE2; the `n > 16` test comes first, so 8-16 pay one size test, and the
 * 17-32 class tests `n > 32` for 33-64 without AVX2; its equal result is the ANDed mask minus 0xffff,
 * 0 when equal, else a negative whose lowest set bit is the last block's first difference:
 * MEMCMP-EQ-24-32), 33-64 one 32-byte block from each end (AVX2; 32 stays on the SSE2
 * blocks: on the AVX2 blocks it took 5 more cycles, MEMCMP-MID-4). Two carry tests on
 * `n - 65` pick the class (SEQ-REGRESS): adding `liella_machine_cmp_wide` (32 only with folding and
 * AVX2 on, else 0) carries for 33-64, which go to the AVX2 blocks with one test (diff's equal lines);
 * adding `liella_machine_cmp_low` next (25 with AVX2, 57 without, 0 with folding off) carries for the
 * other folded sizes (8-32, or 8-64 without AVX2), so the 8-byte class pays two tests, not four. The
 * head probe byte-swaps both words before its compare, so an unequal head orders from its flags.
 * The equality vectors are ANDed and tested once, and an unequal result
 * takes the first differing byte from the blocks' own masks (first block first), re-reads that pair
 * and orders it (-1 or 1): a's byte loaded, b's compared from memory; the last block's position comes
 * from the ANDed mask itself, the blocks before it being equal (MEMCMP-HEAD-2, every class with blocks
 * here, 17-128). Should the re-read pair be equal (a concurrent writer, D-4.21), the call
 * tail-jumps to `liella_hot_compare_rescan` with the index in rcx (the counted rescan from the next
 * byte, as for 65-2048 below; MEMCMP-HEAD-2), and 33-64 without AVX2 take the SSE2 class here
 * (PERF-MEMCMP-NONAVX2; it was a tail-jump to the Rust `liella_hot_compare_tail`): the word at 8
 * (a difference there orders from the two words), then the blocks at 16, n - 32 and n - 16 ANDed into
 * one test, the first differing byte re-read as below. No carry from either add leaves the other sizes: the sign of the
 * sum (`n - 8` with folding on, `n - 65` with it off) is clear only for more than 64 bytes, which
 * reach the 65-128 class here (MEMCMP-MID) when `n <= liella_machine_cmp_mid` (128 with AVX2, else
 * 0: one compare, MEMCMP-MID-3): explicit NULL tests (NULL ->
 * `liella_hot_compare_big`, S6 abort; option B on D-4.22: no fold above 64), then the first 32 bytes
 * as the early probe (sort -C differs within bytes 8-31), then the blocks at 32, n - 64 and n - 32
 * ANDed into one test; the first differing byte (first block first) is re-read and ordered as below
 * (re-read equal, D-4.21 -> `liella_hot_compare_rescan` with the index in rcx: the counted rescan
 * from the next byte, so a reported index that is too low shows in the self-tests; MEMCMP-MID-2). 129 to `liella_machine_cmp_long` (2048 with
 * AVX2, else 0; MEMCMP-MID-4) take the long AVX2 class here: the same NULL tests and 32-byte head
 * block, then 128-byte chunks from byte 32 (four blocks ANDed, one test; a cursor in a and the
 * distance b - a, so every compare folds a plain address) and a last chunk at n - 128 that may
 * overlap the one before; a differing chunk's four masks give the first differing byte, re-read and
 * ordered as above. Without AVX2, 65-128 and everything above take the SSE2 classes at
 * .Lcmp_sse_mid and .Lcmp_sse_long (PERF-MEMCMP-NONAVX2, PERF-MEMCMP-LONG-NONAVX2). Above that word
 * with AVX2, and for a NULL a or b, tail-jump to `liella_hot_compare_big` (the checked classes above
 * 64, no folding decision to take).
 * A set sign means n <= 64: the 0-7 byte classes (MEMCMP-SHORT; no fold: explicit NULL tests first, a
 * NULL a or b tail-jumps to the checked entry for the S6 abort): 4-7 one 4-byte word from each end
 * (after an `n < 8` test, since with folding off 8-64 arrive here too and go to
 * `liella_hot_compare_rest`, the checked entry with the explicit S6 NULL tests), 2-3 one 2-byte word
 * from each end (compared from memory; an unequal word is re-read from b and ordered by bswap; re-read
 * equal (D-4.21) -> the checked entry), 1 the byte, 0 returns 0. Reads only [a, a + n) and [b, b + n). */
	.hidden liella_hot_compare_rest  /* crates/boundary/src/posix/s04_strings.rs */
	.hidden liella_hot_compare_tail
	.hidden liella_hot_compare_big
	.hidden liella_hot_compare_rescan
	.globl liella_machine_compare
	.hidden liella_machine_compare
	.type liella_machine_compare, @function
	.p2align 6
liella_machine_compare:
#ifdef LIELLA_EXPORT
	.globl memcmp
	.type memcmp, @function
memcmp:
#endif
	leaq -65(%rdx), %rax
	addq liella_machine_cmp_wide(%rip), %rax
	jc .Lcmp_wide
	addq liella_machine_cmp_low(%rip), %rax
	jnc .Lcmp_other
	movq (%rdi), %rax
	movq (%rsi), %rcx
	bswapq %rax
	bswapq %rcx
	cmpq %rcx, %rax
	jne .Lcmp_sign
	cmpq $16, %rdx
	ja .Lcmp_17
	movq -8(%rdi,%rdx), %rax
	movq -8(%rsi,%rdx), %rcx
	cmpq %rcx, %rax
	jne .Lcmp_order
	xorl %eax, %eax
	ret
.Lcmp_17:
	cmpq $32, %rdx
	ja .Lcmp_33
	movdqu (%rdi), %xmm0
	movdqu (%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu -16(%rdi,%rdx), %xmm1
	movdqu -16(%rsi,%rdx), %xmm2
	pcmpeqb %xmm2, %xmm1
	pand %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	subl $0xffff, %eax
	jne .Lcmp_diff16
	ret
.Lcmp_diff16:
	pmovmskb %xmm0, %ecx
	xorl $0xffff, %ecx
	jnz .Lcmp_first
	bsfl %eax, %ecx
	leaq -16(%rdx,%rcx), %rcx
	jmp .Lcmp_byte
.Lcmp_33:
	movq 8(%rdi), %rax
	movq 8(%rsi), %rcx
	cmpq %rcx, %rax
	jne .Lcmp_order
	movdqu 16(%rdi), %xmm0
	movdqu 16(%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu -32(%rdi,%rdx), %xmm1
	movdqu -32(%rsi,%rdx), %xmm2
	pcmpeqb %xmm2, %xmm1
	movdqu -16(%rdi,%rdx), %xmm2
	movdqu -16(%rsi,%rdx), %xmm3
	pcmpeqb %xmm3, %xmm2
	pand %xmm0, %xmm2
	pand %xmm1, %xmm2
	pmovmskb %xmm2, %eax
	subl $0xffff, %eax
	jne .Lcmp_diff33
	ret
.Lcmp_diff33:
	pmovmskb %xmm0, %ecx
	movl $16, %r8d
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	pmovmskb %xmm1, %ecx
	leaq -32(%rdx), %r8
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	leaq -16(%rdx), %r8
	movl %eax, %ecx
.Lcmp_at:
	bsfl %ecx, %ecx
	addq %r8, %rcx
	jmp .Lcmp_byte
.Lcmp_wide:
	movq (%rdi), %rax
	movq (%rsi), %rcx
	cmpq %rcx, %rax
	jne .Lcmp_order
	vmovdqu (%rdi), %ymm0
	vmovdqu -32(%rdi,%rdx), %ymm1
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpcmpeqb -32(%rsi,%rdx), %ymm1, %ymm1
	vpand %ymm1, %ymm0, %ymm2
	vpmovmskb %ymm2, %eax
	incl %eax
	jne .Lcmp_diff32
	vzeroupper
	ret
.Lcmp_diff32:
	vpmovmskb %ymm0, %ecx
	vzeroupper
	incl %ecx
	jnz .Lcmp_first
	bsfl %eax, %ecx
	leaq -32(%rdx,%rcx), %rcx
	jmp .Lcmp_byte
.Lcmp_first:
	bsfl %ecx, %ecx
.Lcmp_byte:
	movzbl (%rdi,%rcx), %eax
	cmpb (%rsi,%rcx), %al
	je liella_hot_compare_rescan
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_order:
	bswapq %rax
	bswapq %rcx
	cmpq %rcx, %rax
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_other:
	jns .Lcmp_big
	testq %rdi, %rdi
	je liella_hot_compare_rest
	testq %rsi, %rsi
	je liella_hot_compare_rest
	cmpq $4, %rdx
	jae .Lcmp_4
	cmpl $2, %edx
	jb .Lcmp_01
	movzwl (%rdi), %eax
	cmpw (%rsi), %ax
	jne .Lcmp_h2
	movzwl -2(%rdi,%rdx), %eax
	cmpw -2(%rsi,%rdx), %ax
	jne .Lcmp_t2
	xorl %eax, %eax
	ret
.Lcmp_4:
	cmpq $8, %rdx
	jae liella_hot_compare_rest
	movl (%rdi), %eax
	cmpl (%rsi), %eax
	jne .Lcmp_h4
	movl -4(%rdi,%rdx), %eax
	cmpl -4(%rsi,%rdx), %eax
	jne .Lcmp_t4
	xorl %eax, %eax
	ret
.Lcmp_01:
	testl %edx, %edx
	je .Lcmp_none
	movzbl (%rdi), %eax
	cmpb (%rsi), %al
	jne .Lcmp_sign
.Lcmp_none:
	xorl %eax, %eax
	ret
.Lcmp_h2:
	movzwl (%rsi), %ecx
	bswapl %eax
	bswapl %ecx
	cmpl %ecx, %eax
	je liella_hot_compare_rest
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_t2:
	movzwl -2(%rsi,%rdx), %ecx
	bswapl %eax
	bswapl %ecx
	cmpl %ecx, %eax
	je liella_hot_compare_rest
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_h4:
	movl (%rsi), %ecx
	bswapl %eax
	bswapl %ecx
	cmpl %ecx, %eax
	je liella_hot_compare_rest
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_t4:
	movl -4(%rsi,%rdx), %ecx
	bswapl %eax
	bswapl %ecx
	cmpl %ecx, %eax
	je liella_hot_compare_rest
.Lcmp_sign:
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_big:
	cmpq liella_machine_cmp_mid(%rip), %rdx
	ja .Lcmp_long
	testq %rdi, %rdi
	je liella_hot_compare_big
	testq %rsi, %rsi
	je liella_hot_compare_big
	vmovdqu (%rdi), %ymm0
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	incl %ecx
	jne .Lcmp_big_head
.Lcmp_big_blocks:
	vmovdqu 32(%rdi), %ymm1
	vmovdqu -64(%rdi,%rdx), %ymm2
	vmovdqu -32(%rdi,%rdx), %ymm3
	vpcmpeqb 32(%rsi), %ymm1, %ymm1
	vpcmpeqb -64(%rsi,%rdx), %ymm2, %ymm2
	vpcmpeqb -32(%rsi,%rdx), %ymm3, %ymm3
	vpand %ymm2, %ymm1, %ymm0
	vpand %ymm3, %ymm0, %ymm0
	vpmovmskb %ymm0, %eax
	incl %eax
	jne .Lcmp_big_diff
	vzeroupper
	ret
.Lcmp_big_diff:
	vpmovmskb %ymm1, %ecx
	vpmovmskb %ymm2, %r8d
	vzeroupper
	incl %ecx
	jne .Lcmp_big_32
	incl %r8d
	jne .Lcmp_big_mid
	bsfl %eax, %ecx
	leaq -32(%rdx,%rcx), %rcx
	jmp .Lcmp_big_byte
.Lcmp_big_mid:
	bsfl %r8d, %ecx
	leaq -64(%rdx,%rcx), %rcx
	jmp .Lcmp_big_byte
.Lcmp_big_32:
	bsfl %ecx, %ecx
	addq $32, %rcx
	jmp .Lcmp_big_byte
.Lcmp_big_head:
	vzeroupper
	bsfl %ecx, %ecx
.Lcmp_big_byte:
	movzbl (%rdi,%rcx), %eax
	cmpb (%rsi,%rcx), %al
	je liella_hot_compare_rescan
	sbbl %eax, %eax
	orl $1, %eax
	ret
.Lcmp_long:
	cmpq liella_machine_cmp_long(%rip), %rdx
	ja .Lcmp_sse_mid
	testq %rdi, %rdi
	je liella_hot_compare_big
	testq %rsi, %rsi
	je liella_hot_compare_big
	vmovdqu (%rdi), %ymm0
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	incl %ecx
	jne .Lcmp_big_head
	cmpq $128, %rdx
	jbe .Lcmp_big_blocks
	movq %rsi, %r11
	subq %rdi, %r11
	leaq 32(%rdi), %r9
	leaq -128(%rdi,%rdx), %r8
	cmpq %r8, %r9
	jae .Lcmp_long_last
.Lcmp_long_loop:
	vmovdqu (%r9,%r11), %ymm1
	vmovdqu 32(%r9,%r11), %ymm2
	vmovdqu 64(%r9,%r11), %ymm3
	vmovdqu 96(%r9,%r11), %ymm4
	vpcmpeqb (%r9), %ymm1, %ymm1
	vpcmpeqb 32(%r9), %ymm2, %ymm2
	vpcmpeqb 64(%r9), %ymm3, %ymm3
	vpcmpeqb 96(%r9), %ymm4, %ymm4
	vpand %ymm2, %ymm1, %ymm5
	vpand %ymm4, %ymm3, %ymm0
	vpand %ymm0, %ymm5, %ymm5
	vpmovmskb %ymm5, %eax
	incl %eax
	jne .Lcmp_long_diff
	subq $-128, %r9
	cmpq %r8, %r9
	jb .Lcmp_long_loop
.Lcmp_long_last:
	movq %r8, %r9
	vmovdqu (%r9,%r11), %ymm1
	vmovdqu 32(%r9,%r11), %ymm2
	vmovdqu 64(%r9,%r11), %ymm3
	vmovdqu 96(%r9,%r11), %ymm4
	vpcmpeqb (%r9), %ymm1, %ymm1
	vpcmpeqb 32(%r9), %ymm2, %ymm2
	vpcmpeqb 64(%r9), %ymm3, %ymm3
	vpcmpeqb 96(%r9), %ymm4, %ymm4
	vpand %ymm2, %ymm1, %ymm5
	vpand %ymm4, %ymm3, %ymm0
	vpand %ymm0, %ymm5, %ymm5
	vpmovmskb %ymm5, %eax
	incl %eax
	jne .Lcmp_long_diff
	vzeroupper
	xorl %eax, %eax
	ret
.Lcmp_long_diff:
	vpmovmskb %ymm1, %eax
	vpmovmskb %ymm2, %ecx
	vpmovmskb %ymm3, %r10d
	vpmovmskb %ymm4, %r8d
	vzeroupper
	subq %rdi, %r9
	shlq $32, %rcx
	orq %rcx, %rax
	incq %rax
	jne .Lcmp_long_at
	shlq $32, %r8
	orq %r8, %r10
	incq %r10
	movq %r10, %rax
	addq $64, %r9
.Lcmp_long_at:
	bsfq %rax, %rcx
	addq %r9, %rcx
	jmp .Lcmp_big_byte
/* 65-128 without AVX2 (PERF-MEMCMP-NONAVX2; cmp_mid is 0 only then, so on an AVX2 build only sizes
 * above cmp_long pay this test and .Lcmp_sse_long's cmp_mid test): the explicit NULL tests, the two blocks at 0 and 16 as the early
 * probe (as the AVX2 class's 32-byte head), then the blocks at 32, 48, n - 64, n - 48, n - 32 and
 * n - 16 ANDed into one test; the first differing byte (first block first; the last block's from the
 * ANDed mask, the blocks before it being equal) is re-read and ordered at .Lcmp_byte. */
.Lcmp_sse_mid:
	cmpq $128, %rdx
	ja .Lcmp_sse_long
	testq %rdi, %rdi
	je liella_hot_compare_big
	testq %rsi, %rsi
	je liella_hot_compare_big
	movdqu (%rdi), %xmm0
	movdqu (%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu 16(%rdi), %xmm1
	movdqu 16(%rsi), %xmm2
	pcmpeqb %xmm2, %xmm1
	pand %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	subl $0xffff, %eax
	jne .Lcmp_sse_head
	movdqu 32(%rdi), %xmm0
	movdqu 32(%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu 48(%rdi), %xmm1
	movdqu 48(%rsi), %xmm2
	pcmpeqb %xmm2, %xmm1
	movdqa %xmm0, %xmm7
	pand %xmm1, %xmm7
	movdqu -64(%rdi,%rdx), %xmm2
	movdqu -64(%rsi,%rdx), %xmm3
	pcmpeqb %xmm3, %xmm2
	pand %xmm2, %xmm7
	movdqu -48(%rdi,%rdx), %xmm3
	movdqu -48(%rsi,%rdx), %xmm4
	pcmpeqb %xmm4, %xmm3
	pand %xmm3, %xmm7
	movdqu -32(%rdi,%rdx), %xmm4
	movdqu -32(%rsi,%rdx), %xmm5
	pcmpeqb %xmm5, %xmm4
	pand %xmm4, %xmm7
	movdqu -16(%rdi,%rdx), %xmm5
	movdqu -16(%rsi,%rdx), %xmm6
	pcmpeqb %xmm6, %xmm5
	pand %xmm5, %xmm7
	pmovmskb %xmm7, %eax
	subl $0xffff, %eax
	jne .Lcmp_sse_diff
	ret
.Lcmp_sse_diff:
	pmovmskb %xmm0, %ecx
	movl $32, %r8d
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	pmovmskb %xmm1, %ecx
	movl $48, %r8d
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	pmovmskb %xmm2, %ecx
	leaq -64(%rdx), %r8
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	pmovmskb %xmm3, %ecx
	leaq -48(%rdx), %r8
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	pmovmskb %xmm4, %ecx
	leaq -32(%rdx), %r8
	xorl $0xffff, %ecx
	jnz .Lcmp_at
	leaq -16(%rdx), %r8
	movl %eax, %ecx
	jmp .Lcmp_at
.Lcmp_sse_head:
	pmovmskb %xmm0, %ecx
	xorl $0xffff, %ecx
	jnz .Lcmp_first
	movl $16, %r8d
	movl %eax, %ecx
	jmp .Lcmp_at
/* Above 128 bytes without AVX2 (PERF-MEMCMP-LONG-NONAVX2; it was a tail-jump to the Rust
 * `liella_hot_compare_big`): an AVX2 build reaches here only above cmp_long (its cmp_mid is not 0)
 * and keeps the Rust large class. Otherwise the explicit NULL tests, the blocks at 0 and 16 as the
 * early probe (as the 65-128 class), then 64-byte chunks of four 16-byte blocks ANDed into one
 * test: from a + 32 rounded down to 16 (a aligned, so a's side is compared from memory; the first
 * chunk may overlap the probe), a cursor in a and the distance b - a, while the cursor is below
 * a + n - 64, then a last chunk at n - 64 that may overlap the one before (unaligned, both sides
 * loaded). A differing chunk's four masks give the first differing byte (rcx), re-read and ordered
 * at .Lcmp_big_byte (re-read equal, D-4.21 -> `liella_hot_compare_rescan`). n is read from rdx
 * only (MEMCMP-MID-4's torn-read bound). */
.Lcmp_sse_long:
	cmpq $0, liella_machine_cmp_mid(%rip)
	jne liella_hot_compare_big
	testq %rdi, %rdi
	je liella_hot_compare_big
	testq %rsi, %rsi
	je liella_hot_compare_big
	movdqu (%rdi), %xmm0
	movdqu (%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu 16(%rdi), %xmm1
	movdqu 16(%rsi), %xmm2
	pcmpeqb %xmm2, %xmm1
	pand %xmm0, %xmm1
	pmovmskb %xmm1, %eax
	subl $0xffff, %eax
	jne .Lcmp_sse_head
	movq %rsi, %r11
	subq %rdi, %r11
	leaq 32(%rdi), %r9
	andq $-16, %r9
	leaq -64(%rdi,%rdx), %r8
.Lcmp_sse_loop:
	movdqu (%r9,%r11), %xmm0
	movdqu 16(%r9,%r11), %xmm1
	movdqu 32(%r9,%r11), %xmm2
	movdqu 48(%r9,%r11), %xmm3
	pcmpeqb (%r9), %xmm0
	pcmpeqb 16(%r9), %xmm1
	pcmpeqb 32(%r9), %xmm2
	pcmpeqb 48(%r9), %xmm3
	movdqa %xmm0, %xmm4
	pand %xmm1, %xmm4
	pand %xmm2, %xmm4
	pand %xmm3, %xmm4
	pmovmskb %xmm4, %eax
	cmpl $0xffff, %eax
	jne .Lcmp_sse_ldiff
	addq $64, %r9
	cmpq %r8, %r9
	jb .Lcmp_sse_loop
	movq %r8, %r9
	movdqu (%r9), %xmm0
	movdqu (%r9,%r11), %xmm4
	pcmpeqb %xmm4, %xmm0
	movdqu 16(%r9), %xmm1
	movdqu 16(%r9,%r11), %xmm4
	pcmpeqb %xmm4, %xmm1
	movdqu 32(%r9), %xmm2
	movdqu 32(%r9,%r11), %xmm4
	pcmpeqb %xmm4, %xmm2
	movdqu 48(%r9), %xmm3
	movdqu 48(%r9,%r11), %xmm4
	pcmpeqb %xmm4, %xmm3
	movdqa %xmm0, %xmm4
	pand %xmm1, %xmm4
	pand %xmm2, %xmm4
	pand %xmm3, %xmm4
	pmovmskb %xmm4, %eax
	cmpl $0xffff, %eax
	jne .Lcmp_sse_ldiff
	xorl %eax, %eax
	ret
.Lcmp_sse_ldiff:
	pmovmskb %xmm0, %eax
	pmovmskb %xmm1, %ecx
	pmovmskb %xmm2, %r10d
	pmovmskb %xmm3, %r8d
	shll $16, %ecx
	orl %ecx, %eax
	shll $16, %r8d
	orl %r8d, %r10d
	shlq $32, %r10
	orq %r10, %rax
	notq %rax
	bsfq %rax, %rcx
	subq %rdi, %r9
	addq %r9, %rcx
	jmp .Lcmp_big_byte
#ifdef LIELLA_EXPORT
	.size memcmp, .-memcmp
#endif
	.size liella_machine_compare, .-liella_machine_compare

/* int liella_machine_equal(const void *a, const void *b, size_t n): the direct equality entry
 * (MEMCMP-FOLD, D-4.41 amendment; `bcmp` and `__memcmpeq` in the products): 0 when equal, else 1.
 * The same gates and head probe as the compare entry: different heads return 1 here, equal heads
 * take the same classes as the compare entry (last word, two 16-byte or two 32-byte blocks; 0 or 1
 * from one test, no order); 33-64 without AVX2 tail-jumps to `liella_hot_equal_tail` (Rust), more
 * than 64 bytes (sign clear) take the compare entry's 65-128 AVX2 class (MEMCMP-MID: NULL tests, the
 * 32-byte head block first, then three blocks in one test; 0 or 1) and its 129-2048 class with the
 * last chunk first (an early reject where only the tail differs; MEMCMP-MID-4), then the chunks from
 * byte 32 (0 or 1, nothing located), else `liella_hot_equal_big`;
 * 8-64 with folding off to `liella_hot_equal_rest`
 * (the checked entry, after the 4-7 class's `n < 8` test); 0 to 7 bytes (MEMCMP-SHORT): explicit NULL
 * tests (NULL -> the checked entry), then both end words XORed and ORed into one test (4-7: 4-byte
 * words, 2-3: 2-byte words; 1 the byte). The head probe is the early reject
 * (most unequal pairs differ in their first 8 bytes); proving equality reads every byte once.
 * Reads only [a, a + n) and [b, b + n). */
	.hidden liella_hot_equal_rest
	.hidden liella_hot_equal_tail
	.hidden liella_hot_equal_big
	.section .text.liella_machine_equal,"ax",@progbits
	.globl liella_machine_equal
	.hidden liella_machine_equal
	.type liella_machine_equal, @function
	.p2align 6
liella_machine_equal:
#ifdef LIELLA_EXPORT
	.globl bcmp
	.type bcmp, @function
bcmp:
	.globl __memcmpeq
	.type __memcmpeq, @function
__memcmpeq:
#endif
	leaq -65(%rdx), %rax
	addq liella_machine_cmp_wide(%rip), %rax
	jc .Leq_wide
	addq liella_machine_cmp_low(%rip), %rax
	jnc .Leq_other
	movq (%rdi), %rax
	cmpq (%rsi), %rax
	jne .Leq_one
	cmpq $16, %rdx
	ja .Leq_17
	movq -8(%rdi,%rdx), %rcx
	xorl %eax, %eax
	cmpq -8(%rsi,%rdx), %rcx
	setne %al
	ret
.Leq_17:
	cmpq $32, %rdx
	ja liella_hot_equal_tail
	movdqu (%rdi), %xmm0
	movdqu (%rsi), %xmm1
	pcmpeqb %xmm1, %xmm0
	movdqu -16(%rdi,%rdx), %xmm1
	movdqu -16(%rsi,%rdx), %xmm2
	pcmpeqb %xmm2, %xmm1
	pand %xmm1, %xmm0
	pmovmskb %xmm0, %eax
	subl $0xffff, %eax
	shrl $31, %eax
	ret
.Leq_wide:
	movq (%rdi), %rax
	cmpq (%rsi), %rax
	jne .Leq_one
	vmovdqu (%rdi), %ymm0
	vmovdqu -32(%rdi,%rdx), %ymm1
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpcmpeqb -32(%rsi,%rdx), %ymm1, %ymm1
	vpand %ymm1, %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	vzeroupper
	xorl %eax, %eax
	incl %ecx
	setne %al
	ret
.Leq_one:
	movl $1, %eax
	ret
.Leq_other:
	jns .Leq_big
	testq %rdi, %rdi
	je liella_hot_equal_rest
	testq %rsi, %rsi
	je liella_hot_equal_rest
	cmpq $4, %rdx
	jae .Leq_4
	cmpl $2, %edx
	jb .Leq_01
	movzwl (%rdi), %ecx
	movzwl -2(%rdi,%rdx), %r8d
	xorw (%rsi), %cx
	xorw -2(%rsi,%rdx), %r8w
	xorl %eax, %eax
	orw %r8w, %cx
	setne %al
	ret
.Leq_4:
	cmpq $8, %rdx
	jae liella_hot_equal_rest
	movl (%rdi), %ecx
	movl -4(%rdi,%rdx), %r8d
	xorl (%rsi), %ecx
	xorl -4(%rsi,%rdx), %r8d
	xorl %eax, %eax
	orl %r8d, %ecx
	setne %al
	ret
.Leq_01:
	xorl %eax, %eax
	testl %edx, %edx
	je .Leq_none
	movzbl (%rdi), %ecx
	cmpb (%rsi), %cl
	setne %al
.Leq_none:
	ret
.Leq_big:
	cmpq liella_machine_cmp_mid(%rip), %rdx
	ja .Leq_long
	testq %rdi, %rdi
	je liella_hot_equal_big
	testq %rsi, %rsi
	je liella_hot_equal_big
	vmovdqu (%rdi), %ymm0
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	incl %ecx
	jne .Leq_big_one
.Leq_big_blocks:
	vmovdqu 32(%rdi), %ymm1
	vmovdqu -64(%rdi,%rdx), %ymm2
	vmovdqu -32(%rdi,%rdx), %ymm3
	vpcmpeqb 32(%rsi), %ymm1, %ymm1
	vpcmpeqb -64(%rsi,%rdx), %ymm2, %ymm2
	vpcmpeqb -32(%rsi,%rdx), %ymm3, %ymm3
	vpand %ymm2, %ymm1, %ymm1
	vpand %ymm3, %ymm1, %ymm1
	vpmovmskb %ymm1, %ecx
	vzeroupper
	xorl %eax, %eax
	incl %ecx
	setne %al
	ret
.Leq_big_one:
	vzeroupper
	movl $1, %eax
	ret
.Leq_long:
	cmpq liella_machine_cmp_long(%rip), %rdx
	ja liella_hot_equal_big
	testq %rdi, %rdi
	je liella_hot_equal_big
	testq %rsi, %rsi
	je liella_hot_equal_big
	vmovdqu (%rdi), %ymm0
	vpcmpeqb (%rsi), %ymm0, %ymm0
	vpmovmskb %ymm0, %ecx
	incl %ecx
	jne .Leq_big_one
	cmpq $128, %rdx
	jbe .Leq_big_blocks
	movq %rsi, %r11
	subq %rdi, %r11
	leaq -128(%rdi,%rdx), %r8
	vmovdqu (%r8,%r11), %ymm1
	vmovdqu 32(%r8,%r11), %ymm2
	vmovdqu 64(%r8,%r11), %ymm3
	vmovdqu 96(%r8,%r11), %ymm4
	vpcmpeqb (%r8), %ymm1, %ymm1
	vpcmpeqb 32(%r8), %ymm2, %ymm2
	vpcmpeqb 64(%r8), %ymm3, %ymm3
	vpcmpeqb 96(%r8), %ymm4, %ymm4
	vpand %ymm2, %ymm1, %ymm1
	vpand %ymm4, %ymm3, %ymm3
	vpand %ymm3, %ymm1, %ymm1
	vpmovmskb %ymm1, %ecx
	incl %ecx
	jne .Leq_big_one
	leaq 32(%rdi), %r9
	cmpq %r8, %r9
	jae .Leq_long_zero
.Leq_long_loop:
	vmovdqu (%r9,%r11), %ymm1
	vmovdqu 32(%r9,%r11), %ymm2
	vmovdqu 64(%r9,%r11), %ymm3
	vmovdqu 96(%r9,%r11), %ymm4
	vpcmpeqb (%r9), %ymm1, %ymm1
	vpcmpeqb 32(%r9), %ymm2, %ymm2
	vpcmpeqb 64(%r9), %ymm3, %ymm3
	vpcmpeqb 96(%r9), %ymm4, %ymm4
	vpand %ymm2, %ymm1, %ymm1
	vpand %ymm4, %ymm3, %ymm3
	vpand %ymm3, %ymm1, %ymm1
	vpmovmskb %ymm1, %ecx
	incl %ecx
	jne .Leq_big_one
	subq $-128, %r9
	cmpq %r8, %r9
	jb .Leq_long_loop
.Leq_long_zero:
	vzeroupper
	xorl %eax, %eax
	ret
#ifdef LIELLA_EXPORT
	.size __memcmpeq, .-__memcmpeq
	.size bcmp, .-bcmp
#endif
	.size liella_machine_equal, .-liella_machine_equal
	.text

/* The fold limit: 64 while NULL folding is on, 0 otherwise (crates/boundary/src/dispatch.rs,
 * through `ffi::liella_machine_fold_limit`), and the AVX2 flag byte of the kernels (D-4.25: 1 only
 * when the CPU has AVX2 and the OS saves the YMM state; `ffi::liella_machine_avx2`), and the
 * compare entries' class words (SEQ-REGRESS; `ffi::liella_machine_cmp_wide`/`cmp_low`: wide 32 only
 * while both are on, low 25 with both, 57 with folding alone, 0 with folding off; their sum never
 * exceeds 57, so `n - 65` plus both carries only for 8-64). They share one 64-byte line
 * with nothing else: all are read by the calls and written only by the startup decisions, so no
 * often-written neighbour may share it (64 bytes per process). */
	.bss
	.globl liella_machine_fold_limit
	.hidden liella_machine_fold_limit
	.type liella_machine_fold_limit, @object
	.p2align 6
liella_machine_fold_limit:
	.zero 8
	.size liella_machine_fold_limit, 8
	.globl liella_machine_avx2
	.hidden liella_machine_avx2
	.type liella_machine_avx2, @object
liella_machine_avx2:
	.zero 8
	.size liella_machine_avx2, 1
	.globl liella_machine_cmp_wide
	.hidden liella_machine_cmp_wide
	.type liella_machine_cmp_wide, @object
liella_machine_cmp_wide:
	.zero 8
	.size liella_machine_cmp_wide, 8
	.globl liella_machine_cmp_low
	.hidden liella_machine_cmp_low
	.type liella_machine_cmp_low, @object
liella_machine_cmp_low:
	.zero 8
	.size liella_machine_cmp_low, 8
/* The memchr entry's gate (MEMCHR-FOLD, D-4.48 amendment; machine/scan.S): isize::MAX while NULL
 * folding is on, 0 otherwise; written with the fold limit by the same startup decision. The six
 * words (fold limit, avx2, cmp wide, cmp low, chr limit, cmp mid, cmp long) share one 64-byte line;
 * the padding completes it. */
	.globl liella_machine_chr_limit
	.hidden liella_machine_chr_limit
	.type liella_machine_chr_limit, @object
liella_machine_chr_limit:
	.zero 8
	.size liella_machine_chr_limit, 8
/* The compare entries' 65-128 class word (MEMCMP-MID-3; `ffi::liella_machine_cmp_mid`): 128 while
 * the AVX2 builds run, else 0 (written by `crate::dispatch::select`, never above 64 while the AVX2
 * flag is false). Sizes above 64 with `n > word` tail-jump to the checked entries, so one compare
 * replaces the range test and the flag test. */
	.globl liella_machine_cmp_mid
	.hidden liella_machine_cmp_mid
	.type liella_machine_cmp_mid, @object
liella_machine_cmp_mid:
	.zero 8
	.size liella_machine_cmp_mid, 8
/* The compare entries' 129-2048 class word (MEMCMP-MID-4; `ffi::liella_machine_cmp_long`): 2048
 * (`hot::CMP_LONG`) while the AVX2 builds run, else 0 (written by `crate::dispatch::select` with the
 * 65-128 word, never nonzero while the AVX2 flag is false). Read only by sizes above the 65-128
 * word: `n > word` tail-jumps to the checked entries. The class re-tests `n <= 128` itself, so a
 * call that sees the two words mid-update (65-128 word 0, this one 2048) still runs a class that
 * covers its size; the 65-128 sizes pay nothing for this word. */
	.globl liella_machine_cmp_long
	.hidden liella_machine_cmp_long
	.type liella_machine_cmp_long, @object
liella_machine_cmp_long:
	.zero 16
	.size liella_machine_cmp_long, 8

	.section .note.GNU-stack,"",@progbits
