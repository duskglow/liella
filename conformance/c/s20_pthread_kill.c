/* Spec §20.4 (clause s20_4_c_5, with s20_4_f_0): pthread_kill and pthread_sigqueue target one
 * thread of this process. The handler runs on the target thread; signal 0 only checks; an
 * invalid signal is EINVAL returned (errno untouched); pthread_sigqueue's value and code arrive
 * with the signal; a thread that has exited but not been joined still accepts pthread_kill (0,
 * nothing delivered), while pthread_sigqueue reports ESRCH for it. The same program passes
 * against the reference library (checked 2026-10-04, 20 runs). Each failing CHECK exits with its
 * line number. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

static volatile sig_atomic_t hits;
/* volatile: the handler's stores stay in program order before hits++ (x86 keeps stores ordered). */
static volatile pthread_t seen;
static volatile int seen_value, seen_code;
static int fds[2];
static volatile int finished;

static void on_usr1(int sig)
{
    (void)sig;
    seen = pthread_self();
    hits++;
}

static void on_rt(int sig, siginfo_t *info, void *ctx)
{
    (void)sig;
    (void)ctx;
    seen = pthread_self();
    seen_value = info->si_value.sival_int;
    seen_code = info->si_code;
    hits++;
}

/* Waits on the pipe until main writes; the signals interrupt the read (no SA_RESTART). */
static void *waiter(void *arg)
{
    (void)arg;
    char c;
    while (read(fds[0], &c, 1) != 1) {
    }
    return NULL;
}

static void *quick(void *arg)
{
    (void)arg;
    finished = 1;
    return NULL;
}

static void nap(long ms)
{
    struct timespec t = {0, ms * 1000000L};
    nanosleep(&t, NULL);
}

static int wait_hits(int n)
{
    for (int i = 0; i < 2000 && hits < n; i++)
        nap(1);
    return hits >= n;
}

int main(void)
{
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_usr1;
    CHECK(sigaction(SIGUSR1, &sa, NULL) == 0);
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_rt;
    sa.sa_flags = SA_SIGINFO;
    CHECK(sigaction(SIGRTMIN, &sa, NULL) == 0);
    CHECK(pipe(fds) == 0);

    pthread_t t;
    CHECK(pthread_create(&t, NULL, waiter, NULL) == 0);
    nap(20);
    /* The target thread runs the handler, not the caller. */
    CHECK(pthread_kill(t, SIGUSR1) == 0);
    CHECK(wait_hits(1));
    CHECK(pthread_equal(seen, t) && !pthread_equal(seen, pthread_self()));
    /* Signal 0: existence only. */
    CHECK(pthread_kill(t, 0) == 0 && hits == 1);
    /* An invalid signal: the error number returned, errno untouched. */
    errno = 1234;
    CHECK(pthread_kill(t, 99) == EINVAL && errno == 1234);
    CHECK(pthread_kill(t, -1) == EINVAL && errno == 1234);
    union sigval v = {.sival_int = 77};
    CHECK(pthread_sigqueue(t, 99, v) == EINVAL && errno == 1234);
    /* pthread_sigqueue: the value and SI_QUEUE reach the target thread's handler. */
    CHECK(pthread_sigqueue(t, SIGRTMIN, v) == 0);
    CHECK(wait_hits(2));
    CHECK(pthread_equal(seen, t) && seen_value == 77 && seen_code == SI_QUEUE);
    /* The calling thread itself: delivered before pthread_kill returns (unblocked). */
    CHECK(pthread_kill(pthread_self(), SIGUSR1) == 0);
    CHECK(hits == 3);
    CHECK(pthread_equal(seen, pthread_self()));
    CHECK(write(fds[1], "x", 1) == 1);
    CHECK(pthread_join(t, NULL) == 0);

    /* Exited but not joined: pthread_kill gives 0 (2.34+, s20_4_f_0), pthread_sigqueue ESRCH
     * (pthread_sigqueue(3): not a valid thread); nothing is delivered. */
    pthread_t q;
    CHECK(pthread_create(&q, NULL, quick, NULL) == 0);
    while (!finished)
        nap(1);
    nap(50);
    int before = hits;
    CHECK(pthread_kill(q, 0) == 0);
    CHECK(pthread_kill(q, SIGUSR1) == 0);
    v.sival_int = 5;
    CHECK(pthread_sigqueue(q, SIGRTMIN, v) == ESRCH); /* the 2.34 rule is pthread_kill's alone (as the reference) */
    nap(50);
    CHECK(hits == before);
    CHECK(pthread_join(q, NULL) == 0);
    return 0;
}
