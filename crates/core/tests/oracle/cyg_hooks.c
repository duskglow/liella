/* A -finstrument-functions program for tests/s28_profiling_clauses.rs (s28_1_c_4), built
 * statically against the host C library and against Liella's liella-crt1.o + libc.a. With
 * -DOWN_HOOKS it defines its own __cyg_profile_func_enter/_exit, which must link without a
 * duplicate definition and be the ones the instrumented code calls; without it the library's
 * hooks are called and must have no observable effect (errno kept, nothing printed). */
#include <errno.h>
#include <stdio.h>

static volatile int enters, exits;

#ifdef OWN_HOOKS
__attribute__((no_instrument_function)) void __cyg_profile_func_enter(void *fn, void *site) {
    (void)fn;
    (void)site;
    enters++;
}
__attribute__((no_instrument_function)) void __cyg_profile_func_exit(void *fn, void *site) {
    (void)fn;
    (void)site;
    exits++;
}
#endif

__attribute__((noipa)) int leaf(int x) { return x * 3 + 1; }
__attribute__((noipa)) int mid(int x) { return leaf(x) + leaf(x + 1); }

int main(void) {
    int r = 0;
    errno = 4242;
    for (int i = 0; i < 5; i++)
        r += mid(i);
    int e = errno;
    /* main's own entry is counted; its exit is not yet. */
    printf("r=%d errno=%d enter=%d exit=%d\n", r, e, enters, exits);
    return 0;
}
