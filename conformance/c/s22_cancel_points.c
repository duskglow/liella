/* The cancellation clauses of §9.1, §11.1, §11.3, §11.7, §15.1, §15.2, §20.5 and §23.3: each
 * function listed as a cancellation point acts on a pending request when called (the thread
 * ends with PTHREAD_CANCELED); arc4random is not one (§9.1). */
#define _GNU_SOURCE
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/random.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

static int sv[2];
static int pfd[2];
static int lsock;

static int call(int which)
{
    char b[4] = {0};
    struct timespec ts = {0, 1000};
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGUSR2);
    switch (which) {
    case 0: return open("/dev/null", O_RDONLY);
    case 1: return (int)read(pfd[0], b, 1);
    case 2: return (int)write(pfd[1], "x", 1);
    case 3: { struct pollfd p = {pfd[0], POLLIN, 0}; return poll(&p, 1, 0); }
    case 4: { fd_set r; FD_ZERO(&r); FD_SET(pfd[0], &r); struct timeval tv = {0, 0}; return select(pfd[0] + 1, &r, NULL, NULL, &tv); }
    case 5: return accept(lsock, NULL, NULL);
    case 6: return (int)send(sv[0], "x", 1, MSG_DONTWAIT);
    case 7: return (int)recv(sv[0], b, 1, MSG_DONTWAIT);
    case 8: return (int)getrandom(b, 4, 0);
    case 9: return nanosleep(&ts, NULL);
    case 10: return usleep(1);
    case 11: return (int)sleep(0);
    case 12: return clock_nanosleep(CLOCK_MONOTONIC, 0, &ts, NULL);
    case 13: { siginfo_t i; struct timespec z = {0, 0}; return sigtimedwait(&s, &i, &z); }
    case 14: { struct iovec v = {b, 1}; return (int)readv(pfd[0], &v, 1); }
    case 15: { int ep = epoll_create1(0); struct epoll_event ev; return epoll_wait(ep, &ev, 1, 0); }
    case 16: return fsync(pfd[1]);
    case 17: { struct sockaddr_un { unsigned short f; char p[108]; } a = {1, "/nonexistent"}; int fd = socket(AF_UNIX, SOCK_STREAM, 0); return connect(fd, (struct sockaddr *)&a, sizeof a); }
    case 18: return (int)pread(pfd[0], b, 1, 0);
    case 19: return (int)arc4random(); /* not a cancellation point */
    }
    return 0;
}

static void *worker(void *arg)
{
    int which = (int)(long)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    pthread_cancel(pthread_self()); /* pending: disabled */
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    call(which);
    return (void *)1;
}

int main(void)
{
    if (pipe(pfd) != 0 || socketpair(AF_UNIX, SOCK_STREAM, 0, sv) != 0)
        return 1;
    fcntl(pfd[0], F_SETFL, O_NONBLOCK);
    lsock = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    struct { unsigned short f; unsigned short port; unsigned addr; char z[8]; } in = {2, 0, 0x0100007f, {0}};
    bind(lsock, (struct sockaddr *)&in, sizeof in);
    listen(lsock, 1);
    for (int which = 0; which <= 19; which++) {
        pthread_t t;
        pthread_create(&t, NULL, worker, (void *)(long)which);
        void *r;
        pthread_join(t, &r);
        int cancelled = r == PTHREAD_CANCELED;
        if (which == 19 ? cancelled : !cancelled)
            return 10 + which;
    }
    return 0;
}
