/* POSIX message queues through the real C entry points (§25.3, phase 13), in a private IPC
 * namespace (the RUN-WRAP below; its queues vanish with it, so nothing reaches the system's
 * /dev/mqueue): mq_open's variadic mode and attributes (and their absence without O_CREAT),
 * __mq_open_2, send and receive as cancellation points (s25_3_x_0: a request pending at the
 * call, and one arriving while the call blocks), and a SIGEV_THREAD notification. */
/* RUN-WRAP: unshare --user --ipc --map-current-user */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <mqueue.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

extern mqd_t __mq_open_2(const char *name, int oflag);

static mqd_t q;

static int fail(int code, const char *what)
{
    fprintf(stderr, "FAIL %d: %s (errno %d)\n", code, what, errno);
    return code;
}

/* 0 send, 1 timedsend, 2 receive, 3 timedreceive: with a cancellation request pending. */
static void *pending(void *arg)
{
    int which = (int)(long)arg;
    char b[16];
    struct timespec far;
    clock_gettime(CLOCK_REALTIME, &far);
    far.tv_sec += 30;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    pthread_cancel(pthread_self());
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    switch (which) {
    case 0: mq_send(q, "x", 1, 0); break;
    case 1: mq_timedsend(q, "x", 1, 0, &far); break;
    case 2: mq_receive(q, b, sizeof b, NULL); break;
    case 3: mq_timedreceive(q, b, sizeof b, NULL, &far); break;
    }
    return (void *)1;
}

/* Blocks in mq_receive on the empty queue until cancelled. */
static void *blocked(void *arg)
{
    (void)arg;
    char b[16];
    mq_receive(q, b, sizeof b, NULL);
    return (void *)1;
}

static volatile int notified, value_ok;
static pthread_t main_thread;
static int token;

static void notify_fn(union sigval v)
{
    value_ok = v.sival_ptr == &token && !pthread_equal(pthread_self(), main_thread);
    __atomic_store_n(&notified, 1, __ATOMIC_SEQ_CST);
}

int main(void)
{
    main_thread = pthread_self();
    struct mq_attr a = {.mq_maxmsg = 4, .mq_msgsize = 16}, got;
    q = mq_open("/conf", O_RDWR | O_CREAT | O_EXCL, 0600, &a);
    if (q == (mqd_t)-1)
        return fail(1, "mq_open with O_CREAT, mode and attributes");
    if (mq_getattr(q, &got) != 0 || got.mq_maxmsg != 4 || got.mq_msgsize != 16 || got.mq_curmsgs != 0)
        return fail(2, "the attributes were passed");
    mqd_t q2 = mq_open("/conf", O_RDONLY);
    if (q2 == (mqd_t)-1)
        return fail(3, "mq_open without the optional arguments");
    if (fcntl(q2, F_GETFD) != FD_CLOEXEC)
        return fail(4, "close-on-exec");
    mq_close(q2);
    if (__mq_open_2("/conf", O_RDWR) == (mqd_t)-1)
        return fail(5, "__mq_open_2 without O_CREAT");
    errno = 0;
    if (__mq_open_2("/conf2", O_RDWR | O_CREAT) != (mqd_t)-1 || errno != EINVAL)
        return fail(6, "__mq_open_2 with O_CREAT is EINVAL");
    if (mq_open("/conf2", O_RDONLY) != (mqd_t)-1 || errno != ENOENT)
        return fail(7, "__mq_open_2 created nothing");

    /* Cancellation with a request pending: send and receive act on it. */
    for (int which = 0; which < 4; which++) {
        pthread_t t;
        void *r;
        pthread_create(&t, NULL, pending, (void *)(long)which);
        pthread_join(t, &r);
        if (r != PTHREAD_CANCELED)
            return fail(10 + which, "a cancellation point acts on a pending request");
    }
    /* Cancellation while blocked in mq_receive. */
    char b[16];
    while (mq_timedreceive(q, b, sizeof b, NULL, &(struct timespec){0, 0}) >= 0)
        ;
    pthread_t t;
    void *r;
    pthread_create(&t, NULL, blocked, NULL);
    usleep(100000);
    pthread_cancel(t);
    pthread_join(t, &r);
    if (r != PTHREAD_CANCELED)
        return fail(20, "a blocked mq_receive is cancelled");

    /* SIGEV_THREAD: the function runs in a new thread with the value. */
    struct sigevent sev = {.sigev_notify = SIGEV_THREAD, .sigev_notify_function = notify_fn, .sigev_value.sival_ptr = &token};
    if (mq_notify(q, &sev) != 0)
        return fail(30, "SIGEV_THREAD registration");
    errno = 0;
    if (mq_notify(q, &sev) != -1 || errno != EBUSY)
        return fail(31, "one registrant");
    mq_send(q, "m", 1, 0);
    for (int i = 0; i < 500 && !__atomic_load_n(&notified, __ATOMIC_SEQ_CST); i++)
        usleep(10000);
    if (!notified || !value_ok)
        return fail(32, "the function was called in a new thread with sigev_value");
    mq_close(q);
    if (mq_unlink("/conf") != 0)
        return fail(40, "mq_unlink");
    return 0;
}
