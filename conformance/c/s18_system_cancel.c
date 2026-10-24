/* Spec §18.7 (s18_7_c_1, s18_7_t_0; D-18.5) in a process Liella started: system cancelled at
 * every stage. Each round starts a thread in system("exec sleep 30") and cancels it after a delay
 * swept from 0 to about 3 ms (thread start, the disposition change, the spawn, the wait), then a
 * few rounds wait long enough for the shell to run. After each round: the thread exited
 * cancelled; SIGINT has the handler and SIGQUIT the default action this program set up (never
 * the inherited ones: a job started in the background of a non-interactive shell inherits both
 * ignored); the thread's mask when its cleanup handler runs is the one it started with (SIGUSR1
 * blocked, SIGCHLD not); no child is left. Then two concurrent callers: cancelling one leaves
 * SIGINT and SIGQUIT ignored while the other is inside, cancelling the other restores them.
 * Failures are printed and counted; the exit status is the count (at most 100). */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int fails;
#define CHECK(id, c, what)                                                     \
    do {                                                                       \
        if (!(c)) {                                                            \
            fprintf(stderr, "FAIL %s line %d: %s\n", id, __LINE__, what);      \
            fails++;                                                           \
        }                                                                      \
    } while (0)

static void sleep_us(long us)
{
    struct timespec ts = {us / 1000000, (us % 1000000) * 1000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static void on_int(int sig)
{
    (void)sig;
}

/* The cancelled thread's mask as its cleanup handler saw it: 1 = SIGUSR1 blocked and SIGCHLD
 * not, 2 = anything else, 0 = the handler did not run. */
static volatile int mask_seen;
static void record_mask(void *arg)
{
    (void)arg;
    sigset_t m;
    pthread_sigmask(SIG_SETMASK, NULL, &m);
    mask_seen = sigismember(&m, SIGUSR1) == 1 && sigismember(&m, SIGCHLD) == 0 ? 1 : 2;
}

static void *system_thread(void *arg)
{
    void *r = (void *)1;
    pthread_cleanup_push(record_mask, NULL);
    system((const char *)arg);
    pthread_cleanup_pop(0);
    return r;
}

static void dispositions(void (**i)(int), void (**q)(int))
{
    struct sigaction sa;
    sigaction(SIGINT, NULL, &sa);
    *i = sa.sa_handler;
    sigaction(SIGQUIT, NULL, &sa);
    *q = sa.sa_handler;
}

static void no_child_left(int round)
{
    errno = 0;
    int r = waitpid(-1, NULL, WNOHANG);
    if (!(r == -1 && errno == ECHILD)) {
        fprintf(stderr, "round %d: waitpid -> %d errno %d\n", round, r, errno);
        CHECK("s18_7_c_1", 0, "the cancelled system's shell was reaped");
        while (waitpid(-1, NULL, WNOHANG) > 0) {
        }
    }
}

static void sweep(void)
{
    const int rounds = 220;
    for (int k = 0; k < rounds && fails < 20; k++) {
        long delay = k < 200 ? (k % 50) * 60L : 50000L;
        pthread_t t;
        void *r = NULL;
        mask_seen = 0;
        if (pthread_create(&t, NULL, system_thread, "exec sleep 30") != 0) {
            CHECK("s18_7_c_1", 0, "pthread_create");
            return;
        }
        sleep_us(delay);
        pthread_cancel(t);
        pthread_join(t, &r);
        void (*i)(int), (*q)(int);
        dispositions(&i, &q);
        if (r != PTHREAD_CANCELED || i != on_int || q != SIG_DFL || mask_seen != 1)
            fprintf(stderr, "round %d (delay %ld us): result %p SIGINT %p SIGQUIT %p mask %d\n", k, delay, r,
                    (void *)i, (void *)q, mask_seen);
        CHECK("s18_7_c_1", r == PTHREAD_CANCELED, "system is a cancellation point");
        CHECK("s18_7_c_1", i == on_int, "SIGINT's handler restored after the cancelled system");
        CHECK("s18_7_c_1", q == SIG_DFL, "SIGQUIT's default action restored after the cancelled system");
        CHECK("s18_7_c_1", mask_seen == 1, "the cancelled thread's mask restored (SIGCHLD unblocked)");
        no_child_left(k);
    }
}

/* Two threads inside system at once: the shared change stays until the last one leaves. */
static void two_callers(void)
{
    int p[2];
    if (pipe(p) != 0) {
        CHECK("s18_7_t_0", 0, "pipe");
        return;
    }
    char cmd[64];
    snprintf(cmd, sizeof cmd, "printf x >&%d; exec sleep 30", p[1]);
    pthread_t a, b;
    void *ra = NULL, *rb = NULL;
    pthread_create(&a, NULL, system_thread, cmd);
    pthread_create(&b, NULL, system_thread, cmd);
    /* Both shells have run: both threads are inside system. */
    char buf[2];
    size_t got = 0;
    while (got < 2) {
        ssize_t n = read(p[0], buf + got, 2 - got);
        if (n <= 0 && errno != EINTR)
            break;
        if (n > 0)
            got += (size_t)n;
    }
    CHECK("s18_7_t_0", got == 2, "both shells started");
    void (*i)(int), (*q)(int);
    pthread_cancel(a);
    pthread_join(a, &ra);
    dispositions(&i, &q);
    CHECK("s18_7_t_0", ra == PTHREAD_CANCELED, "the first caller was cancelled");
    CHECK("s18_7_t_0", i == SIG_IGN && q == SIG_IGN, "SIGINT and SIGQUIT stay ignored while another caller is inside");
    pthread_cancel(b);
    pthread_join(b, &rb);
    dispositions(&i, &q);
    CHECK("s18_7_t_0", rb == PTHREAD_CANCELED, "the second caller was cancelled");
    CHECK("s18_7_t_0", i == on_int && q == SIG_DFL, "the last caller out restores SIGINT and SIGQUIT");
    no_child_left(-1);
    close(p[0]);
    close(p[1]);
}

int main(void)
{
    /* Known starting dispositions and mask (whatever this process inherited). */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_int;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGINT, &sa, NULL);
    sa.sa_handler = SIG_DFL;
    sigaction(SIGQUIT, &sa, NULL);
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR1);
    pthread_sigmask(SIG_SETMASK, &m, NULL);
    sweep();
    two_callers();
    return fails > 100 ? 100 : fails;
}
