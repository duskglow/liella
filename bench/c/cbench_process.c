/* Processes and terminals (phase 11): spawning and waiting, the shell interfaces, and the
 * terminal calls on a pseudo-terminal. The same file builds against the products
 * (`cargo xtask bench`) and against the host C library (`cc -O2 bench/c/cbench_process.c`), so
 * benchmarks/progress.md compares identical code. Prints `name<TAB>ns/op` lines, the best of
 * several rounds; the lines are formatted by hand so that the measured functions do not format
 * their own results.
 *
 * The spawn rows run /bin/true (or /bin/sh) from the system, so they are dominated by the kernel
 * and the child program, which are the same for both libraries: compare their ratio, not their
 * absolute value. The last row repeats the plain spawn from a process with a 256 MiB heap whose
 * pages are all touched: a child created without CLONE_VM copies the caller's page tables
 * (D-18.1, D-18.3), so that cost grows with the caller's resident memory.
 *
 * Every call is checked once before it is timed, and every child is reaped; a call that fails
 * ends the program, so a row never times an error path by accident (`waitpid` with no child and
 * `isatty` on /dev/null time their documented failure, which is checked as such).
 *
 * Counters (cbench_counters.h, CBENCH_COUNTERS=1) count this process only (no `inherit`): a spawn
 * row's count is the parent-side path (the library's spawn code and the parent's share of the
 * kernel work), never the child program, which is the same binary for both libraries. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pty.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#include "cbench_counters.h"

extern char **environ;

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
    (void)!write(2, "cbench_process: unexpected result: ", 35);
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
        ctr_begin();                                       \
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
        ctr_end(name, 7.0 * (double)(iters));              \
        put(name, best);                                   \
    } while (0)

static int (*volatile tcgetattr_fn)(int, struct termios *) = tcgetattr;
static int (*volatile tcsetattr_fn)(int, int, const struct termios *) = tcsetattr;
static int (*volatile cfsetspeed_fn)(struct termios *, speed_t) = cfsetspeed;
static speed_t (*volatile cfgetospeed_fn)(const struct termios *) = cfgetospeed;
static int (*volatile ttyname_r_fn)(int, char *, size_t) = ttyname_r;
static int (*volatile isatty_fn)(int) = isatty;
static pid_t (*volatile waitpid_fn)(pid_t, int *, int) = waitpid;

static char *true_argv[] = {"true", NULL};
static posix_spawn_file_actions_t actions;

/* posix_spawn + waitpid: 0 when the child ran and exited with status 0. */
static long spawn_wait(const posix_spawn_file_actions_t *fa)
{
    pid_t pid;
    int status;
    if (posix_spawn(&pid, "/bin/true", fa, NULL, true_argv, environ) != 0)
        return 1;
    if (waitpid(pid, &status, 0) != pid)
        return 2;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 3;
}

/* posix_spawnp of `true` found through PATH (STRESS-ENV-AUDIT-2) + waitpid. */
static long spawnp_wait(void)
{
    pid_t pid;
    int status;
    if (posix_spawnp(&pid, "true", NULL, NULL, true_argv, environ) != 0)
        return 1;
    if (waitpid(pid, &status, 0) != pid)
        return 2;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 3;
}

static long fork_wait(void)
{
    int status;
    pid_t pid = fork();
    if (pid == 0)
        _exit(0);
    if (pid < 0 || waitpid(pid, &status, 0) != pid)
        return 1;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 2;
}

static long vfork_wait(void)
{
    int status;
    pid_t pid = vfork();
    if (pid == 0)
        _exit(0);
    if (pid < 0 || waitpid(pid, &status, 0) != pid)
        return 1;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 2;
}

static long shell(void)
{
    return system("exit 0");
}

static long pipe_true(void)
{
    FILE *f = popen("true", "r");
    if (f == NULL)
        return 1;
    return pclose(f);
}

/* waitpid on no child: -1 with ECHILD. */
static long no_child(void)
{
    int status;
    return waitpid_fn(-1, &status, WNOHANG) == -1 && errno == ECHILD ? 0 : 1;
}

static int master, slave, null_fd;
static struct termios tio, tio2;
static char name[64];

static long getattr(void)
{
    return tcgetattr_fn(slave, &tio);
}

static long setattr(void)
{
    return tcsetattr_fn(slave, TCSANOW, &tio);
}

static long speed(void)
{
    return cfsetspeed_fn(&tio2, B9600) + (long)cfgetospeed_fn(&tio2);
}

static long pair(void)
{
    int m, s;
    if (openpty(&m, &s, NULL, NULL, NULL) != 0)
        return 1;
    close(s);
    close(m);
    return 0;
}

static long tty_name(void)
{
    return ttyname_r_fn(slave, name, sizeof name) == 0 ? name[5] : -1;
}

int main(void)
{
    if (spawn_wait(NULL) != 0)
        fail("posix_spawn /bin/true");
    MEASURE("posix_spawn_true", 200, spawn_wait(NULL));
    if (spawnp_wait() != 0)
        fail("posix_spawnp true");
    MEASURE("posix_spawnp_true", 200, spawnp_wait());

    null_fd = open("/dev/null", O_RDONLY);
    if (null_fd < 0 || posix_spawn_file_actions_init(&actions) != 0 ||
        posix_spawn_file_actions_addopen(&actions, 0, "/dev/null", O_RDONLY, 0) != 0 ||
        posix_spawn_file_actions_adddup2(&actions, 0, 2) != 0 ||
        posix_spawn_file_actions_addclose(&actions, null_fd) != 0)
        fail("posix_spawn_file_actions");
    if (spawn_wait(&actions) != 0)
        fail("posix_spawn with file actions");
    MEASURE("posix_spawn_true_actions", 200, spawn_wait(&actions));

    if (fork_wait() != 0)
        fail("fork");
    MEASURE("fork_exit_wait", 1000, fork_wait());
    if (vfork_wait() != 0)
        fail("vfork");
    MEASURE("vfork_exit_wait", 1000, vfork_wait());

    if (shell() != 0)
        fail("system");
    MEASURE("system_exit0", 100, shell());
    if (pipe_true() != 0)
        fail("popen/pclose");
    MEASURE("popen_pclose_true", 100, pipe_true());

    if (no_child() != 0)
        fail("waitpid ECHILD");
    MEASURE("waitpid_echild", 2000000, no_child());

    if (openpty(&master, &slave, NULL, NULL, NULL) != 0 || tcgetattr(slave, &tio) != 0)
        fail("openpty/tcgetattr");
    tio2 = tio;
    MEASURE("tcgetattr", 2000000, getattr());
    if (setattr() != 0)
        fail("tcsetattr");
    MEASURE("tcsetattr_now", 1000000, setattr());
    if (speed() != B9600)
        fail("cfsetspeed/cfgetospeed");
    MEASURE("cfsetspeed_cfgetospeed", 2000000, speed());
    if (pair() != 0)
        fail("openpty");
    MEASURE("openpty_close", 20000, pair());
    if (tty_name() != 'p' || strncmp(name, "/dev/pts/", 9) != 0)
        fail("ttyname_r");
    MEASURE("ttyname_r", 500000, tty_name());
    if (isatty(slave) != 1 || isatty(null_fd) != 0)
        fail("isatty");
    MEASURE("isatty_pty", 2000000, isatty_fn(slave));
    MEASURE("isatty_devnull", 2000000, isatty_fn(null_fd));

    /* The caller's resident memory: 256 MiB allocated and touched, then the plain spawn again.
     * Last, since the memory stays until the program ends. */
    size_t big = (size_t)256 << 20;
    char *heap = malloc(big);
    if (heap == NULL)
        fail("malloc 256 MiB");
    memset(heap, 1, big);
    sink = heap[big - 1];
    if (spawn_wait(NULL) != 0)
        fail("posix_spawn with a large heap");
    MEASURE("posix_spawn_true_heap256m", 50, spawn_wait(NULL));
    return 0;
}
