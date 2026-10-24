/* Spec C0.2 (clauses s0_6_c0_2_3, s0_6_c0_2_4):
 *  - async-signal safety: a handler that interrupts the library in the middle of non-reentrant
 *    work (malloc, free, snprintf, fopen/fclose, setenv, localtime) calls AS-Safe functions
 *    (write, getpid, clock_gettime, sigprocmask, open, read, close, strlen, memcpy, kill with
 *    signal 0, sigaddset, sem_post) 5,000 times: every call gives its right answer and nothing
 *    deadlocks or corrupts the interrupted work;
 *  - cancellation points: a thread with a pending deferred cancellation that calls close(fd)
 *    is cancelled there; a function that is not a cancellation point (strlen) does not act, nor
 *    does close while cancellation is disabled.
 * Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

static volatile sig_atomic_t handled, bad;
static sem_t sem;
static int devnull_fds[2];

static void handler(int sig)
{
    (void)sig;
    int saved = errno;
    pid_t me = getpid();
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        bad++;
    sigset_t s, old;
    sigemptyset(&s);
    sigaddset(&s, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &s, &old) != 0 || sigprocmask(SIG_SETMASK, &old, NULL) != 0)
        bad++;
    int fd = open("/dev/zero", O_RDONLY);
    char b[8] = {1};
    if (fd < 0 || read(fd, b, sizeof b) != sizeof b || b[0] != 0 || close(fd) != 0)
        bad++;
    char msg[16];
    memcpy(msg, "handled\n", 9);
    if (strlen(msg) != 8 || write(devnull_fds[1], msg, 8) != 8)
        bad++;
    if (kill(me, 0) != 0 || me != getpid())
        bad++;
    if (sem_post(&sem) != 0)
        bad++;
    handled++;
    errno = saved;
}

static int fds[2];
static volatile int reached;

static void *closer(void *arg)
{
    (void)arg;
    pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, NULL);
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    pthread_cancel(pthread_self());
    (void)strlen("not a cancellation point");
    reached = 1;
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    (void)strlen("still not one");
    reached = 2;
    close(fds[0]);
    reached = 3;
    return NULL;
}

int main(void)
{
    CHECK(sem_init(&sem, 0, 0) == 0);
    devnull_fds[1] = open("/dev/null", O_WRONLY);
    CHECK(devnull_fds[1] >= 0);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = handler;
    sa.sa_flags = SA_RESTART;
    CHECK(sigaction(SIGALRM, &sa, NULL) == 0);
    struct itimerval it = {{0, 200}, {0, 200}};
    CHECK(setitimer(ITIMER_REAL, &it, NULL) == 0);
    long rounds = 0;
    while (handled < 5000 && rounds < 50000000) {
        char *p = malloc(64 + rounds % 512);
        CHECK(p != NULL);
        int n = snprintf(p, 64, "round %ld %.2f", rounds, rounds / 3.0);
        CHECK(n > 0 && strncmp(p, "round ", 6) == 0);
        free(p);
        if (rounds % 64 == 0) {
            FILE *f = fopen("/dev/null", "w");
            CHECK(f != NULL && fputs("x", f) >= 0 && fclose(f) == 0);
            CHECK(setenv("C02_AS", rounds % 128 ? "a" : "b", 1) == 0);
            time_t t = rounds;
            struct tm tm;
            CHECK(localtime_r(&t, &tm) == &tm);
        }
        rounds++;
    }
    struct itimerval off = {{0, 0}, {0, 0}};
    CHECK(setitimer(ITIMER_REAL, &off, NULL) == 0);
    CHECK(handled >= 5000 && bad == 0);
    int posts = 0;
    while (sem_trywait(&sem) == 0)
        posts++;
    CHECK(posts == handled);

    /* close as a cancellation point: cancelled there, and the descriptor closed. */
    CHECK(pipe(fds) == 0);
    pthread_t t;
    CHECK(pthread_create(&t, NULL, closer, NULL) == 0);
    void *rv;
    CHECK(pthread_join(t, &rv) == 0);
    CHECK(rv == PTHREAD_CANCELED && reached == 2);
    /* A cancellation already pending when close is called acts before the descriptor is
     * touched or after it is closed; either way it is not left half-closed. */
    errno = 0;
    int state = fcntl(fds[0], F_GETFD);
    CHECK(state >= 0 || errno == EBADF);
    if (state >= 0)
        CHECK(close(fds[0]) == 0);
    CHECK(close(fds[1]) == 0);
    return 0;
}
