/* handles.c - black-box probe of the dlsym special handles (spec §27.1 s27_1_c_3, s27_1_c_1_3).
 * Self-checking, written as the conformance program DL-16 (DL-DLOPEN) adopts once Liella has
 * dlopen/dlsym and the conformance harness links dynamic programs. Linked against libhandle1.so
 * then libhandle2.so (load order 1, 2); both define f, which returns its object's number.
 *   - dlsym(RTLD_DEFAULT, "f") is the first f in the global lookup order: helper 1's.
 *   - dlsym(RTLD_NEXT, "f") called from helper 1 is the next occurrence after helper 1: helper 2's.
 *   - dlsym(dlopen(NULL, RTLD_NOW), "main_sym") finds the executable's exported main_sym.
 *   - a missing name gives NULL and a non-NULL dlerror text, for each of the three handles.
 * Exit status: the number of failed checks. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

int main_sym = 42;                       /* exported: the program is linked with -rdynamic */
extern int f(void);
extern void *handle1_next_f(void);       /* in helper 1: dlsym(RTLD_NEXT, "f") */
extern void *handle1_next_missing(void); /* in helper 1: dlsym(RTLD_NEXT, missing name) */

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "handles: FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)

static int missing_ok(void *s)
{
    const char *e = dlerror();
    if (e) printf("missing: %s\n", e);
    return s == NULL && e != NULL && dlerror() == NULL;
}

int main(void)
{
    int (*d)(void) = (int (*)(void))dlsym(RTLD_DEFAULT, "f");
    CHECK(d != NULL);
    CHECK(d && d() == 1);
    CHECK(d == f);                       /* the program's own binding agrees with RTLD_DEFAULT */

    int (*n)(void) = (int (*)(void))handle1_next_f();
    CHECK(n != NULL);
    CHECK(n && n() == 2);
    CHECK(n != d);

    void *g = dlopen(NULL, RTLD_NOW);
    CHECK(g != NULL);
    int *m = g ? (int *)dlsym(g, "main_sym") : NULL;
    CHECK(m == &main_sym);
    CHECK(m && *m == 42);
    int (*gf)(void) = g ? (int (*)(void))dlsym(g, "f") : NULL;
    CHECK(gf == d);                      /* the global handle searches in the same order */

    dlerror();
    CHECK(missing_ok(dlsym(RTLD_DEFAULT, "handles_no_such_symbol")));
    CHECK(missing_ok(handle1_next_missing()));
    CHECK(g && missing_ok(dlsym(g, "handles_no_such_symbol")));

    printf("default f=%d next f=%d main_sym=%d fails=%d\n", d ? d() : -1, n ? n() : -1, m ? *m : -1, fails);
    return fails;
}
