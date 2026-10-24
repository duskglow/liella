/* Black-box probe of the allocator introspection and debugging interfaces (spec §3.2, §3.3):
 * mallopt and the M_* constants, mallinfo, mallinfo2, malloc_stats, malloc_info, malloc_trim,
 * malloc_usable_size, the hook variables, malloc_get_state/malloc_set_state, mcheck,
 * mcheck_pedantic, mcheck_check_all, mprobe, mtrace, muntrace, mallwatch, tr_break, and the
 * observable effect of the MALLOC_* environment variables and GLIBC_TUNABLES glibc.malloc.*.
 *
 * Usage: mallocdbg_diff MODE [ARG]; normally run by mallocdbg_run.sh PROGRAM SCRATCH [DEBUGLIB].
 *
 * The output is implementation-neutral (D-3.4): no addresses, no PIDs, no byte counts that
 * depend on the allocator's internals. Statistics are printed as relations (grew, unchanged,
 * shrank; at least the request; zero or not), text from malloc_stats and malloc_info with every
 * hexadecimal number replaced by 0x? and every decimal number by N.
 *
 * Names that no installed header declares (the hook variables, __morecore, __default_morecore,
 * malloc_get_state, malloc_set_state, mallwatch, tr_break) are reached only through weak
 * references and dlvsym, both of which may be absent; the probe prints what it finds.
 */
#define _GNU_SOURCE
#undef _FORTIFY_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <malloc.h>
#include <mcheck.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

/* Weak references: NULL when the link did not bind them. */
extern void *dlvsym(void *, const char *, const char *) __attribute__((weak));
extern void *dlopen(const char *, int) __attribute__((weak));
extern void *dlsym(void *, const char *) __attribute__((weak));
extern void *(*__malloc_hook)(size_t, const void *) __attribute__((weak));
extern void (*__free_hook)(void *, const void *) __attribute__((weak));
extern void *malloc_get_state(void) __attribute__((weak));
extern int malloc_set_state(void *) __attribute__((weak));
extern void *mallwatch __attribute__((weak));
extern void tr_break(void) __attribute__((weak));

/* Every allocation escapes through a volatile store, so that the compiler cannot elide an
 * allocation whose block is never used (a malloc/free pair is otherwise removed at -O1). */
static void *volatile sink_;
static void *sinkp(void *p) {
    sink_ = p;
    return p;
}
#define malloc(n) sinkp(malloc(n))
#define calloc(k, n) sinkp(calloc(k, n))
#define realloc(p, n) sinkp(realloc(p, n))
#define memalign(a, n) sinkp(memalign(a, n))
#define aligned_alloc(a, n) sinkp(aligned_alloc(a, n))

static int quiet_ok; /* child(): omit the "child exited 0" line */
static char outbuf[1 << 16];

static void say(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
#include <stdarg.h>
#include "tscale.h"
static void say(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    putchar('\n');
    fflush(stdout);
}

static const char *signame(int s) {
    switch (s) {
    case SIGABRT: return "SIGABRT";
    case SIGSEGV: return "SIGSEGV";
    case SIGBUS: return "SIGBUS";
    case SIGALRM: return "SIGALRM";
    case SIGILL: return "SIGILL";
    case SIGTRAP: return "SIGTRAP";
    case SIGFPE: return "SIGFPE";
    default: return "other";
    }
}

static const char *errname(int e) {
    switch (e) {
    case 0: return "0";
    case EINVAL: return "EINVAL";
    case ENOMEM: return "ENOMEM";
    case ENOSYS: return "ENOSYS";
    case EINTR: return "EINTR";
    case EBADF: return "EBADF";
    case ENOENT: return "ENOENT";
    case EACCES: return "EACCES";
    case ENOTSUP: return "ENOTSUP";
    default: return "other";
    }
}

/* Text normalisation: 0x<hex> -> 0x?; with nums, every run of decimal digits -> N. */
static void norm_line(const char *in, size_t n, int nums, char *out, size_t cap) {
    size_t o = 0;
    for (size_t i = 0; i < n && o + 4 < cap;) {
        unsigned char c = (unsigned char)in[i];
        if (c == '0' && i + 1 < n && in[i + 1] == 'x') {
            size_t j = i + 2;
            while (j < n && strchr("0123456789abcdefABCDEF", in[j]) && in[j]) j++;
            if (j > i + 2) {
                memcpy(out + o, "0x?", 3);
                o += 3;
                i = j;
                continue;
            }
        }
        if (nums && c == ' ') { /* the padding before a right-aligned number depends on its width */
            size_t j = i;
            while (j < n && in[j] == ' ') j++;
            if (j < n && in[j] >= '0' && in[j] <= '9') {
                out[o++] = ' ';
                i = j;
                continue;
            }
        }
        if (nums && c >= '0' && c <= '9') {
            while (i < n && in[i] >= '0' && in[i] <= '9') i++;
            out[o++] = 'N';
            continue;
        }
        if (c == '\t') {
            memcpy(out + o, "\\t", 2);
            o += 2;
        } else if (c < 0x20 || c >= 0x7f) {
            o += (size_t)snprintf(out + o, cap - o, "\\x%02x", c);
        } else {
            out[o++] = (char)c;
        }
        i++;
    }
    out[o] = 0;
}

static void print_text(const char *tag, const char *buf, size_t len, int nums) {
    size_t start = 0, lines = 0;
    static char line[8192];
    for (size_t i = 0; i <= len; i++) {
        if (i == len || buf[i] == '\n') {
            if (i == len && i == start) break;
            norm_line(buf + start, i - start, nums, line, sizeof line);
            say("%s: [%s]%s", tag, line, i == len ? " (no newline)" : "");
            lines++;
            start = i + 1;
        }
    }
    if (lines == 0) say("%s: (empty)", tag);
}

/* Run fn in a child with a time limit; optionally capture its stderr and print it. */
static void child(const char *what, void (*fn)(void), int secs, int capture, int nums) {
    int pfd[2] = {-1, -1};
    if (capture && pipe(pfd) != 0) {
        say("%s: pipe failed", what);
        return;
    }
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        if (capture) {
            close(pfd[0]);
            dup2(pfd[1], 2);
            close(pfd[1]);
        }
        tw_alarm((unsigned)secs);
        fn();
        fflush(stdout);
        _exit(0);
    }
    static char buf[1 << 17];
    size_t len = 0;
    if (capture) {
        close(pfd[1]);
        ssize_t r;
        while ((r = read(pfd[0], buf + len, sizeof buf - 1 - len)) > 0) len += (size_t)r;
        close(pfd[0]);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (capture) print_text("  stderr", buf, len, nums);
    if (WIFSIGNALED(st))
        say("%s: child killed by signal %s", what, signame(WTERMSIG(st)));
    else if (!(quiet_ok && WEXITSTATUS(st) == 0))
        say("%s: child exited %d", what, WEXITSTATUS(st));
}

static const char *rel(size_t before, size_t after) {
    return after > before ? "grew" : after < before ? "shrank" : "unchanged";
}

static const char *zero(size_t v) { return v ? "nonzero" : "zero"; }

/* ------------------------------------------------------------------------- constants */

static void constants(void) {
    say("== constants");
    say("M_MXFAST %d", M_MXFAST);
    say("M_NLBLKS %d", M_NLBLKS);
    say("M_GRAIN %d", M_GRAIN);
    say("M_KEEP %d", M_KEEP);
    say("M_TRIM_THRESHOLD %d", M_TRIM_THRESHOLD);
    say("M_TOP_PAD %d", M_TOP_PAD);
    say("M_MMAP_THRESHOLD %d", M_MMAP_THRESHOLD);
    say("M_MMAP_MAX %d", M_MMAP_MAX);
    say("M_CHECK_ACTION %d", M_CHECK_ACTION);
    say("M_PERTURB %d", M_PERTURB);
    say("M_ARENA_TEST %d", M_ARENA_TEST);
    say("M_ARENA_MAX %d", M_ARENA_MAX);
    say("MCHECK_DISABLED %d MCHECK_OK %d MCHECK_FREE %d MCHECK_HEAD %d MCHECK_TAIL %d", (int)MCHECK_DISABLED,
        (int)MCHECK_OK, (int)MCHECK_FREE, (int)MCHECK_HEAD, (int)MCHECK_TAIL);
    say("sizeof(struct mallinfo) %zu sizeof(struct mallinfo2) %zu", sizeof(struct mallinfo),
        sizeof(struct mallinfo2));
}

/* ------------------------------------------------------------------------- mallopt */

static int g_param, g_value;

static const char *fill_of(const unsigned char *p, size_t n) {
    static char b[32];
    int z = 1;
    for (size_t i = 1; i < n; i++)
        if (p[i] != p[0]) z = 0;
    if (!z) return "mixed";
    snprintf(b, sizeof b, "all 0x%02x", p[0]);
    return b;
}

static void mallopt_one(void) {
    errno = EINTR;
    int r = mallopt(g_param, g_value);
    int e = errno;
    say("mallopt(%d, %d) = %d errno %s", g_param, g_value, r, e == EINTR ? "unchanged" : errname(e));
}

static void mallopt_cases(void) {
    say("== mallopt");
    static const struct {
        int param;
        int values[8];
        int n;
    } cases[] = {
        {M_MXFAST, {0, 1, 64, 128, 160, 161, -1, INT_MAX}, 8},
        {M_NLBLKS, {1, 0}, 2},
        {M_GRAIN, {1, 0}, 2},
        {M_KEEP, {1, 0}, 2},
        {M_TRIM_THRESHOLD, {0, 1, 131072, -1, INT_MAX, INT_MIN}, 6},
        {M_TOP_PAD, {0, 4096, -1, INT_MAX, INT_MIN}, 5},
        {M_MMAP_THRESHOLD, {0, 4096, 131072, 32 << 20, (32 << 20) + 1, 1 << 30, -1, INT_MAX}, 8},
        {M_MMAP_MAX, {0, 1, 65536, -1, INT_MAX}, 5},
        {M_CHECK_ACTION, {0, 1, 2, 3, 5, 7, 8, -1}, 8},
        {M_PERTURB, {0, 0xa5, 256, -1}, 4},
        {M_ARENA_TEST, {0, 1, 8, -1}, 4},
        {M_ARENA_MAX, {0, 1, 4, -1}, 4},
        {0, {1}, 1},
        {5, {1}, 1},
        {100, {1}, 1},
        {-9, {1}, 1},
        {-100, {1}, 1},
        {INT_MIN, {1}, 1},
        {INT_MAX, {1}, 1},
    };
    say("(each call in a fresh child; a child's status is printed only when it did not exit 0)");
    quiet_ok = 1;
    for (size_t c = 0; c < sizeof cases / sizeof cases[0]; c++)
        for (int v = 0; v < cases[c].n; v++) {
            g_param = cases[c].param;
            g_value = cases[c].values[v];
            /* each in a child, so that no setting reaches the next case */
            char what[64];
            snprintf(what, sizeof what, "  (%d, %d)", g_param, g_value);
            child(what, mallopt_one, 5, 0, 0);
        }
    quiet_ok = 0;
}

static void mallopt_effects(void);

/* ------------------------------------------------------------------------- mallinfo */

static void fields_rel(const char *step, struct mallinfo2 a, struct mallinfo2 b) {
    say("%s: arena %s ordblks %s smblks %s hblks %s hblkhd %s usmblks %s fsmblks %s uordblks %s fordblks %s keepcost %s",
        step, rel(a.arena, b.arena), rel(a.ordblks, b.ordblks), rel(a.smblks, b.smblks), rel(a.hblks, b.hblks),
        rel(a.hblkhd, b.hblkhd), rel(a.usmblks, b.usmblks), rel(a.fsmblks, b.fsmblks), rel(a.uordblks, b.uordblks),
        rel(a.fordblks, b.fordblks), rel(a.keepcost, b.keepcost));
}

static void fields_zero(const char *step, struct mallinfo2 a) {
    say("%s: arena %s ordblks %s smblks %s hblks %s hblkhd %s usmblks %s fsmblks %s uordblks %s fordblks %s keepcost %s",
        step, zero(a.arena), zero(a.ordblks), zero(a.smblks), zero(a.hblks), zero(a.hblkhd), zero(a.usmblks),
        zero(a.fsmblks), zero(a.uordblks), zero(a.fordblks), zero(a.keepcost));
}

static const char *as_int(size_t v2, int v) {
    if (v2 <= INT_MAX) return (size_t)v == v2 && v >= 0 ? "equal" : "different";
    if (v == INT_MAX) return "INT_MAX (saturated)";
    if ((uint32_t)v == (uint32_t)v2) return v < 0 ? "low 32 bits (negative)" : "low 32 bits";
    return "other";
}

static void compare_int(const char *step, struct mallinfo2 m2, struct mallinfo m) {
    say("%s: arena %s; ordblks %s; smblks %s; hblks %s; hblkhd %s; usmblks %s; fsmblks %s; uordblks %s; fordblks %s; keepcost %s",
        step, as_int(m2.arena, m.arena), as_int(m2.ordblks, m.ordblks), as_int(m2.smblks, m.smblks),
        as_int(m2.hblks, m.hblks), as_int(m2.hblkhd, m.hblkhd), as_int(m2.usmblks, m.usmblks),
        as_int(m2.fsmblks, m.fsmblks), as_int(m2.uordblks, m.uordblks), as_int(m2.fordblks, m.fordblks),
        as_int(m2.keepcost, m.keepcost));
}

static void mallinfo_basic(void) {
    struct mallinfo2 a = mallinfo2();
    void *small[100];
    for (int i = 0; i < 100; i++) small[i] = malloc(1000);
    struct mallinfo2 b = mallinfo2();
    void *big = malloc(8 << 20);
    struct mallinfo2 c = mallinfo2();
    struct mallinfo ci = mallinfo();
    struct mallinfo2 c2 = mallinfo2();
    free(big);
    struct mallinfo2 d = mallinfo2();
    for (int i = 0; i < 100; i++) free(small[i]);
    struct mallinfo2 e = mallinfo2();
    fields_zero("start (no allocation yet in this process)", a);
    fields_rel("after 100 x malloc(1000)", a, b);
    say("  uordblks grew by at least 100000: %s; arena grew by at least 100000 or was already large enough: %s",
        b.uordblks >= a.uordblks + 100000 ? "yes" : "no", b.arena >= 100000 ? "yes" : "no");
    fields_rel("after malloc(8 MiB)", b, c);
    say("  hblks +1: %s; hblkhd grew by at least 8 MiB: %s; uordblks grew by at least 8 MiB: %s",
        c.hblks == b.hblks + 1 ? "yes" : "no", c.hblkhd >= b.hblkhd + (8 << 20) ? "yes" : "no",
        c.uordblks >= b.uordblks + (8 << 20) ? "yes" : "no");
    say("  fordblks <= arena: %s; keepcost <= fordblks: %s; uordblks + fordblks == arena: %s; uordblks >= hblkhd: %s",
        c.fordblks <= c.arena ? "yes" : "no", c.keepcost <= c.fordblks ? "yes" : "no",
        c.uordblks + c.fordblks == c.arena ? "yes" : "no", c.uordblks >= c.hblkhd ? "yes" : "no");
    say("  mallinfo between two equal mallinfo2 calls: %s",
        memcmp(&c, &c2, sizeof c) == 0 ? "bracketed" : "not bracketed");
    compare_int("  mallinfo vs mallinfo2", c, ci);
    fields_rel("after free(8 MiB block)", c, d);
    say("  hblks back: %s; hblkhd back: %s", d.hblks == b.hblks ? "yes" : "no", d.hblkhd == b.hblkhd ? "yes" : "no");
    fields_rel("after freeing the 100 blocks", d, e);
    say("  uordblks back to start: %s; usmblks %s", e.uordblks == a.uordblks ? "yes" : "no", zero(e.usmblks));
}

static void *thread_alloc(void *arg) {
    void **slot = arg;
    *slot = malloc(5000);
    return NULL;
}

static void *thread_alloc_many(void *arg) {
    void **slots = arg;
    for (int i = 0; i < 200; i++) slots[i] = malloc(1000);
    return NULL;
}

static void mallinfo_thread(void) {
    /* the thread's own allocations (200 KB) dwarf what pthread_create allocates in the caller */
    static void *slots[200];
    /* Both threads on the caller's CPU: a heap with a part per NUMA node (Liella's, D-3.12) maps
     * new memory for a thread that runs on another node, which under load made the arena line
     * depend on where the scheduler put the thread (milestone 6). */
    cpu_set_t was, here;
    int pinned = sched_getaffinity(0, sizeof was, &was) == 0 && sched_getcpu() >= 0;
    if (pinned) {
        CPU_ZERO(&here);
        CPU_SET(sched_getcpu(), &here);
        pinned = sched_setaffinity(0, sizeof here, &here) == 0;
    }
    pthread_t t;
    pthread_create(&t, NULL, thread_alloc, &slots[0]);
    pthread_join(t, NULL); /* a first thread: its creation costs are now paid */
    free(slots[0]);
    struct mallinfo2 a = mallinfo2();
    pthread_create(&t, NULL, thread_alloc_many, slots);
    pthread_join(t, NULL);
    struct mallinfo2 b = mallinfo2();
    fields_rel("after a second thread's 200 x malloc(1000) (blocks alive)", a, b);
    say("  uordblks grew by at least 200000: %s; arena grew by at least 200000: %s",
        b.uordblks >= a.uordblks + 200000 ? "yes" : "no", b.arena >= a.arena + 200000 ? "yes" : "no");
    for (int i = 0; i < 200; i++) free(slots[i]);
    struct mallinfo2 c = mallinfo2();
    say("  after freeing them: uordblks back below the level before the thread + 200000: %s",
        c.uordblks < a.uordblks + 200000 ? "yes" : "no");
    if (pinned)
        sched_setaffinity(0, sizeof was, &was);
}

static void mallinfo_huge_mmap(void) {
    void *p[3];
    int ok = 1;
    struct mallinfo2 a = mallinfo2();
    for (int i = 0; i < 3; i++) {
        p[i] = malloc((size_t)1 << 30); /* never touched: address space only */
        if (!p[i]) ok = 0;
    }
    say("3 x malloc(1 GiB) (untouched): %s", ok ? "all succeeded" : "some failed");
    struct mallinfo2 b = mallinfo2();
    struct mallinfo bi = mallinfo();
    fields_rel("  mallinfo2 change", a, b);
    say("  mallinfo2 hblkhd >= 3 GiB: %s; uordblks >= 3 GiB: %s; arena >= 3 GiB: %s",
        b.hblkhd >= (size_t)3 << 30 ? "yes" : "no", b.uordblks >= (size_t)3 << 30 ? "yes" : "no",
        b.arena >= (size_t)3 << 30 ? "yes" : "no");
    compare_int("  mallinfo vs mallinfo2", b, bi);
    for (int i = 0; i < 3; i++) free(p[i]);
}

static void mallinfo_huge_brk(void) {
    int r = mallopt(M_MMAP_MAX, 0);
    void *p[3];
    int ok = 1;
    struct mallinfo2 a = mallinfo2();
    for (int i = 0; i < 3; i++) {
        p[i] = malloc((size_t)1 << 30);
        if (!p[i]) ok = 0;
    }
    say("mallopt(M_MMAP_MAX, 0) = %d; 3 x malloc(1 GiB) (untouched): %s", r, ok ? "all succeeded" : "some failed");
    struct mallinfo2 b = mallinfo2();
    struct mallinfo bi = mallinfo();
    fields_rel("  mallinfo2 change", a, b);
    say("  mallinfo2 arena >= 3 GiB: %s; uordblks >= 3 GiB: %s; hblkhd >= 3 GiB: %s",
        b.arena >= (size_t)3 << 30 ? "yes" : "no", b.uordblks >= (size_t)3 << 30 ? "yes" : "no",
        b.hblkhd >= (size_t)3 << 30 ? "yes" : "no");
    compare_int("  mallinfo vs mallinfo2", b, bi);
    for (int i = 2; i >= 0; i--) free(p[i]);
    struct mallinfo2 c = mallinfo2();
    say("  after freeing them: arena %s", rel(b.arena, c.arena));
}

static void mallinfo_cases(void) {
    say("== mallinfo and mallinfo2");
    child("-- basic", mallinfo_basic, 20, 0, 0);
    child("-- thread arena", mallinfo_thread, 20, 0, 0);
    say("-- more than 2 GiB through mmap-backed blocks");
    child("   ", mallinfo_huge_mmap, 30, 0, 0);
    say("-- more than 2 GiB through the program break (M_MMAP_MAX 0)");
    child("   ", mallinfo_huge_brk, 30, 0, 0);
}

/* ------------------------------------------------------------------------- usable size */

static void usable_cases(void) {
    say("== malloc_usable_size");
    static const size_t sizes[] = {0, 1, 8, 15, 16, 17, 24, 25, 100, 1000, 4096, 100000, 1 << 20, 8 << 20};
    for (size_t i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        void *p = malloc(sizes[i]);
        size_t u = malloc_usable_size(p);
        say("malloc(%zu): usable >= request %s; usable - request < 4096 + request/64: %s", sizes[i],
            u >= sizes[i] ? "yes" : "no", u - sizes[i] < 4096 + sizes[i] / 64 ? "yes" : "no");
        free(p);
    }
    errno = EINTR;
    size_t z = malloc_usable_size(NULL);
    say("malloc_usable_size(NULL) = %zu errno %s", z, errno == EINTR ? "unchanged" : errname(errno));
    void *p = calloc(3, 33);
    say("calloc(3, 33): usable >= 99 %s", malloc_usable_size(p) >= 99 ? "yes" : "no");
    p = realloc(p, 5000);
    say("realloc to 5000: usable >= 5000 %s", malloc_usable_size(p) >= 5000 ? "yes" : "no");
    p = realloc(p, 10);
    say("realloc to 10: usable >= 10 %s", malloc_usable_size(p) >= 10 ? "yes" : "no");
    free(p);
    p = aligned_alloc(4096, 100);
    say("aligned_alloc(4096, 100): usable >= 100 %s", malloc_usable_size(p) >= 100 ? "yes" : "no");
    free(p);
    p = memalign(64, 1);
    say("memalign(64, 1): usable >= 1 %s", malloc_usable_size(p) >= 1 ? "yes" : "no");
    free(p);
}

/* ------------------------------------------------------------------------- malloc_stats */

static void stats_plain(void) { malloc_stats(); }

static void stats_busy(void) {
    void *a = malloc(1000), *b = malloc(1 << 20);
    void *p = NULL;
    pthread_t t;
    pthread_create(&t, NULL, thread_alloc, &p);
    pthread_join(t, NULL);
    malloc_stats();
    free(a);
    free(b);
    free(p);
}

static void stats_order(void) {
    static char ebuf[256];
    setvbuf(stderr, ebuf, _IOFBF, sizeof ebuf);
    fputs("pending-before|", stderr);
    malloc_stats();
    fputs("after|", stderr);
    fflush(stderr);
}

static void stats_errno(void) {
    errno = EINTR;
    malloc_stats();
    say("  errno after malloc_stats: %s", errno == EINTR ? "unchanged" : errname(errno));
}

static void stats_closed(void) {
    close(2);
    malloc_stats();
    say("  malloc_stats with fd 2 closed returned");
}

static void stats_cases(void) {
    say("== malloc_stats (stderr, numbers replaced)");
    child("-- fresh child", stats_plain, 5, 1, 1);
    child("-- one large block, one small, and a thread's block", stats_busy, 5, 1, 1);
    child("-- stderr fully buffered with pending text", stats_order, 5, 1, 1);
    child("-- errno", stats_errno, 5, 1, 1);
    child("-- fd 2 closed", stats_closed, 5, 0, 0);
}

/* ------------------------------------------------------------------------- malloc_info */

static void info_dump(const char *tag, int options) {
    char *buf = NULL;
    size_t len = 0;
    FILE *f = open_memstream(&buf, &len);
    errno = EINTR;
    int r = malloc_info(options, f);
    int e = errno;
    fclose(f);
    say("%s: malloc_info(%d, memstream) = %d errno %s, %zu bytes written%s", tag, options, r,
        e == EINTR ? "unchanged" : errname(e), len, len ? "" : " (none)");
    if (r == 0) print_text("  xml", buf, len, 1);
    free(buf);
}

static void info_thread(void) {
    void *p = NULL;
    pthread_t t;
    pthread_create(&t, NULL, thread_alloc, &p);
    pthread_join(t, NULL);
    void *big = malloc(1 << 20);
    info_dump("-- after a thread's block and a 1 MiB block", 0);
    free(p);
    free(big);
}

static void info_null(void) { say("  malloc_info(0, NULL) = %d", malloc_info(0, NULL)); }

static void info_readonly(void) {
    FILE *f = fopen("/dev/null", "r");
    errno = EINTR;
    int r = malloc_info(0, f);
    int e = errno;
    say("  malloc_info(0, read-only stream) = %d errno %s; ferror %d", r, e == EINTR ? "unchanged" : errname(e),
        ferror(f) != 0);
    fclose(f);
}

static void info_cases(void) {
    say("== malloc_info (numbers replaced)");
    info_dump("-- options 0", 0);
    info_dump("-- options 1", 1);
    info_dump("-- options -1", -1);
    info_dump("-- options INT_MIN", INT_MIN);
    child("   (thread)", info_thread, 10, 0, 0);
    child("-- NULL stream", info_null, 5, 0, 0);
    child("-- read-only stream", info_readonly, 5, 0, 0);
}

/* ------------------------------------------------------------------------- malloc_trim */

static void trim_seq(void) {
    int r0 = malloc_trim(0);
    static void *blocks[3000];
    for (int i = 0; i < 3000; i++) blocks[i] = malloc(1000);
    int r1 = malloc_trim(0);
    for (int i = 0; i < 2000; i++) free(blocks[i]);
    int r2 = malloc_trim(0);
    int r3 = malloc_trim(0);
    for (int i = 2000; i < 3000; i++) free(blocks[i]);
    int r4 = malloc_trim(0);
    int r5 = malloc_trim(0);
    say("trim at start %d; with 3000 live blocks %d; after freeing the first 2000 %d; again %d; after freeing the rest %d; again %d",
        r0, r1, r2, r3, r4, r5);
}

static void trim_pad(void) {
    mallopt(M_TRIM_THRESHOLD, -1);
    static void *blocks[3000];
    for (int i = 0; i < 3000; i++) blocks[i] = malloc(1000);
    for (int i = 2999; i >= 0; i--) free(blocks[i]);
    struct mallinfo2 a = mallinfo2();
    int r1 = malloc_trim((size_t)1 << 40);
    struct mallinfo2 b = mallinfo2();
    int r2 = malloc_trim(64 << 10);
    struct mallinfo2 c = mallinfo2();
    int r3 = malloc_trim(0);
    struct mallinfo2 d = mallinfo2();
    say("automatic trimming off, 3 MB freed at the top: trim(1 TiB pad) %d arena %s; trim(64 KiB) %d arena %s; trim(0) %d arena %s",
        r1, rel(a.arena, b.arena), r2, rel(b.arena, c.arena), r3, rel(c.arena, d.arena));
    say("  keepcost after trim(0): %s; arena after trim(64 KiB) >= 64 KiB: %s", zero(d.keepcost),
        c.arena >= (64 << 10) ? "yes" : "no");
}

static void trim_mmapped(void) {
    void *p = malloc(16 << 20);
    free(p);
    say("trim after freeing a 16 MiB block: %d", malloc_trim(0));
}

static void trim_thread(void) {
    void *p = NULL;
    pthread_t t;
    pthread_create(&t, NULL, thread_alloc, &p);
    pthread_join(t, NULL);
    malloc_trim(0);
    free(p);
    errno = EINTR;
    int r = malloc_trim(0);
    say("trim after freeing a thread's only block: %d errno %s", r, errno == EINTR ? "unchanged" : errname(errno));
}

static void trim_cases(void) {
    say("== malloc_trim");
    child("-- sequence", trim_seq, 10, 0, 0);
    child("-- pad", trim_pad, 10, 0, 0);
    child("-- mmapped block", trim_mmapped, 10, 0, 0);
    child("-- thread arena", trim_thread, 10, 0, 0);
}

/* ------------------------------------------------------------------------- mallopt effects */

static void eff_perturb(void) {
    int r = mallopt(M_PERTURB, 0xa5);
    unsigned char *p = malloc(200);
    say("mallopt(M_PERTURB, 0xa5) = %d; fresh malloc(200): %s", r, fill_of(p, 200));
    unsigned char *q = calloc(1, 200);
    say("  calloc(1, 200) after it: %s", fill_of(q, 200));
    unsigned char *s = malloc(300000);
    say("  fresh malloc(300000) (mmapped): %s", fill_of(s, 300000));
    p = realloc(p, 400);
    say("  realloc 200 -> 400, bytes 200..399: %s", fill_of(p + 200, 200));
    free(p);
    free(q);
    free(s);
}

static void eff_mmap_threshold(void) {
    struct mallinfo2 a = mallinfo2();
    void *x = malloc(8192);
    struct mallinfo2 b = mallinfo2();
    int r = mallopt(M_MMAP_THRESHOLD, 4096);
    void *y = malloc(8192);
    struct mallinfo2 c = mallinfo2();
    say("malloc(8192) by default: hblks %s; mallopt(M_MMAP_THRESHOLD, 4096) = %d; then malloc(8192): hblks %s", rel(a.hblks, b.hblks),
        r, rel(b.hblks, c.hblks));
    free(x);
    free(y);
    r = mallopt(M_MMAP_THRESHOLD, 64 << 20);
    struct mallinfo2 c2 = mallinfo2();
    void *z = malloc(40 << 20);
    struct mallinfo2 d = mallinfo2();
    say("  mallopt(M_MMAP_THRESHOLD, 64 MiB) = %d; then malloc(40 MiB): hblks %s, arena %s", r, rel(c2.hblks, d.hblks),
        rel(c2.arena, d.arena));
    free(z);
}

static void eff_mmap_max(void) {
    struct mallinfo2 a = mallinfo2();
    void *x = malloc(1 << 20);
    struct mallinfo2 b = mallinfo2();
    int r = mallopt(M_MMAP_MAX, 0);
    void *y = malloc(1 << 20);
    struct mallinfo2 c = mallinfo2();
    say("malloc(1 MiB) by default: hblks %s; mallopt(M_MMAP_MAX, 0) = %d; then malloc(1 MiB): hblks %s, arena %s", rel(a.hblks, b.hblks),
        r, rel(b.hblks, c.hblks), rel(b.arena, c.arena));
    free(x);
    free(y);
}

static void eff_check_action(void) {
    int r = mallopt(M_CHECK_ACTION, 0);
    void *p = malloc(40);
    free(p);
    say("mallopt(M_CHECK_ACTION, 0) = %d; then a double free:", r);
    free(p);
    say("  survived the double free");
}

static void mallopt_effects(void) {
    say("== mallopt effects");
    child("-- M_PERTURB", eff_perturb, 10, 0, 0);
    child("-- M_MMAP_THRESHOLD", eff_mmap_threshold, 10, 0, 0);
    child("-- M_MMAP_MAX", eff_mmap_max, 10, 0, 0);
    child("-- M_CHECK_ACTION 0 and a double free", eff_check_action, 10, 1, 0);
}

/* ------------------------------------------------------------------------- hooks */

static void *libc_handle(void) {
    if (!dlopen) return NULL;
    return dlopen("libc.so.6", RTLD_LAZY | RTLD_NOLOAD);
}

static void *find(void *h, const char *name) {
    if (!dlvsym) return NULL;
    return dlvsym(h, name, "GLIBC_2.2.5");
}

static volatile int hook_calls; /* volatile: changed inside malloc and free (see below) */
/* The hook variables are volatile (as malloc_hook(3) declares them): the compiler assumes that malloc
 * and free do not read the caller's memory and would otherwise drop the stores around the calls. */
static void *(*volatile *mh)(size_t, const void *);
static void (*volatile *fh)(void *, const void *);
static void *(*volatile *rh)(void *, size_t, const void *);
static void *(*volatile *amh)(size_t, size_t, const void *);
static void (*volatile *amc)(void);
static void *(*volatile *mc)(ptrdiff_t);

static void *my_malloc_hook(size_t n, const void *caller) {
    (void)caller;
    hook_calls++;
    void *(*save)(size_t, const void *) = *mh;
    *mh = NULL;
    void *p = malloc(n);
    *mh = save;
    return p;
}
static void my_free_hook(void *p, const void *caller) {
    (void)caller;
    hook_calls++;
    void (*save)(void *, const void *) = *fh;
    *fh = NULL;
    free(p);
    *fh = save;
}
static void *my_realloc_hook(void *p, size_t n, const void *caller) {
    (void)caller;
    hook_calls++;
    void *(*save)(void *, size_t, const void *) = *rh;
    *rh = NULL;
    void *q = realloc(p, n);
    *rh = save;
    return q;
}
static void *my_memalign_hook(size_t a, size_t n, const void *caller) {
    (void)caller;
    hook_calls++;
    void *(*save)(size_t, size_t, const void *) = *amh;
    *amh = NULL;
    void *q = memalign(a, n);
    *amh = save;
    return q;
}
static void my_after_morecore(void) { hook_calls++; }
static void *my_morecore(ptrdiff_t d) {
    (void)d;
    hook_calls++;
    return NULL;
}

static void *hook_lookup_handle;

static void hooks_assign(void) {
    void *h = hook_lookup_handle;
    mh = find(h, "__malloc_hook");
    fh = find(h, "__free_hook");
    rh = find(h, "__realloc_hook");
    amh = find(h, "__memalign_hook");
    amc = find(h, "__after_morecore_hook");
    mc = find(h, "__morecore");
    if (mh) {
        hook_calls = 0;
        *mh = my_malloc_hook;
        void *p = malloc(77);
        *mh = NULL;
        say("  __malloc_hook assigned: malloc returned %s, hook called %d times", p ? "a block" : "NULL", hook_calls);
        free(p);
    }
    if (fh) {
        void *p = malloc(77);
        hook_calls = 0;
        *fh = my_free_hook;
        free(p);
        *fh = NULL;
        say("  __free_hook assigned: hook called %d times", hook_calls);
    }
    if (rh) {
        void *p = malloc(77);
        hook_calls = 0;
        *rh = my_realloc_hook;
        p = realloc(p, 5000);
        *rh = NULL;
        say("  __realloc_hook assigned: realloc returned %s, hook called %d times", p ? "a block" : "NULL", hook_calls);
        free(p);
    }
    if (amh) {
        hook_calls = 0;
        *amh = my_memalign_hook;
        void *p = memalign(64, 100);
        void *q = aligned_alloc(64, 128);
        *amh = NULL;
        say("  __memalign_hook assigned: memalign and aligned_alloc returned %s, hook called %d times",
            p && q ? "blocks" : "NULL", hook_calls);
        free(p);
        free(q);
    }
    if (amc) {
        hook_calls = 0;
        *amc = my_after_morecore;
        static void *b[64];
        for (int i = 0; i < 64; i++) b[i] = malloc(100000);
        *amc = NULL;
        say("  __after_morecore_hook assigned, 6.4 MB from the heap: hook called %d times", hook_calls);
        for (int i = 0; i < 64; i++) free(b[i]);
    }
    if (mc) {
        hook_calls = 0;
        void *(*save)(ptrdiff_t) = *mc;
        *mc = my_morecore;
        static void *b[64];
        int ok = 1;
        for (int i = 0; i < 64; i++)
            if (!(b[i] = malloc(100000))) ok = 0;
        *mc = save;
        say("  __morecore replaced by a failing function, 6.4 MB from the heap: %s, it was called %d times",
            ok ? "all succeeded" : "some failed", hook_calls);
        for (int i = 0; i < 64; i++) free(b[i]);
    }
}

static void hooks_default_morecore(void) {
    void *(*dm)(ptrdiff_t) = find(hook_lookup_handle, "__default_morecore");
    if (!dm) return;
    void *before = sbrk(0);
    errno = EINTR;
    void *r = dm(0);
    int e = errno;
    say("  __default_morecore(0): %s, errno %s", r == before ? "the current break" : r ? "another pointer" : "NULL",
        e == EINTR ? "unchanged" : errname(e));
}

static void get_state(void) {
    void *(*get)(void) = find(hook_lookup_handle, "malloc_get_state");
    if (!get) return;
    errno = 0;
    void *s = get();
    int e = errno;
    say("  malloc_get_state() = %s errno %s", s ? "non-NULL" : "NULL", errname(e));
}

static int set_case;
static void set_state(void) {
    int (*set)(void *) = find(hook_lookup_handle, "malloc_set_state");
    if (!set) return;
    static long junk[512];
    static const char *what[] = {"NULL", "a zeroed 4 KiB buffer", "a 4 KiB buffer of 0xff bytes",
                                 "a heap block of 4 KiB, zeroed"};
    void *arg = NULL;
    if (set_case == 2) memset(junk, 0xff, sizeof junk);
    if (set_case == 1 || set_case == 2) arg = junk;
    if (set_case == 3) arg = calloc(1, 4096);
    errno = 0;
    int r = set(arg);
    int e = errno;
    say("  malloc_set_state(%s) = %d errno %s", what[set_case], r, errname(e));
}

static void get_set_state(void) {
    child("-- malloc_get_state", get_state, 10, 1, 0);
    for (set_case = 0; set_case < 4; set_case++) child("-- malloc_set_state", set_state, 10, 1, 0);
}

static void hooks_cases(const char *heading, int use_default) {
    say("== %s", heading);
    say("weak references (unversioned, resolved at run time): __malloc_hook %s, __free_hook %s, malloc_get_state %s, malloc_set_state %s, mallwatch %s, tr_break %s",
        &__malloc_hook ? "bound" : "unbound", &__free_hook ? "bound" : "unbound", malloc_get_state ? "bound" : "unbound",
        malloc_set_state ? "bound" : "unbound", &mallwatch ? "bound" : "unbound", tr_break ? "bound" : "unbound");
    say("dlopen %s, dlvsym %s", dlopen ? "linked" : "absent", dlvsym ? "linked" : "absent");
    void *libc = libc_handle();
    hook_lookup_handle = use_default ? RTLD_DEFAULT : libc;
    if (!dlvsym || (!use_default && !libc)) { /* RTLD_DEFAULT is the null handle */
        say("no handle for the lookup: the rest of this section is skipped");
        return;
    }
    static const char *names[] = {"__malloc_hook",    "__free_hook",      "__realloc_hook",
                                  "__memalign_hook",  "__after_morecore_hook", "__malloc_initialize_hook",
                                  "__morecore",       "__default_morecore", "malloc_get_state",
                                  "malloc_set_state", "mallwatch",        "tr_break",
                                  "mcheck",           "mcheck_pedantic",  "mcheck_check_all",
                                  "mprobe",           "mtrace",           "muntrace"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        void *a = find(hook_lookup_handle, names[i]);
        void *b = libc ? find(libc, names[i]) : NULL;
        void *d = dlsym ? dlsym(hook_lookup_handle, names[i]) : NULL;
        say("%s@GLIBC_2.2.5: %s%s; unversioned dlsym: %s", names[i], a ? "present" : "absent",
            use_default && a ? (a == b ? " (libc.so.6's)" : " (not libc.so.6's)") : "", d ? "found" : "not found");
    }
    void **mw = find(hook_lookup_handle, "mallwatch");
    if (mw) say("mallwatch initially %s", *mw ? "non-NULL" : "NULL");
    void (*tb)(void) = find(hook_lookup_handle, "tr_break");
    if (tb) {
        tb();
        say("tr_break() returned");
    }
    for (int i = 0; i < 2; i++) {
        void **v = find(hook_lookup_handle, i ? "__free_hook" : "__malloc_hook");
        if (v) say("%s initially %s", i ? "__free_hook" : "__malloc_hook", *v ? "non-NULL" : "NULL");
    }
    child("-- assigning the hooks", hooks_assign, 10, 1, 0);
    child("-- __default_morecore", hooks_default_morecore, 10, 1, 0);
    get_set_state();
}

/* MODE hooks-early: the hooks assigned before the first allocation of the process. */
static volatile int init_hook_calls;
static void my_init_hook(void) { init_hook_calls++; }

static void run_hooks_early(const char *which) {
    void *h = dlvsym ? RTLD_DEFAULT : NULL;
    void (*volatile *ih)(void) = dlvsym && strchr(which, 'i') ? find(h, "__malloc_initialize_hook") : NULL;
    mh = dlvsym && strchr(which, 'm') ? find(h, "__malloc_hook") : NULL;
    fh = dlvsym && strchr(which, 'f') ? find(h, "__free_hook") : NULL;
    if (ih) *ih = my_init_hook;
    if (mh) *mh = my_malloc_hook;
    if (fh) *fh = my_free_hook;
    hook_calls = 0;
    void *p = malloc(50); /* the process's first allocation */
    free(p);
    int calls = hook_calls;
    if (mh) *mh = NULL;
    if (fh) *fh = NULL;
    setvbuf(stdout, outbuf, _IOFBF, sizeof outbuf);
    say("== hook variables assigned before the first allocation (through the global scope): %s", which);
    if (!dlvsym) {
        say("dlvsym absent: skipped");
        return;
    }
    say("__malloc_initialize_hook %s, __malloc_hook %s, __free_hook %s", ih ? "assigned" : "not assigned",
        mh ? "assigned" : "not assigned", fh ? "assigned" : "not assigned");
    say("first malloc and free: malloc/free hooks called %d times, initialize hook called %d times", calls,
        init_hook_calls);
}

/* ------------------------------------------------------------------------- mcheck family */

static volatile int abort_calls; /* volatile: changed inside malloc and free */
static void my_abortfn(enum mcheck_status s) {
    abort_calls++;
    const char *n = s == MCHECK_OK ? "MCHECK_OK" : s == MCHECK_HEAD ? "MCHECK_HEAD" : s == MCHECK_TAIL ? "MCHECK_TAIL"
                  : s == MCHECK_FREE ? "MCHECK_FREE" : s == MCHECK_DISABLED ? "MCHECK_DISABLED" : "other";
    say("  abortfn called with %s", n);
}

static const char *stname(enum mcheck_status s) {
    switch (s) {
    case MCHECK_DISABLED: return "MCHECK_DISABLED";
    case MCHECK_OK: return "MCHECK_OK";
    case MCHECK_FREE: return "MCHECK_FREE";
    case MCHECK_HEAD: return "MCHECK_HEAD";
    case MCHECK_TAIL: return "MCHECK_TAIL";
    default: return "other";
    }
}

static void mcheck_late(void) {
    void *p = malloc(10);
    errno = EINTR;
    int r = mcheck(my_abortfn);
    say("mcheck(abortfn) after allocations = %d errno %s", r, errno == EINTR ? "unchanged" : errname(errno));
    r = mcheck_pedantic(my_abortfn);
    say("mcheck_pedantic(abortfn) after allocations = %d", r);
    r = mcheck(NULL);
    say("mcheck(NULL) after allocations = %d", r);
    say("mprobe(live block) = %s", stname(mprobe(p)));
    char *q = malloc(16);
    char q16 = ((volatile char *)q)[16];
    ((volatile char *)q)[16] = 'x';
    say("mprobe(block written one past its end) = %s", stname(mprobe(q)));
    ((volatile char *)q)[16] = q16; /* restored */
    mcheck_check_all();
    say("mcheck_check_all() returned; abortfn calls %d", abort_calls);
    free(p);
    free(q);
}

static void mtrace_plain(void) {
    errno = EINTR;
    mtrace();
    void *p = malloc(10);
    free(p);
    muntrace();
    say("mtrace(), malloc, free, muntrace() returned; errno %s", errno == EINTR ? "unchanged" : errname(errno));
    muntrace();
    say("muntrace() again returned");
}

/* MODE mcheck-early: mcheck before any allocation, then mprobe and detection with abortfn. */
static int early_rc;

static void early_tail(void) {
    char *p = malloc(10);
    say("mprobe(fresh block) = %s", stname(mprobe(p)));
    ((volatile char *)p)[10] = 'x';
    say("mprobe(after writing p[10]) = %s", stname(mprobe(p)));
    free(p);
    say("free returned; abortfn calls %d", abort_calls);
}
static void early_head(void) {
    char *p = malloc(10);
    char save = p[-1];
    ((volatile char *)p)[-1] ^= 0x5a;
    say("mprobe(after changing p[-1]) = %s", stname(mprobe(p)));
    ((volatile char *)p)[-1] = save;
    say("mprobe(after restoring p[-1]) = %s", stname(mprobe(p)));
    free(p);
    say("free returned; abortfn calls %d", abort_calls);
}
static void early_double(void) {
    char *p = malloc(10);
    free(p);
    say("mprobe(freed block) = %s", stname(mprobe(p)));
    free(p);
    say("second free returned; abortfn calls %d", abort_calls);
}
static void early_checkall(void) {
    char *p = malloc(10);
    char *q = malloc(30);
    char q30 = ((volatile char *)q)[30];
    ((volatile char *)q)[30] = 'y';
    mcheck_check_all();
    say("mcheck_check_all() with a clobbered tail returned; abortfn calls %d", abort_calls);
    ((volatile char *)q)[30] = q30; /* restored */
    free(p);
    free(q);
    say("restored, both freed; abortfn calls %d", abort_calls);
}
static void early_realloc(void) {
    char *p = malloc(10);
    ((volatile char *)p)[10] = 'x';
    p = realloc(p, 100);
    say("realloc of a clobbered block returned %s; abortfn calls %d", p ? "a block" : "NULL", abort_calls);
}
static void early_mtrace_too(void) {
    char *p = malloc(10);
    say("mprobe(live block) = %s", stname(mprobe(p)));
    say("mprobe(NULL) in a child next");
    free(p);
}
static void early_mprobe_null(void) { say("mprobe(NULL) = %s", stname(mprobe(NULL))); }
static void early_mprobe_foreign(void) {
    static char buf[64];
    say("mprobe(static buffer) = %s", stname(mprobe(buf + 32)));
}
static void early_second_mcheck(void) {
    int r = mcheck(my_abortfn);
    say("a second mcheck(abortfn) = %d", r);
}

static void run_mcheck_early(const char *how) {
    say("== mcheck before the first allocation (%s)", how);
    say("mcheck rc %d", early_rc);
    child("-- second call", early_second_mcheck, 5, 1, 0);
    child("-- tail", early_tail, 5, 1, 0);
    child("-- head", early_head, 5, 1, 0);
    child("-- double free", early_double, 5, 1, 0);
    child("-- mcheck_check_all", early_checkall, 5, 1, 0);
    child("-- realloc of a clobbered block", early_realloc, 5, 1, 0);
    child("-- mprobe", early_mtrace_too, 5, 1, 0);
    child("-- mprobe(NULL)", early_mprobe_null, 5, 1, 0);
    child("-- mprobe(not a heap block)", early_mprobe_foreign, 5, 1, 0);
}

/* MODE pedantic */
static void pedantic_body(void) {
    char *a = malloc(10);
    char a10 = ((volatile char *)a)[10];
    ((volatile char *)a)[10] = 'x';
    say("clobbered a's tail; next: malloc(20)");
    char *b = malloc(20);
    say("malloc(20) returned; abortfn calls %d", abort_calls);
    ((volatile char *)a)[10] = a10; /* restored */
    free(a);
    free(b);
    say("restored, both freed; abortfn calls %d", abort_calls);
}

/* MODE default-abort-*: mcheck(NULL) then an error; the default handler's message. */
static void default_tail(void) {
    char *p = malloc(10);
    ((volatile char *)p)[10] = 'x';
    free(p);
    say("free returned");
}
static void default_head(void) {
    char *p = malloc(10);
    ((volatile char *)p)[-1] ^= 0x5a;
    free(p);
    say("free returned");
}
static void default_double(void) {
    char *p = malloc(10);
    free(p);
    free(p);
    say("second free returned");
}

/* MODE mtrace: MALLOC_TRACE names a file; trace a fixed sequence and print the log. */
static void run_mtrace(const char *file) {
    say("== mtrace (MALLOC_TRACE %s)", file ? "set" : "unset");
    if (file) unlink(file);
    errno = EINTR;
    mtrace();
    int e = errno;
    void *a = malloc(10);
    void *b = calloc(4, 8);
    a = realloc(a, 100);
    void *c = memalign(64, 50);
    void *d = NULL;
    int pr = posix_memalign(&d, 64, 70);
    free(b);
    free(c);
    free(d);
    free(NULL);
    void *z = realloc(NULL, 5);
    void *zz = realloc(z, 0);
    muntrace();
    void *after = malloc(3);
    free(after);
    free(a);
    (void)pr;
    (void)zz;
    say("errno after mtrace: %s", e == EINTR ? "unchanged" : errname(e));
    if (!file) return;
    struct stat st;
    if (stat(file, &st) != 0) {
        say("trace file: not created");
        return;
    }
    mode_t um = umask(022);
    umask(um);
    say("trace file: created, mode 0666 less the umask: %s", (st.st_mode & 0777) == (0666 & ~um) ? "yes" : "no");
    FILE *f = fopen(file, "r");
    static char buf[1 << 16];
    size_t len = f ? fread(buf, 1, sizeof buf, f) : 0;
    if (f) fclose(f);
    /* Each line is shown with its shape: the caller as <prog>:(<sym>+<off>)[0x?] or <prog>:[0x?]
     * (or [0x?] alone), every block address as <addr>, sizes kept. */
    size_t start = 0;
    while (start < len) {
        size_t end = start;
        while (end < len && buf[end] != '\n') end++;
        char line[1024], out[2048];
        size_t n = end - start < sizeof line - 1 ? end - start : sizeof line - 1;
        memcpy(line, buf + start, n);
        line[n] = 0;
        size_t o = 0;
        char *save = NULL;
        int field = 0, at = line[0] == '@';
        for (char *t = strtok_r(line, " ", &save); t; t = strtok_r(NULL, " ", &save), field++) {
            char tok[600];
            if (at && field == 1) {
                char *colon = strrchr(t, ':');
                char *paren = strchr(t, '(');
                char *brack = strrchr(t, '[');
                if (colon && paren && brack)
                    snprintf(tok, sizeof tok, "<prog>:(<sym>+<off>)[0x?]");
                else if (colon && brack)
                    snprintf(tok, sizeof tok, "<prog>:[0x?]");
                else if (brack)
                    snprintf(tok, sizeof tok, "[0x?]");
                else
                    snprintf(tok, sizeof tok, "<caller %s>", t);
            } else if (at && field == 3) {
                snprintf(tok, sizeof tok, "%s", strncmp(t, "0x", 2) == 0 ? "<addr>" : t);
            } else {
                snprintf(tok, sizeof tok, "%s", t);
            }
            o += (size_t)snprintf(out + o, sizeof out - o, "%s%s", o ? " " : "", tok);
        }
        out[o] = 0;
        say("  log: [%s]%s", out, end == len ? " (no newline)" : "");
        start = end + 1;
    }
    if (len == 0) say("  log: (empty)");
    unlink(file);
}

/* MODE env: the observable effect of the environment (MALLOC_*_, GLIBC_TUNABLES). */
static void fingerprint(void) {
    struct mallinfo2 m0 = mallinfo2();
    unsigned char *p = malloc(200);
    const char *fill = fill_of(p, 200);
    struct mallinfo2 m1 = mallinfo2();
    void *x = malloc(64 << 10);
    struct mallinfo2 m2 = mallinfo2();
    void *y = malloc(1 << 20);
    struct mallinfo2 m3 = mallinfo2();
    free(x);
    free(y);
    static void *blk[40];
    for (int i = 0; i < 40; i++) blk[i] = malloc(60000);
    struct mallinfo2 m4 = mallinfo2();
    for (int i = 39; i >= 0; i--) free(blk[i]);
    struct mallinfo2 m5 = mallinfo2();
    static void *sm[7];
    for (int i = 0; i < 7; i++) sm[i] = malloc(32);
    struct mallinfo2 m6 = mallinfo2();
    for (int i = 0; i < 7; i++) free(sm[i]);
    struct mallinfo2 m7 = mallinfo2();
    say("fresh malloc(200): %s", fill);
    say("first allocation: arena %s, arena - uordblks >= 1 MiB: %s", rel(m0.arena, m1.arena),
        m1.arena - m1.uordblks >= (1 << 20) ? "yes" : "no");
    say("malloc(64 KiB): hblks %s", rel(m1.hblks, m2.hblks));
    say("malloc(1 MiB): hblks %s", rel(m2.hblks, m3.hblks));
    say("40 x malloc(60000) then freed in reverse: arena %s then %s; back to its size before: %s", rel(m3.arena, m4.arena),
        rel(m4.arena, m5.arena), m5.arena <= m3.arena ? "yes" : "no");
    say("7 x malloc(32) then freed: uordblks %s then %s; back: %s", rel(m5.uordblks, m6.uordblks), rel(m6.uordblks, m7.uordblks),
        m7.uordblks == m5.uordblks ? "yes" : "no");
    free(p);
    void *tp = NULL;
    pthread_t t;
    pthread_create(&t, NULL, thread_alloc, &tp);
    pthread_join(t, NULL);
    char *xml = NULL;
    size_t xlen = 0;
    FILE *f = open_memstream(&xml, &xlen);
    malloc_info(0, f);
    fclose(f);
    int heaps = 0;
    for (char *s = xml; s && (s = strstr(s, "<heap ")); s++) heaps++;
    say("after a second thread's malloc(5000): malloc_info lists %s",
        heaps == 1 ? "one heap" : heaps == 2 ? "two heaps" : "another number of heaps");
    free(xml);
    free(tp);
}

static void env_overrun(void) {
    char *p = malloc(24);
    memset(p, 'a', 24);
    ((volatile char *)p)[24] = 'b';
    free(p);
    void *q = malloc(24);
    free(q);
    say("one-byte overrun, free, malloc, free: survived");
}

static void env_double(void) {
    void *p = malloc(40);
    free(p);
    free(p);
    say("double free: survived");
}

static void env_badptr(void) {
    char *p = malloc(40);
    free(p + 16);
    say("free of an interior pointer: survived");
    (void)p;
}

static void run_env(void) {
    say("fingerprint:");
    fingerprint();
    child("-- one-byte overrun", env_overrun, 5, 1, 0);
    child("-- double free", env_double, 5, 1, 0);
    child("-- free of an interior pointer", env_badptr, 5, 1, 0);
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "main";
    struct rlimit nocore = {0, 0};
    setrlimit(RLIMIT_CORE, &nocore); /* no core files from the crashing children */
    tw_alarm(120);
    if (strcmp(mode, "mcheck-early") == 0 || strcmp(mode, "mcheck-null") == 0 || strcmp(mode, "pedantic") == 0) {
        int pedantic = strcmp(mode, "pedantic") == 0;
        early_rc = pedantic ? mcheck_pedantic(my_abortfn) : mcheck(strcmp(mode, "mcheck-null") == 0 ? NULL : my_abortfn);
        setvbuf(stdout, outbuf, _IOFBF, sizeof outbuf);
        if (strcmp(mode, "mcheck-early") == 0) {
            run_mcheck_early("abortfn");
        } else if (pedantic) {
            say("== mcheck_pedantic before the first allocation");
            say("mcheck_pedantic rc %d", early_rc);
            child("-- clobber, then another malloc", pedantic_body, 5, 1, 0);
            child("-- mcheck_check_all", early_checkall, 5, 1, 0);
        } else {
            say("== mcheck(NULL) before the first allocation: the default handler");
            say("mcheck rc %d", early_rc);
            child("-- tail", default_tail, 5, 1, 0);
            child("-- head", default_head, 5, 1, 0);
            child("-- double free", default_double, 5, 1, 0);
        }
        return 0;
    }
    if (strcmp(mode, "hooks-early") == 0) {
        run_hooks_early(argc > 2 ? argv[2] : "imf");
        return 0;
    }
    setvbuf(stdout, outbuf, _IOFBF, sizeof outbuf);
    if (strcmp(mode, "mtrace") == 0) {
        run_mtrace(getenv("MALLOC_TRACE"));
        return 0;
    }
    if (strcmp(mode, "env") == 0) {
        run_env();
        return 0;
    }
    if (strcmp(mode, "hooks-preloaded") == 0) {
        hooks_cases("hook variables, found through the global scope", 1);
        return 0;
    }
    if (strcmp(mode, "late") == 0) {
        say("== mcheck, mprobe, mtrace after allocations");
        child("-- mcheck family", mcheck_late, 5, 1, 0);
        child("-- mtrace without MALLOC_TRACE", mtrace_plain, 5, 1, 0);
        return 0;
    }
    if (strcmp(mode, "main") != 0) {
        say("unknown mode");
        return 2;
    }
    constants();
    mallopt_cases();
    mallopt_effects();
    mallinfo_cases();
    usable_cases();
    stats_cases();
    info_cases();
    trim_cases();
    hooks_cases("hook variables and compat symbols in libc.so.6", 0);
    say("== mcheck, mprobe, mtrace after allocations");
    child("-- mcheck family", mcheck_late, 5, 1, 0);
    child("-- mtrace without MALLOC_TRACE", mtrace_plain, 5, 1, 0);
    say("done");
    return 0;
}
