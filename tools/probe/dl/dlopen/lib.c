/* lib.c - one source for every shared object of the DL-16P dlopen/dlsym probes (P-1..P-13).
 *
 * Built many times by run.sh with different -D options. Every object prints what runs in it with
 * write(2), so the record shows the order of execution.
 *   -DNAME=X        required: defines fn_X() (returns "X") and a constructor printing "ctor X"
 *   -DWHO           also defines who() (returns "X"), a name several objects and the program define
 *   -DUSE=Y         defines use_Y(), which calls fn_Y() (an undefined reference unless linked with Y)
 *   -DUSE2=Y        the same for a second object
 *   -DUNDEF         references an undefined function (undefined_symbol_xyz), never called
 *   -DCALL=Y        the constructor calls fn_Y() and prints its answer
 *   -DLOOKUP        next_of(name) and default_of(name): dlsym(RTLD_NEXT / RTLD_DEFAULT) from this object
 *   -DP7            the constructor dlopens $P7_B, and $P7_SELF (NOLOAD, then plain); handles kept in p7_*
 *   -DWAITCTOR      the constructor writes one byte to fd $WAIT_STARTED, then reads one from fd $WAIT_RELEASE
 *   -DIFUNC         ifn() is an IFUNC; its resolver prints "resolver X" (raw syscall, no libc call)
 *   -DUSEIFN        call_ifn() calls ifn() of another object
 *   -DTLS11         TLS variables: tv_ie (initial-exec, 42), tv_gd (7), tv_bss (0), and their address getters
 * Public interfaces only.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifndef NAME
#error NAME required
#endif
#define STR2(x) #x
#define STR(x) STR2(x)
#define CAT2(a, b) a##b
#define CAT(a, b) CAT2(a, b)

static void say(const char *what, const char *more)
{
    char b[256];
    int n = snprintf(b, sizeof b, "   %s %s%s%s\n", what, STR(NAME), more ? " " : "", more ? more : "");
    if (n > 0)
        (void)!write(1, b, (size_t)n);
}

const char *CAT(fn_, NAME)(void) { return STR(NAME); }

#ifdef WHO
const char *who(void) { return STR(NAME); }
#endif

#ifdef USE
const char *CAT(fn_, USE)(void);
const char *CAT(use_, USE)(void) { return CAT(fn_, USE)(); }
#endif
#ifdef USE2
const char *CAT(fn_, USE2)(void);
const char *CAT(use_, USE2)(void) { return CAT(fn_, USE2)(); }
#endif

#ifdef UNDEF
int undefined_symbol_xyz(void);
int call_undef(void) { return undefined_symbol_xyz(); }
#endif

#ifdef CALL
const char *CAT(fn_, CALL)(void);
#endif

#ifdef LOOKUP
void *next_of(const char *n) { return dlsym(RTLD_NEXT, n); }
void *default_of(const char *n) { return dlsym(RTLD_DEFAULT, n); }
#endif

#ifdef P7
void *p7_b_handle, *p7_self_noload, *p7_self_open;
static int p7_depth;
#endif

#ifdef IFUNC
static long raw_write(const char *s, unsigned long n)
{
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "0"(1L), "D"(1L), "S"(s), "d"(n) : "rcx", "r11", "memory");
    return r;
}
static int ifn_impl(void) { return 10; }
static void *ifn_res(void)
{
    static const char m[] = "   resolver " STR(NAME) "\n";
    raw_write(m, sizeof m - 1);
    return (void *)ifn_impl;
}
int ifn(void) __attribute__((ifunc("ifn_res")));
int CAT(own_ifn_, NAME)(void) { return ifn(); }
#endif

#ifdef USEIFN
int ifn(void);
int call_ifn(void) { return ifn(); }
#endif

#ifdef TLS11
__thread int tv_ie __attribute__((tls_model("initial-exec"))) = 42;
__thread int tv_gd = 7;
__thread int tv_bss;
int *addr_ie(void) { return &tv_ie; }
int *addr_gd(void) { return &tv_gd; }
int *addr_bss(void) { return &tv_bss; }
#endif

__attribute__((constructor)) static void ctor(void)
{
#ifdef P7
    char d[32];
    snprintf(d, sizeof d, "begin depth=%d", ++p7_depth);
    say("ctor", d);
    const char *b = getenv("P7_B");
    if (b) {
        p7_b_handle = dlopen(b, RTLD_NOW);
        say("ctor", p7_b_handle ? "inner dlopen(B) returned a handle" : dlerror());
    }
    const char *self = getenv("P7_SELF");
    if (self && p7_depth == 1) {
        p7_self_noload = dlopen(self, RTLD_NOW | RTLD_NOLOAD);
        say("ctor", p7_self_noload ? "inner dlopen(self, NOLOAD) returned a handle" : "inner dlopen(self, NOLOAD) returned NULL");
        p7_self_open = dlopen(self, RTLD_NOW);
        say("ctor", p7_self_open ? "inner dlopen(self) returned a handle" : "inner dlopen(self) returned NULL");
    }
    say("ctor", "end");
    p7_depth--;
#else
    say("ctor", NULL);
#endif
#ifdef CALL
    say("ctor", CAT(fn_, CALL)());
#endif
#ifdef WAITCTOR
    const char *s = getenv("WAIT_STARTED"), *r = getenv("WAIT_RELEASE");
    if (s && r) {
        char c = 'x';
        (void)!write(atoi(s), &c, 1);
        (void)!read(atoi(r), &c, 1);
        say("ctor", "released");
    }
#endif
}
