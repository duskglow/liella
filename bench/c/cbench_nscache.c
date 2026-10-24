/* The integrated name-service cache (D-16.10): the same lookups with the cache (the user's shared
 * region, warm) and without it (`liella_nscache_enable(0)`, as with LIELLA_NSCACHE=0), in one
 * process. Liella only (it uses <liella/native.h>); the reference's numbers for the uncached rows
 * are cbench_netdb's. `cargo xtask bench` runs it with a private /dev/shm when it can, so that no
 * region is left in the system's. Prints `name<TAB>ns/op`, the best of several rounds.
 *
 * Nothing reaches a DNS server: "localhost" is in /etc/hosts and the `files` source answers
 * first. Also measured: the first and second lookups of a fresh process (this program run again
 * with execve), with the region valid, with the region created, and with the cache off. */
#define _GNU_SOURCE
#include <netdb.h>
#include <netinet/in.h>
#include <pwd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include <liella/native.h>

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

static void fail(const char *what)
{
    (void)!write(2, "cbench_nscache: unexpected result: ", 35);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        for (int round = 0; round < 7; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        put(name, best);                                   \
    } while (0)

static char dbbuf[4096];

static long pw(void)
{
    struct passwd p, *r = NULL;
    return getpwnam_r("root", &p, dbbuf, sizeof dbbuf, &r) == 0 && r ? (long)r->pw_uid + 1 : -1;
}

static long gai(void)
{
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo("localhost", NULL, &hints, &res) != 0)
        return -1;
    long n = 100;
    for (struct addrinfo *p = res; p; p = p->ai_next)
        n++;
    freeaddrinfo(res);
    return n;
}

static long serv(void)
{
    struct servent s, *r = NULL;
    return getservbyname_r("http", "tcp", &s, dbbuf, sizeof dbbuf, &r) == 0 && r ? (long)ntohs((unsigned short)r->s_port) : -1;
}

/* A fresh process (this program again, through execve) times its first and second lookups:
 * `out` gets the best of `runs` for each (ns). `env` is its whole environment; `fresh` removes
 * the user's region before each run (so the first lookup creates it). */
static void fresh_process(char *const env[], int fresh, int runs, double out[2])
{
    out[0] = out[1] = 1e300;
    char region[64];
    snprintf(region, sizeof region, "/dev/shm/liella-nscache-%u", (unsigned)geteuid());
    for (int i = 0; i < runs; i++) {
        if (fresh)
            unlink(region);
        int fd[2];
        if (pipe(fd) != 0)
            fail("pipe");
        pid_t p = fork();
        if (p == 0) {
            dup2(fd[1], 1);
            char *argv[] = {(char *)"cbench_nscache", (char *)"first", NULL};
            execve("/proc/self/exe", argv, env);
            _exit(127);
        }
        close(fd[1]);
        char buf[128] = {0};
        ssize_t n = read(fd[0], buf, sizeof buf - 1);
        close(fd[0]);
        int st;
        waitpid(p, &st, 0);
        double a, b;
        if (n <= 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0 || sscanf(buf, "%lf %lf", &a, &b) != 2)
            fail("fresh process");
        if (a < out[0])
            out[0] = a;
        if (b < out[1])
            out[1] = b;
    }
}

int main(int argc, char **argv)
{
    if (argc > 1 && !strcmp(argv[1], "first")) {
        /* In the fresh process: the first lookup, then a second (another user). */
        double t0 = now();
        long a = pw();
        double t1 = now();
        struct passwd p, *r = NULL;
        long b = getpwnam_r("daemon", &p, dbbuf, sizeof dbbuf, &r) == 0 ? 1 : -1;
        double t2 = now();
        printf("%.0f %.0f\n", t1 - t0, t2 - t1);
        return a == 1 && b == 1 ? 0 : 1;
    }
    if (pw() != 1 || gai() != 101 || serv() != 80)
        fail("lookups");
    struct liella_nscache_info st;
    if (liella_nscache_status(&st) != 0 || st.hits + st.misses == 0)
        fail("the cache is not in use");
    MEASURE("getpwnam_r_cached", 1000000, pw());
    MEASURE("getaddrinfo_localhost_cached", 200000, gai());
    MEASURE("getservbyname_r_cached", 1000000, serv());
    liella_nscache_enable(0);
    MEASURE("getpwnam_r_uncached", 100000, pw());
    MEASURE("getaddrinfo_localhost_uncached", 100000, gai());
    MEASURE("getservbyname_r_uncached", 50000, serv());
    liella_nscache_enable(1);
    if (pw() != 1)
        fail("enable");
    /* Fresh processes: the region exists and is valid, the region is created, the cache off. */
    char *const on[] = {(char *)"PATH=/usr/bin:/bin", NULL};
    char *const off[] = {(char *)"PATH=/usr/bin:/bin", (char *)"LIELLA_NSCACHE=0", NULL};
    double t[2];
    fresh_process(on, 0, 40, t);
    put("first_getpwnam_r_cached", t[0]);
    put("second_getpwnam_r_cached", t[1]);
    fresh_process(on, 1, 40, t);
    put("first_getpwnam_r_create", t[0]);
    put("second_getpwnam_r_create", t[1]);
    fresh_process(off, 0, 40, t);
    put("first_getpwnam_r_uncached", t[0]);
    put("second_getpwnam_r_uncached", t[1]);
    return 0;
}
