/* Spec §27.3 (s27_3_c_0: the address of a TLS variable of a module loaded by dlopen, for the
 * calling thread) and §27.1 (s27_1_f_0: static TLS exhaustion): objects with PT_TLS opened by
 * dlopen, their variables reached in the global-dynamic, local-dynamic, initial-exec and TLS
 * descriptor models, from the main thread, from a thread created before the dlopen (T1), one
 * created after it (T2) and 32 threads created while the dlopen runs (racer). Each thread sees the
 * initial image, zeros past it, the variable's alignment and a block of its own. Then: a block
 * larger than the static TLS block fails with the probed text, and 80 failed opens of an object
 * with 1 KiB of TLS (an undefined function) each fail for that function, not for TLS room.
 * Each failed check prints its line; the exit status is the number of failed checks. */
/* DLOPEN-HELPERS: dlt_tls dlt_desc dlt_leak dlt_big */
/* HELPER-FLAGS: dlt_desc -mtls-dialect=gnu2 */
/* LINK-EXPORT-DYNAMIC: yes */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static atomic_int fails;
#define CHECK(who, c) do { if (!(c)) { fprintf(stderr, "s27_dlopen_tls_surplus: %s: line %d: %s\n", (who), __LINE__, #c); atomic_fetch_add(&fails, 1); } } while (0)

typedef void *(*getter)(int);
static getter mods[2];
static void *main_gd, *main_dg;
static atomic_int opened;
static sem_t go;
__thread int dlt_prog_tls = 21; /* exported; the helper dlt_tls reaches it through a TLS relocation */

/* Called by the helpers' constructors (exported: LINK-EXPORT-DYNAMIC). */
void dlt_register(int module, getter get)
{
    if (module >= 0 && module < 2)
        mods[module] = get;
}

static int zeros(const char *p, int n)
{
    for (int i = 0; i < n; i++)
        if (p[i])
            return 0;
    return 1;
}

static uintptr_t gap(const void *a, const void *b)
{
    return (uintptr_t)a > (uintptr_t)b ? (uintptr_t)a - (uintptr_t)b : (uintptr_t)b - (uintptr_t)a;
}

/* The calling thread's blocks of both objects: the initial images and zeros (no other thread's
 * values), the alignment, one block reached both through the dtv (GD) and from the thread pointer
 * (IE); then (write) values of its own, read back. */
static void check_thread(const char *who, int write)
{
    CHECK(who, mods[0] != NULL && mods[1] != NULL);
    if (mods[0] == NULL || mods[1] == NULL)
        return;
    int *gd = mods[0](0), *ld = mods[0](1), *ie = mods[0](2);
    char *bss = mods[0](3);
    long *al = mods[0](4);
    int *dg = mods[1](0);
    char *dz = mods[1](1);
    CHECK(who, *gd == 7);
    CHECK(who, *ld == 9);
    CHECK(who, *ie == 11);
    CHECK(who, *al == 5);
    CHECK(who, ((uintptr_t)al & 63) == 0);
    CHECK(who, zeros(bss, 64));
    CHECK(who, gap(gd, ie) < 256 && gap(gd, al) < 256);
    CHECK(who, mods[0](5) == (void *)&dlt_prog_tls && dlt_prog_tls == 21);
    CHECK(who, *dg == 13);
    CHECK(who, zeros(dz, 200));
    if (main_gd != NULL) {
        CHECK(who, (void *)gd != main_gd);
        CHECK(who, (void *)dg != main_dg);
    }
    if (write) {
        *gd = 70;
        *ld = 90;
        *ie = 110;
        *dg = 130;
        dz[199] = 1;
        CHECK(who, *(int *)mods[0](0) == 70 && *(int *)mods[0](1) == 90 && *(int *)mods[0](2) == 110);
        CHECK(who, *(int *)mods[1](0) == 130 && ((char *)mods[1](1))[199] == 1);
    }
}

static void *t1_main(void *arg)
{
    (void)arg;
    while (sem_wait(&go) != 0)
        ;
    check_thread("T1", 1);
    return NULL;
}

static void *t2_main(void *arg)
{
    (void)arg;
    check_thread("T2", 1);
    return NULL;
}

static void *racer(void *arg)
{
    (void)arg;
    while (atomic_load(&opened) == 0)
        sched_yield();
    check_thread("racer", 1);
    return NULL;
}

/* 32 threads created while the main thread runs its dlopen calls. */
static void *creator(void *arg)
{
    (void)arg;
    pthread_t t[32];
    int n = 0;
    for (; n < 32; n++)
        if (pthread_create(&t[n], NULL, racer, NULL) != 0)
            break;
    CHECK("creator", n == 32);
    for (int i = 0; i < n; i++)
        pthread_join(t[i], NULL);
    return NULL;
}

static const char *at(const char *d, const char *f)
{
    static char b[2][4096];
    static int i;
    i ^= 1;
    snprintf(b[i], sizeof b[i], "%s/%s", d, f);
    return b[i];
}

int main(void)
{
    const char *dir = getenv("CONF_HELPERS");
    if (dir == NULL) {
        fprintf(stderr, "s27_dlopen_tls_surplus: CONF_HELPERS is not set\n");
        return 1;
    }
    pthread_t t1, t2, c;
    CHECK("main", sem_init(&go, 0, 0) == 0);
    CHECK("main", pthread_create(&t1, NULL, t1_main, NULL) == 0);
    CHECK("main", pthread_create(&c, NULL, creator, NULL) == 0);
    void *h0 = dlopen(at(dir, "libdlt_tls.so"), RTLD_NOW);
    if (h0 == NULL)
        fprintf(stderr, "s27_dlopen_tls_surplus: %s\n", dlerror());
    void *h1 = dlopen(at(dir, "libdlt_desc.so"), RTLD_NOW);
    if (h1 == NULL)
        fprintf(stderr, "s27_dlopen_tls_surplus: %s\n", dlerror());
    CHECK("main", h0 != NULL && h1 != NULL);
    check_thread("main", 0);
    if (mods[0] != NULL && mods[1] != NULL) {
        main_gd = mods[0](0);
        main_dg = mods[1](0);
        *(int *)mods[0](0) = 71;
        *(int *)mods[1](0) = 131;
    }
    atomic_store(&opened, 1);
    CHECK("main", sem_post(&go) == 0);
    pthread_join(t1, NULL);
    CHECK("main", pthread_create(&t2, NULL, t2_main, NULL) == 0);
    pthread_join(t2, NULL);
    pthread_join(c, NULL);
    if (mods[0] != NULL && mods[1] != NULL)
        CHECK("main", *(int *)mods[0](0) == 71 && *(int *)mods[1](0) == 131 && *(int *)mods[0](2) == 11);

    /* No room in the static TLS block: the probed text, once. */
    const char *big = at(dir, "libdlt_big.so");
    char want[4200];
    snprintf(want, sizeof want, "%s: cannot allocate memory in static TLS block", big);
    void *hb = dlopen(big, RTLD_NOW);
    const char *e = dlerror();
    CHECK("main", hb == NULL && e != NULL && strcmp(e, want) == 0);
    if (hb != NULL || e == NULL || strcmp(e, want) != 0)
        fprintf(stderr, "s27_dlopen_tls_surplus: want [%s], got [%s]\n", want, e ? e : "no text");

    /* A failed open gives its TLS place back: every attempt fails for the undefined function. */
    const char *leak = at(dir, "libdlt_leak.so");
    for (int i = 0; i < 80; i++) {
        void *h = dlopen(leak, RTLD_NOW);
        e = dlerror();
        int ok = h == NULL && e != NULL && strstr(e, "undefined symbol: dlt_leak_missing") != NULL;
        CHECK("main", ok);
        if (!ok) {
            fprintf(stderr, "s27_dlopen_tls_surplus: attempt %d: %s\n", i, e ? e : "no text");
            break;
        }
    }
    return atomic_load(&fails);
}
