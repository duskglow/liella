/* The descriptors the library keeps across calls are never 0, 1 or 2 (validation T12): with the
 * standard descriptors closed, each family that keeps one (database cursors, the utmp file,
 * syslog's socket, mtrace's file, SIGEV_THREAD timers, AIO) is used, and afterwards 0, 1 and 2
 * are still closed and a write to each still fails with EBADF, as it would without the library.
 * The exit status names the first family after which a standard descriptor was taken. */
#define _GNU_SOURCE
#include <aio.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <mcheck.h>
#include <pwd.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <syslog.h>
#include <time.h>
#include <unistd.h>
#include <utmp.h>

static int standard_closed(void)
{
    for (int fd = 0; fd < 3; fd++) {
        errno = 0;
        if (fcntl(fd, F_GETFD) != -1 || write(fd, "x", 1) != -1 || errno != EBADF)
            return 0;
    }
    return 1;
}

static void notified(union sigval v)
{
    (void)v;
}

int main(void)
{
    char path[512];
    const char *dir = getenv("TMPDIR");
    snprintf(path, sizeof path, "%s/liella-privfd-%d", dir && *dir ? dir : "/tmp", (int)getpid());
    int f = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (f < 0 || write(f, "abcd", 4) != 4)
        return 100;
    close(0);
    close(1);
    close(2);
    int step = 1;
    int status = 0;
#define STEP(code) do { code; if (!status && !standard_closed()) status = step; step++; } while (0)
    STEP((void)getpwent());                                   /* 1: the passwd cursor */
    STEP((void)getgrent());                                   /* 2: the group cursor */
    STEP((setutent(), (void)getutent()));                     /* 3: the utmp file */
    STEP((openlog("privfd", LOG_NDELAY, LOG_USER), syslog(LOG_DEBUG, "privfd")));  /* 4: syslog */
    STEP((setenv("MALLOC_TRACE", "/dev/null", 1), mtrace(), free(malloc(1))));    /* 5: mtrace */
    timer_t t;
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = notified;
    STEP((void)timer_create(CLOCK_MONOTONIC, &ev, &t));      /* 6: the timers' epoll set and timerfd */
    struct aiocb cb;
    char buf[4];
    memset(&cb, 0, sizeof cb);
    cb.aio_fildes = f;
    cb.aio_buf = buf;
    cb.aio_nbytes = 4;
    STEP(if (aio_read(&cb) == 0) { const struct aiocb *l[1] = {&cb}; aio_suspend(l, 1, NULL); });  /* 7: AIO */
    muntrace();
    endpwent();
    endgrent();
    endutent();
    closelog();
    timer_delete(t);
    close(f);
    unlink(path);
    return status;
}
