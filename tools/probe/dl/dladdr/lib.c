/* lib.c - the probe object of tools/probe/dl/dladdr/run.sh (DL-18 DL-DLADDR): symbols laid out in
 * one assembler block, in a known order, so the probe can ask dladdr about exact places:
 *   s_a (16 bytes) | 32 bytes of local code (no dynamic symbol) | s_zero (size 0) + 16 bytes |
 *   s_b, s_b_alias (global, same address), s_b_weak (weak, same address), all 16 bytes |
 *   s_outer (40 bytes from s_nt+4, overlapping s_nt, ifres and s_impl) |
 *   s_w1 (weak) then s_w2 (global) at one address | s_nt (STT_NOTYPE, 16) | s_if (IFUNC, resolver
 *   ifres local) | s_impl (the implementation) | s_tail (16, the last function).
 * Data: s_obj (8 bytes), s_tls (STT_TLS, 8 bytes, the first TLS symbol: st_value 0).
 * Built with -DNOTLS it has no TLS segment. */
#include <stdint.h>

__asm__(
    ".text\n"
    ".p2align 6\n"
    ".globl s_a\n.type s_a,@function\ns_a:\n.fill 16,1,0x90\n.size s_a,16\n"
    "loc_gap:\n.fill 32,1,0x90\n"
    ".globl s_zero\n.type s_zero,@function\ns_zero:\n.size s_zero,0\n.fill 16,1,0x90\n"
    ".globl s_b\n.type s_b,@function\n.globl s_b_alias\n.type s_b_alias,@function\n"
    ".weak s_b_weak\n.type s_b_weak,@function\n"
    "s_b:\ns_b_alias:\ns_b_weak:\n.fill 16,1,0x90\n.size s_b,16\n.size s_b_alias,16\n.size s_b_weak,16\n"
    ".weak s_w1\n.type s_w1,@function\n.globl s_w2\n.type s_w2,@function\n"
    "s_w1:\ns_w2:\n.fill 16,1,0x90\n.size s_w1,16\n.size s_w2,16\n"
    ".globl s_outer\n.type s_outer,@function\n.set s_outer,s_nt+4\n.size s_outer,40\n"
    ".globl s_nt\ns_nt:\n.fill 16,1,0x90\n.size s_nt,16\n"
    "ifres:\nleaq .Limpl(%rip),%rax\nret\n.fill 8,1,0x90\n"
    ".globl s_if\n.type s_if,@gnu_indirect_function\n.set s_if,ifres\n.size s_if,16\n"
    ".globl s_impl\n.type s_impl,@function\ns_impl:\n.Limpl:\nmovl $7,%eax\nret\n.fill 10,1,0x90\n.size s_impl,16\n"
    ".globl s_tail\n.type s_tail,@function\ns_tail:\n.fill 16,1,0x90\n.size s_tail,16\n"
    ".data\n.p2align 3\n.globl s_obj\n.type s_obj,@object\ns_obj:\n.quad 5\n.size s_obj,8\n"
#ifndef NOTLS
    ".section .tbss,\"awT\",@nobits\n.p2align 3\n.globl s_tls\n.type s_tls,@object\ns_tls:\n.zero 8\n.size s_tls,8\n"
#endif
    ".text\n");

#ifndef NOTLS
extern __thread long s_tls;
/* The calling thread's s_tls (the probe compares it with RTLD_DI_TLS_DATA). */
long *lib_tls_addr(void) { return &s_tls; }
#else
long *lib_tls_addr(void) { return 0; }
#endif
/* An address in this object's .plt: the probe finds the section by the file's section headers. */
int lib_calls_out(void);
int lib_calls_out(void) { extern int puts(const char *); return puts("") * 0; }
