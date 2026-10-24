/* Spec §18.3, §18.6, §18.7, §18.8, §18.9 in a process Liella started: the variadic exec forms
 * (execl, execle, execlp, also with more arguments than the forms keep on the stack), daemon,
 * the variadic prctl and ptrace (a PEEK of a word that is -1 leaves errno alone), and
 * cancellation of a thread waiting in waitpid and in system (the shell is killed and reaped,
 * SIGINT and SIGQUIT are restored). Failures are printed and counted; the exit status is the
 * count. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/ptrace.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

static int fails;
#define CHECK(c, what)                                                         \
    do {                                                                       \
        if (!(c)) {                                                            \
            fprintf(stderr, "FAIL line %d: %s\n", __LINE__, what);             \
            fails++;                                                           \
        }                                                                      \
    } while (0)

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

/* Run fn in a child with standard output on a pipe: what it printed, and its status. */
static char out[4096];
static int capture(void (*fn)(void))
{
    int p[2];
    if (pipe(p) != 0)
        return -1;
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        close(p[0]);
        dup2(p[1], 1);
        close(p[1]);
        fn();
        _exit(99);
    }
    close(p[1]);
    size_t n = 0;
    ssize_t k;
    while (n + 1 < sizeof out && (k = read(p[0], out + n, sizeof out - 1 - n)) > 0)
        n += (size_t)k;
    out[n] = 0;
    close(p[0]);
    int st = -1;
    waitpid(c, &st, 0);
    return st;
}

#define A10 "a", "a", "a", "a", "a", "a", "a", "a", "a", "a"
#define A100 A10, A10, A10, A10, A10, A10, A10, A10, A10, A10

static void l_plain(void) { execl("/bin/sh", "sh", "-c", "echo \"$0|$1|$2\"", "zero", "one", "two", (char *)NULL); }
static void l_env(void)
{
    char *env[] = {"X=le", NULL};
    execle("/bin/sh", "sh", "-c", "echo \"X=$X\"", (char *)NULL, env);
}
static void l_search(void)
{
    setenv("PATH", "/usr/bin:/bin", 1);
    execlp("sh", "sh", "-c", "echo found", (char *)NULL);
}
static void l_many(void) { execl("/bin/sh", "sh", "-c", "echo $#", "zero", A100, A100, A100, A100, A100, A100, (char *)NULL); }

static void exec_forms(void)
{
    CHECK(capture(l_plain) == 0 && strcmp(out, "zero|one|two\n") == 0, "execl passes its list");
    CHECK(capture(l_env) == 0 && strcmp(out, "X=le\n") == 0, "execle passes the environment after the NULL");
    CHECK(capture(l_search) == 0 && strcmp(out, "found\n") == 0, "execlp searches PATH");
    CHECK(capture(l_many) == 0 && strcmp(out, "600\n") == 0, "execl with 600 arguments");
    errno = 0;
    CHECK(execl("/nonexistent/program", "x", (char *)NULL) == -1 && errno == ENOENT, "a failed execl returns ENOENT");
}

static void daemons(void)
{
    prctl(PR_SET_CHILD_SUBREAPER, 1);
    int p[2];
    if (pipe(p) != 0)
        return;
    pid_t d = fork();
    if (d == 0) {
        close(p[0]);
        dup2(p[1], 9);
        close(p[1]);
        pid_t me = getpid();
        int r = daemon(0, 0);
        char cwd[64] = "";
        ssize_t n = readlink("/proc/self/cwd", cwd, sizeof cwd - 1);
        cwd[n > 0 ? n : 0] = 0;
        struct stat null, f;
        stat("/dev/null", &null);
        int nulls = 0;
        for (int fd = 0; fd < 3; fd++)
            if (fstat(fd, &f) == 0 && S_ISCHR(f.st_mode) && f.st_rdev == null.st_rdev)
                nulls++;
        dprintf(9, "%d %d %d %d %s %d\n", r, getpid() != me, getsid(0) == getpid(), getpgrp() == getpid(), cwd, nulls);
        _exit(0);
    }
    close(p[1]);
    int st = -1;
    waitpid(d, &st, 0);
    CHECK(WIFEXITED(st) && WEXITSTATUS(st) == 0, "the calling process exits 0");
    char buf[128] = "";
    ssize_t n = 0, k;
    while ((k = read(p[0], buf + n, sizeof buf - 1 - (size_t)n)) > 0)
        n += k;
    buf[n > 0 ? n : 0] = 0;
    close(p[0]);
    CHECK(strcmp(buf, "0 1 1 1 / 3\n") == 0, "daemon: new process, session and group leader, /, 0-2 on /dev/null");
    CHECK(waitpid(-1, &st, 0) > 0, "the daemon is reparented here and reaped");
}

static long minus_one = -1;

static void attributes(void)
{
    char name[16] = "";
    CHECK(prctl(PR_SET_NAME, "abcdefghijklmnopqrs") == 0, "prctl PR_SET_NAME");
    CHECK(prctl(PR_GET_NAME, name) == 0 && strcmp(name, "abcdefghijklmno") == 0, "prctl PR_GET_NAME: 15 bytes");
    CHECK(prctl(PR_GET_DUMPABLE) == 1, "prctl returns the value");
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        ptrace(PTRACE_TRACEME, 0, NULL, NULL);
        raise(SIGSTOP);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
    errno = 0;
    long w = ptrace(PTRACE_PEEKDATA, c, &minus_one, NULL);
    CHECK(w == -1 && errno == 0, "PEEKDATA of a -1 word leaves errno 0");
    errno = 0;
    w = ptrace(PTRACE_PEEKDATA, c, (void *)8, NULL);
    CHECK(w == -1 && errno == EIO, "PEEKDATA of a bad address: EIO");
    ptrace(PTRACE_CONT, c, NULL, NULL);
    waitpid(c, &st, 0);
    CHECK(WIFEXITED(st), "the traced child continues");
}

static pid_t sleeper;
static void *wait_thread(void *arg)
{
    (void)arg;
    waitpid(sleeper, NULL, 0);
    return (void *)1;
}
static void *system_thread(void *arg)
{
    (void)arg;
    system("exec sleep 30");
    return (void *)1;
}

static void cancellation(void)
{
    char *argv[] = {"sleep", "30", NULL};
    CHECK(posix_spawn(&sleeper, "/bin/sleep", NULL, NULL, argv, environ) == 0, "spawn a sleeper");
    pthread_t t;
    void *r = NULL;
    pthread_create(&t, NULL, wait_thread, NULL);
    sleep_ms(100);
    pthread_cancel(t);
    pthread_join(t, &r);
    CHECK(r == PTHREAD_CANCELED, "waitpid is a cancellation point");
    kill(sleeper, SIGKILL);
    CHECK(waitpid(sleeper, NULL, 0) == sleeper, "the sleeper is still a child");
    /* system restores the dispositions it found, so start from known ones: a job started in the
     * background of a non-interactive shell (jobrun, a CI loop) inherits SIGINT and SIGQUIT
     * ignored, and "restored" would then rightly be SIG_IGN. */
    signal(SIGINT, SIG_DFL);
    signal(SIGQUIT, SIG_DFL);
    pthread_create(&t, NULL, system_thread, NULL);
    sleep_ms(200);
    pthread_cancel(t);
    pthread_join(t, &r);
    CHECK(r == PTHREAD_CANCELED, "system is a cancellation point");
    struct sigaction sa;
    sigaction(SIGINT, NULL, &sa);
    CHECK(sa.sa_handler == SIG_DFL, "SIGINT restored after the cancelled system");
    sigaction(SIGQUIT, NULL, &sa);
    CHECK(sa.sa_handler == SIG_DFL, "SIGQUIT restored after the cancelled system");
    errno = 0;
    CHECK(waitpid(-1, NULL, WNOHANG) == -1 && errno == ECHILD, "the cancelled system's shell was reaped");
}

int main(void)
{
    exec_forms();
    attributes();
    cancellation();
    daemons();
    return fails;
}
