/* LEGACY-PROBE-COMPAT (LG-0b): the old (compat) versions of the affinity calls, pthread_getattr_np and
 * pthread_sigmask against the default ones. Black-box: each old version is bound with .symver; every
 * call that might take a different argument shape runs in a forked child, so a crash is a result
 * (printed as the signal). Masks are reported as byte counts and relations, never as CPU numbers,
 * except the kernel's own mask size, which the lines are relative to. Every line starts with the
 * versioned name it observes. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

typedef int (*sga_t)(pid_t, size_t, void *);
typedef int (*pga_t)(pthread_t, size_t, void *);
typedef int (*aga_t)(pthread_attr_t *, size_t, void *);
#define OLD(ret, local, sym, ver, ...) extern ret local(__VA_ARGS__); __asm__(".symver " #local ", " #sym "@" #ver);
OLD(int, sched_getaffinity_233, sched_getaffinity, GLIBC_2.3.3, pid_t, size_t, void *)
OLD(int, sched_setaffinity_233, sched_setaffinity, GLIBC_2.3.3, pid_t, size_t, const void *)
OLD(int, pthread_getaffinity_np_233, pthread_getaffinity_np, GLIBC_2.3.3, pthread_t, size_t, void *)
OLD(int, pthread_getaffinity_np_234, pthread_getaffinity_np, GLIBC_2.3.4, pthread_t, size_t, void *)
OLD(int, pthread_setaffinity_np_233, pthread_setaffinity_np, GLIBC_2.3.3, pthread_t, size_t, const void *)
OLD(int, pthread_setaffinity_np_234, pthread_setaffinity_np, GLIBC_2.3.4, pthread_t, size_t, const void *)
OLD(int, pthread_attr_getaffinity_np_233, pthread_attr_getaffinity_np, GLIBC_2.3.3, pthread_attr_t *, size_t, void *)
OLD(int, pthread_attr_getaffinity_np_234, pthread_attr_getaffinity_np, GLIBC_2.3.4, pthread_attr_t *, size_t, void *)
OLD(int, pthread_attr_setaffinity_np_233, pthread_attr_setaffinity_np, GLIBC_2.3.3, pthread_attr_t *, size_t, const void *)
OLD(int, pthread_attr_setaffinity_np_234, pthread_attr_setaffinity_np, GLIBC_2.3.4, pthread_attr_t *, size_t, const void *)
OLD(int, pthread_getattr_np_225, pthread_getattr_np, GLIBC_2.2.5, pthread_t, pthread_attr_t *)
OLD(int, pthread_sigmask_225, pthread_sigmask, GLIBC_2.2.5, int, const sigset_t *, sigset_t *)

static const char *en(int e) { const char *s = strerrorname_np(e); return e == 0 ? "0" : s ? s : "?"; }

#define BIG 8192
static unsigned char cur[BIG];
static long ksize;

/* Bytes of a buffer pre-filled with 0xAA that the call overwrote (index of the last changed byte + 1),
 * and whether the bytes below the kernel size equal the current mask. */
static void report_get(const unsigned char *b, size_t n) {
    size_t last = 0;
    for (size_t i = 0; i < n; i++) if (b[i] != 0xAA) last = i + 1;
    int eq = 1;
    for (size_t i = 0; i < n && i < (size_t)ksize; i++) if (b[i] != cur[i]) eq = 0;
    int zero = 1;
    for (size_t i = (size_t)ksize; i < last; i++) if (b[i] != 0) zero = 0;
    printf(" touched=%zu%s%s", last, last ? (eq ? " mask=current" : " mask=OTHER") : "", last > (size_t)ksize ? (zero ? " tail=zero" : " tail=NONZERO") : "");
}

static const size_t sizes[] = {0, 1, 4, 7, 8, 16, 64, 127, 128, 512, 1024, 4096, BIG, ((size_t)1 << 32) + 128, (size_t)-1};
#define NSIZES (sizeof sizes / sizeof *sizes)

enum kind { SCHED_GET, SCHED_SET, PT_GET, PT_SET, ATTR_GET, ATTR_SET };
static void run_one(const char *name, enum kind k, void *fn, size_t sz, int extra_bit) {
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        static unsigned char b[BIG];
        memset(b, 0xAA, sizeof b);
        if (k == SCHED_SET || k == PT_SET || k == ATTR_SET) { memset(b, 0, sizeof b); memcpy(b, cur, (size_t)ksize); if (extra_bit) b[extra_bit / 8] |= (unsigned char)(1u << (extra_bit % 8)); }
        char szs[32];
        if (sz == (size_t)-1) strcpy(szs, "SIZE_MAX"); else if (sz > 0xffffffffu) strcpy(szs, "2^32+128"); else snprintf(szs, sizeof szs, "%zu", sz);
        int r = 0, e = 0;
        pthread_attr_t at; pthread_attr_init(&at);
        errno = 0;
        switch (k) {
        case SCHED_GET: r = ((sga_t)fn)(0, sz, b); e = errno; break;
        case SCHED_SET: r = ((int (*)(pid_t, size_t, const void *))fn)(0, sz, b); e = errno; break;
        case PT_GET: r = ((pga_t)fn)(pthread_self(), sz, b); break;
        case PT_SET: r = ((int (*)(pthread_t, size_t, const void *))fn)(pthread_self(), sz, b); break;
        case ATTR_GET: r = ((aga_t)fn)(&at, sz, b); break;
        case ATTR_SET: {
            r = ((int (*)(pthread_attr_t *, size_t, const void *))fn)(&at, sz, b);
            static unsigned char g[BIG]; memset(g, 0xAA, sizeof g);
            int r2 = pthread_attr_getaffinity_np(&at, sizeof g, (cpu_set_t *)g);
            size_t last = 0; for (size_t i = 0; i < sizeof g; i++) if (g[i] != 0xAA) last = i + 1;
            printf("%s size=%s: ret=%s readback(default get, %d bytes)=%s touched=%zu\n", name, szs, en(r), BIG, en(r2), last);
            _exit(0);
        }
        }
        printf("%s size=%s%s: ret=%d", name, szs, extra_bit ? " +bit beyond kernel cpus" : "", r);
        if (k == SCHED_GET || k == SCHED_SET) printf(" errno=%s", en(r < 0 ? e : 0)); else printf(" (%s)", en(r));
        if (k == SCHED_GET || k == PT_GET || k == ATTR_GET) report_get(b, sz < BIG ? sz : BIG);
        printf("\n");
        _exit(0);
    }
    int st; waitpid(pid, &st, 0);
    if (WIFSIGNALED(st)) printf("%s size=%zu: child killed by signal %d\n", name, sz, WTERMSIG(st));
}

static void probe_affinity(void) {
    struct { const char *n; enum kind k; void *f; } t[] = {
        {"sched_getaffinity@GLIBC_2.3.3", SCHED_GET, (void *)sched_getaffinity_233},
        {"sched_getaffinity@GLIBC_2.3.4 [default]", SCHED_GET, (void *)sched_getaffinity},
        {"sched_setaffinity@GLIBC_2.3.3", SCHED_SET, (void *)sched_setaffinity_233},
        {"sched_setaffinity@GLIBC_2.3.3 [default 2.3.4]", SCHED_SET, (void *)sched_setaffinity},
        {"pthread_getaffinity_np@GLIBC_2.3.3", PT_GET, (void *)pthread_getaffinity_np_233},
        {"pthread_getaffinity_np@GLIBC_2.3.4", PT_GET, (void *)pthread_getaffinity_np_234},
        {"pthread_getaffinity_np@GLIBC_2.3.4 [default 2.32]", PT_GET, (void *)pthread_getaffinity_np},
        {"pthread_setaffinity_np@GLIBC_2.3.3", PT_SET, (void *)pthread_setaffinity_np_233},
        {"pthread_setaffinity_np@GLIBC_2.3.4", PT_SET, (void *)pthread_setaffinity_np_234},
        {"pthread_setaffinity_np@GLIBC_2.3.4 [default 2.34]", PT_SET, (void *)pthread_setaffinity_np},
        {"pthread_attr_getaffinity_np@GLIBC_2.3.3", ATTR_GET, (void *)pthread_attr_getaffinity_np_233},
        {"pthread_attr_getaffinity_np@GLIBC_2.3.4", ATTR_GET, (void *)pthread_attr_getaffinity_np_234},
        {"pthread_attr_getaffinity_np@GLIBC_2.3.4 [default 2.34]", ATTR_GET, (void *)pthread_attr_getaffinity_np},
        {"pthread_attr_setaffinity_np@GLIBC_2.3.3", ATTR_SET, (void *)pthread_attr_setaffinity_np_233},
        {"pthread_attr_setaffinity_np@GLIBC_2.3.4", ATTR_SET, (void *)pthread_attr_setaffinity_np_234},
        {"pthread_attr_setaffinity_np@GLIBC_2.3.4 [default 2.32]", ATTR_SET, (void *)pthread_attr_setaffinity_np},
    };
    for (size_t i = 0; i < sizeof t / sizeof *t; i++) {
        for (size_t s = 0; s < NSIZES; s++) run_one(t[i].n, t[i].k, t[i].f, sizes[s], 0);
        if (t[i].k == SCHED_SET || t[i].k == PT_SET) run_one(t[i].n, t[i].k, t[i].f, 1024, 8000);
    }
}

/* ---- pthread_getattr_np@GLIBC_2.2.5 against @@GLIBC_2.32 ---- */
static void attr_cmp(const char *who, pthread_t th) {
    pthread_attr_t a0, a1;
    int r0 = pthread_getattr_np_225(th, &a0), r1 = pthread_getattr_np(th, &a1);
    printf("pthread_getattr_np@GLIBC_2.2.5 %s: ret=%s | default: ret=%s", who, en(r0), en(r1));
    if (r0 == 0 && r1 == 0) {
        void *s0, *s1; size_t z0, z1, g0, g1; int d0, d1, sc0, sc1, i0, i1, p0, p1;
        pthread_attr_getstack(&a0, &s0, &z0); pthread_attr_getstack(&a1, &s1, &z1);
        pthread_attr_getguardsize(&a0, &g0); pthread_attr_getguardsize(&a1, &g1);
        pthread_attr_getdetachstate(&a0, &d0); pthread_attr_getdetachstate(&a1, &d1);
        pthread_attr_getscope(&a0, &sc0); pthread_attr_getscope(&a1, &sc1);
        pthread_attr_getinheritsched(&a0, &i0); pthread_attr_getinheritsched(&a1, &i1);
        pthread_attr_getschedpolicy(&a0, &p0); pthread_attr_getschedpolicy(&a1, &p1);
        static unsigned char m0[BIG], m1[BIG];
        memset(m0, 0xAA, BIG); memset(m1, 0xAA, BIG);
        int q0 = pthread_attr_getaffinity_np(&a0, BIG, (cpu_set_t *)m0), q1 = pthread_attr_getaffinity_np(&a1, BIG, (cpu_set_t *)m1);
        printf("; stack %s, guard %s(%zu), detach %d/%d, scope %d/%d, inherit %d/%d, policy %d/%d, affinity-in-attr %s/%s %s",
               s0 == s1 && z0 == z1 ? "equal" : "DIFFER", g0 == g1 ? "equal" : "DIFFER", g1, d0, d1, sc0, sc1, i0, i1, p0, p1, en(q0), en(q1),
               memcmp(m0, m1, BIG) ? "masks DIFFER" : "masks equal");
        pthread_attr_destroy(&a0); pthread_attr_destroy(&a1);
    }
    printf("\n");
}
static void *thr(void *p) { (void)p; attr_cmp("created joinable thread (self)", pthread_self()); return NULL; }
static void *thr2(void *p) { (void)p; attr_cmp("created detached thread (self)", pthread_self()); return NULL; }

/* ---- pthread_sigmask@GLIBC_2.2.5 against @@GLIBC_2.32 ---- */
static void kmask(unsigned long *m) { syscall(SYS_rt_sigprocmask, SIG_BLOCK, NULL, m, 8); }
static void sm_case(int old) {
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        int (*fn)(int, const sigset_t *, sigset_t *) = old ? pthread_sigmask_225 : pthread_sigmask;
        const char *tag = old ? "" : "[default] ";
        sigset_t s, o; unsigned long k;
        memset(&s, 0, sizeof s);
        ((unsigned long *)&s)[0] = (1UL << 31) | (1UL << 32) | (1UL << 33) | (1UL << (SIGUSR1 - 1));
        int r = fn(SIG_BLOCK, &s, &o); kmask(&k);
        printf("pthread_sigmask@GLIBC_2.2.5 %sSIG_BLOCK {USR1,32,33,34}: ret=%s kernel mask=%#lx\n", tag, en(r), k);
        memset(&s, 0xff, sizeof s);
        r = fn(SIG_SETMASK, &s, &o); kmask(&k);
        printf("pthread_sigmask@GLIBC_2.2.5 %sSIG_SETMASK all-ones: ret=%s kernel mask=%#lx old word0=%#lx\n", tag, en(r), k, ((unsigned long *)&o)[0]);
        r = fn(SIG_BLOCK, NULL, &o);
        printf("pthread_sigmask@GLIBC_2.2.5 %squery: ret=%s word0=%#lx\n", tag, en(r), ((unsigned long *)&o)[0]);
        r = fn(99, &s, &o);
        printf("pthread_sigmask@GLIBC_2.2.5 %show=99: ret=%d (%s)\n", tag, r, en(r > 0 ? r : 0));
        r = fn(99, NULL, &o);
        printf("pthread_sigmask@GLIBC_2.2.5 %show=99 set=NULL: ret=%d (%s)\n", tag, r, en(r > 0 ? r : 0));
        _exit(0);
    }
    int st; waitpid(pid, &st, 0);
    if (WIFSIGNALED(st)) printf("pthread_sigmask@GLIBC_2.2.5 %s: child killed by signal %d\n", old ? "old" : "default", WTERMSIG(st));
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    ksize = syscall(SYS_sched_getaffinity, 0, (size_t)BIG, cur);
    printf("# kernel sched_getaffinity size=%ld bytes (the lines below are relative to it)\n", ksize);
    if (ksize <= 0) return 1;
    probe_affinity();
    attr_cmp("main thread", pthread_self());
    pthread_t t; pthread_create(&t, NULL, thr, NULL); pthread_join(t, NULL);
    pthread_attr_t da; pthread_attr_init(&da); pthread_attr_setdetachstate(&da, PTHREAD_CREATE_DETACHED);
    pthread_create(&t, &da, thr2, NULL); sleep(1);
    sm_case(1); sm_case(0);
    return 0;
}
