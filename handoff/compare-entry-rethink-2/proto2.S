/* COMPARE-ENTRY-RETHINK-2 prototype: per-ISA compare entries, no runtime ISA test in any entry.
 * Written from the psABI, the Intel SDM and the optimisation manual only (no libc source).
 *   p_memcmp_sse2  / p_memeq_sse2  : SSE2 entry (any x86-64): small first, then head-first 16-byte blocks
 *   p_memcmp_avx2  / p_memeq_avx2  : AVX2 entry, order A: small first; >= 17: 17-32 (taken), 33-64 falls through
 *   p_memcmp_avx2b / p_memeq_avx2b : AVX2 entry, order B: 33..2048 span test first (1 taken), then small
 *   p_memcmp_ptr   / p_memeq_ptr   : `jmp *slot`, the cost of a static IFUNC (.iplt) or a RELRO entry pointer
 * Words (data, not code pointers): p_head = 17 when NULL folding is on (else SIZE_MAX: never in this harness),
 * p_span = 2016 (33..2048) folded, else 0. memeq returns exactly 0 or 1. n 1-7 keep explicit NULL tests. */
        .data
        .p2align 3
        .globl  p_head, p_span, p_memcmp_slot, p_memeq_slot
p_head:         .quad 17
p_span:         .quad 2016
p_memcmp_slot:  .quad p_memcmp_sse2
p_memeq_slot:   .quad p_memeq_sse2

        .text
/* ---------------------------------------------------------------- shared pieces (macros) */
.macro SMALL_CMP p
        cmpq    $8, %rdx
        jb      \p\()_lt8
        movq    (%rdi), %rax
        movq    (%rsi), %rcx
        cmpq    %rcx, %rax
        jne     \p\()_wd
        movq    -8(%rdi,%rdx), %rax
        movq    -8(%rsi,%rdx), %rcx
        cmpq    %rcx, %rax
        jne     \p\()_wd
        xorl    %eax, %eax
        ret
\p\()_wd:
        bswapq  %rax
        bswapq  %rcx
        cmpq    %rcx, %rax
        sbbl    %eax, %eax
        orl     $1, %eax
        ret
\p\()_lt8:
        testq   %rdx, %rdx
        jz      \p\()_z
        testq   %rdi, %rdi
        jz      p_null_abort
        testq   %rsi, %rsi
        jz      p_null_abort
        cmpq    $4, %rdx
        jb      \p\()_lt4
        movl    (%rdi), %eax
        movl    -4(%rdi,%rdx), %r8d
        movl    (%rsi), %ecx
        movl    -4(%rsi,%rdx), %r9d
        shlq    $32, %r8
        shlq    $32, %r9
        orq     %r8, %rax
        orq     %r9, %rcx
        bswapq  %rax
        bswapq  %rcx
        xorl    %r10d, %r10d
        cmpq    %rcx, %rax
        seta    %r10b
        sbbl    %eax, %eax
        orl     %r10d, %eax
        ret
\p\()_lt4:
        movzbl  (%rdi), %eax
        movzbl  (%rsi), %ecx
        movq    %rdx, %r8
        shrq    $1, %r8
        shll    $16, %eax
        shll    $16, %ecx
        movzbl  (%rdi,%r8), %r9d
        movzbl  (%rsi,%r8), %r10d
        shll    $8, %r9d
        shll    $8, %r10d
        orl     %r9d, %eax
        orl     %r10d, %ecx
        movzbl  -1(%rdi,%rdx), %r9d
        movzbl  -1(%rsi,%rdx), %r10d
        orl     %r9d, %eax
        orl     %r10d, %ecx
        subl    %ecx, %eax
        ret
\p\()_z:
        xorl    %eax, %eax
        ret
.endm

.macro SMALL_EQ p
        cmpq    $8, %rdx
        jb      \p\()_lt8
        movq    (%rdi), %rax
        movq    -8(%rdi,%rdx), %rcx
        xorq    (%rsi), %rax
        xorq    -8(%rsi,%rdx), %rcx
        orq     %rcx, %rax
        setne   %al
        movzbl  %al, %eax
        ret
\p\()_lt8:
        testq   %rdx, %rdx
        jz      \p\()_z
        testq   %rdi, %rdi
        jz      p_null_abort
        testq   %rsi, %rsi
        jz      p_null_abort
        cmpq    $4, %rdx
        jb      \p\()_lt4
        movl    (%rdi), %eax
        movl    -4(%rdi,%rdx), %ecx
        xorl    (%rsi), %eax
        xorl    -4(%rsi,%rdx), %ecx
        orl     %ecx, %eax
        setne   %al
        movzbl  %al, %eax
        ret
\p\()_lt4:
        movzbl  (%rdi), %eax
        movzbl  -1(%rdi,%rdx), %ecx
        xorb    (%rsi), %al
        xorb    -1(%rsi,%rdx), %cl
        orl     %ecx, %eax
        movq    %rdx, %r8
        shrq    $1, %r8
        movzbl  (%rdi,%r8), %ecx
        xorb    (%rsi,%r8), %cl
        orl     %ecx, %eax
        setne   %al
        movzbl  %al, %eax
        ret
\p\()_z:
        xorl    %eax, %eax
        ret
.endm

/* 64 bytes at offset \r: xmm0 = AND of the four equality masks, eax = its byte mask */
.macro B64S r
        movdqu  (%rdi,\r), %xmm0
        movdqu  (%rsi,\r), %xmm1
        pcmpeqb %xmm1, %xmm0
        movdqu  16(%rdi,\r), %xmm2
        movdqu  16(%rsi,\r), %xmm3
        pcmpeqb %xmm3, %xmm2
        movdqu  32(%rdi,\r), %xmm4
        movdqu  32(%rsi,\r), %xmm5
        pcmpeqb %xmm5, %xmm4
        movdqu  48(%rdi,\r), %xmm6
        movdqu  48(%rsi,\r), %xmm7
        pcmpeqb %xmm7, %xmm6
        pand    %xmm2, %xmm0
        pand    %xmm6, %xmm4
        pand    %xmm4, %xmm0
        pmovmskb %xmm0, %eax
.endm

/* 64 bytes at offset \r (AVX2): ymm1, ymm2 = the two masks, eax = mask of their AND */
.macro B64A r
        vmovdqu (%rdi,\r), %ymm1
        vpcmpeqb (%rsi,\r), %ymm1, %ymm1
        vmovdqu 32(%rdi,\r), %ymm2
        vpcmpeqb 32(%rsi,\r), %ymm2, %ymm2
        vpand   %ymm2, %ymm1, %ymm0
        vpmovmskb %ymm0, %eax
.endm

/* ---------------------------------------------------------------- SSE2 ordered entry */
        .p2align 6
        .globl  p_memcmp_sse2
        .type   p_memcmp_sse2, @function
p_memcmp_sse2:
        cmpq    p_head(%rip), %rdx
        jae     .Lsc_big
        SMALL_CMP .Lsc
.Lsc_big:                               /* n >= 17: head block first, before any size dispatch */
        movdqu  (%rdi), %xmm0
        movdqu  (%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        pmovmskb %xmm0, %eax
        subl    $0xffff, %eax
        jnz     .Lsc_d0
        cmpq    $32, %rdx
        ja      .Lsc_gt32
        movdqu  -16(%rdi,%rdx), %xmm0
        movdqu  -16(%rsi,%rdx), %xmm1
        pcmpeqb %xmm1, %xmm0
        pmovmskb %xmm0, %eax
        subl    $0xffff, %eax
        jnz     .Lsc_dn16
        ret
.Lsc_d0:                                /* eax = mask - 0xffff: lowest set bit = first differing byte */
        tzcntl  %eax, %ecx
        movzbl  (%rdi,%rcx), %eax
        movzbl  (%rsi,%rcx), %edx
        subl    %edx, %eax
        ret
.Lsc_dn16:
        leaq    -16(%rdx), %r8
.Lsc_dr8:                               /* same, block at offset r8 */
        tzcntl  %eax, %ecx
        addq    %r8, %rcx
        movzbl  (%rdi,%rcx), %eax
        movzbl  (%rsi,%rcx), %edx
        subl    %edx, %eax
        ret
.Lsc_find:                              /* a difference lies in [r8, r8 + 64) and inside n: walk 16-byte blocks */
        movdqu  (%rdi,%r8), %xmm0
        movdqu  (%rsi,%r8), %xmm1
        pcmpeqb %xmm1, %xmm0
        pmovmskb %xmm0, %eax
        subl    $0xffff, %eax
        jnz     .Lsc_dr8
        addq    $16, %r8
        jmp     .Lsc_find
.Lsc_gt32:
        cmpq    $64, %rdx
        ja      .Lsc_gt64
        movdqu  16(%rdi), %xmm0
        movdqu  16(%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        movdqu  -32(%rdi,%rdx), %xmm2
        movdqu  -32(%rsi,%rdx), %xmm3
        pcmpeqb %xmm3, %xmm2
        movdqu  -16(%rdi,%rdx), %xmm4
        movdqu  -16(%rsi,%rdx), %xmm5
        pcmpeqb %xmm5, %xmm4
        pand    %xmm0, %xmm2
        pand    %xmm4, %xmm2
        pmovmskb %xmm2, %eax
        subl    $0xffff, %eax
        jnz     .Lsc_d33
        ret
.Lsc_d33:
        pmovmskb %xmm0, %eax
        movl    $16, %r8d
        subl    $0xffff, %eax
        jnz     .Lsc_dr8
        movdqu  -32(%rdi,%rdx), %xmm2
        movdqu  -32(%rsi,%rdx), %xmm3
        pcmpeqb %xmm3, %xmm2
        pmovmskb %xmm2, %eax
        leaq    -32(%rdx), %r8
        subl    $0xffff, %eax
        jnz     .Lsc_dr8
        pmovmskb %xmm4, %eax
        leaq    -16(%rdx), %r8
        subl    $0xffff, %eax
        jmp     .Lsc_dr8
.Lsc_gt64:
        cmpq    $128, %rdx
        ja      .Lsc_gt128
        movl    $16, %r8d
        movdqu  16(%rdi), %xmm0
        movdqu  16(%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        movdqu  32(%rdi), %xmm2
        movdqu  32(%rsi), %xmm3
        pcmpeqb %xmm3, %xmm2
        movdqu  48(%rdi), %xmm4
        movdqu  48(%rsi), %xmm5
        pcmpeqb %xmm5, %xmm4
        pand    %xmm2, %xmm0
        pand    %xmm4, %xmm0
        pmovmskb %xmm0, %eax
        cmpl    $0xffff, %eax
        jne     .Lsc_find
        leaq    -64(%rdx), %r8
        B64S    %r8
        cmpl    $0xffff, %eax
        jne     .Lsc_find
        xorl    %eax, %eax
        ret
.Lsc_gt128:
        movl    $16, %r8d
        leaq    -64(%rdx), %r9
.Lsc_loop:
        B64S    %r8
        cmpl    $0xffff, %eax
        jne     .Lsc_find
        addq    $64, %r8
        cmpq    %r9, %r8
        jb      .Lsc_loop
        movq    %r9, %r8
        B64S    %r8
        cmpl    $0xffff, %eax
        jne     .Lsc_find
        xorl    %eax, %eax
        ret
        .size   p_memcmp_sse2, .-p_memcmp_sse2

/* ---------------------------------------------------------------- SSE2 equality entry (0 / 1) */
        .p2align 6
        .globl  p_memeq_sse2
        .type   p_memeq_sse2, @function
p_memeq_sse2:
        cmpq    p_head(%rip), %rdx
        jae     .Lse_big
        SMALL_EQ .Lse
.Lse_big:
        movdqu  (%rdi), %xmm0
        movdqu  (%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        pmovmskb %xmm0, %eax
        cmpl    $0xffff, %eax
        jne     .Lse_ne
        cmpq    $32, %rdx
        ja      .Lse_gt32
        movdqu  -16(%rdi,%rdx), %xmm0
        movdqu  -16(%rsi,%rdx), %xmm1
        pcmpeqb %xmm1, %xmm0
        pmovmskb %xmm0, %eax
        xorl    $0xffff, %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lse_ne:
        movl    $1, %eax
        ret
.Lse_gt32:
        cmpq    $64, %rdx
        ja      .Lse_gt64
        movdqu  16(%rdi), %xmm0
        movdqu  16(%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        movdqu  -32(%rdi,%rdx), %xmm2
        movdqu  -32(%rsi,%rdx), %xmm3
        pcmpeqb %xmm3, %xmm2
        movdqu  -16(%rdi,%rdx), %xmm4
        movdqu  -16(%rsi,%rdx), %xmm5
        pcmpeqb %xmm5, %xmm4
        pand    %xmm2, %xmm0
        pand    %xmm4, %xmm0
        pmovmskb %xmm0, %eax
        xorl    $0xffff, %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lse_gt64:
        cmpq    $128, %rdx
        ja      .Lse_gt128
        movdqu  16(%rdi), %xmm0
        movdqu  16(%rsi), %xmm1
        pcmpeqb %xmm1, %xmm0
        movdqu  32(%rdi), %xmm2
        movdqu  32(%rsi), %xmm3
        pcmpeqb %xmm3, %xmm2
        movdqu  48(%rdi), %xmm4
        movdqu  48(%rsi), %xmm5
        pcmpeqb %xmm5, %xmm4
        pand    %xmm2, %xmm0
        pand    %xmm4, %xmm0
        leaq    -64(%rdx), %r8
        movdqa  %xmm0, %xmm8
        B64S    %r8
        pand    %xmm8, %xmm0
        pmovmskb %xmm0, %eax
        xorl    $0xffff, %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lse_gt128:
        movl    $16, %r8d
        leaq    -64(%rdx), %r9
.Lse_loop:
        B64S    %r8
        cmpl    $0xffff, %eax
        jne     .Lse_ne
        addq    $64, %r8
        cmpq    %r9, %r8
        jb      .Lse_loop
        B64S    %r9
        xorl    $0xffff, %eax
        setne   %al
        movzbl  %al, %eax
        ret
        .size   p_memeq_sse2, .-p_memeq_sse2

/* ---------------------------------------------------------------- AVX2 ordered entry, order A */
        .p2align 6
        .globl  p_memcmp_avx2
        .type   p_memcmp_avx2, @function
p_memcmp_avx2:
        cmpq    p_head(%rip), %rdx
        jae     .Lac_big
        SMALL_CMP .Lac
.Lac_big:                               /* n >= 17 (order B also lands here for 17-32 and > 2048) */
        cmpq    $32, %rdx
        jbe     .Lac_17_32
.Lac_gt32:                              /* order B lands here for 33..2048 */
        vmovdqu (%rdi), %ymm0
        vpcmpeqb (%rsi), %ymm0, %ymm0
        vpmovmskb %ymm0, %eax
        incl    %eax
        jnz     .Lac_d0
        cmpq    $64, %rdx
        ja      .Lac_gt64
        vmovdqu -32(%rdi,%rdx), %ymm0
        vpcmpeqb -32(%rsi,%rdx), %ymm0, %ymm0
        vpmovmskb %ymm0, %eax
        incl    %eax
        jnz     .Lac_dn32
        vzeroupper
        ret
.Lac_dn32:
        leaq    -32(%rdx), %r8
        jmp     .Lac_dr8
.Lac_d0:                                /* eax = mask + 1 (or mask - 0xffff): lowest set bit = first difference */
        vzeroupper
        tzcntl  %eax, %ecx
        movzbl  (%rdi,%rcx), %eax
        movzbl  (%rsi,%rcx), %edx
        subl    %edx, %eax
        ret
.Lac_dn16:
        leaq    -16(%rdx), %r8
.Lac_dr8:
        vzeroupper
        tzcntl  %eax, %ecx
        addq    %r8, %rcx
        movzbl  (%rdi,%rcx), %eax
        movzbl  (%rsi,%rcx), %edx
        subl    %edx, %eax
        ret
.Lac_17_32:
        vmovdqu (%rdi), %xmm0
        vpcmpeqb (%rsi), %xmm0, %xmm0
        vpmovmskb %xmm0, %eax
        subl    $0xffff, %eax
        jnz     .Lac_d0
        vmovdqu -16(%rdi,%rdx), %xmm0
        vpcmpeqb -16(%rsi,%rdx), %xmm0, %xmm0
        vpmovmskb %xmm0, %eax
        subl    $0xffff, %eax
        jnz     .Lac_dn16
        ret
.Lac_gt64:
        cmpq    $128, %rdx
        ja      .Lac_gt128
        vmovdqu 32(%rdi), %ymm1
        vpcmpeqb 32(%rsi), %ymm1, %ymm1
        vmovdqu -64(%rdi,%rdx), %ymm2
        vpcmpeqb -64(%rsi,%rdx), %ymm2, %ymm2
        vmovdqu -32(%rdi,%rdx), %ymm3
        vpcmpeqb -32(%rsi,%rdx), %ymm3, %ymm3
        vpand   %ymm2, %ymm1, %ymm0
        vpand   %ymm3, %ymm0, %ymm0
        vpmovmskb %ymm0, %eax
        incl    %eax
        jnz     .Lac_d65
        vzeroupper
        ret
.Lac_d65:
        vpmovmskb %ymm1, %eax
        movl    $32, %r8d
        incl    %eax
        jnz     .Lac_dr8
        vpmovmskb %ymm2, %eax
        leaq    -64(%rdx), %r8
        incl    %eax
        jnz     .Lac_dr8
        vpmovmskb %ymm3, %eax
        leaq    -32(%rdx), %r8
        incl    %eax
        jmp     .Lac_dr8
.Lac_gt128:
        movl    $32, %r8d
        leaq    -64(%rdx), %r9
.Lac_loop:
        B64A    %r8
        incl    %eax
        jnz     .Lac_ld
        addq    $64, %r8
        cmpq    %r9, %r8
        jb      .Lac_loop
        movq    %r9, %r8
        B64A    %r8
        incl    %eax
        jnz     .Lac_ld
        vzeroupper
        xorl    %eax, %eax
        ret
.Lac_ld:
        vpmovmskb %ymm1, %eax
        incl    %eax
        jnz     .Lac_dr8
        vpmovmskb %ymm2, %eax
        addq    $32, %r8
        incl    %eax
        jmp     .Lac_dr8
        .size   p_memcmp_avx2, .-p_memcmp_avx2

/* ---------------------------------------------------------------- AVX2 equality entry, order A */
        .p2align 6
        .globl  p_memeq_avx2
        .type   p_memeq_avx2, @function
p_memeq_avx2:
        cmpq    p_head(%rip), %rdx
        jae     .Lae_big
        SMALL_EQ .Lae
.Lae_big:
        cmpq    $32, %rdx
        jbe     .Lae_17_32
.Lae_gt32:
        vmovdqu (%rdi), %ymm0
        vpcmpeqb (%rsi), %ymm0, %ymm0
        vpmovmskb %ymm0, %eax
        incl    %eax
        jnz     .Lae_ne
        cmpq    $64, %rdx
        ja      .Lae_gt64
        vmovdqu -32(%rdi,%rdx), %ymm0
        vpcmpeqb -32(%rsi,%rdx), %ymm0, %ymm0
        vpmovmskb %ymm0, %eax
        vzeroupper
        incl    %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lae_ne:
        vzeroupper
        movl    $1, %eax
        ret
.Lae_17_32:
        vmovdqu (%rdi), %xmm0
        vpcmpeqb (%rsi), %xmm0, %xmm0
        vmovdqu -16(%rdi,%rdx), %xmm1
        vpcmpeqb -16(%rsi,%rdx), %xmm1, %xmm1
        vpand   %xmm1, %xmm0, %xmm0
        vpmovmskb %xmm0, %eax
        xorl    $0xffff, %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lae_gt64:
        cmpq    $128, %rdx
        ja      .Lae_gt128
        vmovdqu 32(%rdi), %ymm1
        vpcmpeqb 32(%rsi), %ymm1, %ymm1
        vmovdqu -64(%rdi,%rdx), %ymm2
        vpcmpeqb -64(%rsi,%rdx), %ymm2, %ymm2
        vmovdqu -32(%rdi,%rdx), %ymm3
        vpcmpeqb -32(%rsi,%rdx), %ymm3, %ymm3
        vpand   %ymm2, %ymm1, %ymm1
        vpand   %ymm3, %ymm1, %ymm1
        vpmovmskb %ymm1, %eax
        vzeroupper
        incl    %eax
        setne   %al
        movzbl  %al, %eax
        ret
.Lae_gt128:
        movl    $32, %r8d
        leaq    -64(%rdx), %r9
.Lae_loop:
        B64A    %r8
        incl    %eax
        jnz     .Lae_ne
        addq    $64, %r8
        cmpq    %r9, %r8
        jb      .Lae_loop
        B64A    %r9
        vzeroupper
        incl    %eax
        setne   %al
        movzbl  %al, %eax
        ret
        .size   p_memeq_avx2, .-p_memeq_avx2

/* ---------------------------------------------------------------- AVX2 entries, order B (span test first) */
        .p2align 6
        .globl  p_memcmp_avx2b
        .type   p_memcmp_avx2b, @function
p_memcmp_avx2b:
        leaq    -33(%rdx), %rax
        cmpq    p_span(%rip), %rax
        jb      .Lac_gt32
        cmpq    p_head(%rip), %rdx
        jae     .Lac_big
        SMALL_CMP .Lbc
        .size   p_memcmp_avx2b, .-p_memcmp_avx2b

        .p2align 6
        .globl  p_memeq_avx2b
        .type   p_memeq_avx2b, @function
p_memeq_avx2b:
        leaq    -33(%rdx), %rax
        cmpq    p_span(%rip), %rax
        jb      .Lae_gt32
        cmpq    p_head(%rip), %rdx
        jae     .Lae_big
        SMALL_EQ .Lbe
        .size   p_memeq_avx2b, .-p_memeq_avx2b

/* ---------------------------------------------------------------- bound-pointer entries, null row */
        .p2align 4
        .globl  p_memcmp_ptr, p_memeq_ptr, proto_null, p_null_abort
p_memcmp_ptr:
        jmp     *p_memcmp_slot(%rip)
        .p2align 4
p_memeq_ptr:
        jmp     *p_memeq_slot(%rip)
        .p2align 4
proto_null:
        xorl    %eax, %eax
        ret
p_null_abort:
        ud2
        .section .note.GNU-stack,"",@progbits
