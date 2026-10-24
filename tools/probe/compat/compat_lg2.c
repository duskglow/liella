/* COMPAT-BEHAVIOUR-3 (LG-2, LG-0b probe lines): the old (compat) versions of the LG-2 behaviour rows that
 * compat_libc.c and compat_thread.c do not cover, plus __isnanf128@GLIBC_2.26 (LG-4's started row).
 * Black-box: each old version is bound with .symver and called through a local name next to the default
 * one; only observable results are printed (no addresses, pids or times). Every output line starts with
 * the versioned name it observes. readdir's d_type is forced to DT_UNKNOWN in the ALTDIRFUNC callbacks
 * and results are sorted, so nothing depends on the file system. Run by tools/probe/compat/run.sh. */
#define _GNU_SOURCE
#include <aio.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <glob.h>
#include <math.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern int glob_225(const char *, int, int (*)(const char *, int), glob_t *);
__asm__(".symver glob_225, glob@GLIBC_2.2.5");
extern int glob64_225(const char *, int, int (*)(const char *, int), glob64_t *);
__asm__(".symver glob64_225, glob64@GLIBC_2.2.5");
extern int timer_create_225(clockid_t, struct sigevent *, void *);
__asm__(".symver timer_create_225, timer_create@GLIBC_2.2.5");
extern int timer_settime_225(long, int, const struct itimerspec *, struct itimerspec *);
__asm__(".symver timer_settime_225, timer_settime@GLIBC_2.2.5");
extern int timer_gettime_225(long, struct itimerspec *);
__asm__(".symver timer_gettime_225, timer_gettime@GLIBC_2.2.5");
extern int timer_getoverrun_225(long);
__asm__(".symver timer_getoverrun_225, timer_getoverrun@GLIBC_2.2.5");
extern int timer_delete_225(long);
__asm__(".symver timer_delete_225, timer_delete@GLIBC_2.2.5");
extern int lio_listio_225(int, struct aiocb *const[], int, struct sigevent *);
__asm__(".symver lio_listio_225, lio_listio@GLIBC_2.2.5");
extern int lio_listio64_225(int, struct aiocb64 *const[], int, struct sigevent *);
__asm__(".symver lio_listio64_225, lio_listio64@GLIBC_2.2.5");
typedef int (*main_fn)(int, char **, char **);
extern int start_225(main_fn, int, char **, void (*)(void), void (*)(void), void (*)(void), void *);
__asm__(".symver start_225, __libc_start_main@GLIBC_2.2.5");
extern int start_234(main_fn, int, char **, void (*)(void), void (*)(void), void (*)(void), void *);
__asm__(".symver start_234, __libc_start_main@GLIBC_2.34");
extern void push_defer_225(void *, void (*)(void *), void *);
__asm__(".symver push_defer_225, _pthread_cleanup_push_defer@GLIBC_2.2.5");
extern void pop_restore_225(void *, int);
__asm__(".symver pop_restore_225, _pthread_cleanup_pop_restore@GLIBC_2.2.5");
extern int pthread_atfork_225(void (*)(void), void (*)(void), void (*)(void));
__asm__(".symver pthread_atfork_225, pthread_atfork@GLIBC_2.2.5");
extern int stime_225(const time_t *);
__asm__(".symver stime_225, stime@GLIBC_2.2.5");
extern int isnanf128_226(_Float128);
__asm__(".symver isnanf128_226, __isnanf128@GLIBC_2.26");

static void say(const char *s) { (void)!write(1, s, strlen(s)); }

/* ---- glob / glob64 ---- */
static int n_stat, n_lstat, n_opendir;
static struct dirent ent;
static void *g_opendir(const char *p) { n_opendir++; return opendir(p); }
static void *g_readdir(void *d) {
    struct dirent *e = readdir(d);
    if (!e) return NULL;
    memcpy(&ent, e, sizeof ent);
    ent.d_type = DT_UNKNOWN;
    return &ent;
}
static void g_closedir(void *d) { closedir(d); }
static int g_stat(const char *p, struct stat *st) { n_stat++; return stat(p, st); }
static int g_lstat(const char *p, struct stat *st) { n_lstat++; return lstat(p, st); }

static int cmpstr(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static void glob_case(const char *name, int which, const char *pat, int flags, int alt) {
    glob_t g;
    memset(&g, 0, sizeof g);
    n_stat = n_lstat = n_opendir = 0;
    if (alt) {
        flags |= GLOB_ALTDIRFUNC;
        g.gl_opendir = g_opendir;
        g.gl_readdir = g_readdir;
        g.gl_closedir = g_closedir;
        g.gl_stat = g_stat;
        g.gl_lstat = g_lstat;
    }
    int rc = which == 0 ? glob_225(pat, flags, NULL, &g) : which == 1 ? glob64_225(pat, flags, NULL, (glob64_t *)&g) : glob(pat, flags, NULL, &g);
    /* sorted here so GLOB_NOSORT's readdir order never reaches the record */
    if (rc == 0) qsort(g.gl_pathv + g.gl_offs, g.gl_pathc, sizeof(char *), cmpstr);
    printf("%s rc=%d n=%zu [", name, rc, rc == 0 ? g.gl_pathc : (size_t)0);
    for (size_t i = 0; rc == 0 && i < g.gl_pathc; i++) printf("%s%s", i ? " " : "", g.gl_pathv[i]);
    printf("]");
    if (alt) printf(" stat=%d lstat=%d opendir=%d", n_stat, n_lstat, n_opendir);
    if (rc == 0) globfree(&g);
}

static void probe_glob(void) {
    mkdir("g", 0755);
    mkdir("g/s", 0755);
    close(open("g/a", O_CREAT | O_WRONLY, 0644));
    close(open("g/b", O_CREAT | O_WRONLY, 0644));
    close(open("g/s/f", O_CREAT | O_WRONLY, 0644));
    (void)!symlink("missing", "g/dang");
    (void)!symlink("s", "g/ls");
    static const struct { const char *pat; int flags; const char *what; } cs[] = {
        {"g/*", 0, "0"},
        {"g/*", GLOB_MARK, "MARK"},
        {"g/*", GLOB_ONLYDIR, "ONLYDIR"},
        {"g/*/", 0, "0"},
        {"g/d*", 0, "0"},
        {"g/dang", 0, "0"},
        {"g/dang", GLOB_NOCHECK, "NOCHECK"},
        {"g/missing", GLOB_NOCHECK, "NOCHECK"},
        {"g/missing", 0, "0"},
        {"g/*/f", 0, "0"},
        {"g/l*/f", GLOB_MARK, "MARK"},
        {"g/[ab]", GLOB_NOSORT, "NOSORT"},
    };
    const char *names[] = {"glob@GLIBC_2.2.5", "glob64@GLIBC_2.2.5"};
    for (int which = 0; which < 2; which++)
        for (size_t i = 0; i < sizeof cs / sizeof cs[0]; i++)
            for (int alt = 0; alt < 2; alt++) {
                if ((cs[i].flags & GLOB_NOSORT) && alt) continue;
                char nm[128];
                snprintf(nm, sizeof nm, "%s %s %s%s:", names[which], cs[i].pat, cs[i].what, alt ? "|ALTDIRFUNC" : "");
                glob_case(nm, which, cs[i].pat, cs[i].flags, alt);
                printf(" | default: ");
                glob_case("", 2, cs[i].pat, cs[i].flags, alt);
                printf("\n");
            }
}

/* ---- timer_* @GLIBC_2.2.5 (old timer_t) ---- */
static sem_t tsem;
static void tnotify(union sigval v) { (void)v; sem_post(&tsem); }

static void probe_timer(void) {
    unsigned char buf[16];
    memset(buf, 0xAA, sizeof buf);
    struct sigevent sev;
    memset(&sev, 0, sizeof sev);
    sev.sigev_notify = SIGEV_NONE;
    int rc = timer_create_225(CLOCK_MONOTONIC, &sev, buf);
    int changed = 0, last = -1;
    for (int i = 0; i < 16; i++) if (buf[i] != 0xAA) { changed++; last = i; }
    printf("timer_create@GLIBC_2.2.5 SIGEV_NONE: rc=%d bytes changed=%d highest changed byte=%d\n", rc, changed, last);
    long id = 0;
    memcpy(&id, buf, sizeof id);
    struct itimerspec its = {{0, 0}, {10, 0}}, old, cur;
    memset(&old, 0x55, sizeof old);
    errno = 0;
    rc = timer_settime_225(id, 0, &its, &old);
    printf("timer_settime@GLIBC_2.2.5 10s: rc=%d errno=%d old=%ld.%09ld/%ld.%09ld\n", rc, rc ? errno : 0,
           (long)old.it_value.tv_sec, old.it_value.tv_nsec, (long)old.it_interval.tv_sec, old.it_interval.tv_nsec);
    rc = timer_gettime_225(id, &cur);
    printf("timer_gettime@GLIBC_2.2.5 after settime: rc=%d remaining in [9,10]s=%s interval=%ld\n", rc,
           rc == 0 && cur.it_value.tv_sec >= 9 && cur.it_value.tv_sec <= 10 ? "yes" : "no", (long)cur.it_interval.tv_sec);
    rc = timer_getoverrun_225(id);
    printf("timer_getoverrun@GLIBC_2.2.5 armed SIGEV_NONE: rc=%d\n", rc);
    errno = 0;
    rc = timer_delete_225(id);
    printf("timer_delete@GLIBC_2.2.5: rc=%d errno=%d\n", rc, rc ? errno : 0);
    errno = 0;
    rc = timer_delete_225(id);
    printf("timer_delete@GLIBC_2.2.5 again: rc=%d errno=%d\n", rc, rc ? errno : 0);
    errno = 0;
    rc = timer_gettime_225(id, &cur);
    printf("timer_gettime@GLIBC_2.2.5 deleted: rc=%d errno=%d\n", rc, rc ? errno : 0);
    errno = 0;
    rc = timer_settime_225(4095, 0, &its, NULL);
    printf("timer_settime@GLIBC_2.2.5 id 4095 never created: rc=%d errno=%d\n", rc, rc ? errno : 0);
    errno = 0;
    rc = timer_getoverrun_225(4095);
    printf("timer_getoverrun@GLIBC_2.2.5 id 4095 never created: rc=%d errno=%d\n", rc, rc ? errno : 0);
    /* SIGEV_THREAD through the old interface */
    sem_init(&tsem, 0, 0);
    memset(buf, 0xAA, sizeof buf);
    memset(&sev, 0, sizeof sev);
    sev.sigev_notify = SIGEV_THREAD;
    sev.sigev_notify_function = tnotify;
    rc = timer_create_225(CLOCK_MONOTONIC, &sev, buf);
    memcpy(&id, buf, sizeof id);
    struct itimerspec shot = {{0, 0}, {0, 10000000}};
    int rs = rc == 0 ? timer_settime_225(id, 0, &shot, NULL) : -1;
    struct timespec dl;
    clock_gettime(CLOCK_REALTIME, &dl);
    dl.tv_sec += 5;
    int got = rs == 0 && sem_timedwait(&tsem, &dl) == 0;
    printf("timer_create@GLIBC_2.2.5 SIGEV_THREAD 10ms: create rc=%d settime rc=%d notified=%s delete rc=%d\n", rc, rs, got ? "yes" : "no",
           rc == 0 ? timer_delete_225(id) : -1);
    errno = 0;
    rc = timer_create_225(12345, &sev, buf);
    printf("timer_create@GLIBC_2.2.5 bad clock: rc=%d errno=%d\n", rc, rc ? errno : 0);
}

/* ---- lio_listio{,64} @GLIBC_2.2.5 ---- */
static void lio_case(const char *name, int which, int fd, int mode, int nent, int badfd, int nop) {
    static char b1[4], b2[4];
    struct aiocb a[2];
    memset(a, 0, sizeof a);
    a[0].aio_fildes = badfd ? -1 : fd;
    a[0].aio_buf = b1;
    a[0].aio_nbytes = 3;
    a[0].aio_lio_opcode = nop ? LIO_NOP : LIO_READ;
    a[1].aio_fildes = fd;
    a[1].aio_buf = b2;
    a[1].aio_nbytes = 3;
    a[1].aio_offset = 3;
    a[1].aio_lio_opcode = nop ? LIO_NOP : LIO_READ;
    struct aiocb *l[2] = {&a[0], &a[1]};
    errno = 0;
    int rc = which == 0 ? lio_listio_225(mode, l, nent, NULL) : which == 1 ? lio_listio64_225(mode, (struct aiocb64 *const *)l, nent, NULL) : lio_listio(mode, l, nent, NULL);
    int e = rc ? errno : 0;
    if (mode == LIO_NOWAIT && rc == 0)
        for (int i = 0; i < nent; i++) {
            const struct aiocb *one[1] = {l[i]};
            while (aio_error(l[i]) == EINPROGRESS) aio_suspend(one, 1, NULL);
        }
    printf("%s rc=%d errno=%d", name, rc, e);
    for (int i = 0; i < nent && i < 2; i++) {
        int er = aio_error(l[i]);
        printf(" [%d] error=%d return=%zd", i, er, er == EINPROGRESS ? (ssize_t)-2 : aio_return(l[i]));
    }
}

static void probe_lio(void) {
    int fd = open("lio", O_CREAT | O_RDWR | O_TRUNC, 0644);
    (void)!write(fd, "abcdef", 6);
    static const struct { const char *what; int mode, nent, badfd, nop; } cs[] = {
        {"WAIT 2 reads", LIO_WAIT, 2, 0, 0},
        {"WAIT bad fd first", LIO_WAIT, 2, 1, 0},
        {"NOWAIT 2 reads sig=NULL", LIO_NOWAIT, 2, 0, 0},
        {"NOWAIT bad fd first", LIO_NOWAIT, 2, 1, 0},
        {"WAIT nent=0", LIO_WAIT, 0, 0, 0},
        {"WAIT 2 NOP", LIO_WAIT, 2, 0, 1},
        {"mode 99", 99, 2, 0, 0},
        {"WAIT nent=-1", LIO_WAIT, -1, 0, 0},
    };
    const char *names[] = {"lio_listio@GLIBC_2.2.5", "lio_listio64@GLIBC_2.2.5"};
    for (int which = 0; which < 2; which++)
        for (size_t i = 0; i < sizeof cs / sizeof cs[0]; i++) {
            char nm[96];
            snprintf(nm, sizeof nm, "%s %s:", names[which], cs[i].what);
            lio_case(nm, which, fd, cs[i].mode, cs[i].nent, cs[i].badfd, cs[i].nop);
            printf(" | default:");
            lio_case("", 2, fd, cs[i].mode, cs[i].nent, cs[i].badfd, cs[i].nop);
            printf("\n");
        }
    close(fd);
}

/* ---- __libc_start_main @GLIBC_2.2.5 vs @GLIBC_2.34, each in a child ---- */
static void s_init(void) { say(" init"); }
static void s_fini(void) { say(" fini"); }
static void s_rtld_fini(void) { say(" rtld_fini"); }
static char **s_argv;
static int s_main(int argc, char **argv, char **envp) {
    char m[96];
    snprintf(m, sizeof m, " main(argc=%d argv=%s argv[1]=%s envp=argv+argc+1:%s)", argc, argv == s_argv ? "same" : "other",
             argc > 1 ? argv[1] : "-", envp == argv + argc + 1 ? "yes" : "no");
    say(m);
    return 7;
}

static void probe_start(void) {
    const char *names[] = {"__libc_start_main@GLIBC_2.2.5", "__libc_start_main@GLIBC_2.34"};
    for (int which = 0; which < 2; which++) {
        static char *av[] = {"prog", "one", NULL, "E=1", NULL};
        s_argv = av;
        fflush(stdout);
        say(names[which]);
        say(" init,fini,rtld_fini given:");
        pid_t p = fork();
        if (p == 0) {
            int r = (which ? start_234 : start_225)(s_main, 2, av, s_init, s_fini, s_rtld_fini, &av);
            char m[48];
            snprintf(m, sizeof m, " returned %d", r);
            say(m);
            _exit(99);
        }
        int st = 0;
        waitpid(p, &st, 0);
        printf(" | status %s %d\n", WIFEXITED(st) ? "exit" : "signal", WIFEXITED(st) ? WEXITSTATUS(st) : WTERMSIG(st));
    }
}

/* ---- _pthread_cleanup_push_defer / _pthread_cleanup_pop_restore @GLIBC_2.2.5 ---- */
static int ran;
static void cu(void *a) { ran += (int)(long)a; }
static int curtype(void) {
    int t, u;
    pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, &t);
    pthread_setcanceltype(t, &u);
    return t;
}
static const char *tn(int t) { return t == PTHREAD_CANCEL_DEFERRED ? "DEFERRED" : t == PTHREAD_CANCEL_ASYNCHRONOUS ? "ASYNC" : "?"; }

static void *cu_thread(void *arg) {
    int start = (int)(long)arg;
    _Alignas(16) unsigned char b[128];
    for (int ex = 0; ex < 2; ex++) {
        int t;
        pthread_setcanceltype(start, &t);
        ran = 0;
        push_defer_225(b, cu, (void *)1L);
        int in = curtype();
        pop_restore_225(b, ex);
        printf("_pthread_cleanup_push_defer@GLIBC_2.2.5 from %s: type inside=%s\n", tn(start), tn(in));
        printf("_pthread_cleanup_pop_restore@GLIBC_2.2.5 from %s execute=%d: routine ran=%d type after=%s\n", tn(start), ex, ran, tn(curtype()));
        pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, &t);
    }
    return NULL;
}

static void *cu_cancel(void *arg) {
    (void)arg;
    _Alignas(16) unsigned char b[128];
    ran = 0;
    push_defer_225(b, cu, (void *)10L);
    pthread_cancel(pthread_self());
    pthread_testcancel();
    pop_restore_225(b, 0);
    return (void *)5L;
}

static void probe_cleanup(void) {
    pthread_t t;
    void *r;
    fflush(stdout);
    pthread_create(&t, NULL, cu_thread, (void *)(long)PTHREAD_CANCEL_DEFERRED);
    pthread_join(t, NULL);
    pthread_create(&t, NULL, cu_thread, (void *)(long)PTHREAD_CANCEL_ASYNCHRONOUS);
    pthread_join(t, NULL);
    pthread_create(&t, NULL, cu_cancel, NULL);
    pthread_join(t, &r);
    printf("_pthread_cleanup_push_defer@GLIBC_2.2.5 cancelled at testcancel: canceled=%s routine ran=%d\n", r == PTHREAD_CANCELED ? "yes" : "no", ran);
}

/* ---- pthread_atfork @GLIBC_2.2.5, each version in its own child ---- */
static void pa(void) { say("pA"); }
static void pb(void) { say("pB"); }
static void qa(void) { say("qA"); }
static void qb(void) { say("qB"); }
static void ca(void) { say("cA"); }
static void cb(void) { say("cB"); }

static void probe_atfork(void) {
    for (int which = 0; which < 2; which++) {
        fflush(stdout);
        say(which ? "pthread_atfork default:" : "pthread_atfork@GLIBC_2.2.5:");
        pid_t p = fork();
        if (p == 0) {
            int (*reg)(void (*)(void), void (*)(void), void (*)(void)) = which ? pthread_atfork : pthread_atfork_225;
            int r1 = reg(pa, qa, ca), r2 = reg(pb, qb, cb), r3 = reg(NULL, NULL, NULL);
            char m[64];
            snprintf(m, sizeof m, " rc=%d,%d,%d order:", r1, r2, r3);
            say(m);
            pid_t c = fork();
            if (c == 0) _exit(0);
            waitpid(c, NULL, 0);
            say(" (child then parent handlers above)");
            _exit(0);
        }
        waitpid(p, NULL, 0);
        say("\n");
    }
}

/* ---- __isnanf128 @GLIBC_2.26 ---- */
static void probe_isnanf128(void) {
    _Float128 v[] = {__builtin_nanf128(""), -__builtin_nanf128(""), __builtin_nansf128(""), __builtin_inff128(), -__builtin_inff128(), 0, 1, -1};
    const char *n[] = {"qNaN", "-qNaN", "sNaN", "inf", "-inf", "0", "1", "-1"};
    for (int i = 0; i < 8; i++)
        printf("__isnanf128@GLIBC_2.26 %s: %d | isnan: %d\n", n[i], isnanf128_226(v[i]), isnan(v[i]) ? 1 : 0);
}

/* ---- stime @GLIBC_2.2.5 (LG-1's started row): never sets the clock (skipped as root), NULL in a child ---- */
static void probe_stime(void) {
    if (geteuid() == 0) { printf("stime@GLIBC_2.2.5: skipped (root)\n"); return; }
    time_t now = time(NULL);
    errno = 0;
    int rc = stime_225(&now);
    printf("stime@GLIBC_2.2.5 now, unprivileged: rc=%d errno=%d\n", rc, rc ? errno : 0);
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        errno = 0;
        int r = stime_225(NULL);
        _exit(r == 0 ? 0 : errno & 0x7f);
    }
    int st = 0;
    waitpid(p, &st, 0);
    printf("stime@GLIBC_2.2.5 NULL, unprivileged (child exits with 0 or errno): child %s %d\n", WIFEXITED(st) ? "exit" : "signal",
           WIFEXITED(st) ? WEXITSTATUS(st) : WTERMSIG(st));
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    probe_glob();
    probe_timer();
    probe_lio();
    probe_start();
    probe_cleanup();
    probe_atfork();
    probe_isnanf128();
    probe_stime();
    return 0;
}
