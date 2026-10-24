/* The internal signal (OD-8, D-20.1): signal 32 is the library's, and an application can
 * neither block, ignore nor replace it by any route, nor see it as one of its real-time signals.
 * Applications' real-time signals are SIGRTMIN = 33 .. SIGRTMAX = 64.
 *
 * Every route an application has to the mask, the dispositions and the sets is tried with sets
 * that include 32 (sigfillset, and sets with every bit set by memset, which bypass the set
 * functions). After each, the kernel's own view is read with raw system calls (rt_sigprocmask,
 * rt_sigaction, /proc/self/task/<tid>/status, /proc/self/fdinfo): signal 32 must never be
 * blocked, its action never changed, and no set the library reports back may contain it.
 * Each check prints a line; a failure prints FAIL and the exit status counts the failures.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/signalfd.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <ucontext.h>
#include <unistd.h>

extern char **environ;

static int failures;

static void check(int ok, const char *what)
{
    printf("%s %s\n", ok ? "ok  " : "FAIL", what);
    if (!ok)
        failures++;
}

/* The kernel's 64-bit mask of the calling thread. */
static unsigned long kmask(void)
{
    unsigned long m = 0;
    syscall(SYS_rt_sigprocmask, SIG_BLOCK, NULL, &m, 8);
    return m;
}

#define BIT32 (1UL << 31)

struct kact {
    void *handler;
    unsigned long flags;
    void *restorer;
    unsigned long mask;
};

static struct kact kaction(int sig)
{
    struct kact k;
    memset(&k, 0, sizeof k);
    syscall(SYS_rt_sigaction, sig, NULL, &k, 8);
    return k;
}

/* A set with every bit set, as a program might build it by hand. */
static void allbits(sigset_t *s)
{
    memset(s, 0xff, sizeof *s);
}

/* The SigBlk of thread `tid`, from /proc. */
static unsigned long sigblk(pid_t tid)
{
    char path[64], line[256];
    snprintf(path, sizeof path, "/proc/self/task/%d/status", (int)tid);
    FILE *f = fopen(path, "r");
    unsigned long v = ~0UL;
    if (!f)
        return v;
    while (fgets(line, sizeof line, f))
        if (strncmp(line, "SigBlk:", 7) == 0)
            v = strtoul(line + 7, NULL, 16);
    fclose(f);
    return v;
}

static unsigned long inside_mask;

static void inside(void)
{
    inside_mask = kmask();
}

static void handler(int sig)
{
    (void)sig;
}

static volatile pid_t waiter_tid;
static volatile int waiter_stage;
static volatile int waiter_result;

static void *suspender(void *arg)
{
    (void)arg;
    waiter_tid = (pid_t)syscall(SYS_gettid);
    sigset_t s;
    allbits(&s);
    sigdelset(&s, SIGUSR1);
    waiter_stage = 1;
    sigsuspend(&s);
    waiter_stage = 2;
    return NULL;
}

static void *timed_waiter(void *arg)
{
    (void)arg;
    waiter_tid = (pid_t)syscall(SYS_gettid);
    sigset_t s;
    allbits(&s);
    waiter_stage = 1;
    int r;
    do
        r = sigwaitinfo(&s, NULL);
    while (r == -1 && errno == EINTR);
    waiter_result = r;
    waiter_stage = 2;
    return NULL;
}

static void wait_stage(int n)
{
    while (waiter_stage < n)
        usleep(1000);
    usleep(20000);
}

int main(int argc, char **argv)
{
    (void)argc;
    (void)argv;
    struct kact orig = kaction(32);
    check(orig.handler != SIG_DFL && orig.handler != SIG_IGN, "the library has a handler for 32 at start");
    check(!(kmask() & BIT32), "32 is not blocked at start");

    /* Never an application signal. */
    check(SIGRTMIN == 33 && __libc_current_sigrtmin() == 33, "SIGRTMIN is 33");
    check(SIGRTMAX == 64 && __libc_current_sigrtmax() == 64, "SIGRTMAX is 64");
    check(sigabbrev_np(32) == NULL && sigdescr_np(32) == NULL, "32 has no name or description");
    const char *t = strsignal(33);
    check(t && strcmp(t, "Real-time signal 0") == 0, "strsignal(33) is real-time signal 0");
    t = strsignal(32);
    check(t && strstr(t, "Real-time") == NULL, "strsignal(32) is not a real-time signal's text");
    sigset_t s, old;
    sigemptyset(&s);
    errno = 0;
    check(sigaddset(&s, 32) == -1 && errno == EINVAL, "sigaddset(32) is EINVAL");
    sigfillset(&s);
    check(sigismember(&s, 32) != 1, "sigfillset does not contain 32");

    /* Dispositions. */
    struct sigaction a, oa;
    memset(&a, 0, sizeof a);
    a.sa_handler = SIG_IGN;
    errno = 0;
    check(sigaction(32, &a, &oa) == -1 && errno == EINVAL, "sigaction(32, SIG_IGN) is EINVAL");
    a.sa_handler = handler;
    errno = 0;
    check(sigaction(32, &a, NULL) == -1 && errno == EINVAL, "sigaction(32, handler) is EINVAL");
    errno = 0;
    check(sigaction(32, NULL, &oa) == -1 && errno == EINVAL, "sigaction(32) query is EINVAL");
    errno = 0;
    check(signal(32, SIG_IGN) == SIG_ERR && errno == EINVAL, "signal(32, SIG_IGN) is SIG_ERR, EINVAL");
    errno = 0;
    check(sigset(32, SIG_IGN) == SIG_ERR && errno == EINVAL, "sigset(32, SIG_IGN) is SIG_ERR, EINVAL");
    errno = 0;
    check(sigset(32, SIG_HOLD) == SIG_ERR && errno == EINVAL, "sigset(32, SIG_HOLD) is SIG_ERR, EINVAL");
    errno = 0;
    check(sigignore(32) == -1 && errno == EINVAL, "sigignore(32) is EINVAL");
    errno = 0;
    check(sighold(32) == -1 && errno == EINVAL, "sighold(32) is EINVAL");
    errno = 0;
    check(sigrelse(32) == -1 && errno == EINVAL, "sigrelse(32) is EINVAL");
    errno = 0;
    check(siginterrupt(32, 1) == -1 && errno == EINVAL, "siginterrupt(32) is EINVAL");
    struct kact now = kaction(32);
    check(now.handler == orig.handler && now.flags == orig.flags && now.mask == orig.mask, "the kernel's action for 32 is unchanged");
    /* A handler's mask cannot block it either. */
    allbits(&a.sa_mask);
    a.sa_handler = handler;
    a.sa_flags = 0;
    sigaction(SIGUSR1, &a, NULL);
    check(!(kaction(SIGUSR1).mask & BIT32), "sigaction(SIGUSR1) with an all-bits sa_mask: 32 not in the kernel's sa_mask");
    sigaction(SIGUSR1, NULL, &oa);
    check(sigismember(&oa.sa_mask, 32) != 1, "the sa_mask reported back does not contain 32");

    /* The mask. */
    allbits(&s);
    sigprocmask(SIG_BLOCK, &s, &old);
    check(!(kmask() & BIT32), "sigprocmask(SIG_BLOCK, all bits): 32 not blocked");
    sigprocmask(SIG_SETMASK, &s, NULL);
    check(!(kmask() & BIT32), "sigprocmask(SIG_SETMASK, all bits): 32 not blocked");
    sigprocmask(SIG_BLOCK, NULL, &old);
    check(sigismember(&old, 32) != 1, "the mask reported back does not contain 32");
    sigemptyset(&s);
    sigprocmask(SIG_SETMASK, &s, NULL);
    allbits(&s);
    pthread_sigmask(SIG_SETMASK, &s, &old);
    check(!(kmask() & BIT32), "pthread_sigmask(SIG_SETMASK, all bits): 32 not blocked");
    sigfillset(&s);
    pthread_sigmask(SIG_BLOCK, &s, NULL);
    check(!(kmask() & BIT32), "pthread_sigmask(SIG_BLOCK, sigfillset): 32 not blocked");
    sigemptyset(&s);
    pthread_sigmask(SIG_SETMASK, &s, NULL);
    sigblock(~0);
    check(!(kmask() & BIT32), "sigblock(~0): 32 not blocked");
    sigsetmask(~0);
    check(!(kmask() & BIT32), "sigsetmask(~0): 32 not blocked");
    sigsetmask(0);

    /* User contexts carry a mask. */
    static ucontext_t uc;
    static volatile int stage;
    stage = 0;
    getcontext(&uc);
    if (stage == 0) {
        stage = 1;
        allbits(&uc.uc_sigmask);
        setcontext(&uc);
    }
    check(!(kmask() & BIT32), "setcontext with an all-bits uc_sigmask: 32 not blocked");
    check(sigismember(&uc.uc_sigmask, 32) != 1 || stage == 1, "getcontext's mask does not report 32");
    sigemptyset(&s);
    sigprocmask(SIG_SETMASK, &s, NULL);
    static ucontext_t from, to;
    static char stack[65536];
    getcontext(&to);
    to.uc_stack.ss_sp = stack;
    to.uc_stack.ss_size = sizeof stack;
    to.uc_link = &from;
    allbits(&to.uc_sigmask);
    makecontext(&to, inside, 0);
    inside_mask = BIT32;
    swapcontext(&from, &to);
    check(!(inside_mask & BIT32) && (inside_mask & 1), "swapcontext into an all-bits context: 32 not blocked there, the rest blocked");
    check(!(kmask() & BIT32), "and back: 32 not blocked");
    sigemptyset(&s);
    sigprocmask(SIG_SETMASK, &s, NULL);

    /* signalfd: the descriptor's set (fdinfo "sigmask:") never holds 32. */
    allbits(&s);
    int sfd = signalfd(-1, &s, SFD_CLOEXEC);
    char path[64], line[256];
    snprintf(path, sizeof path, "/proc/self/fdinfo/%d", sfd);
    FILE *f = fopen(path, "r");
    unsigned long sm = ~0UL;
    while (f && fgets(line, sizeof line, f))
        if (strncmp(line, "sigmask:", 8) == 0)
            sm = strtoul(line + 8, NULL, 16);
    if (f)
        fclose(f);
    check(sfd >= 0 && !(sm & BIT32), "signalfd(all bits): 32 not in the descriptor's set");
    close(sfd);
    check(!(kmask() & BIT32), "signalfd leaves the mask alone");

    /* Waits: sigsuspend's temporary mask, sigwaitinfo's set. */
    a.sa_handler = handler;
    sigemptyset(&a.sa_mask);
    a.sa_flags = 0;
    sigaction(SIGUSR1, &a, NULL);
    pthread_t th;
    waiter_stage = 0;
    pthread_create(&th, NULL, suspender, NULL);
    wait_stage(1);
    unsigned long blk = sigblk(waiter_tid);
    check(blk != ~0UL && !(blk & BIT32), "sigsuspend(all bits but SIGUSR1): 32 not blocked while waiting");
    pthread_kill(th, SIGUSR1);
    pthread_join(th, NULL);
    sigemptyset(&s);
    sigaddset(&s, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &s, NULL);
    waiter_stage = 0;
    pthread_create(&th, NULL, timed_waiter, NULL);
    wait_stage(1);
    blk = sigblk(waiter_tid);
    check(blk != ~0UL && !(blk & BIT32), "sigwaitinfo(all bits): 32 not blocked while waiting");
    syscall(SYS_tgkill, getpid(), waiter_tid, 32);
    usleep(20000);
    pthread_kill(th, SIGUSR2);
    pthread_join(th, NULL);
    check(waiter_result == SIGUSR2, "sigwaitinfo(all bits) accepts SIGUSR2, never 32");
    /* 32 made pending behind the library's back is never reported. */
    unsigned long b = BIT32;
    syscall(SYS_rt_sigprocmask, SIG_BLOCK, &b, NULL, 8);
    syscall(SYS_tgkill, getpid(), (pid_t)syscall(SYS_gettid), 32);
    sigset_t pend;
    sigpending(&pend);
    check(sigismember(&pend, 32) != 1, "sigpending never reports 32");
    sigprocmask(SIG_BLOCK, NULL, &old);
    check(sigismember(&old, 32) != 1, "a mask the kernel has 32 in is reported without it");
    allbits(&s);
    struct timespec zero = {0, 0};
    errno = 0;
    int r = sigtimedwait(&s, NULL, &zero);
    check(r != 32, "sigtimedwait(all bits) does not accept a pending 32");
    syscall(SYS_rt_sigprocmask, SIG_UNBLOCK, &b, NULL, 8);
    check(!(kmask() & BIT32), "unblocked again (the pending 32 went to the library's handler)");

    /* Sending 32. */
    errno = 0;
    check(raise(32) != 0 && errno == EINVAL, "raise(32) is EINVAL");
    union sigval v = {0};
    check(pthread_kill(pthread_self(), 32) == EINVAL, "pthread_kill(32) is EINVAL");
    check(pthread_sigqueue(pthread_self(), 32, v) == EINVAL, "pthread_sigqueue(32) is EINVAL");
    /* kill, tgkill and sigqueue are the kernel's (as glibc's): 32 sent to this process reaches
     * the library's handler, which ignores a signal that carries no request. */
    check(kill(getpid(), 32) == 0, "kill(getpid(), 32) is delivered to the library");
    check(tgkill(getpid(), gettid(), 32) == 0, "tgkill(32) is delivered to the library");
    check(sigqueue(getpid(), 32, v) == 0, "sigqueue(32) is delivered to the library");
    usleep(20000);
    now = kaction(32);
    check(now.handler == orig.handler && !(kmask() & BIT32), "and the process goes on, its action and mask for 32 unchanged");

    /* posix_spawn attributes: the child never starts with 32 blocked. */
    posix_spawnattr_t at;
    posix_spawnattr_init(&at);
    allbits(&s);
    posix_spawnattr_setsigmask(&at, &s);
    posix_spawnattr_setsigdefault(&at, &s);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF);
    posix_spawnattr_getsigmask(&at, &old);
    check(sigismember(&old, 32) != 1, "posix_spawnattr_getsigmask does not report 32");
    posix_spawnattr_getsigdefault(&at, &old);
    check(sigismember(&old, 32) != 1, "posix_spawnattr_getsigdefault does not report 32");
    int p[2];
    pipe(p);
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, p[1], 1);
    char *args[] = {"grep", "SigBlk", "/proc/self/status", NULL};
    pid_t c;
    int sr = posix_spawn(&c, "/bin/grep", &fa, &at, args, environ);
    close(p[1]);
    char out[128] = {0};
    ssize_t n = read(p[0], out, sizeof out - 1);
    close(p[0]);
    waitpid(c, NULL, 0);
    unsigned long cb = n > 7 ? strtoul(out + 7, NULL, 16) : ~0UL;
    printf("     the child's SigBlk: %016lx\n", cb);
    check(sr == 0 && cb != ~0UL && !(cb & BIT32), "posix_spawn with an all-bits SETSIGMASK: the child has 32 unblocked");
    check(cb == 0xffffffff7ffbfeffUL, "the child has every other signal blocked (but SIGKILL and SIGSTOP)");

    printf("%d failures\n", failures);
    return failures != 0;
}
