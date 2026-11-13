/* Spec §27.1 (s27_1_c_3 dlsym, RTLD_DEFAULT, RTLD_NEXT, dlvsym; s27_1_c_1_3 dlopen(NULL); the
 * dlsym and dlvsym failure texts of s27_1_c_5): the special handles and their failures, against
 * two linked helpers that both define f (load order: helper 1, helper 2). Adopted from the
 * DLSYM-SPECIAL-HANDLES probe and the "other calls" part of tools/probe/dl/run.sh; the texts are
 * the probed ones of spec/behaviour/dynlink.md (the object a failure names: the program for
 * RTLD_DEFAULT and the global handle, the caller for RTLD_NEXT, the object for its handle).
 *   - dlsym(RTLD_DEFAULT, "f") is the first f in the global scope: helper 1's, the one the
 *     program's own reference binds;
 *   - dlsym(RTLD_NEXT, "f") from helper 1 is the next one after the caller: helper 2's;
 *   - dlsym(dlopen(NULL), ...) searches the global scope, the program's exported symbols first;
 *   - dlsym(handle of helper 2, "f") is helper 2's; dlvsym takes the exact version;
 *   - a failure returns NULL and one dlerror text, then dlerror() is NULL again.
 * Each failed check prints its line; the exit status is the number of failed checks. */
/* LINK-HELPERS: dlh_handle1 dlh_handle2 */
/* LINK-EXPORT-DYNAMIC: yes */
/* PENDING-LIELLA: DL-16b (DL-DLSYM; dlopen from DL-16a) */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main_sym = 42; /* exported (LINK-EXPORT-DYNAMIC) */
extern int f(void);
extern void *handle1_next_f(void);
extern void *handle1_next_missing(void);

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_dlsym_handles: line %d: %s\n", __LINE__, #c); fails++; } } while (0)

/* The failure text is exactly OBJ ": undefined symbol: " WHAT, and it is reported once. */
static int failed_with(const void *ret, const char *obj, const char *what)
{
    char want[4096];
    snprintf(want, sizeof want, "%s: undefined symbol: %s", obj, what);
    const char *e = dlerror();
    int ok = ret == NULL && e != NULL && strcmp(e, want) == 0;
    if (!ok)
        fprintf(stderr, "s27_dlsym_handles: want [%s], got %s [%s]\n", want, ret ? "a result" : "NULL", e ? e : "no text");
    return ok && dlerror() == NULL;
}

int main(int argc, char **argv)
{
    const char *dir = getenv("CONF_HELPERS");
    if (argc < 1 || dir == NULL)
        return 100;
    char h1[4096], h2[4096];
    snprintf(h1, sizeof h1, "%s/libdlh_handle1.so", dir);
    snprintf(h2, sizeof h2, "%s/libdlh_handle2.so", dir);
    const char *prog = argv[0];

    int (*d)(void) = (int (*)(void))dlsym(RTLD_DEFAULT, "f");
    CHECK(d != NULL && d() == 1);
    CHECK(d == f);

    int (*n)(void) = (int (*)(void))handle1_next_f();
    CHECK(n != NULL && n() == 2);
    CHECK(n != d);

    void *g = dlopen(NULL, RTLD_NOW);
    CHECK(g != NULL);
    int *m = g ? (int *)dlsym(g, "main_sym") : NULL;
    CHECK(m == &main_sym && *m == 42);
    CHECK(g && dlsym(g, "f") == (void *)d);

    void *k = dlopen(h2, RTLD_NOW);
    CHECK(k != NULL);
    CHECK(k && dlsym(k, "f") == (void *)n);
    CHECK(dlvsym(RTLD_DEFAULT, "printf", "GLIBC_2.2.5") != NULL);
    CHECK(dlerror() == NULL);

    CHECK(failed_with(dlsym(RTLD_DEFAULT, "handles_no_such_symbol"), prog, "handles_no_such_symbol"));
    CHECK(failed_with(handle1_next_missing(), h1, "handles_no_such_symbol"));
    CHECK(g && failed_with(dlsym(g, "handles_no_such_symbol"), prog, "handles_no_such_symbol"));
    CHECK(k && failed_with(dlsym(k, "handles_no_such_symbol"), h2, "handles_no_such_symbol"));
    CHECK(failed_with(dlvsym(RTLD_DEFAULT, "printf", "GLIBC_9.99"), prog, "printf, version GLIBC_9.99"));
    /* Helper 1 has a version table (it needs the C library) but no version NOVER_1. */
    void *k1 = dlopen(h1, RTLD_NOW);
    CHECK(k1 != NULL);
    CHECK(k1 && failed_with(dlvsym(k1, "f", "NOVER_1"), h1, "f, version NOVER_1"));

    return fails;
}
