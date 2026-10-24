/* Which forms of setjmp save the signal mask, and which jumps restore it (spec §21.1; D-21.4).
 *
 * Black-box probe: public interfaces only. Each line is one pairing of a "set" form and a jump
 * function. Before the set call the mask is exactly {SIGUSR2}; after it, the probe changes the
 * mask to exactly {SIGUSR1} and jumps back. After the jump, "usr1" and "usr2" say whether each
 * signal is blocked: usr1=0 usr2=1 means the mask of the set call was saved and restored;
 * usr1=1 usr2=0 means the mask was left as the jump found it.
 *
 * Set forms:
 *   setjmp(macro)    `setjmp(env)` as C code sees it through <setjmp.h>
 *   setjmp(pointer)  a call through the address of the symbol `setjmp` (no macro applies)
 *   setjmp(symbol)   a direct call of the symbol `setjmp`, declared without the header's macro
 *   _setjmp          `_setjmp(env)`
 *   sigsetjmp(0)     `sigsetjmp(env, 0)` (the header's macro)
 *   sigsetjmp(1)     `sigsetjmp(env, 1)`
 * Jumps (each called by its symbol, so that a fortified build does not change which one runs):
 *   longjmp, _longjmp, siglongjmp, __longjmp_chk.
 */
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

typedef struct __jmp_buf_tag *env_t;

extern int sym_setjmp(env_t) __asm__("setjmp") __attribute__((returns_twice));
extern void sym_longjmp(env_t, int) __asm__("longjmp") __attribute__((noreturn));
extern void sym__longjmp(env_t, int) __asm__("_longjmp") __attribute__((noreturn));
extern void sym_siglongjmp(env_t, int) __asm__("siglongjmp") __attribute__((noreturn));
extern void sym_longjmp_chk(env_t, int) __asm__("__longjmp_chk") __attribute__((noreturn));

static sigjmp_buf env;
static int (*volatile setjmp_ptr)(env_t) = setjmp;

static const char *const jumps[] = { "longjmp", "_longjmp", "siglongjmp", "__longjmp_chk" };

static void set_mask_to(int sig) {
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, sig);
    sigprocmask(SIG_SETMASK, &s, NULL);
}

static int blocked(int sig) {
    sigset_t s;
    sigprocmask(SIG_BLOCK, NULL, &s);
    return sigismember(&s, sig);
}

__attribute__((noinline, noreturn)) static void jump(int j) {
    set_mask_to(SIGUSR1);
    switch (j) {
    case 0: sym_longjmp(env, 1);
    case 1: sym__longjmp(env, 1);
    case 2: sym_siglongjmp(env, 1);
    default: sym_longjmp_chk(env, 1);
    }
}

static void report(const char *form, int j, int r) {
    printf("%-16s %-14s ret=%d usr1=%d usr2=%d\n", form, jumps[j], r, blocked(SIGUSR1), blocked(SIGUSR2));
}

/* One function per set form, so that each set call is in a frame that is live at the jump. */
#define FORM(fn, name, call)                                                                  \
    __attribute__((noinline)) static void fn(int j) {                                         \
        static volatile int r;                                                                \
        set_mask_to(SIGUSR2);                                                                 \
        r = (call);                                                                           \
        if (r == 0)                                                                           \
            jump(j);                                                                          \
        report(name, j, r);                                                                   \
    }

FORM(f_macro, "setjmp(macro)", setjmp(env))
FORM(f_pointer, "setjmp(pointer)", setjmp_ptr(env))
FORM(f_symbol, "setjmp(symbol)", sym_setjmp(env))
FORM(f_under, "_setjmp", _setjmp(env))
FORM(f_sig0, "sigsetjmp(0)", sigsetjmp(env, 0))
FORM(f_sig1, "sigsetjmp(1)", sigsetjmp(env, 1))

int main(void) {
    void (*const forms[])(int) = { f_macro, f_pointer, f_symbol, f_under, f_sig0, f_sig1 };
    for (unsigned f = 0; f < sizeof forms / sizeof forms[0]; f++)
        for (int j = 0; j < 4; j++)
            forms[f](j);
    return 0;
}
