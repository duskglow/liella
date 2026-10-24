/* Raw system-call entry for the class-sys wrappers (csys/README.md).
 *
 * x86_64 Linux system-call convention (kernel ABI): number in rax; arguments in rdi, rsi, rdx,
 * r10, r8, r9; result in rax (a value, or -errno in [-4095, -1]); rcx and r11 are clobbered.
 * Each helper is one statement (a statement expression), so the wrappers stay single-statement
 * and inline completely under cross-language LTO (OD-C3). */
#ifndef LIELLA_RAW_H
#define LIELLA_RAW_H

static inline long liella_raw0(long n)
{
    return ({ long r_; __asm__ volatile ("syscall" : "=a"(r_) : "a"(n) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw1(long n, long a)
{
    return ({ long r_; __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw2(long n, long a, long b)
{
    return ({ long r_; __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a), "S"(b) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw3(long n, long a, long b, long c)
{
    return ({ long r_; __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw4(long n, long a, long b, long c, long d)
{
    return ({ long r_; register long r10_ __asm__("r10") = d;
              __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10_) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw5(long n, long a, long b, long c, long d, long e)
{
    return ({ long r_; register long r10_ __asm__("r10") = d; register long r8_ __asm__("r8") = e;
              __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10_), "r"(r8_) : "rcx", "r11", "memory"); r_; });
}

static inline long liella_raw6(long n, long a, long b, long c, long d, long e, long f)
{
    return ({ long r_; register long r10_ __asm__("r10") = d; register long r8_ __asm__("r8") = e; register long r9_ __asm__("r9") = f;
              __asm__ volatile ("syscall" : "=a"(r_) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10_), "r"(r8_), "r"(r9_) : "rcx", "r11", "memory"); r_; });
}

#endif
