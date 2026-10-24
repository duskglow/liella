/* Spec §22.3 and §11.3 (D-22.11, CANCEL-FAST): read, write, pread, pwrite and close stay
 * cancellation points on their fast path, and the fast path stays taken.
 * 1. A per-call INSTRUCTION CEILING in a single-threaded process: a traced child runs each call
 *    between two raise() markers and the parent single-steps it (PTRACE_SINGLESTEP; exact, no
 *    PMU needed): (steps with K calls - steps with 0 calls) / K. Each result is checked too.
 * 2. Results: byte counts, -1 with errno (EBADF) on a bad descriptor, errno untouched on success.
 * 3. A pending self-request (pthread_cancel(pthread_self())) acts at each call, in a
 *    single-threaded process (its main thread) and in a created thread; with cancellation
 *    disabled the call returns normally and the request stays pending.
 * 4. A request arriving while a thread is blocked in read acts (deferred and asynchronous type);
 *    close acts after closing the descriptor. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <sys/prctl.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Per-call ceilings (instructions, loop bookkeeping included), measured on keke (CANCEL-FAST):
 * read/write/pread/pwrite 58-63 on the fast path, read 79 through the former path
 * (cancel::point after the eight-argument window); close 72-75. CANCEL-FAST-2 (direct entries,
 * machine/points.S): read 36, write 39, pread 36, pwrite 39, close 32; the Rust path (liella_read_rest) 59. */
#define K 8

static ssize_t (*volatile f_read)(int, void *, size_t) = read;
static ssize_t (*volatile f_write)(int, const void *, size_t) = write;
static ssize_t (*volatile f_pread)(int, void *, size_t, off_t) = pread;
static ssize_t (*volatile f_pwrite)(int, const void *, size_t, off_t) = pwrite;
static int (*volatile f_close)(int) = close;

enum { C_READ, C_WRITE, C_PREAD, C_PWRITE, C_CLOSE, NCASES };
static const char *const names[NCASES] = {"read", "write", "pread", "pwrite", "close"};
static const long ceilings[NCASES] = {45, 45, 45, 45, 40};
static int zero_fd, null_fd;
static int spare[2 * K + 2];
static int nspare;

static int body(int c, int calls)
{
    char b[4];
    int bad = 0;
    for (int i = 0; i < calls; i++) {
        switch (c) {
        case C_READ:
            bad += f_read(zero_fd, b, 1) != 1;
            break;
        case C_WRITE:
            bad += f_write(null_fd, b, 1) != 1;
            break;
        case C_PREAD:
            bad += f_pread(zero_fd, b, 1, 0) != 1;
            break;
        case C_PWRITE:
            bad += f_pwrite(null_fd, b, 1, 0) != 1;
            break;
        default:
            bad += f_close(spare[--nspare]) != 0;
            break;
        }
    }
    return bad;
}

static int child(void)
{
    zero_fd = open("/dev/zero", O_RDONLY);
    null_fd = open("/dev/null", O_WRONLY);
    for (nspare = 0; nspare < (int)(sizeof spare / sizeof spare[0]); nspare++)
        if ((spare[nspare] = open("/dev/null", O_RDONLY)) < 0)
            return 99;
    if (zero_fd < 0 || null_fd < 0)
        return 99;
    if (ptrace(PTRACE_TRACEME, 0, NULL, NULL) != 0)
        return 100;
    raise(SIGSTOP);
    int bad = 0;
    for (int c = 0; c < NCASES; c++) {
        bad += body(c, 1);
        for (int calls = 0; calls <= K; calls += K) {
            raise(SIGUSR1);
            bad += body(c, calls);
            raise(SIGUSR2);
        }
    }
    return bad > 98 ? 98 : bad;
}

static long steps_to_end(pid_t pid)
{
    long n = 0;
    for (;;) {
        int status;
        if (ptrace(PTRACE_SINGLESTEP, pid, NULL, NULL) != 0 || waitpid(pid, &status, 0) != pid || !WIFSTOPPED(status))
            return -1;
        if (WSTOPSIG(status) == SIGUSR2)
            return n;
        if (WSTOPSIG(status) != SIGTRAP)
            return -1;
        n++;
    }
}

static int to_start(pid_t pid, int *code)
{
    for (;;) {
        int status;
        if (ptrace(PTRACE_CONT, pid, NULL, NULL) != 0 || waitpid(pid, &status, 0) != pid)
            return -1;
        if (WIFEXITED(status)) {
            *code = WEXITSTATUS(status);
            return 1;
        }
        if (!WIFSTOPPED(status))
            return -1;
        if (WSTOPSIG(status) == SIGUSR1)
            return 0;
    }
}

static int failures;

static void check(int ok, const char *what)
{
    if (!ok) {
        printf("FAIL: %s\n", what);
        failures++;
    }
}

static int ceiling(void)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid < 0)
        return printf("FAIL: fork\n"), 1;
    if (pid == 0)
        _exit(child());
    int status, code = -1;
    if (waitpid(pid, &status, 0) != pid || !WIFSTOPPED(status))
        return printf("FAIL: the child did not stop for tracing\n"), 1;
    for (int c = 0; c < NCASES; c++) {
        long s[2];
        for (int k = 0; k < 2; k++) {
            int r = to_start(pid, &code);
            s[k] = r == 0 ? steps_to_end(pid) : -1;
            if (s[k] < 0) {
                printf("FAIL: tracing %s (child exit %d)\n", names[c], code);
                if (r != 1)
                    kill(pid, SIGKILL);
                waitpid(pid, &status, 0);
                return 1;
            }
        }
        long per = (s[1] - s[0]) / K;
        int over = per > ceilings[c] || s[1] <= s[0];
        failures += over;
        printf("%s %-8s %4ld instructions/call%s\n", over ? "FAIL:" : "ok   ", names[c], per,
               over ? " (over the ceiling: the fast path was not taken)" : "");
    }
    if (to_start(pid, &code) != 1) {
        printf("FAIL: the child did not exit\n");
        kill(pid, SIGKILL);
        waitpid(pid, &status, 0);
        return 1;
    }
    check(code == 0, "the traced child's results (98: wrong results, 99: open, 100: PTRACE_TRACEME)");
    printf("CEILING_NOTE: ceilings 45 (read, write, pread, pwrite), 40 (close) instructions/call");
    printf("%c", 10);
    return 0;
}

static void results(void)
{
    char b[8];
    int zero = open("/dev/zero", O_RDONLY), null = open("/dev/null", O_WRONLY);
    check(zero >= 0 && null >= 0, "open /dev/zero, /dev/null");
    errno = 1234;
    check(f_read(zero, b, 5) == 5 && errno == 1234, "read returns the count, errno untouched");
    check(f_write(null, b, 7) == 7 && errno == 1234, "write returns the count, errno untouched");
    check(f_pread(zero, b, 3, 9) == 3 && errno == 1234, "pread returns the count, errno untouched");
    check(f_pwrite(null, b, 2, 9) == 2 && errno == 1234, "pwrite returns the count, errno untouched");
    check(f_read(zero, b, 0) == 0, "read of 0 bytes");
    errno = 0;
    check(f_read(-1, b, 1) == -1 && errno == EBADF, "read on a bad descriptor: -1, EBADF");
    errno = 0;
    check(f_write(-1, b, 1) == -1 && errno == EBADF, "write on a bad descriptor: -1, EBADF");
    errno = 0;
    check(f_pread(-1, b, 1, 0) == -1 && errno == EBADF, "pread on a bad descriptor: -1, EBADF");
    errno = 0;
    check(f_pwrite(-1, b, 1, 0) == -1 && errno == EBADF, "pwrite on a bad descriptor: -1, EBADF");
    errno = 1234;
    check(f_close(zero) == 0 && errno == 1234, "close returns 0, errno untouched");
    errno = 0;
    check(f_close(zero) == -1 && errno == EBADF, "close of a closed descriptor: -1, EBADF");
    close(null);
}

/* One call `c` with a pending self-request. Writes 'C' to `out` from the cleanup handler if the
 * call acted, 'R' if it returned. */
static int self_out;
static int self_fd;

static void note_cleanup(void *arg)
{
    (void)arg;
    char m = 'C';
    if (write(self_out, &m, 1) != 1)
        _exit(97);
}

static void *self_cancel(void *arg)
{
    int c = (int)(long)arg;
    char b[2];
    pthread_cleanup_push(note_cleanup, NULL);
    pthread_cancel(pthread_self());
    switch (c) {
    case C_READ:
        f_read(self_fd, b, 1);
        break;
    case C_WRITE:
        f_write(self_fd, b, 1);
        break;
    case C_PREAD:
        f_pread(self_fd, b, 1, 0);
        break;
    case C_PWRITE:
        f_pwrite(self_fd, b, 1, 0);
        break;
    default:
        f_close(self_fd);
        break;
    }
    pthread_cleanup_pop(0);
    char m = 'R';
    if (write(self_out, &m, 1) != 1)
        _exit(97);
    return NULL;
}

static int target_fd(int c)
{
    return c == C_READ || c == C_PREAD ? open("/dev/zero", O_RDONLY) : open("/dev/null", O_WRONLY);
}

/* Reads what the child wrote to the pipe (at most 3 bytes) after it ended. */
static void expect_note(int rd, const char *want, const char *what)
{
    char got[4] = {0};
    ssize_t n = read(rd, got, 3);
    if (n < 0)
        n = 0;
    got[n] = 0;
    if (strcmp(got, want) != 0) {
        printf("FAIL: %s: wrote \"%s\", want \"%s\"\n", what, got, want);
        failures++;
    }
}

/* In a forked, single-threaded child: the main thread itself (thread = 0) or a created thread. */
static void self_requests(int thread)
{
    for (int c = 0; c < NCASES; c++) {
        int p[2];
        if (pipe(p) != 0) {
            check(0, "pipe");
            return;
        }
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
            close(p[0]);
            self_out = p[1];
            self_fd = target_fd(c);
            if (self_fd < 0)
                _exit(96);
            if (!thread) {
                self_cancel((void *)(long)c);
                _exit(0);
            }
            pthread_t t;
            void *res = NULL;
            if (pthread_create(&t, NULL, self_cancel, (void *)(long)c) != 0 || pthread_join(t, &res) != 0)
                _exit(95);
            if (c == C_CLOSE && fcntl(self_fd, F_GETFD) != -1)
                _exit(94);
            _exit(res == PTHREAD_CANCELED ? 0 : 93);
        }
        close(p[1]);
        int status;
        waitpid(pid, &status, 0);
        char what[96];
        snprintf(what, sizeof what, "pending self-request at %s (%s)", names[c], thread ? "created thread" : "single-threaded main");
        expect_note(p[0], "C", what);
        check(WIFEXITED(status) && WEXITSTATUS(status) == 0, what);
        close(p[0]);
    }
}

/* Cancellation disabled: the call returns normally and the request stays pending. */
static void *disabled(void *arg)
{
    (void)arg;
    char b[2];
    int zero = open("/dev/zero", O_RDONLY);
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    pthread_cancel(pthread_self());
    int ok = f_read(zero, b, 1) == 1 && f_write(-1, b, 1) == -1 && f_close(zero) == 0;
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    if (!ok)
        return (void *)1;
    pthread_testcancel();
    return (void *)2;
}

static int blocked_fd;
static volatile int blocked_started;

static void *blocked(void *arg)
{
    if (arg)
        pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, NULL);
    char b[2];
    blocked_started = 1;
    f_read(blocked_fd, b, 1);
    return (void *)3;
}

static void blocked_request(int async)
{
    int p[2];
    if (pipe(p) != 0) {
        check(0, "pipe");
        return;
    }
    blocked_fd = p[0];
    blocked_started = 0;
    pthread_t t;
    void *res = NULL;
    check(pthread_create(&t, NULL, blocked, async ? (void *)1 : NULL) == 0, "pthread_create");
    while (!blocked_started)
        sched_yield();
    struct timespec ts = {0, 20 * 1000 * 1000};
    nanosleep(&ts, NULL);
    pthread_cancel(t);
    pthread_join(t, &res);
    check(res == PTHREAD_CANCELED, async ? "an asynchronous request acts in a blocked read" : "a deferred request acts in a blocked read");
    close(p[0]);
    close(p[1]);
}

/* 5. D-22.11 (CANCEL-FAST-2): a kernel's own ECANCELED with no request due is an ordinary error
 * result: -1 with errno ECANCELED, the thread runs on. A forked child installs a seccomp filter
 * (no privilege needed after PR_SET_NO_NEW_PRIVS) that fails read and pread64 on descriptor 963
 * with ECANCELED without running them, then calls both in its main thread and in a created one. */
#define ECANCEL_FD 963

static void *kernel_ecanceled(void *arg)
{
    (void)arg;
    char b[1];
    errno = 0;
    if (f_read(ECANCEL_FD, b, 1) != -1 || errno != ECANCELED)
        return (void *)1;
    errno = 0;
    if (f_pread(ECANCEL_FD, b, 1, 0) != -1 || errno != ECANCELED)
        return (void *)2;
    return (void *)0;
}

static void kernel_ecanceled_child_test(void)
{
    pid_t pid = fork();
    if (pid == 0) {
        struct sock_filter f[] = {
            BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
            BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, 0, 1, 0),  /* read: to the descriptor test */
            BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, 17, 0, 3), /* pread64, else allow */
            BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[0])),
            BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, ECANCEL_FD, 0, 1),
            BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | ECANCELED),
            BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        };
        struct sock_fprog prog = {(unsigned short)(sizeof f / sizeof f[0]), f};
        if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog, 0, 0) != 0)
            _exit(90);
        if (kernel_ecanceled(NULL) != NULL)
            _exit(91);
        pthread_t t;
        void *res = (void *)9;
        if (pthread_create(&t, NULL, kernel_ecanceled, NULL) != 0 || pthread_join(t, &res) != 0)
            _exit(92);
        _exit(res == NULL ? 77 : 93); /* 77: a cancelled main thread exits the process with 0 */
    }
    int st = 0;
    check(pid > 0 && waitpid(pid, &st, 0) == pid, "fork/waitpid (kernel ECANCELED)");
    if (WIFEXITED(st) && WEXITSTATUS(st) == 90)
        printf("NOTE: seccomp filter not installable here\n");
    check(WIFEXITED(st) && WEXITSTATUS(st) == 77, "a kernel ECANCELED with no request due: -1, errno ECANCELED, no cancellation (main thread and created thread)");
}

/* 6. D-22.11 (FUTEX-DUE): the same rule for the futex waits of pthread_cond_wait, sem_wait and
 * pthread_join. The waiting thread installs a seccomp filter (its own thread only) that fails its
 * FUTEX_WAIT and FUTEX_WAIT_BITSET calls with ECANCELED; with no request due each wait goes on
 * (spurious wakes at most) until the forked child's main thread releases it. */
static pthread_mutex_t fd_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t fd_cv = PTHREAD_COND_INITIALIZER;
static int fd_ready;
static sem_t fd_sem;
static int fd_pipe[2];
static pthread_t fd_target;

static int futex_ecanceled_filter(void)
{
    struct sock_filter f[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, 202, 0, 4), /* futex, else allow */
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[1])),
        BPF_STMT(BPF_ALU | BPF_AND | BPF_K, 0x7f),     /* the command without PRIVATE/CLOCK_REALTIME */
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, 0, 2, 0),   /* FUTEX_WAIT */
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, 9, 1, 0),   /* FUTEX_WAIT_BITSET */
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | ECANCELED),
    };
    struct sock_fprog prog = {(unsigned short)(sizeof f / sizeof f[0]), f};
    return prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 && prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog, 0, 0) == 0;
}

static void *fd_waiter(void *arg)
{
    long which = (long)arg;
    if (!futex_ecanceled_filter())
        return (void *)90;
    if (which == 0) {
        pthread_mutex_lock(&fd_m);
        while (!fd_ready)
            pthread_cond_wait(&fd_cv, &fd_m);
        pthread_mutex_unlock(&fd_m);
    } else if (which == 1) {
        while (sem_wait(&fd_sem) != 0) {
        }
    } else {
        void *res = (void *)9;
        if (pthread_join(fd_target, &res) != 0 || res != NULL)
            return (void *)91;
    }
    return NULL;
}

static void *fd_exits_on_byte(void *arg)
{
    (void)arg;
    char b;
    while (read(fd_pipe[0], &b, 1) != 1) {
    }
    return NULL;
}

/* One wait (0 cond_wait, 1 sem_wait, 2 join) in a forked child: 77 = returned, 94 = cancelled. */
static int futex_ecanceled_case(int which)
{
    pid_t pid = fork();
    if (pid == 0) {
        alarm(20);
        pthread_t w;
        void *res = (void *)9;
        if (sem_init(&fd_sem, 0, 0) != 0 || pipe(fd_pipe) != 0)
            _exit(92);
        if (which == 2 && pthread_create(&fd_target, NULL, fd_exits_on_byte, NULL) != 0)
            _exit(92);
        if (pthread_create(&w, NULL, fd_waiter, (void *)(long)which) != 0)
            _exit(92);
        usleep(50000); /* the waiter is in its wait (or spinning through spurious wakes) */
        if (which == 0) {
            pthread_mutex_lock(&fd_m);
            fd_ready = 1;
            pthread_cond_signal(&fd_cv);
            pthread_mutex_unlock(&fd_m);
        } else if (which == 1) {
            sem_post(&fd_sem);
        } else if (write(fd_pipe[1], "x", 1) != 1) {
            _exit(92);
        }
        if (pthread_join(w, &res) != 0)
            _exit(93);
        _exit(res == NULL ? 77 : res == PTHREAD_CANCELED ? 94 : (int)(long)res);
    }
    int st = 0;
    if (pid <= 0 || waitpid(pid, &st, 0) != pid)
        return -1;
    return WIFEXITED(st) ? WEXITSTATUS(st) : 128 + WTERMSIG(st);
}

static void futex_ecanceled_child_test(void)
{
    static const char *const what[3] = {
        "FUTEX-DUE pthread_cond_wait: a kernel ECANCELED from its futex wait with no request due does not cancel",
        "FUTEX-DUE sem_wait: a kernel ECANCELED from its futex wait with no request due does not cancel",
        "FUTEX-DUE pthread_join: a kernel ECANCELED from its futex wait with no request due does not cancel",
    };
    for (int i = 0; i < 3; i++) {
        int r = futex_ecanceled_case(i);
        if (r != 77)
            printf("FUTEX-DUE case %d: child status %d (94: cancelled, 142: hung)\n", i, r);
        check(r == 77, what[i]);
    }
}

int main(void)
{
    kernel_ecanceled_child_test();
    futex_ecanceled_child_test();
    if (ceiling())
        return 1;
    results();
    self_requests(0);
    self_requests(1);
    pthread_t t;
    void *res = NULL;
    check(pthread_create(&t, NULL, disabled, NULL) == 0 && pthread_join(t, &res) == 0, "pthread_create (disabled)");
    check(res == PTHREAD_CANCELED, "disabled: the calls return normally, the request stays pending for testcancel");
    blocked_request(0);
    blocked_request(1);
    if (failures)
        printf("%d failures\n", failures);
    else
        printf("PASS\n");
    return failures != 0;
}
