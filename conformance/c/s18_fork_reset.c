/* Spec §18.2: the child of fork has only the calling thread, so none of the library's helper
 * threads or queued requests is inherited, and the library state they own starts afresh in the
 * child (D-18.6, D-18.8). In a process that used a SIGEV_THREAD timer, aio and getaddrinfo_a,
 * and whose other thread holds a stream's lock at fork, the child: gets a timer notification,
 * completes an aio request and a getaddrinfo_a request, and locks the stream. A child forked
 * before any of them was used can use them for the first time (and so can its own child).
 * A hang in a child is reported by its alarm (exit by SIGALRM), not by the runner's limit. */
#define _GNU_SOURCE
#include <aio.h>
#include <errno.h>
#include <netdb.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static sem_t fired, held, release;
static FILE *stream;
static int aio_fd = -1;
static timer_t parent_timer;

static void on_timer(union sigval v)
{
    (void)v;
    sem_post(&fired);
}

static int wait_sem(sem_t *s)
{
    while (sem_wait(s) != 0)
        if (errno != EINTR)
            return -1;
    return 0;
}

/* One SIGEV_THREAD notification. keep: leave the timer in place (armed, a long period), so a
 * later fork happens with the timer and its notification machinery alive. */
static int use_timer(int keep)
{
    timer_t t;
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = on_timer;
    if (timer_create(CLOCK_MONOTONIC, &ev, &t) != 0)
        return 1;
    struct itimerspec its;
    memset(&its, 0, sizeof its);
    its.it_value.tv_nsec = 1000000;
    if (keep)
        its.it_interval.tv_sec = 3600;
    if (timer_settime(t, 0, &its, NULL) != 0)
        return 2;
    if (wait_sem(&fired) != 0)
        return 3;
    if (keep) {
        parent_timer = t;
        return 0;
    }
    return timer_delete(t) != 0 ? 4 : 0;
}

static int use_aio(void)
{
    static char buf[16] = "0123456789abcdef";
    struct aiocb cb;
    memset(&cb, 0, sizeof cb);
    cb.aio_fildes = aio_fd;
    cb.aio_buf = buf;
    cb.aio_nbytes = sizeof buf;
    cb.aio_sigevent.sigev_notify = SIGEV_NONE;
    if (aio_write(&cb) != 0)
        return 11;
    const struct aiocb *l[1] = {&cb};
    while (aio_error(&cb) == EINPROGRESS)
        if (aio_suspend(l, 1, NULL) != 0 && errno != EINTR)
            return 12;
    if (aio_error(&cb) != 0 || aio_return(&cb) != (ssize_t)sizeof buf)
        return 13;
    return 0;
}

static int use_gai(void)
{
    struct addrinfo hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_flags = AI_NUMERICHOST;
    struct gaicb g;
    memset(&g, 0, sizeof g);
    g.ar_name = "127.0.0.1";
    g.ar_request = &hints;
    struct gaicb *l[1] = {&g};
    if (getaddrinfo_a(GAI_WAIT, l, 1, NULL) != 0)
        return 21;
    if (gai_error(&g) != 0 || g.ar_result == NULL)
        return 22;
    freeaddrinfo(g.ar_result);
    return 0;
}

static int use_all(void)
{
    int r = use_timer(0);
    if (r == 0)
        r = use_aio();
    if (r == 0)
        r = use_gai();
    return r;
}

static int reap(pid_t pid)
{
    int st = 0;
    if (syscall(SYS_wait4, pid, &st, 0, NULL) != pid)
        return -1;
    if ((st & 0x7f) == 0)
        return (st >> 8) & 0xff;
    return 100 + (st & 0x7f);
}

static void *holder(void *arg)
{
    (void)arg;
    flockfile(stream);
    sem_post(&held);
    wait_sem(&release);
    funlockfile(stream);
    return NULL;
}

int main(void)
{
    if (sem_init(&fired, 0, 0) != 0 || sem_init(&held, 0, 0) != 0 || sem_init(&release, 0, 0) != 0)
        return 1;
    stream = tmpfile();
    if (stream == NULL)
        return 2;
    aio_fd = fileno(stream);

    /* A child forked before any use: the first use there initialises, and a grandchild
     * forked after that use resets what the child started. */
    pid_t pid = fork();
    if (pid == 0) {
        alarm(30);
        int r = use_all();
        if (r != 0)
            _exit(r);
        pid_t g = fork();
        if (g == 0) {
            alarm(30);
            _exit(use_all());
        }
        r = reap(g);
        _exit(r == 0 ? 0 : 50);
    }
    int r = reap(pid);
    if (r != 0)
        fprintf(stderr, "s18_fork_reset: first child status %d\n", r);
    if (r != 0)
        return r == 50 ? 3 : 4;

    /* The parent uses every subsystem (the timer stays armed), another thread holds the
     * stream's lock across the fork. */
    if (use_timer(1) != 0 || use_aio() != 0 || use_gai() != 0)
        return 5;
    pthread_t th;
    if (pthread_create(&th, NULL, holder, NULL) != 0)
        return 6;
    if (wait_sem(&held) != 0)
        return 7;
    pid = fork();
    if (pid == 0) {
        alarm(30);
        if (ftrylockfile(stream) != 0)
            _exit(30);
        funlockfile(stream);
        _exit(use_all());
    }
    r = reap(pid);
    sem_post(&release);
    pthread_join(th, NULL);
    if (r != 0)
        fprintf(stderr, "s18_fork_reset: second child status %d\n", r);
    if (r != 0)
        return r >= 100 ? 8 : 9;
    if (timer_delete(parent_timer) != 0)
        return 10;
    return 0;
}
