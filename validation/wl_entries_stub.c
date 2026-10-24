/* WL-ENTRIES: LD_PRELOAD counter of the calls a PROGRAM (and its other shared libraries) makes into the C
 * library's exported functions. Built by validation/wl_entries_run.sh against a generated wle_syms.h:
 *   #define WLE_N <count>
 *   #define WLE_SYMS X(0, fopen, "GLIBC_2.2.5") X(1, ...) ...
 * one entry for every libc function the measured programs import (from their dynamic symbol tables).
 *
 * Each entry gets a signature-agnostic trampoline: `lock incq counter; jmp *real`. It touches no argument
 * register (r11 only), keeps the caller's stack and return address, so the callee sees the original call.
 * real = dlvsym(RTLD_NEXT, name, version), resolved for every entry in a constructor; a call that arrives before
 * the constructor ran (another library's constructor) takes a slow path that saves the argument registers and
 * resolves that entry first. Nothing inside the C library is observed: only calls that bind to these exported
 * names from outside it are counted.
 *
 * Counts are written at ELF fini to $WLE_DIR/<program_invocation_short_name>.<pid> ("name<TAB>calls" lines,
 * non-zero only). A fork child starts from zero (pthread_atfork); a process that ends with _exit or a fatal
 * signal writes nothing. The stub itself calls only dlvsym/dlsym/pthread_atfork (never counted: the generator
 * leaves them out) and raw system calls. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stddef.h>

#include "wle_syms.h"

#define WLE_HIDDEN __attribute__((visibility("hidden")))

WLE_HIDDEN unsigned long wle_cnt[WLE_N];
WLE_HIDDEN void *wle_real[WLE_N];
WLE_HIDDEN void *wle_resolve(unsigned long i);

#define X(i, n, v)                                                                                           \
    __asm__(".text\n.globl " #n "\n.type " #n ",@function\n" #n ":\n"                                        \
            "\tlock incq wle_cnt+8*" #i "(%rip)\n"                                                           \
            "\tmovq wle_real+8*" #i "(%rip),%r11\n"                                                          \
            "\ttestq %r11,%r11\n\tjz 1f\n\tjmp *%r11\n"                                                       \
            "1:\tmovl $" #i ",%r11d\n\tjmp wle_slow\n.size " #n ",.-" #n "\n");
WLE_SYMS
#undef X

/* Slow path: r11 = entry index. Saves the integer argument registers, rax (vector count of a variadic call),
 * r10 and xmm0-7, resolves, restores, and jumps to the real function with the original stack. */
__asm__(".text\n.globl wle_slow\n.hidden wle_slow\n.type wle_slow,@function\nwle_slow:\n"
        "\tpushq %rbp\n\tmovq %rsp,%rbp\n\tandq $-16,%rsp\n\tsubq $192,%rsp\n"
        "\tmovq %rdi,0(%rsp)\n\tmovq %rsi,8(%rsp)\n\tmovq %rdx,16(%rsp)\n\tmovq %rcx,24(%rsp)\n"
        "\tmovq %r8,32(%rsp)\n\tmovq %r9,40(%rsp)\n\tmovq %rax,48(%rsp)\n\tmovq %r10,56(%rsp)\n"
        "\tmovdqu %xmm0,64(%rsp)\n\tmovdqu %xmm1,80(%rsp)\n\tmovdqu %xmm2,96(%rsp)\n\tmovdqu %xmm3,112(%rsp)\n"
        "\tmovdqu %xmm4,128(%rsp)\n\tmovdqu %xmm5,144(%rsp)\n\tmovdqu %xmm6,160(%rsp)\n\tmovdqu %xmm7,176(%rsp)\n"
        "\tmovq %r11,%rdi\n\tcall wle_resolve\n\tmovq %rax,%r11\n"
        "\tmovq 0(%rsp),%rdi\n\tmovq 8(%rsp),%rsi\n\tmovq 16(%rsp),%rdx\n\tmovq 24(%rsp),%rcx\n"
        "\tmovq 32(%rsp),%r8\n\tmovq 40(%rsp),%r9\n\tmovq 48(%rsp),%rax\n\tmovq 56(%rsp),%r10\n"
        "\tmovdqu 64(%rsp),%xmm0\n\tmovdqu 80(%rsp),%xmm1\n\tmovdqu 96(%rsp),%xmm2\n\tmovdqu 112(%rsp),%xmm3\n"
        "\tmovdqu 128(%rsp),%xmm4\n\tmovdqu 144(%rsp),%xmm5\n\tmovdqu 160(%rsp),%xmm6\n\tmovdqu 176(%rsp),%xmm7\n"
        "\tmovq %rbp,%rsp\n\tpopq %rbp\n\tjmp *%r11\n.size wle_slow,.-wle_slow\n");

static const char *const wle_name[WLE_N] = {
#define X(i, n, v) [i] = #n,
    WLE_SYMS
#undef X
};
static const char *const wle_ver[WLE_N] = {
#define X(i, n, v) [i] = v,
    WLE_SYMS
#undef X
};

static long wle_sys(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static size_t wle_len(const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    return n;
}
static char *wle_put(char *p, const char *s) {
    while (*s) *p++ = *s++;
    return p;
}
static char *wle_putu(char *p, unsigned long v) {
    char t[24];
    int i = 0;
    do t[i++] = (char)('0' + v % 10); while ((v /= 10) != 0);
    while (i) *p++ = t[--i];
    return p;
}
static void wle_die(const char *what, const char *name) {
    wle_sys(1, 2, (long)what, (long)wle_len(what));
    wle_sys(1, 2, (long)name, (long)wle_len(name));
    wle_sys(1, 2, (long)"\n", 1);
    wle_sys(231, 127, 0, 0);
}

static int wle_busy;
WLE_HIDDEN void *wle_resolve(unsigned long i) {
    void *p = __atomic_load_n(&wle_real[i], __ATOMIC_ACQUIRE);
    if (p) return p;
    if (wle_busy) wle_die("wl_entries: recursive resolve of ", wle_name[i]);
    wle_busy = 1;
    p = dlvsym(RTLD_NEXT, wle_name[i], wle_ver[i]);
    wle_busy = 0;
    if (!p) wle_die("wl_entries: cannot resolve ", wle_name[i]);
    __atomic_store_n(&wle_real[i], p, __ATOMIC_RELEASE);
    return p;
}

static void wle_child(void) {
    for (unsigned long i = 0; i < WLE_N; i++) __atomic_store_n(&wle_cnt[i], 0, __ATOMIC_RELAXED);
}

__attribute__((constructor(101))) static void wle_init(void) {
    for (unsigned long i = 0; i < WLE_N; i++) wle_resolve(i);
    int (*af)(void (*)(void), void (*)(void), void (*)(void)) =
        (int (*)(void (*)(void), void (*)(void), void (*)(void)))dlsym(RTLD_NEXT, "pthread_atfork");
    if (af) af(NULL, NULL, wle_child);
}

extern char **environ;
static char wle_path[4096];
static char wle_buf[WLE_N * 80 + 64];

__attribute__((destructor(101))) static void wle_dump(void) {
    const char *dir = NULL;
    for (char **e = environ; e && *e; e++) {
        const char *s = *e;
        if (s[0] == 'W' && s[1] == 'L' && s[2] == 'E' && s[3] == '_' && s[4] == 'D' && s[5] == 'I' && s[6] == 'R' &&
            s[7] == '=') {
            dir = s + 8;
            break;
        }
    }
    if (!dir || wle_len(dir) + wle_len(program_invocation_short_name) + 32 > sizeof wle_path) return;
    char *p = wle_put(wle_path, dir);
    p = wle_put(p, "/");
    p = wle_put(p, program_invocation_short_name);
    p = wle_put(p, ".");
    p = wle_putu(p, (unsigned long)wle_sys(39, 0, 0, 0));
    *p = 0;
    char *b = wle_buf;
    for (unsigned long i = 0; i < WLE_N; i++) {
        unsigned long c = __atomic_load_n(&wle_cnt[i], __ATOMIC_RELAXED);
        if (!c) continue;
        b = wle_put(b, wle_name[i]);
        b = wle_put(b, "\t");
        b = wle_putu(b, c);
        b = wle_put(b, "\n");
    }
    long fd = wle_sys(2, (long)wle_path, 01 | 0100 | 01000 | 02000000, 0644); /* O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC */
    if (fd < 0) return;
    wle_sys(1, fd, (long)wle_buf, (long)(b - wle_buf));
    wle_sys(3, fd, 0, 0);
}
