/* Black-box probes for the spec-versus-man-page review (milestone 3, 2026-10-04): one section per
 * finding, printing what the reference library does. Public interfaces only; nothing here
 * changes the system (the clock, privileged state); the DNS search-list case is in its own run
 * (specman_dns.sh). Build: cc -std=gnu17 -O1 specman_probe.c -lm -lrt -pthread.
 * Sections are named after the finding: p<part>-<number>. With -DNO_LIBM the libm sections and rpmatch
 * are left out, and isastream (for Liella's static products, which have no
 * libm, rpmatch or compatibility symbols yet). */
#define _GNU_SOURCE
#include <aio.h>
#include <errno.h>
#include <fcntl.h>
#include <fenv.h>
#include <locale.h>
#include <math.h>
#include <mqueue.h>
#include <pthread.h>
#include <search.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/msg.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>

/* A compatibility symbol in glibc 2.39 (no default version): bound by its version. */
#ifndef NO_LIBM
__asm__(".symver isastream,isastream@GLIBC_2.2.5");
#endif
extern int isastream(int);
extern int rpmatch(const char *);

static const char *en(int e)
{
    static char b[32];
    switch (e) {
    case 0: return "0";
    case EINVAL: return "EINVAL";
    case EDOM: return "EDOM";
    case ERANGE: return "ERANGE";
    case ENOMEM: return "ENOMEM";
    case EILSEQ: return "EILSEQ";
    case ENOENT: return "ENOENT";
    case EBUSY: return "EBUSY";
    case EBADF: return "EBADF";
    case ENOSYS: return "ENOSYS";
    case EPERM: return "EPERM";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case EAFNOSUPPORT: return "EAFNOSUPPORT";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    case EPROTONOSUPPORT: return "EPROTONOSUPPORT";
    default: snprintf(b, sizeof b, "errno %d", e); return b;
    }
}

#ifndef NO_LIBM
static const char *fx(void)
{
    static char b[64];
    int f = fetestexcept(FE_ALL_EXCEPT);
    snprintf(b, sizeof b, "%s%s%s%s%s", f & FE_INVALID ? "INVALID " : "", f & FE_DIVBYZERO ? "DIVBYZERO " : "", f & FE_OVERFLOW ? "OVERFLOW " : "",
             f & FE_UNDERFLOW ? "UNDERFLOW " : "", f & FE_INEXACT ? "INEXACT" : "");
    return b;
}

#endif
#define MATH(label, expr)                                                                                    \
    do {                                                                                                     \
        feclearexcept(FE_ALL_EXCEPT);                                                                        \
        errno = 0;                                                                                           \
        volatile double r_ = (expr);                                                                         \
        int e_ = errno;                                                                                      \
        printf("  %s = %g errno=%s flags=%s\n", label, (double)r_, en(e_), fx());                            \
    } while (0)

static void *thread_strsignal(void *arg)
{
    *(char **)arg = strsignal(1000);
    return NULL;
}

static int cmp(const void *a, const void *b)
{
    return *(const int *)a - *(const int *)b;
}

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);

    printf("== p1-1 initstate with fewer than 8 bytes\n");
    {
        static char st[256], small[7];
        initstate(1, st, sizeof st);
        errno = 0;
        char *r = initstate(1, small, sizeof small);
        printf("  initstate(n=7): %s errno=%s\n", r == NULL ? "NULL" : "non-NULL (the previous state)", en(errno));
    }

    printf("== p1-2 math_errhandling\n  math_errhandling = %d (MATH_ERRNO %d, MATH_ERREXCEPT %d)\n", math_errhandling, MATH_ERRNO, MATH_ERREXCEPT);

#ifndef NO_LIBM
    printf("== p1-3 domain errors\n");
    MATH("lrint(NAN)", (double)lrint(NAN));
    MATH("lrint(1e300)", (double)lrint(1e300));
    MATH("lround(1e300)", (double)lround(1e300));
    {
        int q;
        MATH("remquo(1, 0)", remquo(1.0, 0.0, &q));
    }
    MATH("fma(INFINITY, 0, 1)", fma(INFINITY, 0.0, 1.0));
    MATH("sqrt(-1)", sqrt(-1.0));
    MATH("log(-1)", log(-1.0));

    printf("== p1-4 pole and range errors\n");
    MATH("logb(0)", logb(0.0));
    MATH("log(0)", log(0.0));
    MATH("hypot(1e-308, 1e-308) (no underflow)", hypot(1e-308, 1e-308));
    MATH("hypot(4.9e-324, 0) (subnormal)", hypot(4.9e-324, 0.0));
    MATH("erf(1e-310)", erf(1e-310));
    MATH("erfc(30)", erfc(30.0));
    MATH("fma(1e308, 10, 0)", fma(1e308, 10.0, 0.0));
    MATH("exp(1000)", exp(1000.0));
    MATH("exp(-1000)", exp(-1000.0));

    printf("== p1-5 feenableexcept/fedisableexcept/fegetexcept\n");
    {
        int a = fegetexcept();
        int b = feenableexcept(FE_DIVBYZERO);
        int c = fegetexcept();
        int d = fedisableexcept(FE_DIVBYZERO);
        int e = fegetexcept();
        printf("  fegetexcept=%#x feenableexcept(DIVBYZERO)=%#x then fegetexcept=%#x fedisableexcept(DIVBYZERO)=%#x then fegetexcept=%#x\n", a, b, c, d, e);
        printf("  feenableexcept(0x40000)=%d\n", feenableexcept(0x40000));
    }

#endif
    printf("== p1-6 getsubopt on an unknown suboption\n");
    {
        char opts[] = "foo=bar,ro";
        char *p = opts, *v = (char *)1;
        char *const tokens[] = {"ro", "rw", NULL};
        int r = getsubopt(&p, tokens, &v);
        printf("  getsubopt(\"foo=bar,...\") = %d, *valuep = %s\n", r, v == NULL ? "NULL" : v == (char *)1 ? "untouched" : v);
        char opts2[] = "foo,ro";
        p = opts2;
        r = getsubopt(&p, tokens, &v);
        printf("  getsubopt(\"foo,...\") = %d, *valuep = %s\n", r, v == NULL ? "NULL" : v);
    }

    printf("== p1-7 malloc above PTRDIFF_MAX\n");
    {
        errno = 0;
        void *m = malloc((size_t)PTRDIFF_MAX + 1);
        printf("  malloc(PTRDIFF_MAX+1) = %s errno=%s\n", m ? "non-NULL" : "NULL", en(errno));
        free(m);
    }

    printf("== p1-8 mbrtowc(NULL) with a partial character pending\n");
    {
        setlocale(LC_CTYPE, "C.UTF-8");
        mbstate_t st;
        memset(&st, 0, sizeof st);
        wchar_t w;
        size_t r = mbrtowc(&w, "\xc3", 1, &st);
        errno = 0;
        size_t r2 = mbrtowc(NULL, NULL, 0, &st);
        printf("  after a partial character (%zd): mbrtowc(NULL, NULL, 0) = %zd errno=%s; mbsinit after = %d\n", (ssize_t)r, (ssize_t)r2, en(errno), mbsinit(&st));
        setlocale(LC_CTYPE, "C");
    }

    printf("== p1-9 localeconv and nl_langinfo pointers\n");
    {
        struct lconv *a = localeconv();
        char *dp = a->decimal_point;
        struct lconv *b = localeconv();
        printf("  localeconv twice: same struct %s, same decimal_point pointer %s\n", a == b ? "yes" : "no", b->decimal_point == dp ? "yes" : "no");
    }

#ifndef NO_LIBM
    printf("== p1-10 rpmatch\n");
    {
        const char *in[] = {"y", "Y", "n", "N", "yes", "ja", "nein", "j", "x", NULL};
        for (const char *l = "C"; l; l = strcmp(l, "C") == 0 ? "xx_YN.UTF-8" : NULL) {
            if (!setlocale(LC_MESSAGES, l)) {
                printf("  locale %s: not available\n", l);
                continue;
            }
            printf("  %s:", l);
            for (int i = 0; in[i]; i++)
                printf(" %s=%d", in[i], rpmatch(in[i]));
            printf("\n");
        }
        setlocale(LC_MESSAGES, "C");
    }

#endif
    printf("== p1-13 ecvt digit cap\n");
    {
        int dec, sign;
        printf("  strlen(ecvt(1/3, 30)) = %zu; strlen(ecvt(1/3, 17)) = %zu; strlen(ecvt(1/3, 100)) = %zu\n", strlen(ecvt(1.0 / 3, 30, &dec, &sign)),
               strlen(ecvt(1.0 / 3, 17, &dec, &sign)), strlen(ecvt(1.0 / 3, 100, &dec, &sign)));
    }

#ifndef NO_LIBM
    printf("== p1-14 y0/y1/yn underflow\n");
    MATH("y0(1e300)", y0(1e300));
    MATH("y1(1e300)", y1(1e300));
    MATH("yn(2, 1e300)", yn(2, 1e300));
    MATH("j0(1e300)", j0(1e300));

#endif
    printf("== p2-1 aio_return twice\n");
    {
        char path[] = "/tmp/specman-aio-XXXXXX";
        int fd = mkstemp(path);
        unlink(path);
        write(fd, "hello", 5);
        static char buf[8];
        struct aiocb cb;
        memset(&cb, 0, sizeof cb);
        cb.aio_fildes = fd;
        cb.aio_buf = buf;
        cb.aio_nbytes = 5;
        aio_read(&cb);
        const struct aiocb *l[1] = {&cb};
        while (aio_error(&cb) == EINPROGRESS)
            aio_suspend(l, 1, NULL);
        errno = 0;
        ssize_t a = aio_return(&cb);
        int ea = errno;
        errno = 0;
        ssize_t b = aio_return(&cb);
        printf("  first aio_return = %zd errno=%s; second = %zd errno=%s\n", a, en(ea), b, en(errno));
        close(fd);
    }

    printf("== p2-2 aio_read with an invalid offset or descriptor (submission versus aio_error)\n");
    {
        static char buf[8];
        struct aiocb cb;
        memset(&cb, 0, sizeof cb);
        cb.aio_fildes = -1;
        cb.aio_buf = buf;
        cb.aio_nbytes = 5;
        errno = 0;
        int r = aio_read(&cb);
        int e = errno;
        if (r == 0) {
            const struct aiocb *l[1] = {&cb};
            while (aio_error(&cb) == EINPROGRESS)
                aio_suspend(l, 1, NULL);
            printf("  fd -1: aio_read = 0; aio_error = %s; aio_return = %zd\n", en(aio_error(&cb)), aio_return(&cb));
        } else {
            printf("  fd -1: aio_read = %d errno=%s\n", r, en(e));
        }
    }

    printf("== p2-3 socketpair in other domains\n");
    {
        int sv[2];
        int doms[] = {AF_UNIX, AF_INET, AF_TIPC};
        const char *names[] = {"AF_UNIX", "AF_INET", "AF_TIPC"};
        for (int i = 0; i < 3; i++) {
            errno = 0;
            int r = socketpair(doms[i], SOCK_STREAM, 0, sv);
            printf("  socketpair(%s, SOCK_STREAM) = %d errno=%s\n", names[i], r, en(errno));
            if (r == 0) {
                close(sv[0]);
                close(sv[1]);
            }
            errno = 0;
            r = socketpair(doms[i], SOCK_SEQPACKET, 0, sv);
            printf("  socketpair(%s, SOCK_SEQPACKET) = %d errno=%s\n", names[i], r, en(errno));
            if (r == 0) {
                close(sv[0]);
                close(sv[1]);
            }
        }
    }

    printf("== p2-6 tdelete of the root of a non-empty tree\n");
    {
        void *root = NULL;
        static int k[3] = {5, 3, 8};
        for (int i = 0; i < 3; i++)
            tsearch(&k[i], &root, cmp);
        void *n3 = tfind(&k[1], &root, cmp), *n8 = tfind(&k[2], &root, cmp);
        void *r = tdelete(&k[0], &root, cmp);
        void *n3b = tfind(&k[1], &root, cmp), *n8b = tfind(&k[2], &root, cmp);
        printf("  delete the root 5 of {5,3,8}: result %s; node 3 %s, node 8 %s after\n",
               r == NULL ? "NULL" : r == n3b ? "is the node of 3" : r == n8b ? "is the node of 8" : "is no node now in the tree", n3 == n3b ? "kept" : "moved",
               n8 == n8b ? "kept" : "moved");
        void *one = NULL;
        tsearch(&k[0], &one, cmp);
        r = tdelete(&k[0], &one, cmp);
        printf("  delete the only node: result %s, root %s\n", r ? "non-NULL" : "NULL", one ? "non-NULL" : "NULL");
    }

    printf("== p3-1 posix_spawn of a file that does not exist\n");
    {
        pid_t pid = 0;
        char *argv[] = {"x", NULL};
        int r = posix_spawn(&pid, "/nonexistent/specman", NULL, NULL, argv, NULL);
        printf("  posix_spawn = %s", en(r));
        if (r == 0) {
            int st;
            waitpid(pid, &st, 0);
            printf(", child exit status %d", WEXITSTATUS(st));
        }
        printf("\n");
        r = posix_spawnp(&pid, "specman-no-such-command", NULL, NULL, argv, NULL);
        printf("  posix_spawnp = %s\n", en(r));
    }

    printf("== p3-2 waitid of _exit(300)\n");
    {
        pid_t c = fork();
        if (c == 0)
            _exit(300);
        siginfo_t si;
        memset(&si, 0, sizeof si);
        waitid(P_PID, c, &si, WEXITED);
        printf("  si_status = %d\n", si.si_status);
    }

    printf("== p3-3 msgrcv with a negative type\n");
    {
        int q = msgget(IPC_PRIVATE, IPC_CREAT | 0600);
        struct {
            long t;
            char c[4];
        } m;
        long types[] = {3, 1, 2};
        for (int i = 0; i < 3; i++) {
            m.t = types[i];
            m.c[0] = 'a' + i;
            msgsnd(q, &m, 1, 0);
        }
        msgrcv(q, &m, 1, -3, IPC_NOWAIT);
        printf("  queue types 3,1,2; msgrcv(-3) receives type %ld\n", m.t);
        msgctl(q, IPC_RMID, NULL);
    }

    printf("== p3-4 settimeofday with a time zone (unprivileged: nothing can change)\n");
    {
        struct timeval tv;
        gettimeofday(&tv, NULL);
        struct timezone tz = {0, 0};
        errno = 0;
        int r = settimeofday(&tv, &tz);
        printf("  settimeofday(&tv, &tz) = %d errno=%s\n", r, en(errno));
        errno = 0;
        r = settimeofday(NULL, &tz);
        printf("  settimeofday(NULL, &tz) = %d errno=%s\n", r, en(errno));
        errno = 0;
        r = settimeofday(&tv, NULL);
        printf("  settimeofday(&tv, NULL) = %d errno=%s\n", r, en(errno));
    }

    printf("== p3-5 gettimeofday's time-zone argument\n");
    {
        struct timeval tv;
        struct timezone tz;
        memset(&tz, 0x55, sizeof tz);
        int r = gettimeofday(&tv, &tz);
        printf("  gettimeofday(&tv, &tz) = %d; tz after = {%d, %d} (was 0x55 bytes)\n", r, tz.tz_minuteswest, tz.tz_dsttime);
    }

    printf("== p3-6 mq_notify registered twice by the same process\n");
    {
        mqd_t q = mq_open("/specman-mq", O_RDWR | O_CREAT | O_EXCL, 0600, NULL);
        mq_unlink("/specman-mq");
        struct sigevent ev;
        memset(&ev, 0, sizeof ev);
        ev.sigev_notify = SIGEV_NONE;
        int a = mq_notify(q, &ev);
        errno = 0;
        int b = mq_notify(q, &ev);
        printf("  first = %d; second = %d errno=%s\n", a, b, en(errno));
        mq_close(q);
    }

    printf("== p3-7 shm_open name length\n");
    {
        char name[400];
        for (int len = 254; len <= 257; len++) {
            name[0] = '/';
            memset(name + 1, 'n', (size_t)len);
            name[len + 1] = 0;
            errno = 0;
            int fd = shm_open(name, O_RDWR | O_CREAT | O_EXCL, 0600);
            printf("  name of %d bytes after '/': %s\n", len, fd >= 0 ? "ok" : en(errno));
            if (fd >= 0) {
                close(fd);
                shm_unlink(name);
            }
        }
    }

#ifndef NO_LIBM
    printf("== p3-8 isastream\n");
    {
        errno = 0;
        int a = isastream(0);
        int ea = errno;
        errno = 0;
        int b = isastream(999);
        printf("  isastream(0) = %d errno=%s; isastream(999) = %d errno=%s\n", a, en(ea), b, en(errno));
    }

#endif
    printf("== p3-9 strsignal's buffer for an unknown signal\n");
    {
        char *mine = strsignal(1000);
        char *theirs = NULL;
        pthread_t t;
        pthread_create(&t, NULL, thread_strsignal, &theirs);
        pthread_join(t, NULL);
        printf("  main %s; another thread's buffer %s\n", mine, mine == theirs ? "the same" : "different");
    }

    printf("== p3-11 SIGRTMIN\n  SIGRTMIN = %d, __libc_current_sigrtmin() = %d\n", SIGRTMIN, __libc_current_sigrtmin());
    return 0;
}
