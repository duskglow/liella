/* POSIX asynchronous I/O end to end (spec §11.11; D-11.8). Black-box: built against the host C
 * library and against Liella's products, the outputs must match line for line. Each line is a
 * call's result and errno, an aio_error/aio_return pair, or a fact about a notification. Nothing
 * prints a descriptor number, an address, a PID or a time; waits are shown only against coarse
 * thresholds. Requests that must stay in progress are reads from an empty pipe or writes into a
 * full one, which cannot complete until the probe feeds or drains the pipe.
 *
 * Usage: aio_diff DIR             the record (DIR: an empty scratch directory)
 *        aio_diff DIR extra       observations outside the record (aio.md, "Outside the record")
 *        aio_diff DIR afterexec … internal: the image the exec section starts
 *
 * Every section runs in its own forked child, so each starts with no AIO state, a crash ends
 * only that section, and each has its own alarm. _FORTIFY_SOURCE (on by default in Ubuntu's
 * compiler at -O1) is undefined so that the plain functions are called. */
#undef _FORTIFY_SOURCE
#define _GNU_SOURCE
#include <aio.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "tscale.h"

static const char *D;
static pthread_t main_t;
/* How long a wait for a completion may last before it is reported as still in progress. It only
 * ends a wait that would otherwise hang: a request returns as soon as it completes, and on a
 * loaded machine with a slow disk an fsync can take minutes (seen on the timing VM: over 120 s
 * while the rest of the test suite was writing). */
#define SLOW_MS 600000
static int wait_limit_ms = SLOW_MS;

static void P(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    putchar('\n');
    fflush(stdout);
}

static const char *en(int e)
{
    static char b[4][24];
    static int k;
    switch (e) {
    case -1: return "-1";
    case 0: return "0";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case EINTR: return "EINTR";
    case EIO: return "EIO";
    case EBADF: return "EBADF";
    case EAGAIN: return "EAGAIN";
    case ENOMEM: return "ENOMEM";
    case EFAULT: return "EFAULT";
    case EISDIR: return "EISDIR";
    case EINVAL: return "EINVAL";
    case EFBIG: return "EFBIG";
    case ESPIPE: return "ESPIPE";
    case EPIPE: return "EPIPE";
    case ENOSYS: return "ENOSYS";
    case EOVERFLOW: return "EOVERFLOW";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    case ECANCELED: return "ECANCELED";
    case EINPROGRESS: return "EINPROGRESS";
    case EROFS: return "EROFS";
    case ENOTCONN: return "ENOTCONN";
    default: break;
    }
    char *p = b[k++ & 3];
    snprintf(p, sizeof b[0], "errno#%d", e);
    return p;
}

static const char *signame(int s)
{
    static char b[24];
    switch (s) {
    case SIGPIPE: return "SIGPIPE";
    case SIGALRM: return "SIGALRM";
    case SIGSEGV: return "SIGSEGV";
    case SIGBUS: return "SIGBUS";
    case SIGABRT: return "SIGABRT";
    case SIGXFSZ: return "SIGXFSZ";
    case SIGKILL: return "SIGKILL";
    case SIGUSR1: return "SIGUSR1";
    case SIGUSR2: return "SIGUSR2";
    case SIGFPE: return "SIGFPE";
    case SIGILL: return "SIGILL";
    case SIGTRAP: return "SIGTRAP";
    default: break;
    }
    if (s >= SIGRTMIN && s <= SIGRTMAX)
        snprintf(b, sizeof b, "SIGRTMIN+%d", s - SIGRTMIN);
    else
        snprintf(b, sizeof b, "signal#%d", s);
    return b;
}

static const char *cancel_name(int r)
{
    static char b[24];
    if (r == AIO_CANCELED) return "AIO_CANCELED";
    if (r == AIO_NOTCANCELED) return "AIO_NOTCANCELED";
    if (r == AIO_ALLDONE) return "AIO_ALLDONE";
    snprintf(b, sizeof b, "%d", r);
    return b;
}

static const char *code_name(int c)
{
    static char b[24];
    if (c == SI_ASYNCIO) return "SI_ASYNCIO";
    if (c == SI_QUEUE) return "SI_QUEUE";
    if (c == SI_USER) return "SI_USER";
    if (c == SI_TKILL) return "SI_TKILL";
    if (c == SI_KERNEL) return "SI_KERNEL";
    snprintf(b, sizeof b, "%d", c);
    return b;
}

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

/* Long enough for a request handed to the library to have started (a pipe read to be blocked in
 * the kernel). Only the cancel cases depend on this; see aio.md. */
static void settle(void) { sleep_ms(400); }

static char *path(const char *name)
{
    static char b[4][PATH_MAX];
    static int k;
    char *p = b[k++ & 3];
    snprintf(p, PATH_MAX, "%s/%s", D, name);
    return p;
}

static unsigned char pat(long i) { return (unsigned char)(i * 7 + 3); }

static void mkdata(const char *name, long n)
{
    int fd = open(path(name), O_WRONLY | O_CREAT | O_TRUNC, 0600);
    unsigned char buf[4096];
    for (long i = 0; i < n;) {
        long k = n - i < 4096 ? n - i : 4096;
        for (long j = 0; j < k; j++)
            buf[j] = pat(i + j);
        if (write(fd, buf, (size_t)k) != k)
            P("fixture write failed");
        i += k;
    }
    close(fd);
}

static int data_ok(const unsigned char *b, long off, ssize_t n)
{
    for (ssize_t i = 0; i < n; i++)
        if (b[i] != pat(off + i))
            return 0;
    return 1;
}

static void mk(struct aiocb *cb, int fd, const volatile void *buf, size_t n, off_t off)
{
    memset(cb, 0, sizeof *cb);
    cb->aio_fildes = fd;
    cb->aio_buf = (volatile void *)(uintptr_t)buf;
    cb->aio_nbytes = n;
    cb->aio_offset = off;
    cb->aio_sigevent.sigev_notify = SIGEV_NONE;
}


/* aio_error then aio_return, in that order (argument evaluation order is unspecified). */
static const char *ST(struct aiocb *cb)
{
    static char b[4][64];
    static int k;
    char *o = b[k++ & 3];
    int e = aio_error(cb);
    ssize_t r = aio_return(cb);
    snprintf(o, sizeof b[0], "aio_error=%s aio_return=%zd", en(e), r);
    return o;
}
static const char *ST64(struct aiocb64 *cb)
{
    static char b[4][64];
    static int k;
    char *o = b[k++ & 3];
    int e = aio_error64(cb);
    ssize_t r = aio_return64(cb);
    snprintf(o, sizeof b[0], "aio_error64=%s aio_return64=%zd", en(e), r);
    return o;
}

static int done(const struct aiocb *cb) { return aio_error(cb) != EINPROGRESS; }

static int wait_done(const struct aiocb *cb, int ms)
{
    for (int i = 0; i < ms; i++) {
        if (done(cb))
            return 1;
        sleep_ms(1);
    }
    return done(cb);
}

/* Print a submission's outcome: on failure the errno; on success the final aio_error and
 * aio_return. Returns aio_return's value (or -2/-3). */
static ssize_t fin(const char *label, int rc, int err, struct aiocb *cb)
{
    if (rc != 0) {
        P("%s: submit=%d errno=%s", label, rc, en(err));
        return -2;
    }
    if (!wait_done(cb, wait_limit_ms)) {
        P("%s: submit=0 still EINPROGRESS after %d s", label, wait_limit_ms / 1000);
        return -3;
    }
    int e = aio_error(cb);
    errno = 0;
    ssize_t r = aio_return(cb);
    int re = errno;
    if (r == -1)
        P("%s: submit=0 error=%s return=-1 (errno after aio_return %s)", label, en(e), en(re));
    else
        P("%s: submit=0 error=%s return=%zd", label, en(e), r);
    return r;
}

#define SUBMIT(label, expr, cb)          \
    ({                                   \
        errno = 0;                       \
        int rc_ = (expr);                \
        int e_ = errno;                  \
        fin(label, rc_, e_, cb);         \
    })

static void block_sig(int s)
{
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, s);
    pthread_sigmask(SIG_BLOCK, &m, NULL);
}

static void unblock_sig(int s)
{
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, s);
    pthread_sigmask(SIG_UNBLOCK, &m, NULL);
}

/* Wait for one signal of `s` (blocked) for up to ms; returns the signal or -1. */
static int take(int s, siginfo_t *si, long ms)
{
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, s);
    struct timespec to = {ms / 1000, (ms % 1000) * 1000000L};
    memset(si, 0, sizeof *si);
    int r;
    do
        r = sigtimedwait(&m, si, &to);
    while (r == -1 && errno == EINTR);
    return r;
}

static int pending(int s)
{
    sigset_t m;
    sigpending(&m);
    return sigismember(&m, s);
}

static int fionread(int fd)
{
    int q = -1;
    ioctl(fd, FIONREAD, &q);
    return q;
}

/* Wait until a pipe holds `want` bytes (a write request is then in progress). */
static int wait_fill(int rfd, int want)
{
    int q = 0;
    for (int i = 0; i < 5000 && q < want; i++) {
        q = fionread(rfd);
        if (q < want)
            sleep_ms(1);
    }
    return q;
}

static long drain(int fd, long want)
{
    char b[8192];
    long got = 0;
    while (got < want) {
        ssize_t r = read(fd, b, sizeof b);
        if (r <= 0)
            break;
        got += r;
    }
    return got;
}

/* A thread that writes to a descriptor after a delay. */
struct dw {
    int fd;
    int ms;
    const void *buf;
    size_t n;
};
static void *dw_run(void *a)
{
    struct dw *d = a;
    sleep_ms(d->ms);
    if (write(d->fd, d->buf, d->n) < 0)
        P("delayed write failed");
    return NULL;
}

/* A thread that sends SIGUSR1 to the main thread after a delay. */
static void *kick_run(void *a)
{
    sleep_ms((long)(intptr_t)a);
    pthread_kill(main_t, SIGUSR1);
    return NULL;
}
static pthread_t kick(int ms)
{
    pthread_t t;
    pthread_create(&t, NULL, kick_run, (void *)(intptr_t)ms);
    return t;
}

static volatile sig_atomic_t hcount;
static void hnd(int s)
{
    (void)s;
    hcount++;
}
static void on_usr1(int restart)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = hnd;
    sa.sa_flags = restart ? SA_RESTART : 0;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGUSR1, &sa, NULL);
}

static long g_arg;
static int E;

/* errno only for a failed call: on success errno is unspecified. */
static const char *ee(long r, int e) { return r == -1 ? en(e) : "-"; }

static void run_section(const char *name, void (*fn)(void), int secs)
{
    P("== %s", name);
    pid_t p = fork();
    if (p == 0) {
        tw_alarm((unsigned)secs);
        main_t = pthread_self();
        fn();
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    while (waitpid(p, &st, 0) == -1 && errno == EINTR) {
    }
    if (WIFEXITED(st))
        P("-- end %s: exit %d", name, WEXITSTATUS(st));
    else if (WIFSIGNALED(st))
        P("-- end %s: killed by %s", name, signame(WTERMSIG(st)));
}

/* ------------------------------------------------------------------ constants and layout */

static void s_consts(void)
{
    P("AIO_CANCELED=%d AIO_NOTCANCELED=%d AIO_ALLDONE=%d", AIO_CANCELED, AIO_NOTCANCELED, AIO_ALLDONE);
    P("LIO_READ=%d LIO_WRITE=%d LIO_NOP=%d LIO_WAIT=%d LIO_NOWAIT=%d", LIO_READ, LIO_WRITE, LIO_NOP, LIO_WAIT, LIO_NOWAIT);
    P("SIGEV_SIGNAL=%d SIGEV_NONE=%d SIGEV_THREAD=%d SI_ASYNCIO=%d", SIGEV_SIGNAL, SIGEV_NONE, SIGEV_THREAD, SI_ASYNCIO);
    P("sizeof(struct aiocb)=%zu sizeof(struct aiocb64)=%zu sizeof(struct aioinit)=%zu", sizeof(struct aiocb), sizeof(struct aiocb64), sizeof(struct aioinit));
    P("offsetof aiocb: aio_fildes=%zu aio_lio_opcode=%zu aio_reqprio=%zu aio_buf=%zu aio_nbytes=%zu aio_sigevent=%zu aio_offset=%zu",
      offsetof(struct aiocb, aio_fildes), offsetof(struct aiocb, aio_lio_opcode), offsetof(struct aiocb, aio_reqprio),
      offsetof(struct aiocb, aio_buf), offsetof(struct aiocb, aio_nbytes), offsetof(struct aiocb, aio_sigevent),
      offsetof(struct aiocb, aio_offset));
    P("offsetof aiocb64: aio_fildes=%zu aio_lio_opcode=%zu aio_reqprio=%zu aio_buf=%zu aio_nbytes=%zu aio_sigevent=%zu aio_offset=%zu",
      offsetof(struct aiocb64, aio_fildes), offsetof(struct aiocb64, aio_lio_opcode), offsetof(struct aiocb64, aio_reqprio),
      offsetof(struct aiocb64, aio_buf), offsetof(struct aiocb64, aio_nbytes), offsetof(struct aiocb64, aio_sigevent),
      offsetof(struct aiocb64, aio_offset));
    P("offsetof aioinit: aio_threads=%zu aio_num=%zu aio_idle_time=%zu", offsetof(struct aioinit, aio_threads),
      offsetof(struct aioinit, aio_num), offsetof(struct aioinit, aio_idle_time));
#ifdef AIO_PRIO_DELTA_MAX
    P("AIO_PRIO_DELTA_MAX=%d", AIO_PRIO_DELTA_MAX);
#else
    P("AIO_PRIO_DELTA_MAX undefined");
#endif
#ifdef AIO_LISTIO_MAX
    P("AIO_LISTIO_MAX=%ld", (long)AIO_LISTIO_MAX);
#else
    P("AIO_LISTIO_MAX undefined");
#endif
#ifdef AIO_MAX
    P("AIO_MAX=%ld", (long)AIO_MAX);
#else
    P("AIO_MAX undefined");
#endif
#ifdef _POSIX_ASYNCHRONOUS_IO
    P("_POSIX_ASYNCHRONOUS_IO=%ld", (long)_POSIX_ASYNCHRONOUS_IO);
#endif
#ifdef _POSIX_PRIORITIZED_IO
    P("_POSIX_PRIORITIZED_IO=%ld", (long)_POSIX_PRIORITIZED_IO);
#endif
    errno = 0;
    long v = sysconf(_SC_AIO_LISTIO_MAX);
    E = errno;
    P("sysconf(_SC_AIO_LISTIO_MAX)=%ld errno=%s", v, en(E));
    errno = 0;
    v = sysconf(_SC_AIO_MAX);
    E = errno;
    P("sysconf(_SC_AIO_MAX)=%ld errno=%s", v, en(E));
    errno = 0;
    v = sysconf(_SC_AIO_PRIO_DELTA_MAX);
    E = errno;
    P("sysconf(_SC_AIO_PRIO_DELTA_MAX)=%ld errno=%s", v, en(E));
    errno = 0;
    v = sysconf(_SC_ASYNCHRONOUS_IO);
    E = errno;
    P("sysconf(_SC_ASYNCHRONOUS_IO)=%ld errno=%s", v, en(E));
    errno = 0;
    v = sysconf(_SC_PRIORITIZED_IO);
    E = errno;
    P("sysconf(_SC_PRIORITIZED_IO)=%ld errno=%s", v, en(E));
}

/* ------------------------------------------------------------------ aio_init */

/* Exercise the interface after aio_init: reads, a write, a list and a notification thread. */
static volatile int simple_thread_calls;
static void simple_thread(union sigval v)
{
    (void)v;
    __atomic_add_fetch(&simple_thread_calls, 1, __ATOMIC_SEQ_CST);
}

static void after_init(void)
{
    mkdata("init.dat", 300);
    int fd = open(path("init.dat"), O_RDWR);
    unsigned char b1[100], b2[100];
    struct aiocb c1, c2, c3, *l[2];
    mk(&c1, fd, b1, 100, 0);
    mk(&c2, fd, b2, 100, 250);
    aio_read(&c1);
    aio_read(&c2);
    wait_done(&c1, 5000);
    wait_done(&c2, 5000);
    ssize_t r1 = aio_return(&c1), r2 = aio_return(&c2);
    P("two aio_read: return=%zd,%zd data=%s", r1, r2, data_ok(b1, 0, r1) && data_ok(b2, 250, r2) ? "ok" : "bad");
    mk(&c3, fd, "zz", 2, 300);
    SUBMIT("aio_write", aio_write(&c3), &c3);
    mk(&c1, fd, b1, 10, 0);
    c1.aio_lio_opcode = LIO_READ;
    mk(&c2, fd, b2, 10, 298);
    c2.aio_lio_opcode = LIO_READ;
    l[0] = &c1;
    l[1] = &c2;
    errno = 0;
    int r = lio_listio(LIO_WAIT, l, 2, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, 2 reads): %d errno=%s returns=%zd,%zd", r, ee(r, E), aio_return(&c1), aio_return(&c2));
    mk(&c3, fd, b1, 10, 0);
    c3.aio_sigevent.sigev_notify = SIGEV_THREAD;
    c3.aio_sigevent.sigev_notify_function = simple_thread;
    /* Wait for THIS request's notification: the count before it, not 0 (the second call in
       "aio_init twice" starts at 1, and a wait for "not 0" there waited for nothing, so under
       load the line read the count before the notification thread had run). The 50 ms after
       it give a duplicate call time to show; the count printed stays the section's total. */
    int before = __atomic_load_n(&simple_thread_calls, __ATOMIC_SEQ_CST);
    aio_read(&c3);
    for (int i = 0; i < 5000 && __atomic_load_n(&simple_thread_calls, __ATOMIC_SEQ_CST) == before; i++)
        sleep_ms(1);
    sleep_ms(50);
    P("SIGEV_THREAD notification: calls=%d return=%zd", simple_thread_calls, aio_return(&c3));
    close(fd);
    unlink(path("init.dat"));
}

static void s_init_small(void)
{
    struct aioinit ai;
    memset(&ai, 0, sizeof ai);
    ai.aio_threads = 1;
    ai.aio_num = 1;
    ai.aio_idle_time = 0;
    aio_init(&ai);
    P("aio_init(threads=1, num=1, idle_time=0): returned");
    after_init();
}

static void s_init_negative(void)
{
    struct aioinit ai;
    memset(&ai, 0xff, sizeof ai);
    aio_init(&ai);
    P("aio_init(every field -1): returned");
    after_init();
}

static void s_init_twice(void)
{
    struct aioinit ai;
    memset(&ai, 0, sizeof ai);
    ai.aio_threads = 4;
    ai.aio_num = 64;
    ai.aio_idle_time = 5;
    aio_init(&ai);
    after_init();
    ai.aio_threads = 1;
    aio_init(&ai);
    P("aio_init again after requests completed: returned");
    after_init();
}

/* ------------------------------------------------------------------ aio_read on a regular file */

static void s_read(void)
{
    mkdata("data", 1000);
    int fd = open(path("data"), O_RDONLY);
    lseek(fd, 10, SEEK_SET);
    unsigned char buf[4096];
    struct aiocb cb;
    char label[160];
    static const struct {
        long off;
        size_t n;
    } cs[] = {{0, 100}, {500, 100}, {950, 100}, {999, 100}, {1000, 100}, {5000, 100}, {0, 0}};
    for (size_t i = 0; i < sizeof cs / sizeof cs[0]; i++) {
        memset(buf, 0xee, sizeof buf);
        mk(&cb, fd, buf, cs[i].n, cs[i].off);
        snprintf(label, sizeof label, "aio_read off=%ld n=%zu (file of 1000 bytes)", cs[i].off, cs[i].n);
        ssize_t r = SUBMIT(label, aio_read(&cb), &cb);
        if (r > 0)
            P("  data matches the file: %s; byte after the data untouched: %s", data_ok(buf, cs[i].off, r) ? "yes" : "no", buf[r] == 0xee ? "yes" : "no");
    }
    P("file offset after the reads (set to 10 before; POSIX: unspecified): %ld", (long)lseek(fd, 0, SEEK_CUR));

    mk(&cb, fd, buf, 10, 0);
    cb.aio_lio_opcode = LIO_WRITE;
    SUBMIT("aio_read with aio_lio_opcode=LIO_WRITE", aio_read(&cb), &cb);
    mk(&cb, fd, buf, 10, 0);
    cb.aio_lio_opcode = LIO_NOP;
    SUBMIT("aio_read with aio_lio_opcode=LIO_NOP", aio_read(&cb), &cb);
    mk(&cb, fd, buf, 10, 0);
    cb.aio_lio_opcode = 99;
    SUBMIT("aio_read with aio_lio_opcode=99", aio_read(&cb), &cb);

    static const long offs[] = {-1, LONG_MIN, LONG_MAX, LONG_MAX - 5};
    static const char *offn[] = {"-1", "LLONG_MIN", "LLONG_MAX", "LLONG_MAX-5"};
    for (int i = 0; i < 4; i++) {
        mk(&cb, fd, buf, 10, offs[i]);
        snprintf(label, sizeof label, "aio_read off=%s n=10", offn[i]);
        SUBMIT(label, aio_read(&cb), &cb);
    }
    static const size_t ns[] = {(size_t)SSIZE_MAX + 1, SIZE_MAX, (size_t)SSIZE_MAX, (size_t)INT_MAX + 1};
    static const char *nn[] = {"SSIZE_MAX+1", "SIZE_MAX", "SSIZE_MAX", "INT_MAX+1"};
    /* Far from the top of the address space: Linux 5.10 refuses (EFAULT) a count whose end
     * passes the top of user space, before it shortens the count to MAX_RW_COUNT, so a buffer
     * on a stack placed within 2 GiB of the top made the INT_MAX+1 case fail at random (2 of 9
     * runs; the stack's place is random), whatever the library (kernel-5.10 validation,
     * 2026-10-05). */
    static unsigned char low[4096];
    for (int i = 0; i < 4; i++) {
        mk(&cb, fd, low, ns[i], 0);
        snprintf(label, sizeof label, "aio_read n=%s (buffer of 4096, file of 1000)", nn[i]);
        ssize_t r = SUBMIT(label, aio_read(&cb), &cb);
        if (r > 0)
            P("  data matches the file: %s", data_ok(low, 0, r) ? "yes" : "no");
    }
    static const int prios[] = {-1, 0, 1, 20, 21, INT_MAX, INT_MIN};
    for (int i = 0; i < 7; i++) {
        mk(&cb, fd, buf, 10, 0);
        cb.aio_reqprio = prios[i];
        snprintf(label, sizeof label, "aio_read aio_reqprio=%d", prios[i]);
        SUBMIT(label, aio_read(&cb), &cb);
    }

    int wo = open(path("data"), O_WRONLY);
    mk(&cb, wo, buf, 10, 0);
    SUBMIT("aio_read on a write-only descriptor", aio_read(&cb), &cb);
    close(wo);
    int cl = open(path("data"), O_RDONLY);
    close(cl);
    mk(&cb, cl, buf, 10, 0);
    SUBMIT("aio_read on a closed descriptor", aio_read(&cb), &cb);
    mk(&cb, -1, buf, 10, 0);
    SUBMIT("aio_read on fd -1", aio_read(&cb), &cb);
    mk(&cb, INT_MAX, buf, 10, 0);
    SUBMIT("aio_read on fd INT_MAX", aio_read(&cb), &cb);
    int dfd = open(D, O_RDONLY | O_DIRECTORY);
    mk(&cb, dfd, buf, 10, 0);
    SUBMIT("aio_read on a directory", aio_read(&cb), &cb);
    close(dfd);
    int pfd = open(path("data"), O_PATH);
    mk(&cb, pfd, buf, 10, 0);
    SUBMIT("aio_read on an O_PATH descriptor", aio_read(&cb), &cb);
    close(pfd);

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = fd;
    c64.aio_buf = buf;
    c64.aio_nbytes = 200;
    c64.aio_offset = 900;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    errno = 0;
    int r = aio_read64(&c64);
    int e = errno;
    for (int i = 0; r == 0 && i < wait_limit_ms && aio_error64(&c64) == EINPROGRESS; i++)
        sleep_ms(1);
    P("aio_read64 off=900 n=200: submit=%d errno=%s %s data=%s", r, ee(r, e), ST64(&c64), data_ok(buf, 900, 100) ? "ok" : "bad");
    close(fd);
    unlink(path("data"));
}

/* ------------------------------------------------------------------ aio_write on a regular file */

static void fsz_write(const char *label, int fd, off_t off);

static void s_write(void)
{
    int fd = open(path("w"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    struct aiocb cb;
    struct stat st;
    char got[200];
    mk(&cb, fd, "hello", 5, 0);
    SUBMIT("aio_write \"hello\" off=0", aio_write(&cb), &cb);
    mk(&cb, fd, "world", 5, 100);
    SUBMIT("aio_write \"world\" off=100", aio_write(&cb), &cb);
    fstat(fd, &st);
    memset(got, 'x', sizeof got);
    pread(fd, got, 105, 0);
    int zeros = 1;
    for (int i = 5; i < 100; i++)
        zeros &= got[i] == 0;
    P("size=%ld contents: hello=%s hole of zeros=%s world=%s", (long)st.st_size, memcmp(got, "hello", 5) == 0 ? "yes" : "no", zeros ? "yes" : "no", memcmp(got + 100, "world", 5) == 0 ? "yes" : "no");
    P("file offset after the writes (0 before; POSIX: unspecified): %ld", (long)lseek(fd, 0, SEEK_CUR));
    mk(&cb, fd, "", 0, 500);
    SUBMIT("aio_write n=0 off=500", aio_write(&cb), &cb);
    fstat(fd, &st);
    P("size after n=0 at 500: %ld", (long)st.st_size);

    int ap = open(path("w"), O_WRONLY | O_APPEND);
    mk(&cb, ap, "tail", 4, 0);
    SUBMIT("aio_write \"tail\" off=0 on an O_APPEND descriptor", aio_write(&cb), &cb);
    mk(&cb, ap, "TAIL", 4, 3);
    SUBMIT("aio_write \"TAIL\" off=3 on an O_APPEND descriptor", aio_write(&cb), &cb);
    fstat(fd, &st);
    memset(got, 0, sizeof got);
    pread(fd, got, 8, 105);
    P("size=%ld bytes 105..112=\"%.8s\" start still hello=%s", (long)st.st_size, got, pread(fd, got + 100, 5, 0) == 5 && memcmp(got + 100, "hello", 5) == 0 ? "yes" : "no");
    close(ap);

    char label[120];
    static const long offs[] = {-1, LONG_MIN, LONG_MAX - 2, LONG_MAX};
    static const char *offn[] = {"-1", "LLONG_MIN", "LLONG_MAX-2", "LLONG_MAX"};
    for (int i = 0; i < 4; i++) {
        mk(&cb, fd, "abcde", 5, offs[i]);
        snprintf(label, sizeof label, "aio_write off=%s n=5", offn[i]);
        SUBMIT(label, aio_write(&cb), &cb);
    }
    mk(&cb, fd, "abcde", (size_t)SSIZE_MAX + 1, 0);
    SUBMIT("aio_write n=SSIZE_MAX+1", aio_write(&cb), &cb);
    mk(&cb, fd, "abcde", 5, 0);
    cb.aio_reqprio = 21;
    SUBMIT("aio_write aio_reqprio=21", aio_write(&cb), &cb);
    mk(&cb, fd, "abcde", 5, 0);
    cb.aio_reqprio = -1;
    SUBMIT("aio_write aio_reqprio=-1", aio_write(&cb), &cb);
    mk(&cb, fd, "abcde", 5, 0);
    cb.aio_lio_opcode = LIO_READ;
    SUBMIT("aio_write with aio_lio_opcode=LIO_READ", aio_write(&cb), &cb);
    pread(fd, got, 5, 0);
    P("  bytes 0..4 now \"%.5s\"", got);

    int ro = open(path("w"), O_RDONLY);
    mk(&cb, ro, "abcde", 5, 0);
    SUBMIT("aio_write on a read-only descriptor", aio_write(&cb), &cb);
    close(ro);
    mk(&cb, -1, "abcde", 5, 0);
    SUBMIT("aio_write on fd -1", aio_write(&cb), &cb);

    /* RLIMIT_FSIZE with SIGXFSZ ignored. The limit is lowered only around the request: it also
     * applies to the probe's own output when that is a regular file. */
    signal(SIGXFSZ, SIG_IGN);
    fsz_write("RLIMIT_FSIZE 1000, SIGXFSZ ignored: aio_write off=2000 n=10", fd, 2000);
    fsz_write("RLIMIT_FSIZE 1000, SIGXFSZ ignored: aio_write off=995 n=10", fd, 995);

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = fd;
    c64.aio_buf = "sixty4";
    c64.aio_nbytes = 6;
    c64.aio_offset = 10;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    errno = 0;
    int r = aio_write64(&c64);
    int e = errno;
    for (int i = 0; r == 0 && i < wait_limit_ms && aio_error64(&c64) == EINPROGRESS; i++)
        sleep_ms(1);
    pread(fd, got, 6, 10);
    P("aio_write64 off=10 \"sixty4\": submit=%d errno=%s %s bytes=\"%.6s\"", r, ee(r, e), ST64(&c64), got);
    close(fd);
    unlink(path("w"));
}

/* An aio_write of 10 bytes at `off` with the soft RLIMIT_FSIZE at 1000 while it runs (the
 * request is complete, and 200 ms have passed, before the limit is raised again and the line is
 * printed). */
static void fsz_write(const char *label, int fd, off_t off)
{
    struct rlimit rl;
    getrlimit(RLIMIT_FSIZE, &rl);
    rlim_t old = rl.rlim_cur;
    struct aiocb cb;
    mk(&cb, fd, "0123456789", 10, off);
    rl.rlim_cur = 1000;
    setrlimit(RLIMIT_FSIZE, &rl);
    errno = 0;
    int rc = aio_write(&cb);
    int e = errno;
    if (rc == 0)
        wait_done(&cb, SLOW_MS);
    sleep_ms(200);
    rl.rlim_cur = old;
    setrlimit(RLIMIT_FSIZE, &rl);
    fin(label, rc, e, &cb);
}

/* SIGXFSZ at its default disposition: the write that exceeds RLIMIT_FSIZE. */
static void s_xfsz_default(void)
{
    int fd = open(path("x"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path("x"));
    fsz_write("SIGXFSZ default: aio_write off=2000 n=10 past RLIMIT_FSIZE 1000", fd, 2000);
    P("process still alive; SIGXFSZ pending in the caller: %s", pending(SIGXFSZ) ? "yes" : "no");
}

static void s_xfsz_blocked(void)
{
    block_sig(SIGXFSZ);
    int fd = open(path("x"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path("x"));
    fsz_write("SIGXFSZ blocked in the caller: aio_write off=2000 n=10 past RLIMIT_FSIZE 1000", fd, 2000);
    P("SIGXFSZ pending in the caller: %s", pending(SIGXFSZ) ? "yes" : "no");
}

/* ------------------------------------------------------------------ pipes */

static void s_pipe(void)
{
    int p[2];
    pipe(p);
    char buf[200];
    struct aiocb cb;
    write(p[1], "0123456789", 10);
    mk(&cb, p[0], buf, 100, 0);
    SUBMIT("pipe: aio_read n=100 off=0 with 10 bytes in the pipe", aio_read(&cb), &cb);
    write(p[1], "0123456789", 10);
    mk(&cb, p[0], buf, 100, 12345);
    SUBMIT("pipe: aio_read n=100 off=12345 with 10 bytes in the pipe", aio_read(&cb), &cb);
    mk(&cb, p[1], "abc", 3, 777);
    SUBMIT("pipe: aio_write \"abc\" off=777", aio_write(&cb), &cb);
    memset(buf, 0, sizeof buf);
    int q0 = fionread(p[0]);
    int n0 = (int)read(p[0], buf, sizeof buf);
    P("  the pipe then holds %d bytes: \"%.*s\"", q0, n0, buf);
    mk(&cb, p[1], "abc", 3, -1);
    SUBMIT("pipe: aio_write off=-1", aio_write(&cb), &cb);
    drain(p[0], fionread(p[0]));
    mk(&cb, p[0], buf, 10, -1);
    write(p[1], "q", 1);
    SUBMIT("pipe: aio_read off=-1 with 1 byte in the pipe", aio_read(&cb), &cb);
    drain(p[0], fionread(p[0]));

    /* A read blocked on an empty pipe stays EINPROGRESS until data arrives. */
    mk(&cb, p[0], buf, 100, 0);
    errno = 0;
    int r = aio_read(&cb);
    E = errno;
    P("pipe: aio_read on an empty pipe: submit=%d errno=%s", r, ee(r, E));
    settle();
    P("  after 400 ms: aio_error=%s", en(aio_error(&cb)));
    write(p[1], "xyz", 3);
    fin("  after writing 3 bytes", 0, 0, &cb);

    /* Nonblocking empty pipe. */
    fcntl(p[0], F_SETFL, O_NONBLOCK);
    mk(&cb, p[0], buf, 100, 0);
    SUBMIT("pipe: aio_read on an empty O_NONBLOCK pipe", aio_read(&cb), &cb);
    fcntl(p[0], F_SETFL, 0);

    /* Partial write into a nonblocking pipe of 65536 bytes. */
    P("F_SETPIPE_SZ 65536: %d", fcntl(p[1], F_SETPIPE_SZ, 65536));
    static char big[100000];
    memset(big, 'b', sizeof big);
    fcntl(p[1], F_SETFL, O_NONBLOCK);
    mk(&cb, p[1], big, sizeof big, 0);
    SUBMIT("pipe: aio_write of 100000 bytes into an empty O_NONBLOCK pipe of 65536", aio_write(&cb), &cb);
    mk(&cb, p[1], big, 10, 0);
    SUBMIT("pipe: aio_write of 10 bytes into the full O_NONBLOCK pipe", aio_write(&cb), &cb);
    P("  drained %ld bytes", drain(p[0], fionread(p[0])));
    fcntl(p[1], F_SETFL, 0);

    /* A blocking write that fills the pipe is in progress until the reader drains it. */
    mk(&cb, p[1], big, sizeof big, 0);
    errno = 0;
    r = aio_write(&cb);
    E = errno;
    P("pipe: blocking aio_write of 100000 bytes: submit=%d errno=%s", r, ee(r, E));
    int q = wait_fill(p[0], 65536);
    P("  the pipe fills to %d bytes; aio_error=%s", q, en(aio_error(&cb)));
    P("  the reader drains %ld bytes", drain(p[0], 100000));
    fin("  then", 0, 0, &cb);

    /* Writer closed: the read sees end of file. */
    close(p[1]);
    mk(&cb, p[0], buf, 100, 0);
    SUBMIT("pipe: aio_read with the write end closed and the pipe empty", aio_read(&cb), &cb);
    close(p[0]);

    /* EPIPE with SIGPIPE ignored. */
    signal(SIGPIPE, SIG_IGN);
    pipe(p);
    close(p[0]);
    mk(&cb, p[1], "abc", 3, 0);
    SUBMIT("pipe: aio_write with the read end closed, SIGPIPE ignored", aio_write(&cb), &cb);
    close(p[1]);
}

static void s_sigpipe_default(void)
{
    int p[2];
    pipe(p);
    close(p[0]);
    struct aiocb cb;
    mk(&cb, p[1], "abc", 3, 0);
    SUBMIT("SIGPIPE default: aio_write with the read end closed", aio_write(&cb), &cb);
    sleep_ms(200);
    P("process still alive; SIGPIPE pending in the caller: %s", pending(SIGPIPE) ? "yes" : "no");
}

static void s_sigpipe_blocked(void)
{
    block_sig(SIGPIPE);
    int p[2];
    pipe(p);
    close(p[0]);
    struct aiocb cb;
    mk(&cb, p[1], "abc", 3, 0);
    SUBMIT("SIGPIPE blocked in the caller: aio_write with the read end closed", aio_write(&cb), &cb);
    sleep_ms(200);
    P("SIGPIPE pending in the caller: %s", pending(SIGPIPE) ? "yes" : "no");
}

/* ------------------------------------------------------------------ sockets */

static void s_socket(void)
{
    signal(SIGPIPE, SIG_IGN);
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    char buf[200];
    struct aiocb cb;
    mk(&cb, sv[0], "ping", 4, 99);
    SUBMIT("stream socket: aio_write \"ping\" off=99", aio_write(&cb), &cb);
    memset(buf, 0, sizeof buf);
    P("  the peer receives \"%.*s\"", (int)recv(sv[1], buf, sizeof buf, 0), buf);
    send(sv[1], "pong!!", 6, 0);
    mk(&cb, sv[0], buf, 100, 5);
    SUBMIT("stream socket: aio_read n=100 off=5 with 6 bytes queued", aio_read(&cb), &cb);

    mk(&cb, sv[0], buf, 100, 0);
    errno = 0;
    int r = aio_read(&cb);
    E = errno;
    P("stream socket: aio_read with nothing queued: submit=%d errno=%s", r, ee(r, E));
    settle();
    P("  after 400 ms: aio_error=%s", en(aio_error(&cb)));
    struct dw d = {sv[1], 100, "late", 4};
    pthread_t t;
    pthread_create(&t, NULL, dw_run, &d);
    fin("  the peer sends 4 bytes 100 ms later", 0, 0, &cb);
    pthread_join(t, NULL);

    shutdown(sv[1], SHUT_WR);
    mk(&cb, sv[0], buf, 100, 0);
    SUBMIT("stream socket: aio_read after the peer's shutdown(SHUT_WR)", aio_read(&cb), &cb);
    close(sv[1]);
    sleep_ms(50);
    mk(&cb, sv[0], "x", 1, 0);
    SUBMIT("stream socket: aio_write after the peer closed, SIGPIPE ignored", aio_write(&cb), &cb);
    struct aiocb f;
    mk(&f, sv[0], NULL, 0, 0);
    SUBMIT("stream socket: aio_fsync(O_SYNC)", aio_fsync(O_SYNC, &f), &f);
    close(sv[0]);

    socketpair(AF_UNIX, SOCK_DGRAM, 0, sv);
    send(sv[1], "0123456789", 10, 0);
    send(sv[1], "abc", 3, 0);
    mk(&cb, sv[0], buf, 4, 0);
    ssize_t n = SUBMIT("datagram socket: aio_read n=4 of a 10-byte datagram", aio_read(&cb), &cb);
    P("  got \"%.*s\"", (int)(n > 0 ? n : 0), buf);
    mk(&cb, sv[0], buf, 100, 0);
    n = SUBMIT("datagram socket: the next aio_read n=100", aio_read(&cb), &cb);
    P("  got \"%.*s\"", (int)(n > 0 ? n : 0), buf);
    close(sv[0]);
    close(sv[1]);
}

/* ------------------------------------------------------------------ aio_fsync */

static void s_fsync(void)
{
    int fd = open(path("f"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    write(fd, "data", 4);
    struct aiocb cb;
    char label[120];
    mk(&cb, fd, NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on a regular file", aio_fsync(O_SYNC, &cb), &cb);
    mk(&cb, fd, NULL, 0, 0);
    SUBMIT("aio_fsync(O_DSYNC) on a regular file", aio_fsync(O_DSYNC, &cb), &cb);
    static const int ops[] = {0, O_RDWR, O_SYNC | O_APPEND, O_DSYNC | O_RDWR, -1, O_RSYNC};
    static const char *opn[] = {"0", "O_RDWR", "O_SYNC|O_APPEND", "O_DSYNC|O_RDWR", "-1", "O_RSYNC"};
    for (int i = 0; i < 6; i++) {
        mk(&cb, fd, NULL, 0, 0);
        snprintf(label, sizeof label, "aio_fsync(%s)", opn[i]);
        SUBMIT(label, aio_fsync(ops[i], &cb), &cb);
    }
    mk(&cb, fd, NULL, (size_t)-1, -1);
    cb.aio_reqprio = 999;
    cb.aio_lio_opcode = 99;
    SUBMIT("aio_fsync(O_SYNC) with offset -1, n=SIZE_MAX, reqprio 999, opcode 99", aio_fsync(O_SYNC, &cb), &cb);
    int ro = open(path("f"), O_RDONLY);
    mk(&cb, ro, NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on a read-only descriptor", aio_fsync(O_SYNC, &cb), &cb);
    close(ro);
    mk(&cb, -1, NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on fd -1", aio_fsync(O_SYNC, &cb), &cb);
    mk(&cb, -1, NULL, 0, 0);
    SUBMIT("aio_fsync(0) on fd -1", aio_fsync(0, &cb), &cb);
    int cl = open(path("f"), O_RDONLY);
    close(cl);
    mk(&cb, cl, NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on a closed descriptor", aio_fsync(O_SYNC, &cb), &cb);
    int dn = open("/dev/null", O_WRONLY);
    mk(&cb, dn, NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on /dev/null", aio_fsync(O_SYNC, &cb), &cb);
    close(dn);
    int p[2];
    pipe(p);
    mk(&cb, p[1], NULL, 0, 0);
    SUBMIT("aio_fsync(O_SYNC) on a pipe write end", aio_fsync(O_SYNC, &cb), &cb);

    /* A sync on a pipe while a read on the same descriptor is blocked. */
    char buf[16];
    struct aiocb rd;
    mk(&rd, p[0], buf, 10, 0);
    aio_read(&rd);
    settle();
    mk(&cb, p[0], NULL, 0, 0);
    errno = 0;
    int r = aio_fsync(O_SYNC, &cb);
    E = errno;
    P("aio_fsync(O_SYNC) on a pipe read end with a read blocked on it: submit=%d errno=%s", r, ee(r, E));
    if (r == 0) {
        settle();
        P("  after 400 ms: the sync's aio_error=%s, the read's aio_error=%s", en(aio_error(&cb)), en(aio_error(&rd)));
    }
    write(p[1], "abc", 3);
    fin("  the read, after 3 bytes are written", 0, 0, &rd);
    if (r == 0)
        fin("  the sync", 0, 0, &cb);
    close(p[0]);
    close(p[1]);

    /* A sync submitted right after a write: the write is complete when the sync is. */
    int ok = 0;
    for (int i = 0; i < 50; i++) {
        struct aiocb w, s;
        mk(&w, fd, "x", 1, i);
        aio_write(&w);
        mk(&s, fd, NULL, 0, 0);
        aio_fsync(O_DSYNC, &s);
        wait_done(&s, SLOW_MS);
        ok += done(&w);
        wait_done(&w, SLOW_MS);
        aio_return(&w);
        aio_return(&s);
    }
    P("aio_write then aio_fsync(O_DSYNC) on one descriptor, 50 times: the write was complete when the sync completed %d/50", ok);

    /* Notification for a sync. */
    int sig = SIGRTMIN + 1;
    block_sig(sig);
    mk(&cb, fd, NULL, 0, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = sig;
    cb.aio_sigevent.sigev_value.sival_int = 31;
    aio_fsync(O_SYNC, &cb);
    siginfo_t si;
    int got = take(sig, &si, 5000);
    P("aio_fsync with SIGEV_SIGNAL: signal=%s si_code=%s value=%d %s", got > 0 ? signame(got) : "none", code_name(si.si_code), si.si_value.sival_int, ST(&cb));

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = fd;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    errno = 0;
    r = aio_fsync64(O_DSYNC, &c64);
    E = errno;
    int e = errno;
    for (int i = 0; r == 0 && i < wait_limit_ms && aio_error64(&c64) == EINPROGRESS; i++)
        sleep_ms(1);
    P("aio_fsync64(O_DSYNC): submit=%d errno=%s %s", r, ee(r, e), ST64(&c64));
    errno = 0;
    r = aio_fsync64(0, &c64);
    E = errno;
    P("aio_fsync64(0): submit=%d errno=%s", r, ee(r, E));
    close(fd);
    unlink(path("f"));
}

/* ------------------------------------------------------------------ aio_error, aio_return */

static void s_status(void)
{
    struct aiocb z;
    memset(&z, 0, sizeof z);
    errno = 0;
    int e = aio_error(&z);
    E = errno;
    P("aio_error on a zeroed, never submitted aiocb: %d (%s) errno=%s", e, e > 0 ? en(e) : "-", ee(e, E));
    errno = 0;
    ssize_t r = aio_return(&z);
    E = errno;
    P("aio_return on it: %zd errno=%s", r, ee(r, E));

    int p[2];
    pipe(p);
    char buf[16];
    struct aiocb cb;
    mk(&cb, p[0], buf, 10, 0);
    aio_read(&cb);
    settle();
    errno = 0;
    e = aio_error(&cb);
    E = errno;
    P("blocked pipe read: aio_error=%s errno=%s", en(e), ee(e, E));
    errno = 0;
    r = aio_return(&cb);
    E = errno;
    P("  aio_return before completion: %zd errno=%s", r, ee(r, E));
    P("  aio_error after that: %s", en(aio_error(&cb)));
    write(p[1], "abcd", 4);
    wait_done(&cb, SLOW_MS);
    errno = 0;
    e = aio_error(&cb);
    E = errno;
    P("  after 4 bytes arrive: aio_error=%s errno=%s", en(e), ee(e, E));
    errno = 0;
    r = aio_return(&cb);
    E = errno;
    P("  aio_return: %zd errno=%s", r, ee(r, E));
    errno = 0;
    r = aio_return(&cb);
    E = errno;
    P("  aio_return a second time: %zd errno=%s", r, ee(r, E));
    errno = 0;
    e = aio_error(&cb);
    E = errno;
    P("  aio_error after aio_return: %d (%s) errno=%s", e, e > 0 ? en(e) : "-", ee(e, E));

    /* The same aiocb reused for a new request. */
    write(p[1], "ef", 2);
    mk(&cb, p[0], buf, 10, 0);
    SUBMIT("the same aiocb reused for a new read", aio_read(&cb), &cb);

    /* A request that fails. */
    int wo = open("/dev/null", O_WRONLY);
    mk(&cb, wo, buf, 10, 0);
    errno = 0;
    int s = aio_read(&cb);
    e = errno;
    if (s == 0) {
        wait_done(&cb, SLOW_MS);
        int ae = aio_error(&cb);
        errno = 0;
        r = aio_return(&cb);
        E = errno;
        P("aio_read on a write-only /dev/null: submit=0 aio_error=%s aio_return=%zd errno after aio_return=%s", en(ae), r, ee(r, E));
    } else {
        P("aio_read on a write-only /dev/null: submit=%d errno=%s", s, ee(s, e));
    }
    close(wo);

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = p[0];
    c64.aio_buf = buf;
    c64.aio_nbytes = 10;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    aio_read64(&c64);
    settle();
    P("aio_error64 on a blocked aio_read64: %s", en(aio_error64(&c64)));
    write(p[1], "g", 1);
    for (int i = 0; i < SLOW_MS && aio_error64(&c64) == EINPROGRESS; i++)
        sleep_ms(1);
    P("  after 1 byte: %s", ST64(&c64));
    P("  aio_error on the same block through the non-64 function: %s", en(aio_error((struct aiocb *)&c64)));
    close(p[0]);
    close(p[1]);
}

/* ------------------------------------------------------------------ aio_suspend */

static int susp(const char *label, const struct aiocb *const *l, int n, long ms)
{
    struct timespec to = {ms / 1000, (ms % 1000) * 1000000L};
    long t0 = now_ms();
    errno = 0;
    int r = aio_suspend(l, n, ms < 0 ? NULL : &to);
    int e = errno;
    long el = now_ms() - t0;
    if (ms > 0 && r == -1 && e == EAGAIN)
        P("%s: %d errno=%s waited at least the timeout: %s", label, r, ee(r, e), el >= ms - 5 ? "yes" : "no");
    else
        P("%s: %d errno=%s", label, r, ee(r, e));
    return r;
}

static void s_suspend(void)
{
    int p[2];
    pipe(p);
    char buf[16], dbuf[16];
    struct aiocb B, Dn, R;
    mk(&B, p[0], buf, 10, 0);
    aio_read(&B);
    int fd = open("/dev/zero", O_RDONLY);
    mk(&Dn, fd, dbuf, 10, 0);
    aio_read(&Dn);
    wait_done(&Dn, SLOW_MS);
    mk(&R, fd, dbuf, 10, 0);
    aio_read(&R);
    wait_done(&R, SLOW_MS);
    aio_return(&R);
    settle();

    const struct aiocb *l1[] = {&B};
    susp("aio_suspend({blocked}, 1, {0,0})", l1, 1, 0);
    susp("aio_suspend({blocked}, 1, 150 ms)", l1, 1, 150);
    const struct aiocb *l2[] = {&Dn};
    susp("aio_suspend({completed}, 1, NULL)", l2, 1, -1);
    const struct aiocb *l3[] = {NULL, &B, &Dn};
    susp("aio_suspend({NULL, blocked, completed}, 3, NULL)", l3, 3, -1);
    const struct aiocb *l4[] = {NULL, NULL};
    susp("aio_suspend({NULL, NULL}, 2, {0,0})", l4, 2, 0);
    susp("aio_suspend({NULL, NULL}, 2, 100 ms)", l4, 2, 100);
    susp("aio_suspend({blocked}, 0, 100 ms)", l1, 0, 100);
    const struct aiocb *l5[] = {&R};
    susp("aio_suspend({completed and already aio_return'ed}, 1, {0,0})", l5, 1, 0);
    struct aiocb Z;
    memset(&Z, 0, sizeof Z);
    const struct aiocb *l6[] = {&Z};
    susp("aio_suspend({zeroed, never submitted}, 1, {0,0}) (POSIX: undefined)", l6, 1, 0);

    /* Interrupted by a signal: a handler without SA_RESTART. */
    on_usr1(0);
    hcount = 0;
    pthread_t k = kick(100);
    susp("aio_suspend({blocked}, 1, NULL), SIGUSR1 handler (no SA_RESTART) after 100 ms", l1, 1, -1);
    pthread_join(k, NULL);
    P("  handler ran: %d", (int)hcount);
    on_usr1(1);
    hcount = 0;
    k = kick(100);
    susp("aio_suspend({blocked}, 1, 600 ms), SIGUSR1 handler with SA_RESTART after 100 ms", l1, 1, 600);
    pthread_join(k, NULL);
    P("  handler ran: %d", (int)hcount);
    /* A blocked signal does not interrupt. */
    block_sig(SIGUSR1);
    k = kick(50);
    susp("aio_suspend({blocked}, 1, 300 ms), SIGUSR1 blocked, sent after 50 ms", l1, 1, 300);
    pthread_join(k, NULL);
    P("  SIGUSR1 pending: %s", pending(SIGUSR1) ? "yes" : "no");
    siginfo_t si;
    take(SIGUSR1, &si, 0);
    unblock_sig(SIGUSR1);

    /* Woken by the completion. */
    struct dw d = {p[1], 150, "abc", 3};
    pthread_t t;
    pthread_create(&t, NULL, dw_run, &d);
    susp("aio_suspend({blocked}, 1, NULL), 3 bytes written 150 ms later", l1, 1, -1);
    pthread_join(t, NULL);
    P("  %s", ST(&B));

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = p[0];
    c64.aio_buf = buf;
    c64.aio_nbytes = 10;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    aio_read64(&c64);
    const struct aiocb64 *l64[] = {&c64};
    struct timespec to = {0, 100000000};
    errno = 0;
    int r = aio_suspend64(l64, 1, &to);
    E = errno;
    P("aio_suspend64({blocked}, 1, 100 ms): %d errno=%s", r, ee(r, E));
    write(p[1], "z", 1);
    errno = 0;
    r = aio_suspend64(l64, 1, NULL);
    E = errno;
    P("aio_suspend64 after 1 byte: %d errno=%s return=%zd", r, ee(r, E), aio_return64(&c64));
    close(fd);
    close(p[0]);
    close(p[1]);
}

/* Each of these may block for good; they run in their own sections with a short alarm. */
static void s_suspend_null_forever(void)
{
    const struct aiocb *l[] = {NULL, NULL};
    errno = 0;
    int r = aio_suspend(l, 2, NULL);
    E = errno;
    P("aio_suspend({NULL, NULL}, 2, NULL): %d errno=%s", r, ee(r, E));
}

static void s_suspend_badtime(void)
{
    int p[2];
    pipe(p);
    char buf[8];
    struct aiocb B;
    mk(&B, p[0], buf, 4, 0);
    aio_read(&B);
    settle();
    const struct aiocb *l[] = {&B};
    struct timespec to = {0, 0};
    const char *what = "";
    switch (g_arg) {
    case 0: to.tv_nsec = -1; what = "{0,-1}"; break;
    case 1: to.tv_nsec = 1000000000; what = "{0,1000000000}"; break;
    case 2: to.tv_sec = -1; what = "{-1,0}"; break;
    default: to.tv_sec = LONG_MAX; to.tv_nsec = 0; what = "{LONG_MAX,0}"; break;
    }
    if (g_arg == 3) {
        on_usr1(0);
        hcount = 0;
        pthread_t k = kick(200);
        errno = 0;
        int r = aio_suspend(l, 1, &to);
        E = errno;
        P("aio_suspend({blocked}, 1, %s), SIGUSR1 handler after 200 ms: %d errno=%s; the handler had run at the return: %s", what, r, ee(r, E), hcount ? "yes" : "no");
        pthread_join(k, NULL);
        return;
    }
    long t0 = now_ms();
    errno = 0;
    int r = aio_suspend(l, 1, &to);
    E = errno;
    P("aio_suspend({blocked}, 1, %s): %d errno=%s returned within 1 s: %s", what, r, ee(r, E), now_ms() - t0 < 1000 ? "yes" : "no");
}

static void s_suspend_negative_nent(void)
{
    int p[2];
    pipe(p);
    char buf[8];
    struct aiocb B;
    mk(&B, p[0], buf, 4, 0);
    aio_read(&B);
    const struct aiocb *l[] = {&B};
    struct timespec to = {0, 100000000};
    errno = 0;
    int r = aio_suspend(l, -1, &to);
    E = errno;
    P("aio_suspend({blocked}, -1, 100 ms): %d errno=%s", r, ee(r, E));
}

/* ------------------------------------------------------------------ aio_cancel */

static void s_cancel(void)
{
    errno = 0;
    int r = aio_cancel(-1, NULL);
    E = errno;
    P("aio_cancel(-1, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    int cl = open("/dev/null", O_RDONLY);
    close(cl);
    errno = 0;
    r = aio_cancel(cl, NULL);
    E = errno;
    P("aio_cancel(closed fd, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    mkdata("c.dat", 100);
    int fd = open(path("c.dat"), O_RDONLY);
    int fd2 = open(path("c.dat"), O_RDONLY);
    errno = 0;
    r = aio_cancel(fd, NULL);
    E = errno;
    P("aio_cancel(fd with no requests ever, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    char buf[128];
    struct aiocb C;
    mk(&C, fd, buf, 50, 0);
    aio_read(&C);
    wait_done(&C, SLOW_MS);
    errno = 0;
    r = aio_cancel(fd, &C);
    E = errno;
    P("aio_cancel(fd, completed request): %s errno=%s; then %s", cancel_name(r), ee(r, E), ST(&C));
    errno = 0;
    r = aio_cancel(fd, &C);
    E = errno;
    P("aio_cancel(fd, request already aio_return'ed): %s errno=%s", cancel_name(r), ee(r, E));
    errno = 0;
    r = aio_cancel(fd, NULL);
    E = errno;
    P("aio_cancel(fd, NULL) after everything completed: %s errno=%s", cancel_name(r), ee(r, E));
    mk(&C, fd, buf, 50, 0);
    aio_read(&C);
    wait_done(&C, SLOW_MS);
    errno = 0;
    r = aio_cancel(fd2, &C);
    E = errno;
    P("aio_cancel(other valid fd, completed request): %s errno=%s", cancel_name(r), ee(r, E));
    errno = 0;
    r = aio_cancel(-1, &C);
    E = errno;
    P("aio_cancel(-1, completed request): %s errno=%s", cancel_name(r), ee(r, E));
    aio_return(&C);
    struct aiocb Z;
    mk(&Z, fd, buf, 10, 0);
    errno = 0;
    r = aio_cancel(fd, &Z);
    E = errno;
    P("aio_cancel(fd, never submitted aiocb naming fd): %s errno=%s", cancel_name(r), ee(r, E));
    mk(&Z, fd2, buf, 10, 0);
    errno = 0;
    r = aio_cancel(fd, &Z);
    E = errno;
    P("aio_cancel(fd, never submitted aiocb naming another fd): %s errno=%s", cancel_name(r), ee(r, E));

    /* Requests on an empty pipe. A is blocked in a read; B and C are queued behind it. */
    int sig = SIGRTMIN + 1;
    block_sig(sig);
    int p[2];
    pipe(p);
    char ba[16], bb[16], bc[16];
    struct aiocb A, B, Cc;
    mk(&A, p[0], ba, 5, 0);
    A.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    A.aio_sigevent.sigev_signo = sig;
    A.aio_sigevent.sigev_value.sival_int = 1;
    aio_read(&A);
    settle();
    errno = 0;
    r = aio_cancel(fd2, &A);
    E = errno;
    P("pipe read blocked (A): aio_cancel(another valid fd, &A): %s errno=%s; A's aio_error=%s", cancel_name(r), ee(r, E), en(aio_error(&A)));
    errno = 0;
    r = aio_cancel(-1, &A);
    E = errno;
    P("  aio_cancel(-1, &A): %s errno=%s", cancel_name(r), ee(r, E));
    errno = 0;
    r = aio_cancel(p[0], &A);
    E = errno;
    P("  aio_cancel(pipe, &A): %s errno=%s; A's aio_error=%s", cancel_name(r), ee(r, E), en(aio_error(&A)));
    mk(&B, p[0], bb, 5, 0);
    B.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    B.aio_sigevent.sigev_signo = sig;
    B.aio_sigevent.sigev_value.sival_int = 42;
    aio_read(&B);
    mk(&Cc, p[0], bc, 5, 0);
    Cc.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    Cc.aio_sigevent.sigev_signo = sig;
    Cc.aio_sigevent.sigev_value.sival_int = 43;
    aio_read(&Cc);
    settle();
    P("  second and third reads on the same pipe (B, C): aio_error=%s,%s", en(aio_error(&B)), en(aio_error(&Cc)));
    errno = 0;
    r = aio_cancel(p[0], &B);
    int e = errno;
    int be = aio_error(&B);
    errno = 0;
    ssize_t br = aio_return(&B);
    P("  aio_cancel(pipe, &B): %s errno=%s; B's aio_error=%s aio_return=%zd", cancel_name(r), ee(r, e), en(be), br);
    siginfo_t si;
    int got = take(sig, &si, 1000);
    P("  notification for B: signal=%s si_code=%s value=%d", got > 0 ? signame(got) : "none", got > 0 ? code_name(si.si_code) : "-", got > 0 ? si.si_value.sival_int : 0);
    errno = 0;
    r = aio_cancel(p[0], &B);
    E = errno;
    P("  aio_cancel(pipe, &B) again: %s errno=%s", cancel_name(r), ee(r, E));
    errno = 0;
    r = aio_cancel(p[0], NULL);
    E = errno;
    P("  aio_cancel(pipe, NULL) with A blocked and C queued: %s errno=%s; aio_error A=%s C=%s", cancel_name(r), ee(r, E), en(aio_error(&A)), en(aio_error(&Cc)));
    got = take(sig, &si, 1000);
    P("  notification for C: signal=%s value=%d; aio_return C=%zd", got > 0 ? signame(got) : "none", got > 0 ? si.si_value.sival_int : 0, done(&Cc) ? aio_return(&Cc) : (ssize_t)-99);
    got = take(sig, &si, 300);
    P("  further notifications within 300 ms: %s", got > 0 ? "yes" : "none");
    write(p[1], "hello", 5);
    wait_done(&A, SLOW_MS);
    got = take(sig, &si, 2000);
    P("  5 bytes written: A %s data=\"%.5s\"; notification value=%d", ST(&A), ba, got > 0 ? si.si_value.sival_int : 0);
    errno = 0;
    r = aio_cancel(p[0], NULL);
    E = errno;
    P("  aio_cancel(pipe, NULL) afterwards: %s errno=%s", cancel_name(r), ee(r, E));

    /* A write in progress: the pipe has filled. */
    fcntl(p[1], F_SETPIPE_SZ, 65536);
    static char big[100000];
    struct aiocb W;
    mk(&W, p[1], big, sizeof big, 0);
    aio_write(&W);
    int q = wait_fill(p[0], 65536);
    errno = 0;
    r = aio_cancel(p[1], &W);
    E = errno;
    P("pipe write of 100000 in progress (%d in the pipe): aio_cancel(pipe, &W): %s errno=%s; aio_error=%s", q, cancel_name(r), ee(r, E), en(aio_error(&W)));
    P("  the reader drains %ld bytes", drain(p[0], 100000));
    fin("  then", 0, 0, &W);
    close(p[0]);
    close(p[1]);

    /* A socket read blocked. */
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    struct aiocb S;
    mk(&S, sv[0], ba, 5, 0);
    aio_read(&S);
    settle();
    errno = 0;
    r = aio_cancel(sv[0], NULL);
    E = errno;
    P("socket read blocked: aio_cancel(socket, NULL): %s errno=%s; aio_error=%s", cancel_name(r), ee(r, E), en(aio_error(&S)));
    send(sv[1], "12345", 5, 0);
    fin("  after the peer sends 5 bytes", 0, 0, &S);
    close(sv[0]);
    close(sv[1]);

    struct aiocb64 c64;
    memset(&c64, 0, sizeof c64);
    c64.aio_fildes = fd;
    c64.aio_buf = buf;
    c64.aio_nbytes = 10;
    c64.aio_sigevent.sigev_notify = SIGEV_NONE;
    aio_read64(&c64);
    for (int i = 0; i < SLOW_MS && aio_error64(&c64) == EINPROGRESS; i++)
        sleep_ms(1);
    errno = 0;
    r = aio_cancel64(fd, &c64);
    E = errno;
    P("aio_cancel64(fd, completed request): %s errno=%s; aio_return64=%zd", cancel_name(r), ee(r, E), aio_return64(&c64));
    errno = 0;
    r = aio_cancel64(-1, NULL);
    E = errno;
    P("aio_cancel64(-1, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    close(fd);
    close(fd2);
    unlink(path("c.dat"));
}

/* ------------------------------------------------------------------ lio_listio */

static volatile int lt_calls, lt_value, lt_not_main, lt_all_done;
static struct aiocb *lt_list[8];
static int lt_n;
static void lt_fn(union sigval v)
{
    int all = 1;
    for (int i = 0; i < lt_n; i++)
        if (lt_list[i] && aio_error(lt_list[i]) == EINPROGRESS)
            all = 0;
    lt_all_done = all;
    lt_value = v.sival_int;
    lt_not_main = !pthread_equal(pthread_self(), main_t);
    __atomic_add_fetch(&lt_calls, 1, __ATOMIC_SEQ_CST);
}

static void entries(const char *label, struct aiocb **l, int n)
{
    char line[600];
    int k = snprintf(line, sizeof line, "%s:", label);
    for (int i = 0; i < n; i++) {
        if (!l[i]) {
            k += snprintf(line + k, sizeof line - (size_t)k, " [%d] NULL", i);
            continue;
        }
        wait_done(l[i], 3000);
        int e = aio_error(l[i]);
        errno = 0;
        ssize_t r = aio_return(l[i]);
        k += snprintf(line + k, sizeof line - (size_t)k, " [%d] %s/%zd", i, en(e), r);
    }
    P("%s", line);
}

static void s_lio(void)
{
    int fd = open(path("l"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    int wo = open(path("l"), O_WRONLY);
    unlink(path("l"));
    struct aiocb a, b, c, d, e5;
    struct aiocb *l[8];
    char rb[64], rb2[64];
    int r;

    errno = 0;
    l[0] = NULL;
    r = lio_listio(5, l, 1, NULL);
    E = errno;
    P("lio_listio(mode 5, {NULL}, 1): %d errno=%s", r, ee(r, E));
    errno = 0;
    r = lio_listio(-1, l, 1, NULL);
    E = errno;
    P("lio_listio(mode -1, {NULL}, 1): %d errno=%s", r, ee(r, E));
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 0, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, list, 0): %d errno=%s", r, ee(r, E));
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 1, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, {NULL}, 1): %d errno=%s", r, ee(r, E));

    /* Writes, a NOP and a NULL, waited for. */
    mk(&a, fd, "aaaa", 4, 0);
    a.aio_lio_opcode = LIO_WRITE;
    mk(&b, -1, NULL, 0, 0);
    b.aio_lio_opcode = LIO_NOP;
    mk(&c, fd, "bbbb", 4, 4);
    c.aio_lio_opcode = LIO_WRITE;
    l[0] = &a;
    l[1] = &b;
    l[2] = NULL;
    l[3] = &c;
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 4, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, {write, NOP on fd -1, NULL, write}): %d errno=%s; at return aio_error=%s,%s", r, ee(r, E), en(aio_error(&a)), en(aio_error(&c)));
    errno = 0;
    int ne = aio_error(&b);
    E = errno;
    P("  the LIO_NOP entry: aio_error=%d (%s) errno=%s", ne, ne > 0 ? en(ne) : "-", ee(ne, E));
    l[1] = NULL;
    entries("  entries (aio_error/aio_return)", l, 4);
    memset(rb, 0, sizeof rb);
    pread(fd, rb, 8, 0);
    P("  file holds \"%.8s\"", rb);

    /* Reads and writes mixed. */
    mk(&a, fd, rb, 8, 0);
    a.aio_lio_opcode = LIO_READ;
    mk(&c, fd, "cc", 2, 8);
    c.aio_lio_opcode = LIO_WRITE;
    l[0] = &a;
    l[1] = &c;
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 2, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, {read 8 at 0, write 2 at 8}): %d errno=%s", r, ee(r, E));
    entries("  entries", l, 2);

    /* An entry with an unknown opcode. */
    mk(&a, fd, "dd", 2, 10);
    a.aio_lio_opcode = LIO_WRITE;
    mk(&b, fd, rb, 2, 0);
    b.aio_lio_opcode = 99;
    l[0] = &a;
    l[1] = &b;
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 2, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, {write, opcode 99}): %d errno=%s", r, ee(r, E));
    wait_done(&a, 3000);
    P("  write entry: %s", ST(&a));
    errno = 0;
    ne = aio_error(&b);
    E = errno;
    P("  opcode-99 entry: aio_error=%d (%s) errno=%s", ne, ne > 0 ? en(ne) : "-", ee(ne, E));

    /* Failing entries: an error found later (read on a write-only fd) and errors that may be found
     * at submission (fd -1, reqprio 99, offset -1). */
    static const char *what[] = {"read on a write-only fd", "read on fd -1", "read with aio_reqprio 99", "read with offset -1", "write on fd -1"};
    for (int k = 0; k < 5; k++) {
        for (int mode = 0; mode < 2; mode++) {
            mk(&a, fd, "ok", 2, 20);
            a.aio_lio_opcode = LIO_WRITE;
            mk(&b, k == 0 ? wo : k == 1 || k == 4 ? -1 : fd, rb, 4, k == 3 ? -1 : 0);
            b.aio_lio_opcode = k == 4 ? LIO_WRITE : LIO_READ;
            if (k == 2)
                b.aio_reqprio = 99;
            mk(&c, fd, rb2, 4, 0);
            c.aio_lio_opcode = LIO_READ;
            l[0] = &a;
            l[1] = &b;
            l[2] = &c;
            errno = 0;
            r = lio_listio(mode ? LIO_NOWAIT : LIO_WAIT, l, 3, NULL);
            E = errno;
            int er = errno;
            char label[160];
            snprintf(label, sizeof label, "lio_listio(%s, {write, %s, read}): %d errno=%s; entries", mode ? "LIO_NOWAIT" : "LIO_WAIT", what[k], r, ee(r, er));
            entries(label, l, 3);
        }
    }

    /* LIO_NOWAIT with a signal: it comes once, after every entry is done. */
    int sig = SIGRTMIN + 2;
    block_sig(sig);
    int p[2];
    pipe(p);
    mk(&a, fd, "nn", 2, 30);
    a.aio_lio_opcode = LIO_WRITE;
    mk(&b, p[0], rb, 3, 0);
    b.aio_lio_opcode = LIO_READ;
    l[0] = &a;
    l[1] = &b;
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = sig;
    ev.sigev_value.sival_int = 77;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 2, &ev);
    E = errno;
    P("lio_listio(LIO_NOWAIT, {file write, pipe read}, SIGEV_SIGNAL 77): %d errno=%s", r, ee(r, E));
    siginfo_t si;
    int got = take(sig, &si, 400);
    wait_done(&a, 3000);
    P("  before the pipe has data: signal within 400 ms=%s; file write aio_error=%s, pipe read aio_error=%s", got > 0 ? "yes" : "none", en(aio_error(&a)), en(aio_error(&b)));
    write(p[1], "xyz", 3);
    got = take(sig, &si, 5000);
    P("  after 3 bytes: signal=%s si_code=%s value=%d; at receipt both entries done: %s", got > 0 ? signame(got) : "none", got > 0 ? code_name(si.si_code) : "-", got > 0 ? si.si_value.sival_int : 0, done(&a) && done(&b) ? "yes" : "no");
    got = take(sig, &si, 300);
    P("  further signals within 300 ms: %s", got > 0 ? "yes" : "none");
    entries("  entries", l, 2);

    /* LIO_NOWAIT with per-entry signals as well as the list's. */
    int esig = SIGRTMIN + 3;
    block_sig(esig);
    mk(&a, fd, "pe", 2, 40);
    a.aio_lio_opcode = LIO_WRITE;
    a.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    a.aio_sigevent.sigev_signo = esig;
    a.aio_sigevent.sigev_value.sival_int = 5;
    mk(&b, fd, rb, 2, 40);
    b.aio_lio_opcode = LIO_READ;
    l[0] = &a;
    l[1] = &b;
    ev.sigev_value.sival_int = 78;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 2, &ev);
    E = errno;
    int g1 = take(sig, &si, 3000);
    int v1 = si.si_value.sival_int;
    int g2 = take(esig, &si, 3000);
    P("lio_listio(LIO_NOWAIT, entry 0 with its own SIGEV_SIGNAL 5, list SIGEV_SIGNAL 78): %d; list signal=%s value=%d; entry signal=%s value=%d code=%s", r, g1 > 0 ? "yes" : "none", v1, g2 > 0 ? "yes" : "none", si.si_value.sival_int, g2 > 0 ? code_name(si.si_code) : "-");
    entries("  entries", l, 2);

    /* LIO_WAIT ignores the list's sigevent but not the entries'. */
    mk(&a, fd, "wi", 2, 50);
    a.aio_lio_opcode = LIO_WRITE;
    a.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    a.aio_sigevent.sigev_signo = esig;
    a.aio_sigevent.sigev_value.sival_int = 6;
    l[0] = &a;
    ev.sigev_value.sival_int = 79;
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 1, &ev);
    E = errno;
    g1 = take(sig, &si, 300);
    g2 = take(esig, &si, 3000);
    P("lio_listio(LIO_WAIT, entry with SIGEV_SIGNAL 6, list SIGEV_SIGNAL 79): %d errno=%s; list signal=%s; entry signal=%s value=%d", r, ee(r, E), g1 > 0 ? "yes" : "none", g2 > 0 ? "yes" : "none", g2 > 0 ? si.si_value.sival_int : 0);
    aio_return(&a);

    /* LIO_NOWAIT with no entries, or only NOP and NULL entries, and a signal. */
    mk(&b, fd, NULL, 0, 0);
    b.aio_lio_opcode = LIO_NOP;
    l[0] = NULL;
    l[1] = &b;
    ev.sigev_value.sival_int = 80;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 0, &ev);
    E = errno;
    g1 = take(sig, &si, 500);
    P("lio_listio(LIO_NOWAIT, 0 entries, SIGEV_SIGNAL 80): %d errno=%s; signal=%s value=%d", r, ee(r, E), g1 > 0 ? "yes" : "none", g1 > 0 ? si.si_value.sival_int : 0);
    ev.sigev_value.sival_int = 81;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 2, &ev);
    E = errno;
    g1 = take(sig, &si, 500);
    P("lio_listio(LIO_NOWAIT, {NULL, NOP}, SIGEV_SIGNAL 81): %d errno=%s; signal=%s value=%d", r, ee(r, E), g1 > 0 ? "yes" : "none", g1 > 0 ? si.si_value.sival_int : 0);

    /* LIO_NOWAIT with SIGEV_THREAD. */
    mk(&a, fd, "th", 2, 60);
    a.aio_lio_opcode = LIO_WRITE;
    mk(&b, fd, rb, 4, 0);
    b.aio_lio_opcode = LIO_READ;
    l[0] = &a;
    l[1] = &b;
    lt_list[0] = &a;
    lt_list[1] = &b;
    lt_n = 2;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = lt_fn;
    ev.sigev_value.sival_int = 99;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 2, &ev);
    E = errno;
    for (int i = 0; i < 5000 && lt_calls == 0; i++)
        sleep_ms(1);
    sleep_ms(100);
    P("lio_listio(LIO_NOWAIT, 2 entries, SIGEV_THREAD 99): %d errno=%s; calls=%d value=%d not the caller's thread=%d all entries done at the call=%d", r, ee(r, E), lt_calls, lt_value, lt_not_main, lt_all_done);
    entries("  entries", l, 2);
    lt_calls = 0;
    errno = 0;
    l[0] = NULL;
    r = lio_listio(LIO_NOWAIT, l, 1, &ev);
    E = errno;
    for (int i = 0; i < 500 && lt_calls == 0; i++)
        sleep_ms(1);
    P("lio_listio(LIO_NOWAIT, {NULL}, SIGEV_THREAD): %d errno=%s; calls within 500 ms=%d", r, ee(r, E), lt_calls);

    /* LIO_NOWAIT with no sigevent. */
    mk(&a, fd, "ns", 2, 70);
    a.aio_lio_opcode = LIO_WRITE;
    l[0] = &a;
    errno = 0;
    r = lio_listio(LIO_NOWAIT, l, 1, NULL);
    E = errno;
    P("lio_listio(LIO_NOWAIT, 1 write, NULL sigevent): %d errno=%s", r, ee(r, E));
    entries("  entries", l, 1);

    {
        int n = 5000;
        struct aiocb *cbs = calloc((size_t)n, sizeof *cbs);
        struct aiocb **lst = calloc((size_t)n, sizeof *lst);
        static char one = 'w';
        for (int i = 0; i < n; i++) {
            mk(&cbs[i], fd, &one, 1, 1000 + i);
            cbs[i].aio_lio_opcode = LIO_WRITE;
            lst[i] = &cbs[i];
        }
        errno = 0;
        r = lio_listio(LIO_WAIT, lst, n, NULL);
        E = errno;
        int okc = 0;
        for (int i = 0; i < n; i++)
            okc += aio_error(&cbs[i]) == 0 && aio_return(&cbs[i]) == 1;
        struct stat st;
        fstat(fd, &st);
        P("lio_listio(LIO_WAIT, 5000 one-byte writes): %d errno=%s; entries ok=%d; file size=%ld", r, ee(r, E), okc, (long)st.st_size);
        free(cbs);
        free(lst);
    }

    /* lio_listio64. */
    struct aiocb64 x, y;
    memset(&x, 0, sizeof x);
    memset(&y, 0, sizeof y);
    x.aio_fildes = fd;
    x.aio_buf = "64";
    x.aio_nbytes = 2;
    x.aio_offset = 90;
    x.aio_lio_opcode = LIO_WRITE;
    x.aio_sigevent.sigev_notify = SIGEV_NONE;
    y.aio_fildes = fd;
    y.aio_buf = rb;
    y.aio_nbytes = 2;
    y.aio_offset = 0;
    y.aio_lio_opcode = LIO_READ;
    y.aio_sigevent.sigev_notify = SIGEV_NONE;
    struct aiocb64 *l64[] = {&x, &y};
    errno = 0;
    r = lio_listio64(LIO_WAIT, l64, 2, NULL);
    E = errno;
    P("lio_listio64(LIO_WAIT, {write, read}): %d errno=%s; returns %zd,%zd", r, ee(r, E), aio_return64(&x), aio_return64(&y));
    errno = 0;
    r = lio_listio64(7, l64, 2, NULL);
    E = errno;
    P("lio_listio64(mode 7): %d errno=%s", r, ee(r, E));
    (void)d;
    (void)e5;
    close(p[0]);
    close(p[1]);
    close(wo);
    close(fd);
}

/* Large lists of NULL entries, or of one LIO_NOP aiocb repeated (g_arg: log2 of the length;
 * negative for the NOP form). */
static void s_lio_big(void)
{
    int sh = g_arg < 0 ? (int)-g_arg : (int)g_arg;
    int n = 1 << sh;
    struct rlimit rs;
    getrlimit(RLIMIT_STACK, &rs);
    P("stack limit (RLIMIT_STACK soft): %s", rs.rlim_cur == RLIM_INFINITY ? "unlimited" : rs.rlim_cur == 8u << 20 ? "8 MiB" : "other");
    struct aiocb **lst = calloc((size_t)n, sizeof *lst);
    struct aiocb b;
    mk(&b, 0, NULL, 0, 0);
    b.aio_lio_opcode = LIO_NOP;
    if (g_arg < 0)
        for (int i = 0; i < n; i++)
            lst[i] = &b;
    errno = 0;
    int r = lio_listio(LIO_WAIT, lst, n, NULL);
    E = errno;
    P("lio_listio(LIO_WAIT, %d %s): %d errno=%s", n, g_arg < 0 ? "entries, all the same LIO_NOP aiocb" : "NULL entries", r, ee(r, E));
    free(lst);
}

/* /proc/self/task/<tid>/<what> into b (NUL-terminated); its length, or -1. */
static int task_file(pid_t tid, const char *what, char *b, size_t n)
{
    char p[64];
    snprintf(p, sizeof p, "/proc/self/task/%d/%s", (int)tid, what);
    int fd = open(p, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return -1;
    ssize_t r = read(fd, b, n - 1);
    close(fd);
    if (r < 0)
        return -1;
    b[r] = 0;
    return (int)r;
}

/* Whether thread tid is parked in a blocking call: state S, and no context switch (voluntary or
 * not) over 10 samples 10 ms apart, within bound_ms. A thread that is running, runnable or
 * waking up does not pass; a thread blocked in one call does, whatever the call. */
static int task_parked(pid_t tid, long bound_ms)
{
    long end = now_ms() + bound_ms, last = -1;
    int stable = 0;
    while (now_ms() < end) {
        char b[2048];
        char state = 0;
        long sw = -1;
        if (task_file(tid, "stat", b, sizeof b) > 0) {
            char *c = strrchr(b, ')');
            if (c && c[1] == ' ')
                state = c[2];
        }
        if (task_file(tid, "status", b, sizeof b) > 0) {
            /* "voluntary_ctxt_switches:" and "nonvoluntary_ctxt_switches:": the sum of both. */
            sw = 0;
            for (char *s = strstr(b, "voluntary_ctxt_switches:"); s; s = strstr(s + 1, "voluntary_ctxt_switches:"))
                sw += atol(s + strlen("voluntary_ctxt_switches:"));
        }
        if (state == 'S' && sw >= 0 && sw == last) {
            if (++stable >= 10)
                return 1;
        } else {
            stable = 0;
        }
        last = sw;
        sleep_ms(10);
    }
    return 0;
}

/* The sequencing of s_lio_eintr, without wall-clock delays: once the file write is complete and
 * the caller is parked (so LIO_WAIT is blocked on the pipe read alone), SIGUSR1 to the caller;
 * with SA_RESTART, once the handler has run and the caller is parked again, 2 bytes into the
 * pipe. The 20 s bounds only end a wait that would otherwise hang (a failure is printed). */
struct lio_kick {
    pid_t tid;
    const struct aiocb *write;
    int pipe_w, restart;
    const char *fail;
};
static void *lio_kick_run(void *v)
{
    struct lio_kick *k = v;
    long end = now_ms() + 20000;
    while (!done(k->write) && now_ms() < end)
        sleep_ms(1);
    if (!done(k->write))
        k->fail = "the file write did not complete within 20 s";
    else if (!task_parked(k->tid, 20000))
        k->fail = "the caller was not blocked within 20 s of the file write completing";
    pthread_kill(main_t, SIGUSR1);
    end = now_ms() + 20000;
    while (!hcount && now_ms() < end)
        sleep_ms(1);
    if (!hcount && !k->fail)
        k->fail = "the handler did not run within 20 s";
    if (k->restart) {
        if (!task_parked(k->tid, 20000) && !k->fail)
            k->fail = "the caller was not blocked again within 20 s of the handler";
        if (write(k->pipe_w, "ok", 2) != 2 && !k->fail)
            k->fail = "the pipe write failed";
    }
    return NULL;
}

/* LIO_WAIT interrupted by a signal (g_arg 0: handler without SA_RESTART; 1: with SA_RESTART).
 * Explicit sequencing (lio_kick_run), not delays: under load a 200 ms signal could arrive before
 * the file write had completed, and the reference then took another path. */
static void s_lio_eintr(void)
{
    int fd = open(path("le"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path("le"));
    int p[2];
    pipe(p);
    char rb[8];
    struct aiocb a, b, *l[2];
    mk(&a, fd, "in", 2, 0);
    a.aio_lio_opcode = LIO_WRITE;
    mk(&b, p[0], rb, 2, 0);
    b.aio_lio_opcode = LIO_READ;
    l[0] = &a;
    l[1] = &b;
    on_usr1((int)g_arg);
    hcount = 0;
    /* The forked section runs on the process's first thread: its thread id is the pid. */
    struct lio_kick lk = {getpid(), &a, p[1], (int)g_arg, NULL};
    pthread_t k;
    pthread_create(&k, NULL, lio_kick_run, &lk);
    errno = 0;
    int r = lio_listio(LIO_WAIT, l, 2, NULL);
    int er = errno;
    pthread_join(k, NULL);
    P("lio_listio(LIO_WAIT, {file write, blocked pipe read}), SIGUSR1 handler %s once the write is complete and the call blocked%s: %d errno=%s handler ran=%d",
      g_arg ? "with SA_RESTART" : "without SA_RESTART", g_arg ? ", then 2 bytes into the pipe" : "", r, ee(r, er), (int)hcount);
    if (lk.fail)
        P("  sequencing failed: %s", lk.fail);
    if (g_arg) {
        entries("  entries", l, 2);
        return;
    }
    wait_done(&a, 3000);
    P("  file write aio_error=%s; pipe read aio_error=%s", en(aio_error(&a)), en(aio_error(&b)));
    sleep_ms(300);
    P("  300 ms later the process is alive; now writing 2 bytes into the pipe");
    write(p[1], "ok", 2);
    wait_done(&b, 3000);
    sleep_ms(300);
    P("  the pipe read completed: %s", done(&b) ? "yes" : "no");
    entries("  entries", l, 2);
}

static void s_lio_negative(void)
{
    struct aiocb *l[1] = {NULL};
    static const int ns[] = {-1, INT_MIN, -1, INT_MIN};
    int mode = g_arg < 2 ? LIO_WAIT : LIO_NOWAIT;
    int n = ns[g_arg];
    errno = 0;
    int r = lio_listio(mode, l, n, NULL);
    E = errno;
    P("lio_listio(%s, {NULL}, %d): %d errno=%s", mode == LIO_WAIT ? "LIO_WAIT" : "LIO_NOWAIT", n, r, ee(r, E));
}

/* ------------------------------------------------------------------ notifications */

static volatile int nt_calls, nt_value, nt_not_main, nt_detached, nt_usr1, nt_usr2, nt_err, nt_big;
static struct aiocb *nt_cb;
static size_t nt_want;
static void nt_fn(union sigval v)
{
    nt_value = v.sival_int;
    nt_not_main = !pthread_equal(pthread_self(), main_t);
    sigset_t cur;
    pthread_sigmask(SIG_BLOCK, NULL, &cur);
    nt_usr1 = sigismember(&cur, SIGUSR1);
    nt_usr2 = sigismember(&cur, SIGUSR2);
    pthread_attr_t at;
    int ds = -1;
    size_t ss = 0;
    if (pthread_getattr_np(pthread_self(), &at) == 0) {
        pthread_attr_getdetachstate(&at, &ds);
        pthread_attr_getstacksize(&at, &ss);
        pthread_attr_destroy(&at);
    }
    nt_detached = ds == PTHREAD_CREATE_DETACHED;
    nt_big = nt_want ? ss >= nt_want : 0;
    nt_err = nt_cb ? aio_error(nt_cb) : -1;
    __atomic_add_fetch(&nt_calls, 1, __ATOMIC_SEQ_CST);
}

static void wait_calls(volatile int *c, int n)
{
    for (int i = 0; i < 5000 && *c < n; i++)
        sleep_ms(1);
    sleep_ms(100);
}

static volatile int pt_go, pt_seen;
static int pt_sig;
static void *pt_run(void *a)
{
    (void)a;
    while (!__atomic_load_n(&pt_go, __ATOMIC_SEQ_CST))
        sleep_ms(1);
    pt_seen = pending(pt_sig);
    __atomic_store_n(&pt_go, 2, __ATOMIC_SEQ_CST);
    return NULL;
}

static volatile int h_in_main, h_code, h_value, h_calls;
static void info_hnd(int s, siginfo_t *si, void *u)
{
    (void)s;
    (void)u;
    h_in_main = pthread_equal(pthread_self(), main_t);
    h_code = si->si_code;
    h_value = si->si_value.sival_int;
    h_calls++;
}

static void s_notify(void)
{
    mkdata("n.dat", 100);
    int fd = open(path("n.dat"), O_RDONLY);
    char buf[128];
    struct aiocb cb;
    siginfo_t si;
    int s0 = SIGRTMIN, s1 = SIGRTMIN + 1, s4 = SIGRTMIN + 4;
    for (int s = SIGRTMIN; s <= SIGRTMIN + 8; s++)
        block_sig(s);
    sigset_t all;
    sigemptyset(&all);
    for (int s = SIGRTMIN; s <= SIGRTMAX; s++)
        sigaddset(&all, s);
    sigaddset(&all, SIGIO);
    sigaddset(&all, SIGALRM);
    sigaddset(&all, SIGUSR1);
    sigaddset(&all, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &all, NULL);
    struct timespec to = {0, 300000000};

    mk(&cb, fd, buf, 10, 0);
    SUBMIT("SIGEV_NONE read", aio_read(&cb), &cb);
    int g = sigtimedwait(&all, &si, &to);
    P("  any signal within 300 ms: %s", g > 0 ? signame(g) : "none");

    memset(&cb, 0, sizeof cb);
    cb.aio_fildes = fd;
    cb.aio_buf = buf;
    cb.aio_nbytes = 10;
    SUBMIT("zeroed aio_sigevent (SIGEV_SIGNAL with signal 0) read", aio_read(&cb), &cb);
    g = sigtimedwait(&all, &si, &to);
    P("  any signal within 300 ms: %s", g > 0 ? signame(g) : "none");

    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = s4;
    cb.aio_sigevent.sigev_value.sival_int = 1234;
    aio_read(&cb);
    g = take(s4, &si, 5000);
    P("SIGEV_SIGNAL SIGRTMIN+4 value 1234: signal=%s si_signo ok=%d si_code=%s value=%d si_pid is the process=%d si_uid is the user=%d si_errno=%d; at receipt %s",
      g > 0 ? signame(g) : "none", si.si_signo == s4, code_name(si.si_code), si.si_value.sival_int, si.si_pid == getpid(), si.si_uid == getuid(), si.si_errno, ST(&cb));
    g = take(s4, &si, 300);
    P("  a second signal within 300 ms: %s", g > 0 ? "yes" : "none");

    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = s0;
    cb.aio_sigevent.sigev_value.sival_ptr = &cb;
    aio_read(&cb);
    g = take(s0, &si, 5000);
    P("SIGEV_SIGNAL SIGRTMIN with sival_ptr=&aiocb: signal=%s pointer is the aiocb=%d", g > 0 ? signame(g) : "none", si.si_value.sival_ptr == (void *)&cb);
    aio_return(&cb);

    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = SIGUSR2;
    cb.aio_sigevent.sigev_value.sival_int = 7;
    aio_read(&cb);
    g = take(SIGUSR2, &si, 5000);
    P("SIGEV_SIGNAL SIGUSR2 value 7: signal=%s si_code=%s value=%d", g > 0 ? signame(g) : "none", code_name(si.si_code), si.si_value.sival_int);
    aio_return(&cb);

    /* A failing request notifies too. */
    int wo = open("/dev/null", O_WRONLY);
    mk(&cb, wo, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = s1;
    cb.aio_sigevent.sigev_value.sival_int = 11;
    errno = 0;
    int r = aio_read(&cb);
    int e = errno;
    g = take(s1, &si, 3000);
    P("SIGEV_SIGNAL on a failing read (write-only fd): submit=%d errno=%s signal=%s value=%d aio_error=%s", r, ee(r, e), g > 0 ? signame(g) : "none", g > 0 ? si.si_value.sival_int : 0, r == 0 ? en(aio_error(&cb)) : "-");
    if (r == 0)
        aio_return(&cb);
    close(wo);

    /* Process-directed or thread-directed: both threads block the signal. */
    pt_sig = SIGRTMIN + 5;
    pthread_t t;
    pthread_create(&t, NULL, pt_run, NULL);
    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = pt_sig;
    aio_read(&cb);
    wait_done(&cb, 5000);
    sleep_ms(300);
    __atomic_store_n(&pt_go, 1, __ATOMIC_SEQ_CST);
    pthread_join(t, NULL);
    P("SIGEV_SIGNAL with the signal blocked in every application thread: pending as seen by the submitting thread=%s, by another thread=%s", pending(pt_sig) ? "yes" : "no", pt_seen ? "yes" : "no");
    take(pt_sig, &si, 100);
    aio_return(&cb);

    /* A handler, unblocked only in the submitting thread. */
    int hs = SIGRTMIN + 6;
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = info_hnd;
    sa.sa_flags = SA_SIGINFO;
    sigemptyset(&sa.sa_mask);
    sigaction(hs, &sa, NULL);
    pt_go = 0;
    pt_sig = hs;
    pthread_create(&t, NULL, pt_run, NULL);
    unblock_sig(hs);
    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    cb.aio_sigevent.sigev_signo = hs;
    cb.aio_sigevent.sigev_value.sival_int = 21;
    aio_read(&cb);
    for (int i = 0; i < 5000 && h_calls == 0; i++)
        sleep_ms(1);
    sleep_ms(100);
    __atomic_store_n(&pt_go, 1, __ATOMIC_SEQ_CST);
    pthread_join(t, NULL);
    P("SIGEV_SIGNAL to an SA_SIGINFO handler unblocked only in the submitting thread: calls=%d ran in the submitting thread=%d si_code=%s value=%d", h_calls, h_in_main, code_name(h_code), h_value);
    block_sig(hs);
    aio_return(&cb);

    /* SIGEV_THREAD. SIGUSR1 is blocked in the caller, SIGUSR2 is not. */
    unblock_sig(SIGUSR2);
    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_THREAD;
    cb.aio_sigevent.sigev_notify_function = nt_fn;
    cb.aio_sigevent.sigev_value.sival_int = 5678;
    nt_cb = &cb;
    aio_read(&cb);
    wait_calls(&nt_calls, 1);
    P("SIGEV_THREAD value 5678: calls=%d value=%d not the caller's thread=%d detached=%d SIGUSR1 (blocked in the caller) blocked=%d SIGUSR2 (unblocked in the caller) blocked=%d aio_error at the call=%s",
      nt_calls, nt_value, nt_not_main, nt_detached, nt_usr1, nt_usr2, en(nt_err));
    aio_return(&cb);
    block_sig(SIGUSR2);

    /* SIGEV_THREAD with attributes: a 4 MiB stack and joinable. */
    pthread_attr_t at;
    pthread_attr_init(&at);
    pthread_attr_setstacksize(&at, 4u << 20);
    pthread_attr_setdetachstate(&at, PTHREAD_CREATE_JOINABLE);
    nt_calls = 0;
    nt_want = 4u << 20;
    mk(&cb, fd, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_THREAD;
    cb.aio_sigevent.sigev_notify_function = nt_fn;
    cb.aio_sigevent.sigev_notify_attributes = &at;
    cb.aio_sigevent.sigev_value.sival_int = 9;
    aio_read(&cb);
    wait_calls(&nt_calls, 1);
    P("SIGEV_THREAD with attributes (stack 4 MiB, joinable): calls=%d value=%d stack at least 4 MiB=%d detached=%d", nt_calls, nt_value, nt_big, nt_detached);
    aio_return(&cb);
    pthread_attr_destroy(&at);
    nt_want = 0;

    /* SIGEV_THREAD for a failing request. */
    wo = open("/dev/null", O_WRONLY);
    nt_calls = 0;
    mk(&cb, wo, buf, 10, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_THREAD;
    cb.aio_sigevent.sigev_notify_function = nt_fn;
    cb.aio_sigevent.sigev_value.sival_int = 10;
    errno = 0;
    r = aio_read(&cb);
    e = errno;
    if (r == 0)
        wait_calls(&nt_calls, 1);
    P("SIGEV_THREAD on a failing read (write-only fd): submit=%d errno=%s calls=%d aio_error at the call=%s", r, ee(r, e), nt_calls, r == 0 ? en(nt_err) : "-");
    if (r == 0)
        aio_return(&cb);
    close(wo);

    /* Three requests with SIGEV_THREAD: one call each. */
    nt_calls = 0;
    nt_cb = NULL;
    struct aiocb m[3];
    for (int i = 0; i < 3; i++) {
        mk(&m[i], fd, buf + 10 * i, 10, 10 * i);
        m[i].aio_sigevent.sigev_notify = SIGEV_THREAD;
        m[i].aio_sigevent.sigev_notify_function = nt_fn;
        aio_read(&m[i]);
    }
    wait_calls(&nt_calls, 3);
    sleep_ms(200);
    P("three requests with SIGEV_THREAD: calls=%d", nt_calls);
    for (int i = 0; i < 3; i++)
        aio_return(&m[i]);
    (void)s1;
    close(fd);
    unlink(path("n.dat"));
}

static void s_notify_bad(void)
{
    mkdata("nb.dat", 10);
    int fd = open(path("nb.dat"), O_RDONLY);
    unlink(path("nb.dat"));
    char buf[16];
    struct aiocb cb;
    mk(&cb, fd, buf, 10, 0);
    const char *what = "";
    switch (g_arg) {
    case 0:
        cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
        cb.aio_sigevent.sigev_signo = 65;
        what = "SIGEV_SIGNAL with signal 65";
        break;
    case 1:
        cb.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
        cb.aio_sigevent.sigev_signo = -1;
        what = "SIGEV_SIGNAL with signal -1";
        break;
    case 2:
        cb.aio_sigevent.sigev_notify = 99;
        what = "sigev_notify 99";
        break;
    case 3:
        cb.aio_sigevent.sigev_notify = SIGEV_THREAD_ID;
        cb.aio_sigevent.sigev_signo = SIGRTMIN;
        what = "SIGEV_THREAD_ID";
        break;
    default:
        cb.aio_sigevent.sigev_notify = SIGEV_THREAD;
        cb.aio_sigevent.sigev_notify_function = NULL;
        what = "SIGEV_THREAD with a NULL function";
        break;
    }
    block_sig(SIGRTMIN);
    char label[100];
    snprintf(label, sizeof label, "aio_read with %s", what);
    if (g_arg == 4) {
        /* Print nothing until the notification has had time to run: a crash in the
         * notification thread would otherwise race with the line. */
        errno = 0;
        int rc = aio_read(&cb);
        int e = errno;
        sleep_ms(500);
        fin(label, rc, e, &cb);
        P("  process still alive after 500 ms");
        return;
    }
    SUBMIT(label, aio_read(&cb), &cb);
    sleep_ms(200);
    P("  process still alive after 200 ms");
}

/* ------------------------------------------------------------------ ordering per descriptor */

static void s_order(void)
{
    int fd = open(path("o"), O_RDWR | O_CREAT | O_TRUNC | O_APPEND, 0600);
    struct aiocb cbs[8];
    char ch[8], got[16];
    int ok = 0;
    for (int r = 0; r < 100; r++) {
        ftruncate(fd, 0);
        for (int k = 0; k < 8; k++) {
            ch[k] = (char)('a' + k);
            mk(&cbs[k], fd, &ch[k], 1, 0);
            aio_write(&cbs[k]);
        }
        for (int k = 0; k < 8; k++) {
            wait_done(&cbs[k], SLOW_MS);
            aio_return(&cbs[k]);
        }
        memset(got, 0, sizeof got);
        pread(fd, got, 8, 0);
        ok += memcmp(got, "abcdefgh", 8) == 0;
    }
    P("O_APPEND file, 8 one-byte aio_write per round: data in submission order in %d/100 rounds", ok);
    close(fd);

    fd = open(path("o"), O_RDWR | O_TRUNC);
    ok = 0;
    for (int r = 0; r < 100; r++) {
        for (int k = 0; k < 8; k++) {
            ch[k] = (char)('a' + k);
            mk(&cbs[k], fd, &ch[k], 1, 0);
            aio_write(&cbs[k]);
        }
        for (int k = 0; k < 8; k++) {
            wait_done(&cbs[k], SLOW_MS);
            aio_return(&cbs[k]);
        }
        pread(fd, got, 1, 0);
        ok += got[0] == 'h';
    }
    P("regular file, 8 one-byte aio_write to offset 0 per round: the last submitted is the byte left in %d/100 rounds", ok);
    static const char *pats[] = {"AAAAAAAA", "BBBBBB", "CCCC", "DD"};
    ok = 0;
    for (int r = 0; r < 100; r++) {
        for (int k = 0; k < 4; k++) {
            mk(&cbs[k], fd, pats[k], strlen(pats[k]), 0);
            aio_write(&cbs[k]);
        }
        for (int k = 0; k < 4; k++) {
            wait_done(&cbs[k], SLOW_MS);
            aio_return(&cbs[k]);
        }
        pread(fd, got, 8, 0);
        ok += memcmp(got, "DDCCBBAA", 8) == 0;
    }
    P("regular file, overlapping aio_write of 8, 6, 4, 2 bytes at offset 0 per round: \"DDCCBBAA\" (submission order) in %d/100 rounds", ok);
    close(fd);
    unlink(path("o"));

    int p[2];
    pipe(p);
    ok = 0;
    for (int r = 0; r < 100; r++) {
        for (int k = 0; k < 8; k++) {
            ch[k] = (char)('a' + k);
            mk(&cbs[k], p[1], &ch[k], 1, 0);
            aio_write(&cbs[k]);
        }
        for (int k = 0; k < 8; k++) {
            wait_done(&cbs[k], SLOW_MS);
            aio_return(&cbs[k]);
        }
        read(p[0], got, 8);
        ok += memcmp(got, "abcdefgh", 8) == 0;
    }
    P("pipe, 8 one-byte aio_write per round: data in submission order in %d/100 rounds", ok);

    /* Reads queued on an empty pipe receive the bytes in submission order. */
    ok = 0;
    for (int r = 0; r < 20; r++) {
        char b[4] = {0};
        for (int k = 0; k < 4; k++) {
            mk(&cbs[k], p[0], &b[k], 1, 0);
            aio_read(&cbs[k]);
        }
        for (int k = 0; k < 4; k++) {
            write(p[1], &"wxyz"[k], 1);
            /* wait until one more request is done */
            for (int i = 0; i < 10000; i++) {
                int n = 0;
                for (int j = 0; j < 4; j++)
                    n += done(&cbs[j]);
                if (n > k)
                    break;
                sleep_ms(1);
            }
        }
        for (int k = 0; k < 4; k++) {
            wait_done(&cbs[k], SLOW_MS);
            aio_return(&cbs[k]);
        }
        ok += memcmp(b, "wxyz", 4) == 0;
    }
    P("pipe, 4 one-byte aio_read queued on an empty pipe, bytes fed one at a time: each read got the byte of its rank in %d/20 rounds", ok);

    /* A request on another descriptor is not held up by a blocked one. */
    char rb[8];
    struct aiocb B, F;
    mk(&B, p[0], rb, 4, 0);
    aio_read(&B);
    settle();
    int z = open("/dev/zero", O_RDONLY);
    mk(&F, z, rb + 4, 4, 0);
    aio_read(&F);
    int fdone = wait_done(&F, 5000);
    P("a read of /dev/zero while a pipe read is blocked: completes=%s; the pipe read still %s", fdone ? "yes" : "no", en(aio_error(&B)));
    aio_return(&F);
    write(p[1], "abcd", 4);
    wait_done(&B, SLOW_MS);
    aio_return(&B);
    close(z);
    close(p[0]);
    close(p[1]);
}

/* ------------------------------------------------------------------ fork */

static void s_fork(void)
{
    int p[2];
    pipe(p);
    char pb[16], db[16];
    struct aiocb Pc, Dc;
    mk(&Pc, p[0], pb, 4, 0);
    aio_read(&Pc);
    int z = open("/dev/zero", O_RDONLY);
    mk(&Dc, z, db, 8, 0);
    aio_read(&Dc);
    wait_done(&Dc, SLOW_MS);
    settle();
    P("parent: pipe read blocked (P), /dev/zero read complete and not yet returned (D)");
    pid_t c = fork();
    if (c == 0) {
        wait_limit_ms = 3000;
        P("child: aio_error P=%s D=%s", en(aio_error(&Pc)), en(aio_error(&Dc)));
        errno = 0;
        ssize_t r = aio_return(&Dc);
        E = errno;
        P("child: aio_return D=%zd errno=%s", r, ee(r, E));
        const struct aiocb *l[] = {&Pc};
        struct timespec to = {0, 100000000};
        errno = 0;
        int s = aio_suspend(l, 1, &to);
        E = errno;
        P("child: aio_suspend({P}, 1, 100 ms)=%d errno=%s", s, ee(s, E));
        errno = 0;
        s = aio_cancel(p[0], &Pc);
        E = errno;
        P("child: aio_cancel(pipe, &P)=%s errno=%s; P=%s", cancel_name(s), ee(s, E), en(aio_error(&Pc)));
        errno = 0;
        s = aio_cancel(p[0], NULL);
        E = errno;
        P("child: aio_cancel(pipe, NULL)=%s errno=%s", cancel_name(s), ee(s, E));
        struct aiocb N;
        mk(&N, z, db, 6, 0);
        SUBMIT("child: a new aio_read of /dev/zero", aio_read(&N), &N);
        struct aiocb Q;
        mk(&Q, p[0], pb, 4, 0);
        aio_read(&Q);
        settle();
        P("child: its own pipe read: %s", en(aio_error(&Q)));
        errno = 0;
        s = aio_cancel(p[0], &Q);
        E = errno;
        P("child: aio_cancel(pipe, its own read)=%s errno=%s", cancel_name(s), ee(s, E));
        fflush(stdout);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
    P("parent: child exit=%d; aio_error P=%s D=%s", WIFEXITED(st) ? WEXITSTATUS(st) : -1, en(aio_error(&Pc)), en(aio_error(&Dc)));
    write(p[1], "four", 4);
    fin("parent: P after 4 bytes", 0, 0, &Pc);
    fin("parent: D", 0, 0, &Dc);
    close(z);
}

/* fork with no request pending: the child can use AIO. */
static void s_fork_idle(void)
{
    int z = open("/dev/zero", O_RDONLY);
    char db[16];
    struct aiocb Dc;
    mk(&Dc, z, db, 8, 0);
    SUBMIT("parent: /dev/zero read before fork", aio_read(&Dc), &Dc);
    pid_t c = fork();
    if (c == 0) {
        wait_limit_ms = 3000;
        struct aiocb N;
        mk(&N, z, db, 6, 0);
        SUBMIT("child: a new aio_read of /dev/zero", aio_read(&N), &N);
        fflush(stdout);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
    P("parent: child exit=%d", WIFEXITED(st) ? WEXITSTATUS(st) : -1);
    mk(&Dc, z, db, 4, 0);
    SUBMIT("parent: another read after the fork", aio_read(&Dc), &Dc);
    close(z);
}

/* ------------------------------------------------------------------ exec */

static void s_exec(void)
{
    int p1[2], p2[2];
    pipe(p1);
    pipe(p2);
    fcntl(p2[1], F_SETPIPE_SZ, 65536);
    int sig = SIGRTMIN + 1;
    block_sig(sig);
    char rb[16];
    static char big[100000];
    struct aiocb R, W;
    mk(&R, p1[0], rb, 10, 0);
    R.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    R.aio_sigevent.sigev_signo = sig;
    aio_read(&R);
    mk(&W, p2[1], big, sizeof big, 0);
    W.aio_sigevent.sigev_notify = SIGEV_SIGNAL;
    W.aio_sigevent.sigev_signo = sig;
    aio_write(&W);
    int q = wait_fill(p2[0], 65536);
    settle();
    P("before exec: a read blocked on pipe 1 (%s), a write of 100000 bytes to pipe 2 in progress (%d bytes in it, %s)", en(aio_error(&R)), q, en(aio_error(&W)));
    char a[3][16];
    snprintf(a[0], sizeof a[0], "%d", p1[0]);
    snprintf(a[1], sizeof a[1], "%d", p1[1]);
    snprintf(a[2], sizeof a[2], "%d", p2[0]);
    tw_alarm(10);
    fflush(stdout);
    execl("/proc/self/exe", "aio_diff", D, "afterexec", a[0], a[1], a[2], (char *)NULL);
    P("exec failed: %s", en(errno));
}

static int after_exec(char **argv)
{
    alarm(0);
    int r1 = atoi(argv[3]), w1 = atoi(argv[4]), r2 = atoi(argv[5]);
    int sig = SIGRTMIN + 1;
    P("after exec: completion signal pending=%s", pending(sig) ? "yes" : "no");
    int q = fionread(r2);
    sleep_ms(300);
    P("after exec: pipe 2 holds %d bytes; 300 ms later %d", q, fionread(r2));
    P("after exec: drained %ld bytes from pipe 2", drain(r2, fionread(r2)));
    sleep_ms(300);
    P("after exec: pipe 2 holds %d bytes 300 ms after draining", fionread(r2));
    write(w1, "Z", 1);
    sleep_ms(300);
    P("after exec: a byte written to pipe 1 is still there 300 ms later: %s", fionread(r1) == 1 ? "yes" : "no");
    P("after exec: completion signal pending=%s", pending(sig) ? "yes" : "no");
    return 0;
}

/* ------------------------------------------------------------------ close with requests pending */

static void s_close(void)
{
    int p[2];
    pipe(p);
    char ba[16], bb[16];
    struct aiocb A, B;
    mk(&A, p[0], ba, 3, 0);
    aio_read(&A);
    settle();
    mk(&B, p[0], bb, 3, 0);
    aio_read(&B);
    settle();
    P("pipe: read A blocked, read B queued: %s, %s", en(aio_error(&A)), en(aio_error(&B)));
    long t0 = now_ms();
    errno = 0;
    int r = close(p[0]);
    E = errno;
    P("close(read end): %d errno=%s returned within 1 s: %s", r, ee(r, E), now_ms() - t0 < 1000 ? "yes" : "no");
    settle();
    P("  400 ms later: A=%s B=%s", en(aio_error(&A)), en(aio_error(&B)));
    write(p[1], "abc", 3);
    int ad = wait_done(&A, 3000);
    P("  after writing 3 bytes: A done=%s", ad ? "yes" : "no");
    if (ad)
        fin("  A", 0, 0, &A);
    int bd = wait_done(&B, 1000);
    if (!bd) {
        write(p[1], "def", 3);
        bd = wait_done(&B, 3000);
        P("  B needed 3 more bytes: done=%s", bd ? "yes" : "no");
    }
    if (bd)
        fin("  B", 0, 0, &B);
    if (!ad) {
        write(p[1], "ghi", 3);
        P("  A after 3 more bytes: done=%s", wait_done(&A, 3000) ? "yes" : "no");
        if (done(&A))
            fin("  A", 0, 0, &A);
    }
    struct aiocb C;
    mk(&C, p[0], ba, 3, 0);
    SUBMIT("aio_read on the closed descriptor", aio_read(&C), &C);
    errno = 0;
    r = aio_cancel(p[0], NULL);
    E = errno;
    P("aio_cancel(closed descriptor, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    close(p[1]);
}

/* ------------------------------------------------------------------ the *64 interfaces together */

static void s_lfs(void)
{
    int fd = open(path("lfs"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    struct aiocb64 w, rd, s;
    char b[16] = {0};
    memset(&w, 0, sizeof w);
    w.aio_fildes = fd;
    w.aio_buf = "large";
    w.aio_nbytes = 5;
    w.aio_offset = (off64_t)5 << 30;
    w.aio_sigevent.sigev_notify = SIGEV_NONE;
    errno = 0;
    int r = aio_write64(&w);
    E = errno;
    P("aio_write64 at 5 GiB: submit=%d errno=%s", r, ee(r, E));
    const struct aiocb64 *l[] = {&w};
    errno = 0;
    r = aio_suspend64(l, 1, NULL);
    E = errno;
    P("  aio_suspend64: %d errno=%s; %s", r, ee(r, E), ST64(&w));
    struct stat st;
    fstat(fd, &st);
    P("  file size=%lld", (long long)st.st_size);
    memset(&rd, 0, sizeof rd);
    rd.aio_fildes = fd;
    rd.aio_buf = b;
    rd.aio_nbytes = 5;
    rd.aio_offset = (off64_t)5 << 30;
    rd.aio_lio_opcode = LIO_READ;
    rd.aio_sigevent.sigev_notify = SIGEV_NONE;
    struct aiocb64 *ll[] = {&rd};
    errno = 0;
    r = lio_listio64(LIO_WAIT, ll, 1, NULL);
    E = errno;
    P("lio_listio64 read at 5 GiB: %d errno=%s; return=%zd data=\"%.5s\"", r, ee(r, E), aio_return64(&rd), b);
    errno = 0;
    r = aio_cancel64(fd, NULL);
    E = errno;
    P("aio_cancel64(fd, NULL): %s errno=%s", cancel_name(r), ee(r, E));
    memset(&s, 0, sizeof s);
    s.aio_fildes = fd;
    s.aio_sigevent.sigev_notify = SIGEV_NONE;
    errno = 0;
    r = aio_fsync64(O_SYNC, &s);
    E = errno;
    const struct aiocb64 *l2[] = {&s};
    aio_suspend64(l2, 1, NULL);
    P("aio_fsync64(O_SYNC): submit=%d errno=%s; %s", r, ee(r, E), ST64(&s));
    /* The non-64 functions accept the same blocks (struct aiocb has a 64-bit offset). */
    struct aiocb n;
    mk(&n, fd, b, 5, (off_t)5 << 30);
    SUBMIT("aio_read at 5 GiB through struct aiocb", aio_read(&n), &n);
    close(fd);
    unlink(path("lfs"));
}

/* ------------------------------------------------------------------ resource limits */

static void *noop(void *a) { return a; }

static void s_limits(void)
{
    wait_limit_ms = 3000;
    struct rlimit rl = {1, 1};
    setrlimit(RLIMIT_NPROC, &rl);
    pthread_t t;
    int r = pthread_create(&t, NULL, noop, NULL);
    P("RLIMIT_NPROC 1 (the user has other processes): pthread_create=%s", en(r));
    if (r == 0)
        pthread_join(t, NULL);
    int z = open("/dev/zero", O_RDONLY);
    char b[16];
    struct aiocb cb;
    mk(&cb, z, b, 8, 0);
    SUBMIT("  aio_read of /dev/zero", aio_read(&cb), &cb);
    int fd = open(path("lim"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    unlink(path("lim"));
    mk(&cb, fd, "abc", 3, 0);
    SUBMIT("  aio_write", aio_write(&cb), &cb);
    mk(&cb, fd, NULL, 0, 0);
    SUBMIT("  aio_fsync(O_SYNC)", aio_fsync(O_SYNC, &cb), &cb);
    struct aiocb a1, a2, *l[2] = {&a1, &a2};
    mk(&a1, z, b, 4, 0);
    a1.aio_lio_opcode = LIO_READ;
    mk(&a2, fd, "de", 2, 3);
    a2.aio_lio_opcode = LIO_WRITE;
    errno = 0;
    r = lio_listio(LIO_WAIT, l, 2, NULL);
    E = errno;
    P("  lio_listio(LIO_WAIT, 2 entries): %d errno=%s", r, ee(r, E));
    entries("    entries", l, 2);
    mk(&cb, z, b, 8, 0);
    cb.aio_sigevent.sigev_notify = SIGEV_THREAD;
    cb.aio_sigevent.sigev_notify_function = simple_thread;
    simple_thread_calls = 0;
    SUBMIT("  aio_read of /dev/zero with SIGEV_THREAD", aio_read(&cb), &cb);
    sleep_ms(300);
    P("    notification calls=%d", simple_thread_calls);
    close(fd);
    close(z);
}

/* ------------------------------------------------------------------ outside the record */

static void x_pool_one(void)
{
    struct aioinit ai;
    memset(&ai, 0, sizeof ai);
    ai.aio_threads = (int)g_arg;
    ai.aio_num = 32;
    ai.aio_idle_time = 1;
    if (g_arg > 0)
        aio_init(&ai);
    int p[2];
    pipe(p);
    char b[8];
    struct aiocb B, F;
    mk(&B, p[0], b, 4, 0);
    aio_read(&B);
    settle();
    int z = open("/dev/zero", O_RDONLY);
    mk(&F, z, b + 4, 4, 0);
    aio_read(&F);
    int d = wait_done(&F, 1000);
    P("%s: /dev/zero read completes within 1 s while a pipe read is blocked: %s", g_arg > 0 ? "aio_init(aio_threads=1)" : "no aio_init", d ? "yes" : "no");
    write(p[1], "abcd", 4);
    wait_done(&B, 3000);
    P("  after the pipe gets data: pipe read %s, /dev/zero read %s", en(aio_error(&B)), en(aio_error(&F)));
}

static void x_fork_later(void)
{
    int z = open("/dev/zero", O_RDONLY);
    char db[16];
    struct aiocb Dc;
    mk(&Dc, z, db, 8, 0);
    aio_read(&Dc);
    wait_done(&Dc, 3000);
    aio_return(&Dc);
    sleep_ms(g_arg);
    pid_t c = fork();
    if (c == 0) {
        wait_limit_ms = 3000;
        struct aiocb N;
        mk(&N, z, db, 6, 0);
        char label[100];
        snprintf(label, sizeof label, "fork %ld ms after the last completion; child: aio_read of /dev/zero", g_arg);
        SUBMIT(label, aio_read(&N), &N);
        fflush(stdout);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
}

static void x_prio(void)
{
    int p[2];
    pipe(p);
    char b[4] = {0};
    struct aiocb c[4];
    static const int pr[4] = {0, 5, 0, 2};
    mk(&c[0], p[0], &b[0], 1, 0);
    aio_read(&c[0]);
    settle();
    for (int k = 1; k < 4; k++) {
        mk(&c[k], p[0], &b[k], 1, 0);
        c[k].aio_reqprio = pr[k];
        aio_read(&c[k]);
    }
    settle();
    for (int k = 0; k < 4; k++) {
        write(p[1], &"1234"[k], 1);
        for (int i = 0; i < 5000; i++) {
            int n = 0;
            for (int j = 0; j < 4; j++)
                n += done(&c[j]);
            if (n > k)
                break;
            sleep_ms(1);
        }
    }
    P("pipe reads with aio_reqprio A=0 (blocked first), then B=5, C=0, D=2 queued; bytes 1..4 fed one at a time: A=%c B=%c C=%c D=%c", b[0], b[1], b[2], b[3]);
}

static void x_dup(void)
{
    int p[2];
    pipe(p);
    int d = dup(p[0]);
    char b[4] = {0};
    struct aiocb A, B;
    mk(&A, p[0], &b[0], 1, 0);
    aio_read(&A);
    settle();
    mk(&B, d, &b[1], 1, 0);
    aio_read(&B);
    settle();
    int r = aio_cancel(d, &B);
    P("pipe read blocked on fd, second read on dup(fd): aio_cancel(dup, &second)=%s; second's aio_error=%s", cancel_name(r), en(aio_error(&B)));
    write(p[1], "xy", 2);
    wait_done(&A, 3000);
    wait_done(&B, 3000);
}

/* ------------------------------------------------------------------ main */

static int count_left(void)
{
    DIR *dp = opendir(D);
    int n = 0;
    struct dirent *e;
    while (dp && (e = readdir(dp)))
        if (strcmp(e->d_name, ".") && strcmp(e->d_name, ".."))
            n++;
    if (dp)
        closedir(dp);
    return n;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: aio_diff DIR [extra]\n");
        return 2;
    }
    D = argv[1];
    struct rlimit core = {0, 0};
    setrlimit(RLIMIT_CORE, &core);
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    if (argc >= 6 && strcmp(argv[2], "afterexec") == 0)
        return after_exec(argv);
    if (argc >= 3 && strcmp(argv[2], "extra") == 0) {
        g_arg = 0;
        run_section("extra: no aio_init", x_pool_one, 20);
        g_arg = 1;
        run_section("extra: aio_init aio_threads=1", x_pool_one, 20);
        run_section("extra: priorities", x_prio, 20);
        static const long waits[] = {0, 500, 1500, 3000};
        for (int i = 0; i < 4; i++) {
            g_arg = waits[i];
            run_section("extra: fork after a delay", x_fork_later, 20);
        }
        run_section("extra: dup", x_dup, 20);
        return 0;
    }
    if (argc >= 3 && strcmp(argv[2], "lio-eintr") == 0) {
        /* Only the LIO_WAIT interrupted cases, for repeat loops under load (aio.md, "Timing"). */
        for (g_arg = 0; g_arg < 2; g_arg++)
            run_section("lio_listio interrupted", s_lio_eintr, 30);
        return 0;
    }
    if (argc >= 3 && strcmp(argv[2], "init-twice") == 0) {
        /* Only the "aio_init twice" case, for repeat loops under load (aio.md, "Timing"). */
        run_section("aio_init twice", s_init_twice, 30);
        return 0;
    }
    P("running as root: %s", geteuid() == 0 ? "yes" : "no");
    run_section("constants", s_consts, 10);
    run_section("aio_init small", s_init_small, 30);
    run_section("aio_init negative", s_init_negative, 30);
    run_section("aio_init twice", s_init_twice, 30);
    run_section("aio_read, regular file", s_read, 600);
    run_section("aio_write, regular file", s_write, 600);
    run_section("SIGXFSZ default", s_xfsz_default, 20);
    run_section("SIGXFSZ blocked", s_xfsz_blocked, 20);
    run_section("pipes", s_pipe, 60);
    run_section("SIGPIPE default", s_sigpipe_default, 20);
    run_section("SIGPIPE blocked", s_sigpipe_blocked, 20);
    run_section("sockets", s_socket, 60);
    run_section("aio_fsync", s_fsync, 1800);
    run_section("aio_error, aio_return", s_status, 600);
    run_section("aio_suspend", s_suspend, 60);
    run_section("aio_suspend all NULL, no timeout (alarm 2 s)", s_suspend_null_forever, 2);
    for (g_arg = 0; g_arg < 4; g_arg++)
        run_section("aio_suspend invalid timeout (alarm 10 s)", s_suspend_badtime, 10);
    run_section("aio_suspend negative nent (alarm 10 s)", s_suspend_negative_nent, 10);
    run_section("aio_cancel", s_cancel, 60);
    run_section("lio_listio", s_lio, 600);
    static const long bigs[] = {10, -10, 16, -16, 20, -20};
    for (int i = 0; i < 6; i++) {
        g_arg = bigs[i];
        run_section("lio_listio long list", s_lio_big, 30);
    }
    for (g_arg = 0; g_arg < 2; g_arg++)
        run_section("lio_listio interrupted", s_lio_eintr, 30);
    for (g_arg = 0; g_arg < 4; g_arg++)
        run_section("lio_listio negative nent (alarm 10 s)", s_lio_negative, 10);
    run_section("notifications", s_notify, 60);
    for (g_arg = 0; g_arg < 5; g_arg++)
        run_section("invalid notification (alarm 10 s)", s_notify_bad, 10);
    run_section("ordering", s_order, 600);
    run_section("fork with a request pending", s_fork, 30);
    run_section("fork with no request pending", s_fork_idle, 30);
    run_section("exec", s_exec, 30);
    run_section("close with requests pending", s_close, 30);
    run_section("*64 interfaces, offsets past 4 GiB", s_lfs, 1800);
    run_section("RLIMIT_NPROC 1", s_limits, 30);
    P("cleanup: %d entries left in <dir>", count_left());
    return 0;
}
