/* Spec §19.4 forkpty and the §19.1/§19.4 cancellation points, in a process Liella started (its
 * fork and its threads). forkpty: the parent receives the master (and only it), name is the
 * slave's path; the child leads a new session whose controlling terminal is the slave, on
 * descriptors 0-2, with termp and winp applied; the child's output and exit status reach the
 * parent. Cancellation: with a request pending, tcdrain and openpty act on it (cancellation
 * points), tcsetattr and tcflush return. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <pty.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

static int count_fds(void)
{
    int n = 0;
    for (int fd = 0; fd < 256; fd++)
        if (fcntl(fd, F_GETFD) != -1)
            n++;
    return n;
}

/* Everything the master gives within a second of quiet. */
static size_t drain_master(int m, char *buf, size_t cap)
{
    size_t got = 0;
    struct pollfd p = {m, POLLIN, 0};
    while (got + 1 < cap && poll(&p, 1, 1000) == 1 && (p.revents & POLLIN)) {
        ssize_t n = read(m, buf + got, cap - 1 - got);
        if (n <= 0)
            break;
        got += (size_t)n;
    }
    buf[got] = 0;
    return got;
}

static int forkpty_case(int with_args)
{
    struct termios want;
    memset(&want, 0, sizeof want);
    want.c_iflag = IXON;
    want.c_cflag = CS8 | CREAD;
    want.c_lflag = ISIG | ICANON;
    want.c_cc[VINTR] = 3;
    want.c_cc[VMIN] = 1;
    cfsetspeed(&want, B57600);
    struct winsize w = {30, 100, 0, 0};
    char name[64] = "unset";
    int m = -77;
    int before = count_fds();
    pid_t pid = with_args ? forkpty(&m, name, &want, &w) : forkpty(&m, NULL, NULL, NULL);
    if (pid < 0)
        return 10;
    if (pid == 0) {
        alarm(10);
        int bad = 0;
        if (getsid(0) != getpid() || tcgetpgrp(0) != getpgrp())
            bad |= 1;
        if (!isatty(0) || !isatty(1) || !isatty(2) || m != -77)
            bad |= 2;
        for (int fd = 3; fd < 256; fd++)
            if (isatty(fd))
                bad |= 4;
        if (with_args) {
            char *t = ttyname(0);
            if (!t || strcmp(t, name) != 0)
                bad |= 8;
            struct termios u;
            struct winsize ws;
            if (tcgetattr(0, &u) != 0 || u.c_iflag != want.c_iflag || u.c_lflag != want.c_lflag || cfgetospeed(&u) != B57600 || u.c_cc[VINTR] != 3)
                bad |= 16;
            if (ioctl(0, TIOCGWINSZ, &ws) != 0 || ws.ws_row != 30 || ws.ws_col != 100)
                bad |= 32;
        }
        if (write(1, "hello\n", 6) != 6)
            bad |= 64;
        _exit(bad ? 100 + bad : 7);
    }
    int fail = 0;
    if (count_fds() != before + 1 || !isatty(m))
        fail = 11; /* the master only */
    if (with_args) {
        char *p = ptsname(m);
        if (!p || strcmp(p, name) != 0 || strncmp(name, "/dev/pts/", 9) != 0)
            fail = 12;
    } else if (strcmp(name, "unset") != 0) {
        fail = 13;
    }
    char out[256];
    drain_master(m, out, sizeof out);
    /* Without termp the default output processing turns \n into \r\n. */
    if (strcmp(out, with_args ? "hello\n" : "hello\r\n") != 0 && !fail)
        fail = 14;
    int st = 0;
    if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st))
        return 15;
    if (WEXITSTATUS(st) != 7)
        return 100 + WEXITSTATUS(st) % 100;
    close(m);
    return fail;
}

/* ------------------------------------------------------------------ cancellation */

static int c_m, c_s, c_pipe[2];
static int (*c_op)(void);
static struct termios c_t;

static void *c_thread(void *arg)
{
    (void)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    char ch;
    if (read(c_pipe[0], &ch, 1) != 1) /* the main thread has cancelled us */
        return (void *)2;
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    c_op();
    return (void *)1;
}

static int c_tcdrain(void) { return tcdrain(c_s); }
static int c_tcsetattr(void) { return tcsetattr(c_s, TCSANOW, &c_t); }
static int c_tcflush(void) { return tcflush(c_s, TCIFLUSH); }
static int c_openpty(void)
{
    int a, b;
    int r = openpty(&a, &b, NULL, NULL, NULL);
    if (r == 0) {
        close(a);
        close(b);
    }
    return r;
}

/* 1 when the operation acted on the pending request, 0 when it returned. */
static int cancelled(int (*op)(void))
{
    if (pipe(c_pipe) != 0)
        return -1;
    c_op = op;
    pthread_t th;
    if (pthread_create(&th, NULL, c_thread, NULL) != 0)
        return -1;
    pthread_cancel(th);
    if (write(c_pipe[1], "x", 1) != 1)
        return -1;
    void *res;
    pthread_join(th, &res);
    close(c_pipe[0]);
    close(c_pipe[1]);
    return res == PTHREAD_CANCELED ? 1 : res == (void *)1 ? 0 : -1;
}

int main(void)
{
    alarm(60);
    /* Descriptors inherited from the runner (a test process's ptys among them) are not ours. */
    for (int fd = 3; fd < 256; fd++)
        close(fd);
    int r = forkpty_case(1);
    if (r)
        return r;
    r = forkpty_case(0);
    if (r)
        return 20 + r;
    if (openpty(&c_m, &c_s, NULL, NULL, NULL) != 0 || tcgetattr(c_s, &c_t) != 0)
        return 40;
    if (cancelled(c_tcdrain) != 1)
        return 41;
    if (cancelled(c_openpty) != 1)
        return 42;
    if (cancelled(c_tcsetattr) != 0)
        return 43;
    if (cancelled(c_tcflush) != 0)
        return 44;
    /* A NULL master for forkpty: EFAULT, no child. */
    errno = 0;
    if (forkpty(NULL, NULL, NULL, NULL) != -1 || errno != EFAULT)
        return 45;
    close(c_m);
    close(c_s);
    return 0;
}
