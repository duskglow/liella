/* Process lifetime (STARTUP-COST): fork+exec+wait of a tiny program, measured end to end. The same file
 * builds against the products (`cargo xtask bench`) and against the host C library (dynamic and static),
 * and it is its own child: the rows spawn /proc/self/exe with a mode argument, so the child is linked
 * the way the parent is (Liella static, glibc dynamic or glibc static) and the row measures that
 * library's exec-to-main, main body and exit. Prints `name<TAB>ns/op` lines, the best of several rounds.
 *
 *   spawn_bin_true     the system's /bin/true (same child for every build): the spawn + wait path alone
 *   exec_true_static   child returns 0 from main at once (exec-to-main + exit)
 *   exec_true_echo     child writes one line to stdout (/dev/null) with puts and exits
 *   exec_hello_locale  child does setlocale(LC_ALL, "") + printf with LC_ALL=en_US.UTF-8
 *
 * Instruction rows (CI-STARTUP): user-mode instructions of ONE child process, exec to exit, counted by a
 * perf counter the parent attaches to the forked child before it execs (enable_on_exec, exclude_kernel:
 * the kernel's exec work is left out, ld.so and the library's start-up are in). The child waits on a pipe
 * until the counter is open, so nothing before the exec is counted. Each row prints the best ns of its
 * spawns and, with CBENCH_COUNTERS=1, a `ctr` line whose instructions AND user-instructions fields both
 * hold this user-mode count per process (other fields -1); the minimum of 5 children (ASLR moves a few
 * instructions). These rows carry instruction limits (bench/instr_limits.tsv) for the per-train CI check.
 *   startup_instr_c        start + exit, no setlocale (the C locale)
 *   startup_instr_setloc   setlocale(LC_ALL, "") under LC_ALL=en_US.UTF-8, exit
 *   startup_instr_printf   as startup_instr_setloc + one printf("%s %d\n")
 *   startup_instr_wctype   as startup_instr_setloc + iswalpha, towupper and wcwidth of U+00E9
 *
 * The kernel's exec/mmap/exit work is common to both libraries; compare ratios. Every child is checked
 * once before it is timed and reaped; a failing child ends the program. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/perf_event.h>
#include <locale.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>

#ifndef SYS_perf_event_open
#define SYS_perf_event_open 298
#endif

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
    (void)!write(2, "cbench_startup: unexpected result: ", 35);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

/* The child is this program: keep the stress constructor out of it (see cbench_rows.h), so the
 * start-up rows measure the library's start-up and nothing the bench harness adds. */
#define CBENCH_NO_STRESS
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

extern char **environ;

static posix_spawn_file_actions_t actions;
static char *env_plain[] = {"PATH=/usr/bin:/bin", NULL};
static char *env_locale[] = {"PATH=/usr/bin:/bin", "LC_ALL=en_US.UTF-8", NULL};

/* Spawn this program in `mode` with stdout on /dev/null; 0 when the child exits with status 0. */
static long run(char *mode, char **envp)
{
    char *argv[] = {"cbench_startup", mode, NULL};
    pid_t pid;
    int status;
    if (posix_spawn(&pid, "/proc/self/exe", &actions, NULL, argv, envp) != 0)
        return 1;
    if (waitpid(pid, &status, 0) != pid)
        return 2;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 3;
}

/* The parent side alone: the same child (the system's /bin/true) for every build, so a difference
 * between builds is the library's posix_spawn + waitpid path, not its startup code. Subtract it from
 * the exec rows to read the child's own startup. */
static long run_bin_true(void)
{
    char *argv[] = {"true", NULL};
    pid_t pid;
    int status;
    if (posix_spawn(&pid, "/bin/true", &actions, NULL, argv, env_plain) != 0)
        return 1;
    if (waitpid(pid, &status, 0) != pid)
        return 2;
    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 3;
}

/* One instruction row: 5 children in `mode`, each counted from its exec to its exit (user mode only). */
static double instr_row(const char *name, char *mode, char **envp)
{
    double best_ns = 1e300, best_instr = -1.0;
    for (int r = 0; r < 5; r++) {
        int pfd[2];
        if (pipe(pfd) != 0)
            fail("pipe");
        double t0 = now();
        pid_t pid = fork();
        if (pid < 0)
            fail("fork");
        if (pid == 0) {
            char c;
            close(pfd[1]);
            if (read(pfd[0], &c, 1) != 1)
                _exit(9);
            close(pfd[0]);
            int dn = open("/dev/null", O_WRONLY);
            if (dn < 0 || dup2(dn, 1) < 0)
                _exit(8);
            char *argv[] = {"cbench_startup", mode, NULL};
            execve("/proc/self/exe", argv, envp);
            _exit(127);
        }
        close(pfd[0]);
        struct perf_event_attr a;
        memset(&a, 0, sizeof a);
        a.size = sizeof a;
        a.type = PERF_TYPE_HARDWARE;
        a.config = PERF_COUNT_HW_INSTRUCTIONS;
        a.disabled = 1;
        a.enable_on_exec = 1;
        a.exclude_kernel = 1;
        a.exclude_hv = 1;
        int fd = (int)syscall(SYS_perf_event_open, &a, pid, -1, -1, 0UL);
        int status;
        if (write(pfd[1], "x", 1) != 1)
            fail("pipe write");
        close(pfd[1]);
        if (waitpid(pid, &status, 0) != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
            fail(name);
        double ns = now() - t0;
        if (ns < best_ns)
            best_ns = ns;
        unsigned long long v = 0;
        if (fd >= 0 && read(fd, &v, sizeof v) == (ssize_t)sizeof v && v > 0 && (best_instr < 0 || (double)v < best_instr))
            best_instr = (double)v;
        if (fd >= 0)
            close(fd);
    }
    const char *on = getenv("CBENCH_COUNTERS");
    if (on && on[0] == '1') {
        char buf[160];
        int n = snprintf(buf, sizeof buf, "ctr\t%s\t%.3f\t-1\t-1\t-1\t%.3f\t-1\n", name, best_instr, best_instr);
        if (n > 0)
            (void)!write(2, buf, (size_t)n < sizeof buf ? (size_t)n : sizeof buf - 1);
    }
    return best_ns;
}

static long run_true(void) { return run("true", env_plain); }
static long run_echo(void) { return run("echo", env_plain); }
static long run_locale(void) { return run("locale", env_locale); }

int main(int argc, char **argv)
{
    if (argc > 1) {
        if (strcmp(argv[1], "true") == 0)
            return 0;
        if (strcmp(argv[1], "echo") == 0)
            return puts("hello, world") < 0;
        if (strcmp(argv[1], "locale") == 0) {
            if (setlocale(LC_ALL, "") == NULL)
                return 2;
            return printf("%s: %d\n", "hello, world", 1234567) < 0;
        }
        if (strcmp(argv[1], "setlocale") == 0)
            return setlocale(LC_ALL, "") == NULL ? 2 : 0;
        if (strcmp(argv[1], "printf") == 0) {
            if (setlocale(LC_ALL, "") == NULL)
                return 2;
            return printf("%s %d\n", "hello", 1234567) < 0;
        }
        if (strcmp(argv[1], "wctype") == 0) {
            if (setlocale(LC_ALL, "") == NULL)
                return 2;
            volatile wint_t c = 0xE9;
            return iswalpha(c) && towupper(c) == 0xC9 && wcwidth((wchar_t)c) == 1 ? 0 : 3;
        }
        return 4;
    }
    if (posix_spawn_file_actions_init(&actions) != 0 ||
        posix_spawn_file_actions_addopen(&actions, 1, "/dev/null", O_WRONLY, 0) != 0)
        fail("posix_spawn_file_actions");
    if (run_true() != 0)
        fail("child true");
    if (run_bin_true() != 0)
        fail("/bin/true");
    if (run_echo() != 0)
        fail("child echo");
    if (run_locale() != 0)
        fail("child locale (setlocale failed: en_US.UTF-8 installed?)");
    if (!cb_skip("startup_instr_c")) {
        double t = instr_row("startup_instr_c", "true", env_plain);
        put("startup_instr_c", t);
    }
    if (!cb_skip("startup_instr_setloc")) {
        double t = instr_row("startup_instr_setloc", "setlocale", env_locale);
        put("startup_instr_setloc", t);
    }
    if (!cb_skip("startup_instr_printf")) {
        double t = instr_row("startup_instr_printf", "printf", env_locale);
        put("startup_instr_printf", t);
    }
    if (!cb_skip("startup_instr_wctype")) {
        double t = instr_row("startup_instr_wctype", "wctype", env_locale);
        put("startup_instr_wctype", t);
    }
    MEASURE("spawn_bin_true", 300, run_bin_true());
    MEASURE("exec_true_static", 300, run_true());
    MEASURE("exec_true_echo", 300, run_echo());
    MEASURE("exec_hello_locale", 300, run_locale());
    return 0;
}
