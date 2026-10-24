/* Processes end to end (spec §18.3-18.9; phase 11): the exec family, posix_spawn and pidfd_spawn
 * with every attribute and file action, process descriptors, waiting and the status encoding,
 * system and popen, process groups and sessions, daemon, and the prctl/personality/ptrace
 * conventions. Built against the host C library and against Liella's products, the outputs must
 * match line for line. Black-box: every line is a result, an errno, a wait status, or a fact a
 * child reports about itself, stated relative to the probe (no PIDs, times or addresses).
 *
 * Usage: process_diff DIR [root|noshell], normally through process_run.sh. DIR is a scratch
 * directory (its path is printed as <dir>). Without a second argument the unprivileged cases run
 * and the fixture is written into DIR. `root` runs the cases that need privileges (RESETIDS, a
 * real-time policy) and `noshell` the cases where /bin/sh cannot be executed; the harness runs
 * both in private mount namespaces. The program runs copies of itself as children: a first
 * argument starting with '@' selects a helper mode (see helper()). */
#define _GNU_SOURCE
/* The descriptors the popen shell itself holds, listed by the shell without a pipeline: `ls /proc/$$/fd | ...` raced with
 * the shell's own pipe ends (a shell delayed on a loaded machine still held them while ls read the directory: extra
 * fds 3-5, seen in either library). The glob's own directory descriptor shows as the same extra number on both sides. */
#define FDS_OF_SHELL "set -- /proc/$$/fd/*; for f; do printf '%s ' \"${f##*/}\"; done"
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/personality.h>
#include <sys/pidfd.h>
#include <sys/prctl.h>
#include <sys/ptrace.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "tscale.h"

extern char **environ;

static char dir[PATH_MAX];
static char self[1024];

static void out(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    fflush(stdout);
}

/* The result of a call and the errno it left (errno read after the call). */
#define SHOW(label, expr)                                  \
    do {                                                   \
        errno = 0;                                         \
        long r_ = (long)(expr);                            \
        int e_ = errno;                                    \
        out("%s: %ld errno=%d\n", label, r_, e_);          \
    } while (0)
/* A result computed by the previous statement, with the errno it left. */
#define SHOWV(label, v) out("%s: %ld errno=%d\n", label, (long)(v), errno)

static const char *P(const char *name)
{
    static char p[8][PATH_MAX + 64];
    static int k;
    k = (k + 1) % 8;
    snprintf(p[k], sizeof p[k], "%s/%s", dir, name);
    return p[k];
}

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static void wfile(const char *name, const char *text, mode_t mode)
{
    int fd = open(P(name), O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd >= 0) {
        if (write(fd, text, strlen(text)) < 0) {
        }
        close(fd);
    }
    chmod(P(name), mode);
}

/* Replace every occurrence of the scratch directory with <dir>. */
static void norm(char *dst, size_t n, const char *src, const char *d)
{
    size_t dl = d ? strlen(d) : 0, o = 0;
    while (*src && o + 6 < n) {
        if (dl && strncmp(src, d, dl) == 0) {
            memcpy(dst + o, "<dir>", 5);
            o += 5;
            src += dl;
        } else {
            dst[o++] = *src++;
        }
    }
    dst[o] = 0;
}

static const char *stat_str(int st)
{
    static char b[4][96];
    static int k;
    k = (k + 1) % 4;
    if (WIFEXITED(st))
        snprintf(b[k], sizeof b[k], "exit %d raw=0x%x", WEXITSTATUS(st), (unsigned)st);
    else if (WIFSIGNALED(st))
        snprintf(b[k], sizeof b[k], "signal %d core=%d raw=0x%x", WTERMSIG(st), WCOREDUMP(st) ? 1 : 0, (unsigned)st);
    else if (WIFSTOPPED(st))
        snprintf(b[k], sizeof b[k], "stopped %d raw=0x%x", WSTOPSIG(st), (unsigned)st);
    else if (WIFCONTINUED(st))
        snprintf(b[k], sizeof b[k], "continued raw=0x%x", (unsigned)st);
    else
        snprintf(b[k], sizeof b[k], "other raw=0x%x", (unsigned)st);
    return b[k];
}

static int lowest_free_fd(void)
{
    int fd = open("/dev/null", O_RDONLY);
    if (fd >= 0)
        close(fd);
    return fd;
}

static volatile sig_atomic_t counts[65];
static void count_sig(int s) { counts[s]++; }

static void child_guard(void)
{
    prctl(PR_SET_PDEATHSIG, SIGKILL);
    tw_alarm(30);
}

/* ------------------------------------------------------------------ helper modes */

static void siglist(char *b, size_t n, const sigset_t *s)
{
    size_t o = 0;
    b[o++] = '[';
    for (int i = 1; i <= 64 && o + 8 < n; i++)
        if (sigismember(s, i) == 1)
            o += (size_t)snprintf(b + o, n - o, "%s%d", b[o - 1] == '[' ? "" : ",", i);
    b[o++] = ']';
    b[o] = 0;
}

static const char *rel(pid_t v, const char *envname)
{
    const char *e = getenv(envname);
    return e && atol(e) == (long)v ? "yes" : "no";
}

static int report(void)
{
    sigset_t m, p, ign, caught;
    sigprocmask(SIG_BLOCK, NULL, &m);
    sigpending(&p);
    sigemptyset(&ign);
    sigemptyset(&caught);
    for (int i = 1; i <= 64; i++) {
        struct sigaction sa;
        if (sigaction(i, NULL, &sa) == 0) {
            if (sa.sa_handler == SIG_IGN)
                sigaddset(&ign, i);
            else if (sa.sa_handler != SIG_DFL)
                sigaddset(&caught, i);
        }
    }
    char bm[256], bp[256], bi[256], bc[256], fds[256], cwd[1024], ncwd[1024];
    siglist(bm, sizeof bm, &m);
    siglist(bp, sizeof bp, &p);
    siglist(bi, sizeof bi, &ign);
    siglist(bc, sizeof bc, &caught);
    size_t o = 0;
    fds[o++] = '[';
    for (int fd = 0; fd < 64; fd++)
        if (fcntl(fd, F_GETFD) >= 0)
            o += (size_t)snprintf(fds + o, sizeof fds - o, "%s%d", fds[o - 1] == '[' ? "" : ",", fd);
    fds[o++] = ']';
    fds[o] = 0;
    if (!getcwd(cwd, sizeof cwd))
        snprintf(cwd, sizeof cwd, "(errno %d)", errno);
    norm(ncwd, sizeof ncwd, cwd, getenv("PROBE_DIR"));
    pid_t me = getpid(), pg = getpgrp(), sid = getsid(0);
    const char *pgs = pg == me ? "own" : strcmp(rel(pg, "PROBE_PPGID"), "yes") == 0 ? "parent" : strcmp(rel(pg, "PROBE_XPGID"), "yes") == 0 ? "x" : "other";
    const char *sids = sid == me ? "own" : strcmp(rel(sid, "PROBE_PSID"), "yes") == 0 ? "parent" : "other";
    char ids[128];
    if (getenv("PROBE_SHOWIDS"))
        snprintf(ids, sizeof ids, "r=%d e=%d rg=%d eg=%d", (int)getuid(), (int)geteuid(), (int)getgid(), (int)getegid());
    else
        snprintf(ids, sizeof ids, "%s", getuid() == geteuid() && getgid() == getegid() ? "same" : "differ");
    struct sched_param sp = {0};
    sched_getparam(0, &sp);
    printf("  report: blocked=%s pending=%s ignored=%s caught=%s fds=%s cwd=%s pgid=%s sid=%s ids=%s sched=%d/%d", bm, bp, bi, bc, fds, ncwd, pgs, sids, ids, sched_getscheduler(0),
           sp.sched_priority);
    if (getenv("PROBE_EXPID"))
        printf(" same_pid=%s", rel(me, "PROBE_EXPID"));
    printf("\n");
    fflush(stdout);
    return 0;
}

static int helper(int argc, char **argv)
{
    const char *m = argv[1];
    if (strcmp(m, "@true") == 0)
        return 0;
    if (strcmp(m, "@exit") == 0)
        return argc > 2 ? atoi(argv[2]) : 0;
    if (strcmp(m, "@args") == 0) {
        printf("  args: argc=%d", argc);
        for (int i = 0; i < argc; i++)
            printf(" [%s]", argv[i]);
        printf(" X=%s\n", getenv("X") ? getenv("X") : "(unset)");
        fflush(stdout);
        return 0;
    }
    if (strcmp(m, "@report") == 0)
        return report();
    if (strcmp(m, "@sleep") == 0) {
        sleep_ms(argc > 2 ? atol(argv[2]) : 1000);
        return 0;
    }
    if (strcmp(m, "@ready") == 0) { /* @ready FD MS: say ready on FD, then sleep */
        int fd = atoi(argv[2]);
        if (write(fd, "r", 1) < 0) {
        }
        close(fd);
        sleep_ms(atol(argv[3]));
        return 0;
    }
    if (strcmp(m, "@fg") == 0) { /* @fg FD: is our group the terminal's foreground group */
        int fd = atoi(argv[2]);
        errno = 0;
        pid_t f = tcgetpgrp(fd);
        printf("  fg: tcgetpgrp=%s errno=%d own_group_leader=%s\n", f == getpgrp() ? "own-group" : f < 0 ? "-1" : "other", errno, getpgrp() == getpid() ? "yes" : "no");
        fflush(stdout);
        return 0;
    }
    fprintf(stderr, "unknown helper %s\n", m);
    return 100;
}

/* ------------------------------------------------------------------ capture runner */

/* Fork; in the child, stdout and stderr go to a pipe and fn runs (it execs, or returns after a
 * failure, whose errno is printed). The parent prints the child's lines with the scratch
 * directory as <dir>, then the wait status. */
static void capture(const char *label, void (*fn)(void *), void *arg)
{
    int p[2];
    if (pipe(p) != 0)
        return;
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        child_guard();
        close(p[0]);
        dup2(p[1], 1);
        dup2(p[1], 2);
        if (p[1] > 2)
            close(p[1]);
        errno = 0;
        fn(arg);
        printf("returned errno=%d\n", errno);
        fflush(stdout);
        _exit(99);
    }
    close(p[1]);
    size_t cap = 4096, len = 0;
    char *buf = malloc(cap);
    for (;;) {
        if (len + 1024 > cap)
            buf = realloc(buf, cap *= 2);
        ssize_t n = read(p[0], buf + len, cap - len - 1);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0)
            break;
        len += (size_t)n;
    }
    buf[len] = 0;
    close(p[0]);
    int st = 0;
    waitpid(pid, &st, 0);
    char *s = buf;
    while (*s) {
        char *e = strchr(s, '\n');
        if (e)
            *e = 0;
        char nl[4096];
        norm(nl, sizeof nl, s, dir);
        out("%s: | %s\n", label, nl);
        if (!e)
            break;
        s = e + 1;
    }
    free(buf);
    out("%s: %s\n", label, stat_str(st));
}

/* ------------------------------------------------------------------ fixture */

static void fixture(void)
{
    static const char *dirs[] = {"bin_noexec", "bin_ok", "bin_ok2", "bin_plain", "bin_dir", "bin_dir/prog", "bin_loop", "bin_badinterp", "bin_busy", "sub", "empty"};
    for (size_t i = 0; i < sizeof dirs / sizeof *dirs; i++)
        mkdir(P(dirs[i]), 0755);
    /* The scripts print $0 with the scratch directory as <dir> (PROBE_DIR, when the environment
     * has it; the capture runner normalises the rest). */
#define A0 "a=$0; if [ -n \"${PROBE_DIR-}\" ]; then case $a in \"$PROBE_DIR\"/*) a=\"<dir>/${a#\"$PROBE_DIR\"/}\";; esac; fi; "
    wfile("bin_noexec/prog", "#!/bin/sh\necho noexec\n", 0644);
    wfile("bin_ok/prog", "#!/bin/sh\n" A0 "echo \"ok argv0=$a args=$* X=${X-unset}\"\n", 0755);
    wfile("bin_ok2/prog", "#!/bin/sh\n" A0 "echo \"ok2 argv0=$a args=$*\"\n", 0755);
    wfile("bin_plain/plain", A0 "echo \"plain argv0=$a argc=$# args=$* X=${X-unset}\"\n", 0755);
    wfile("bin_plain/prog", A0 "echo \"plain-prog argv0=$a args=$*\"\n", 0755);
    if (symlink("prog", P("bin_loop/prog")) != 0)
        out("symlink failed\n");
    wfile("bin_badinterp/prog", "#!/nonexistent/interp\necho bad\n", 0755);
    wfile("bin_busy/prog", "#!/bin/sh\necho busy\n", 0755);
    wfile("notadir", "x\n", 0644);
    wfile("afile", "afile\n", 0644);
    wfile("sub/insub", "insub\n", 0644);
    wfile("mode000", "secret\n", 0000);
    wfile("notashell", "not a shell\n", 0644);
    wfile("script", "#!/bin/sh\necho \"script argv0=$0 args=$*\"\n", 0755);
    wfile("cwdprog", "#!/bin/sh\necho cwdprog\n", 0755);
    if (symlink(self, P("selflink")) != 0)
        out("symlink failed\n");
}

/* ------------------------------------------------------------------ §18.3 exec */

enum { K_EXECV, K_EXECVP, K_EXECVE, K_EXECVPE, K_EXECL, K_EXECLP, K_EXECLE };

struct ex {
    const char *label;
    int kind;
    const char *file;     /* '=' prefix: literal; else for non-p forms relative to DIR */
    const char *pathspec; /* PATH: elements relative to DIR unless '/' or '=' prefixed */
    int path_mode;        /* 0 set pathspec, 1 unset, 2 leave */
    const char *cwd;      /* NULL: stay; "": DIR; else DIR/cwd */
    const char *argv[5];
    const char *x;    /* X in environ */
    const char *envp; /* for the e forms: "X=...;PATH-spec" */
};

static char *mkpath(const char *spec)
{
    static char buf[4][8192];
    static int k;
    k = (k + 1) % 4;
    char *o = buf[k];
    o[0] = 0;
    const char *s = spec;
    for (;;) {
        const char *e = strchr(s, ':');
        size_t n = e ? (size_t)(e - s) : strlen(s);
        if (n > 0 && s[0] == '=') {
            s++;
            n--;
        } else if (n > 0 && s[0] != '/') {
            strcat(o, dir);
            strcat(o, "/");
        }
        strncat(o, s, n);
        if (!e)
            break;
        strcat(o, ":");
        s = e + 1;
    }
    return o;
}

static void ex_child(void *arg)
{
    struct ex *x = arg;
    if (x->cwd && chdir(x->cwd[0] ? P(x->cwd) : dir) != 0)
        printf("chdir failed\n");
    if (x->path_mode == 1)
        unsetenv("PATH");
    else if (x->path_mode == 0)
        setenv("PATH", mkpath(x->pathspec), 1);
    if (x->x)
        setenv("X", x->x, 1);
    const char *file = x->file;
    if (file[0] == '=')
        file++;
    else if (x->kind == K_EXECV || x->kind == K_EXECVE || x->kind == K_EXECL || x->kind == K_EXECLE)
        file = P(file);
    char *argv[5] = {0};
    for (int i = 0; i < 4 && x->argv[i]; i++)
        argv[i] = (char *)x->argv[i];
    char envx[256] = "X=from-envp", envpath[8192] = "PATH=/nonexistent-envp";
    char *envp[3] = {envx, envpath, NULL};
    if (x->envp) {
        snprintf(envpath, sizeof envpath, "PATH=%s", mkpath(x->envp));
    }
    errno = 0;
    switch (x->kind) {
    case K_EXECV:
        execv(file, argv);
        break;
    case K_EXECVP:
        execvp(file, argv);
        break;
    case K_EXECVE:
        execve(file, argv, envp);
        break;
    case K_EXECVPE:
        execvpe(file, argv, envp);
        break;
    case K_EXECL:
        execl(file, "l0", "l1", "l2", (char *)NULL);
        break;
    case K_EXECLP:
        execlp(file, "lp0", "lp1", (char *)NULL);
        break;
    case K_EXECLE:
        execle(file, "le0", "le1", (char *)NULL, envp);
        break;
    }
}

static void exec_search(void)
{
    static char longname[300], longdir[4200];
    memset(longname, 'n', sizeof longname - 1);
    memset(longdir, 0, sizeof longdir);
    longdir[0] = '=';
    longdir[1] = '/';
    memset(longdir + 2, 'd', 4090);
    char longpath[8300];
    snprintf(longpath, sizeof longpath, "%s:bin_ok", longdir);
    /* EXECVP-PATH: the search reuses one candidate buffer, so a long missing element before a
       short one must leave nothing behind; and a long PATH (150 missing elements) still
       remembers EACCES and reaches the last element. */
    static char longmiss[300], manymiss[4400], many_noexec_ok[4400 + 64], many_noexec[4400 + 64];
    memset(longmiss, 'm', sizeof longmiss - 1);
    memcpy(longmiss, "/nonexistent/", 13);
    longmiss[13 + 240] = 0;
    char longmiss_ok[400];
    snprintf(longmiss_ok, sizeof longmiss_ok, "%s:bin_ok", longmiss);
    for (int i = 0; i < 150; i++)
        snprintf(manymiss + strlen(manymiss), sizeof manymiss - strlen(manymiss), "/nonexistent/stress/dir%03d:", i);
    snprintf(many_noexec_ok, sizeof many_noexec_ok, "%sbin_noexec:bin_ok", manymiss);
    snprintf(many_noexec, sizeof many_noexec, "%sbin_noexec", manymiss);
    struct ex t[] = {
        {"execvp EACCES then ok", K_EXECVP, "prog", "bin_noexec:bin_ok", 0, NULL, {"prog", "a1", NULL}, NULL, NULL},
        {"execvp EACCES only", K_EXECVP, "prog", "bin_noexec", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp EACCES then missing dir", K_EXECVP, "prog", "bin_noexec:nonexistent", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp missing dir then EACCES", K_EXECVP, "prog", "nonexistent:bin_noexec", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp not found anywhere", K_EXECVP, "prog", "nonexistent:empty", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp directory named prog then ok", K_EXECVP, "prog", "bin_dir:bin_ok", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp directory named prog only", K_EXECVP, "prog", "bin_dir", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp symlink loop then ok", K_EXECVP, "prog", "bin_loop:bin_ok", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp symlink loop only", K_EXECVP, "prog", "bin_loop", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp missing interpreter then ok", K_EXECVP, "prog", "bin_badinterp:bin_ok", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp missing interpreter only", K_EXECVP, "prog", "bin_badinterp", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp file as PATH element then ok", K_EXECVP, "prog", "notadir:bin_ok", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp file as PATH element only", K_EXECVP, "prog", "notadir", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp ENOEXEC shell fallback", K_EXECVP, "plain", "bin_plain", 0, NULL, {"plain", "a", "b c", NULL}, "1", NULL},
        {"execvp ENOEXEC fallback argv0 differs", K_EXECVP, "plain", "bin_plain", 0, NULL, {"alias", "a", NULL}, NULL, NULL},
        {"execvp ENOEXEC fallback empty argv", K_EXECVP, "plain", "bin_plain", 0, NULL, {NULL}, NULL, NULL},
        {"execvp ENOEXEC first then ok", K_EXECVP, "prog", "bin_plain:bin_ok", 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp ENOEXEC with slash", K_EXECVP, "bin_plain/plain", "/nonexistent", 0, "", {"plain", "z", NULL}, NULL, NULL},
        {"execv ENOEXEC no fallback", K_EXECV, "bin_plain/plain", "bin_plain", 0, NULL, {"plain", NULL}, NULL, NULL},
        {"execve ENOEXEC no fallback", K_EXECVE, "bin_plain/plain", "bin_plain", 0, NULL, {"plain", NULL}, NULL, NULL},
        {"execvpe ENOEXEC fallback env from envp", K_EXECVPE, "plain", "bin_plain", 0, NULL, {"plain", NULL}, "from-environ", "bin_ok"},
        {"execvpe searches caller PATH not envp", K_EXECVPE, "prog", "bin_ok", 0, NULL, {"prog", NULL}, NULL, "bin_ok2"},
        {"execvpe caller PATH unset envp PATH set", K_EXECVPE, "prog", "", 1, NULL, {"prog", NULL}, NULL, "bin_ok2"},
        {"execvp PATH unset finds sh", K_EXECVP, "sh", "", 1, NULL, {"sh", "-c", "echo default-path-found", NULL}, NULL, NULL},
        {"execvp PATH unset cwd not searched", K_EXECVP, "cwdprog", "", 1, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH empty string", K_EXECVP, "cwdprog", "", 0, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH leading empty", K_EXECVP, "cwdprog", ":/nonexistent", 0, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH trailing empty", K_EXECVP, "cwdprog", "/nonexistent:", 0, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH middle empty", K_EXECVP, "cwdprog", "/nonexistent::/nonexistent2", 0, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH dot element", K_EXECVP, "cwdprog", "=.", 0, "", {"cwdprog", NULL}, NULL, NULL},
        {"execvp PATH relative element", K_EXECVP, "prog", "=bin_ok", 0, "", {"prog", NULL}, NULL, NULL},
        {"execvp name with slash ignores PATH", K_EXECVP, "bin_ok/prog", "/nonexistent", 0, "", {"prog", NULL}, NULL, NULL},
        {"execvp name with slash missing", K_EXECVP, "bin_ok/nothere", "bin_ok", 0, "", {"x", NULL}, NULL, NULL},
        {"execvp empty name", K_EXECVP, "", "bin_ok", 0, "", {"x", NULL}, NULL, NULL},
        {"execvp name longer than NAME_MAX", K_EXECVP, longname, "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execvp PATH element longer than PATH_MAX then ok", K_EXECVP, "prog", longpath, 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp long missing element then short ok", K_EXECVP, "prog", longmiss_ok, 0, NULL, {"prog", "a1", NULL}, NULL, NULL},
        {"execvp 150 missing elements then EACCES then ok", K_EXECVP, "prog", many_noexec_ok, 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execvp 150 missing elements then EACCES only", K_EXECVP, "prog", many_noexec, 0, NULL, {"prog", NULL}, NULL, NULL},
        {"execv relative name without slash", K_EXECV, "=prog", "/nonexistent", 0, "bin_ok", {"prog", NULL}, NULL, NULL},
        {"execv name without slash not searched", K_EXECV, "=prog", "bin_ok", 0, "empty", {"prog", NULL}, NULL, NULL},
        {"execv empty path", K_EXECV, "=", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv EACCES", K_EXECV, "bin_noexec/prog", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv ENOENT", K_EXECV, "bin_ok/nothere", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv ENOTDIR", K_EXECV, "notadir/prog", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv ELOOP", K_EXECV, "bin_loop/prog", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv directory", K_EXECV, "bin_dir/prog", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv missing interpreter", K_EXECV, "bin_badinterp/prog", "bin_ok", 0, NULL, {"x", NULL}, NULL, NULL},
        {"execv NULL argv[0] list", K_EXECV, "bin_ok/prog", "bin_ok", 0, NULL, {NULL}, NULL, NULL},
        {"execl", K_EXECL, "bin_ok/prog", "bin_ok", 0, NULL, {NULL}, NULL, NULL},
        {"execlp search", K_EXECLP, "prog", "bin_noexec:bin_ok", 0, NULL, {NULL}, NULL, NULL},
        {"execlp ENOEXEC fallback", K_EXECLP, "plain", "bin_plain", 0, NULL, {NULL}, NULL, NULL},
        {"execle env", K_EXECLE, "bin_ok/prog", "bin_ok", 0, NULL, {NULL}, "from-environ", NULL},
        {"execle ENOEXEC no fallback", K_EXECLE, "bin_plain/plain", "bin_ok", 0, NULL, {NULL}, NULL, NULL},
    };
    for (size_t i = 0; i < sizeof t / sizeof *t; i++)
        capture(t[i].label, ex_child, &t[i]);
}

static void busy_child(void *arg)
{
    (void)arg;
    int w = open(P("bin_busy/prog"), O_WRONLY);
    if (w < 0)
        printf("open for write failed %d\n", errno);
    setenv("PATH", mkpath("bin_busy:bin_ok"), 1);
    char *argv[] = {"prog", NULL};
    errno = 0;
    execvp("prog", argv);
}

static void busy_only_child(void *arg)
{
    (void)arg;
    int w = open(P("bin_busy/prog"), O_WRONLY);
    if (w < 0)
        printf("open for write failed %d\n", errno);
    char *argv[] = {"prog", NULL};
    errno = 0;
    execv(P("bin_busy/prog"), argv);
}

/* E2BIG: one string at and over MAX_ARG_STRLEN, totals against a quarter of RLIMIT_STACK. */
struct big {
    size_t one;      /* length of one argument (0: none) */
    int count;       /* number of small arguments */
    size_t each;     /* their length */
    int in_env;      /* small strings in the environment instead */
    rlim_t stack;    /* RLIMIT_STACK to set (0: leave) */
    int use_p;       /* execvp with PATH=bin_ok:bin_ok2 and a script name */
};

static void big_child(void *arg)
{
    struct big *b = arg;
    if (b->stack) {
        struct rlimit rl = {b->stack, b->stack};
        getrlimit(RLIMIT_STACK, &rl);
        rl.rlim_cur = b->stack;
        if (setrlimit(RLIMIT_STACK, &rl) != 0)
            printf("setrlimit failed %d\n", errno);
    }
    int n = 3 + b->count + 1;
    char **argv = calloc((size_t)n, sizeof *argv);
    char **envp = calloc((size_t)b->count + 2, sizeof *envp);
    int a = 0, e = 0;
    argv[a++] = "self";
    argv[a++] = "@true";
    if (b->one) {
        char *s = malloc(b->one + 1);
        memset(s, 'a', b->one);
        s[b->one] = 0;
        argv[a++] = s;
    }
    for (int i = 0; i < b->count; i++) {
        char *s = malloc(b->each + 1);
        memset(s, 'b', b->each);
        s[b->each] = 0;
        if (b->in_env) {
            s[0] = 'E';
            s[1] = '=';
            envp[e++] = s;
        } else {
            argv[a++] = s;
        }
    }
    errno = 0;
    if (b->use_p) {
        setenv("PATH", mkpath("bin_ok:bin_ok2"), 1);
        argv[0] = "prog";
        execvp("prog", argv);
    } else {
        execve(self, argv, envp);
    }
}

static void exec_limits(void)
{
    struct big t[] = {
        {131071, 0, 0, 0, 0, 0}, /* strlen + NUL = 32 pages */
        {131072, 0, 0, 0, 0, 0},
        {0, 200, 1000, 0, 1 << 20, 0},  /* 200 KB under a 1 MiB stack */
        {0, 300, 1000, 0, 1 << 20, 0},  /* 300 KB under a 1 MiB stack */
        {0, 300, 1000, 1, 1 << 20, 0},  /* the same in the environment */
        {0, 15, 100000, 0, 8 << 20, 0}, /* 1.5 MB under 8 MiB */
        {0, 25, 100000, 0, 8 << 20, 0}, /* 2.5 MB under 8 MiB */
        {0, 100, 1000, 0, 256 << 10, 0}, /* 100 KB under a 256 KiB stack (quarter is 64 KB) */
        {131072, 0, 0, 0, 0, 1},        /* execvp returns E2BIG */
    };
    const char *labels[] = {"E2BIG one string 131071", "E2BIG one string 131072", "E2BIG 200KB stack 1MiB", "E2BIG 300KB stack 1MiB", "E2BIG 300KB env stack 1MiB", "E2BIG 1.5MB stack 8MiB",
                            "E2BIG 2.5MB stack 8MiB", "E2BIG 100KB stack 256KiB", "E2BIG execvp"};
    for (size_t i = 0; i < sizeof t / sizeof *t; i++)
        capture(labels[i], big_child, &t[i]);

    /* sysconf(_SC_ARG_MAX) follows RLIMIT_STACK. */
    rlim_t stacks[] = {8 << 20, 1 << 20, 256 << 10, 64 << 10, RLIM_INFINITY};
    for (size_t i = 0; i < sizeof stacks / sizeof *stacks; i++) {
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
            struct rlimit rl;
            getrlimit(RLIMIT_STACK, &rl);
            rl.rlim_cur = stacks[i];
            if (stacks[i] == RLIM_INFINITY && rl.rlim_max != RLIM_INFINITY) {
                printf("sysconf(_SC_ARG_MAX) stack=inf: hard limit not infinite\n");
                _exit(0);
            }
            setrlimit(RLIMIT_STACK, &rl);
            long v = sysconf(_SC_ARG_MAX);
            if (stacks[i] == RLIM_INFINITY)
                printf("sysconf(_SC_ARG_MAX) stack=inf: %ld\n", v);
            else
                printf("sysconf(_SC_ARG_MAX) stack=%lu: %ld (stack/4=%lu)\n", (unsigned long)stacks[i], v, (unsigned long)stacks[i] / 4);
            fflush(stdout);
            _exit(0);
        }
        waitpid(pid, NULL, 0);
    }
}

/* fexecve and execveat. */
struct fx {
    int kind;
};

static void fx_child(void *arg)
{
    int k = ((struct fx *)arg)->kind;
    char *argv[] = {"fx0", "@args", "fx2", NULL};
    char *sargv[] = {"s0", "s1", NULL};
    int fd;
    errno = 0;
    switch (k) {
    case 0: /* fexecve of the program itself */
        fd = open(self, O_RDONLY);
        fexecve(fd, argv, environ);
        break;
    case 1: /* through an O_PATH descriptor */
        fd = open(self, O_PATH);
        fexecve(fd, argv, environ);
        break;
    case 2: /* a script, descriptor without close-on-exec, as fd 10 */
        fd = open(P("script"), O_RDONLY);
        dup2(fd, 10);
        close(fd);
        fexecve(10, sargv, environ);
        break;
    case 3: /* a script whose descriptor is close-on-exec */
        fd = open(P("script"), O_RDONLY | O_CLOEXEC);
        dup2(fd, 10);
        fcntl(10, F_SETFD, FD_CLOEXEC);
        fexecve(10, sargv, environ);
        break;
    case 4: /* not executable */
        fd = open(P("afile"), O_RDONLY);
        fexecve(fd, argv, environ);
        break;
    case 5:
        fexecve(-1, argv, environ);
        break;
    case 6:
        fexecve(50, argv, environ);
        break;
    case 7: /* a directory */
        fd = open(dir, O_RDONLY | O_DIRECTORY);
        fexecve(fd, argv, environ);
        break;
    case 8: /* no #! line: no shell fallback */
        fd = open(P("bin_plain/plain"), O_RDONLY);
        fexecve(fd, sargv, environ);
        break;
    case 9: /* execveat relative to a directory descriptor */
        fd = open(P("bin_ok"), O_RDONLY | O_DIRECTORY);
        execveat(fd, "prog", sargv, environ, 0);
        break;
    case 10: /* empty path without AT_EMPTY_PATH */
        fd = open(self, O_RDONLY);
        execveat(fd, "", argv, environ, 0);
        break;
    case 11: /* empty path with AT_EMPTY_PATH */
        fd = open(self, O_RDONLY);
        execveat(fd, "", argv, environ, AT_EMPTY_PATH);
        break;
    case 12: /* AT_SYMLINK_NOFOLLOW on a symbolic link */
        fd = open(dir, O_RDONLY | O_DIRECTORY);
        execveat(fd, "selflink", argv, environ, AT_SYMLINK_NOFOLLOW);
        break;
    case 13: /* the same link followed */
        fd = open(dir, O_RDONLY | O_DIRECTORY);
        execveat(fd, "selflink", argv, environ, 0);
        break;
    case 14: /* unknown flag */
        execveat(AT_FDCWD, self, argv, environ, 0x1);
        break;
    case 15: /* bad descriptor with a relative path */
        execveat(-5, "prog", argv, environ, 0);
        break;
    case 16: /* bad descriptor with an absolute path */
        execveat(-5, self, argv, environ, 0);
        break;
    case 17: /* AT_FDCWD relative */
        if (chdir(P("bin_ok")) == 0)
            execveat(AT_FDCWD, "prog", sargv, environ, 0);
        break;
    case 18: /* fexecve with an explicit environment */
    {
        char *envp[] = {"X=fexecve-env", NULL};
        fd = open(self, O_RDONLY);
        fexecve(fd, argv, envp);
        break;
    }
    }
}

static void exec_fd(void)
{
    const char *labels[] = {"fexecve self", "fexecve O_PATH", "fexecve script fd 10", "fexecve script cloexec", "fexecve not executable", "fexecve -1", "fexecve closed fd",
                            "fexecve directory", "fexecve no #! line", "execveat dirfd relative", "execveat empty path no flag", "execveat AT_EMPTY_PATH",
                            "execveat AT_SYMLINK_NOFOLLOW link", "execveat link followed", "execveat flag 0x1", "execveat bad fd relative", "execveat bad fd absolute",
                            "execveat AT_FDCWD relative", "fexecve envp"};
    for (int i = 0; i < (int)(sizeof labels / sizeof *labels); i++) {
        struct fx f = {i};
        capture(labels[i], fx_child, &f);
    }
}

/* What exec keeps: the PID, descriptors without close-on-exec, the mask, ignored signals, the
 * cwd; caught signals become default. */
static void keep_child(void *arg)
{
    (void)arg;
    signal(SIGUSR1, SIG_IGN);
    signal(SIGUSR2, count_sig);
    sigset_t s;
    sigemptyset(&s);
    sigaddset(&s, SIGTERM);
    sigprocmask(SIG_BLOCK, &s, NULL);
    kill(getpid(), SIGTERM);
    int a = open(P("afile"), O_RDONLY);
    dup2(a, 7);
    int b = open(P("afile"), O_RDONLY | O_CLOEXEC);
    dup2(b, 8);
    fcntl(8, F_SETFD, FD_CLOEXEC);
    close(a);
    close(b);
    if (chdir(P("sub")) != 0)
        printf("chdir failed\n");
    char pid[32];
    snprintf(pid, sizeof pid, "%d", (int)getpid());
    setenv("PROBE_EXPID", pid, 1);
    char *argv[] = {"keep", "@report", NULL};
    errno = 0;
    execv(self, argv);
}

/* ------------------------------------------------------------------ §18.4 spawn */

static void set_rel_env(void)
{
    char b[32];
    snprintf(b, sizeof b, "%d", (int)getpgrp());
    setenv("PROBE_PPGID", b, 1);
    snprintf(b, sizeof b, "%d", (int)getsid(0));
    setenv("PROBE_PSID", b, 1);
    setenv("PROBE_DIR", dir, 1);
}

static char *rep_argv[] = {"rep", "@report", NULL};

/* Spawn; on success wait and print the status; on failure say whether a child is left to reap
 * and whether a descriptor leaked. */
static void sp(const char *label, int usep, const char *file, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *at, char *const *argv, char *const *envp)
{
    fflush(stdout);
    int before = lowest_free_fd();
    pid_t pid = -1;
    int r = usep ? posix_spawnp(&pid, file, fa, at, argv, envp ? envp : environ) : posix_spawn(&pid, file, fa, at, argv, envp ? envp : environ);
    if (r == 0) {
        int st = 0;
        pid_t w = waitpid(pid, &st, 0);
        out("%s: r=0 %s\n", label, w == pid ? stat_str(st) : "wait failed");
        return;
    }
    int st = 0;
    errno = 0;
    pid_t w = waitpid(-1, &st, WNOHANG);
    const char *left = w > 0 ? "reaped-now" : w == 0 ? "running" : errno == ECHILD ? "none" : "?";
    if (w == 0)
        waitpid(-1, &st, 0);
    out("%s: r=%d child_left=%s%s%s fd_leak=%s\n", label, r, left, w != -1 ? " " : "", w != -1 ? stat_str(st) : "", lowest_free_fd() != before ? "yes" : "no");
}

static void fa_errors(void)
{
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    out("addopen fd=-1: %d\n", posix_spawn_file_actions_addopen(&fa, -1, "x", O_RDONLY, 0));
    out("addopen fd=100000: %d\n", posix_spawn_file_actions_addopen(&fa, 100000, "x", O_RDONLY, 0));
    out("addopen fd=INT_MAX: %d\n", posix_spawn_file_actions_addopen(&fa, INT_MAX, "x", O_RDONLY, 0));
    out("addclose fd=-1: %d\n", posix_spawn_file_actions_addclose(&fa, -1));
    out("addclose fd=100000: %d\n", posix_spawn_file_actions_addclose(&fa, 100000));
    out("adddup2 fd=-1: %d\n", posix_spawn_file_actions_adddup2(&fa, -1, 5));
    out("adddup2 newfd=-1: %d\n", posix_spawn_file_actions_adddup2(&fa, 1, -1));
    out("adddup2 newfd=100000: %d\n", posix_spawn_file_actions_adddup2(&fa, 1, 100000));
    out("addfchdir fd=-1: %d\n", posix_spawn_file_actions_addfchdir_np(&fa, -1));
    out("addclosefrom fd=-1: %d\n", posix_spawn_file_actions_addclosefrom_np(&fa, -1));
    out("addclosefrom fd=100000: %d\n", posix_spawn_file_actions_addclosefrom_np(&fa, 100000));
    out("addtcsetpgrp fd=-1: %d\n", posix_spawn_file_actions_addtcsetpgrp_np(&fa, -1));
    out("addchdir empty: %d\n", posix_spawn_file_actions_addchdir_np(&fa, ""));
    out("addopen fd=5 accepted: %d\n", posix_spawn_file_actions_addopen(&fa, 5, "x", O_RDONLY, 0));
    out("destroy: %d\n", posix_spawn_file_actions_destroy(&fa));
    posix_spawn_file_actions_init(&fa);
    out("empty actions spawn:\n");
    sp("  empty actions", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
}

/* Each file action failing in the child. */
static void fa_fail(void)
{
    int filefd = open(P("afile"), O_RDONLY);
    dup2(filefd, 20);
    close(filefd);
    struct {
        const char *label;
        int k;
    } t[] = {
        {"fail addopen missing", 0},      {"fail addopen O_CREAT in missing dir", 1}, {"fail addopen directory O_WRONLY", 2}, {"fail addopen mode 000", 3},
        {"fail addclose not open", 4},    {"fail adddup2 from closed", 5},            {"fail adddup2 same closed", 6},        {"fail addchdir missing", 7},
        {"fail addchdir file", 8},        {"fail addfchdir closed", 9},               {"fail addfchdir file", 10},            {"fail addtcsetpgrp not a tty", 11},
        {"fail addtcsetpgrp closed", 12}, {"fail second action after a create", 13},  {"fail exec after a create", 14},       {"fail closefrom then dup2", 15},
        {"fail addopen O_EXCL existing", 16},     {"fail addfchdir -1 (accepted when added)", 17}, {"fail addchdir empty (accepted when added)", 18},
    };
    for (size_t i = 0; i < sizeof t / sizeof *t; i++) {
        posix_spawn_file_actions_t fa;
        posix_spawn_file_actions_init(&fa);
        const char *file = self;
        unlink(P("created1"));
        switch (t[i].k) {
        case 0:
            posix_spawn_file_actions_addopen(&fa, 5, P("missing"), O_RDONLY, 0);
            break;
        case 1:
            posix_spawn_file_actions_addopen(&fa, 5, P("nodir/x"), O_WRONLY | O_CREAT, 0644);
            break;
        case 2:
            posix_spawn_file_actions_addopen(&fa, 5, dir, O_WRONLY, 0);
            break;
        case 3:
            posix_spawn_file_actions_addopen(&fa, 5, P("mode000"), O_RDONLY, 0);
            break;
        case 4:
            posix_spawn_file_actions_addclose(&fa, 40);
            break;
        case 5:
            posix_spawn_file_actions_adddup2(&fa, 40, 5);
            break;
        case 6:
            posix_spawn_file_actions_adddup2(&fa, 40, 40);
            break;
        case 7:
            posix_spawn_file_actions_addchdir_np(&fa, P("missing"));
            break;
        case 8:
            posix_spawn_file_actions_addchdir_np(&fa, P("afile"));
            break;
        case 9:
            posix_spawn_file_actions_addfchdir_np(&fa, 40);
            break;
        case 10:
            posix_spawn_file_actions_addfchdir_np(&fa, 20);
            break;
        case 11:
            posix_spawn_file_actions_addtcsetpgrp_np(&fa, 20);
            break;
        case 12:
            posix_spawn_file_actions_addtcsetpgrp_np(&fa, 40);
            break;
        case 13:
            posix_spawn_file_actions_addopen(&fa, 5, P("created1"), O_WRONLY | O_CREAT, 0644);
            posix_spawn_file_actions_addopen(&fa, 6, P("missing"), O_RDONLY, 0);
            break;
        case 14:
            posix_spawn_file_actions_addopen(&fa, 5, P("created1"), O_WRONLY | O_CREAT, 0644);
            file = P("missing-program");
            break;
        case 15:
            posix_spawn_file_actions_addclosefrom_np(&fa, 3);
            posix_spawn_file_actions_adddup2(&fa, 20, 6);
            break;
        case 16:
            posix_spawn_file_actions_addopen(&fa, 5, P("afile"), O_WRONLY | O_CREAT | O_EXCL, 0644);
            break;
        case 17:
            posix_spawn_file_actions_addfchdir_np(&fa, -1);
            break;
        case 18:
            posix_spawn_file_actions_addchdir_np(&fa, "");
            break;
        }
        sp(t[i].label, 0, file, &fa, NULL, rep_argv, NULL);
        if (t[i].k == 13 || t[i].k == 14) {
            struct stat stt;
            out("  created1 exists: %s\n", stat(P("created1"), &stt) == 0 ? "yes" : "no");
        }
        posix_spawn_file_actions_destroy(&fa);
    }
    close(20);
}

static void fa_ok(void)
{
    posix_spawn_file_actions_t fa;

    /* open, dup2, close in order */
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 5, P("afile"), O_RDONLY, 0);
    posix_spawn_file_actions_adddup2(&fa, 5, 6);
    posix_spawn_file_actions_addclose(&fa, 5);
    sp("ok open 5, dup2 5->6, close 5", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);

    /* dup2(fd, fd) clears close-on-exec */
    int c = open(P("afile"), O_RDONLY | O_CLOEXEC);
    dup2(c, 7);
    fcntl(7, F_SETFD, FD_CLOEXEC);
    close(c);
    sp("ok cloexec fd 7 without action", 0, self, NULL, NULL, rep_argv, NULL);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, 7, 7);
    sp("ok adddup2 7->7 clears cloexec", 0, self, &fa, NULL, rep_argv, NULL);
    out("  parent fd 7 still cloexec: %s\n", fcntl(7, F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    posix_spawn_file_actions_destroy(&fa);
    close(7);

    /* closefrom */
    int a = open(P("afile"), O_RDONLY);
    dup2(a, 7);
    dup2(a, 8);
    dup2(a, 12);
    close(a);
    sp("ok inherited 7 8 12", 0, self, NULL, NULL, rep_argv, NULL);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclosefrom_np(&fa, 3);
    sp("ok closefrom 3", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclosefrom_np(&fa, 8);
    sp("ok closefrom 8", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclosefrom_np(&fa, 3);
    posix_spawn_file_actions_addopen(&fa, 9, P("afile"), O_RDONLY, 0);
    sp("ok closefrom 3 then open 9", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    close(7);
    close(8);
    close(12);

    /* chdir: later relative actions and the program path resolve in the new directory */
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addchdir_np(&fa, P("sub"));
    posix_spawn_file_actions_addopen(&fa, 5, "insub", O_RDONLY, 0);
    sp("ok chdir sub then open relative", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 5, "insub", O_RDONLY, 0);
    posix_spawn_file_actions_addchdir_np(&fa, P("sub"));
    sp("ok open relative then chdir sub", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addchdir_np(&fa, P("bin_ok"));
    {
        char *argv[] = {"prog", "rel", NULL};
        sp("ok chdir bin_ok then spawn relative prog", 0, "prog", &fa, NULL, argv, NULL);
        setenv("PATH", ".", 1);
        sp("ok chdir bin_ok then spawnp with PATH=.", 1, "prog", &fa, NULL, argv, NULL);
        setenv("PATH", "/usr/bin:/bin", 1);
    }
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addchdir_np(&fa, "sub");
    {
        int cw = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        if (chdir(dir) == 0)
            sp("ok chdir relative sub", 0, self, &fa, NULL, rep_argv, NULL);
        if (fchdir(cw) != 0)
            out("fchdir back failed\n");
        close(cw);
    }
    posix_spawn_file_actions_destroy(&fa);

    /* fchdir */
    int d = open(P("sub"), O_RDONLY | O_DIRECTORY);
    dup2(d, 9);
    close(d);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addfchdir_np(&fa, 9);
    sp("ok fchdir 9 (sub)", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclose(&fa, 9);
    posix_spawn_file_actions_addfchdir_np(&fa, 9);
    sp("fail close 9 then fchdir 9", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    close(9);

    /* open onto stdout */
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 1, P("spawn_out"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    sp("ok addopen fd 1 to a file", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    FILE *f = fopen(P("spawn_out"), "r");
    char line[1024];
    while (f && fgets(line, sizeof line, f))
        out("  file:%s", line);
    if (f)
        fclose(f);

    /* close of an fd that the parent has open */
    a = open(P("afile"), O_RDONLY);
    dup2(a, 7);
    close(a);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclose(&fa, 7);
    sp("ok addclose 7", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addclose(&fa, 7);
    posix_spawn_file_actions_addclose(&fa, 7);
    sp("addclose 7 twice", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
    close(7);

    /* the same action list used twice */
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 5, P("afile"), O_RDONLY, 0);
    sp("reuse 1", 0, self, &fa, NULL, rep_argv, NULL);
    sp("reuse 2", 0, self, &fa, NULL, rep_argv, NULL);
    posix_spawn_file_actions_destroy(&fa);
}

static void exec_fail_spawn(void)
{
    char *argv[] = {"prog", "x", NULL};
    sp("spawn missing", 0, P("missing"), NULL, NULL, argv, NULL);
    sp("spawn not executable", 0, P("bin_noexec/prog"), NULL, NULL, argv, NULL);
    sp("spawn directory", 0, P("bin_dir/prog"), NULL, NULL, argv, NULL);
    sp("spawn no #! line", 0, P("bin_plain/plain"), NULL, NULL, argv, NULL);
    sp("spawn ELOOP", 0, P("bin_loop/prog"), NULL, NULL, argv, NULL);
    sp("spawn ENOTDIR", 0, P("notadir/prog"), NULL, NULL, argv, NULL);
    sp("spawn missing interpreter", 0, P("bin_badinterp/prog"), NULL, NULL, argv, NULL);
    sp("spawn empty path", 0, "", NULL, NULL, argv, NULL);
    sp("spawn script ok", 0, P("bin_ok/prog"), NULL, NULL, argv, NULL);
    {
        char *big = malloc(131073);
        memset(big, 'a', 131072);
        big[131072] = 0;
        char *bargv[] = {"self", "@true", big, NULL};
        sp("spawn E2BIG", 0, self, NULL, NULL, bargv, NULL);
        free(big);
    }
    setenv("PATH", mkpath("bin_noexec:bin_ok"), 1);
    sp("spawnp EACCES then ok", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("bin_noexec"), 1);
    sp("spawnp EACCES only", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("nonexistent:empty"), 1);
    sp("spawnp not found", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("bin_plain"), 1);
    {
        char *pargv[] = {"plain", "a", NULL};
        sp("spawnp no #! line", 1, "plain", NULL, NULL, pargv, NULL);
    }
    setenv("PATH", mkpath("bin_plain:bin_ok"), 1);
    sp("spawnp no #! line first then ok", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("bin_loop:bin_ok"), 1);
    sp("spawnp ELOOP then ok", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("bin_badinterp:bin_ok"), 1);
    sp("spawnp missing interpreter then ok", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("notadir:bin_ok"), 1);
    sp("spawnp file element then ok", 1, "prog", NULL, NULL, argv, NULL);
    setenv("PATH", mkpath("bin_ok"), 1);
    {
        char penv[8300];
        snprintf(penv, sizeof penv, "PATH=%s", mkpath("bin_ok2"));
        char denv[PATH_MAX + 16];
        snprintf(denv, sizeof denv, "PROBE_DIR=%s", dir);
        char *envp[] = {penv, "X=envp", denv, NULL};
        sp("spawnp searches caller PATH not envp", 1, "prog", NULL, NULL, argv, envp);
    }
    sp("spawnp empty name", 1, "", NULL, NULL, argv, NULL);
    {
        int cw = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        if (chdir(dir) == 0) {
            sp("spawnp name with slash", 1, "bin_ok2/prog", NULL, NULL, argv, NULL);
            unsetenv("PATH");
            char *shargv[] = {"sh", "-c", "echo spawnp-default-path-found", NULL};
            sp("spawnp PATH unset finds sh", 1, "sh", NULL, NULL, shargv, NULL);
            char *cargv[] = {"cwdprog", NULL};
            sp("spawnp PATH unset cwd not searched", 1, "cwdprog", NULL, NULL, cargv, NULL);
            setenv("PATH", "", 1);
            sp("spawnp PATH empty string", 1, "cwdprog", NULL, NULL, cargv, NULL);
            setenv("PATH", "/nonexistent:", 1);
            sp("spawnp PATH trailing empty", 1, "cwdprog", NULL, NULL, cargv, NULL);
        }
        if (fchdir(cw) != 0)
            out("fchdir back failed\n");
        close(cw);
    }
    setenv("PATH", "/usr/bin:/bin", 1);

    /* pid may be NULL */
    fflush(stdout);
    int r = posix_spawn(NULL, self, NULL, NULL, rep_argv, environ);
    int st = 0;
    pid_t w = wait(&st);
    out("spawn pid NULL: r=%d waited=%s %s\n", r, w > 0 ? "yes" : "no", stat_str(st));

    /* with SIGCHLD ignored: errors are still reported; the child is reaped automatically */
    signal(SIGCHLD, SIG_IGN);
    pid_t pid = -1;
    r = posix_spawn(&pid, P("missing"), NULL, NULL, rep_argv, environ);
    out("spawn missing with SIGCHLD ignored: r=%d\n", r);
    fflush(stdout);
    r = posix_spawn(&pid, self, NULL, NULL, rep_argv, environ);
    errno = 0;
    w = waitpid(pid, &st, 0);
    int e = errno;
    out("spawn with SIGCHLD ignored: r=%d waitpid=%d errno=%d\n", r, (int)w, e);
    signal(SIGCHLD, SIG_DFL);
}

static void show_attr_defaults(void)
{
    posix_spawnattr_t at;
    out("attr init: %d\n", posix_spawnattr_init(&at));
    short fl = -1;
    pid_t pg = -1;
    sigset_t s;
    int pol = -1;
    struct sched_param prm = {-1};
    int cg = -2;
    char b[256];
    posix_spawnattr_getflags(&at, &fl);
    posix_spawnattr_getpgroup(&at, &pg);
    memset(&s, 0xff, sizeof s);
    posix_spawnattr_getsigmask(&at, &s);
    siglist(b, sizeof b, &s);
    out("attr defaults: flags=%d pgroup=%d sigmask=%s", fl, (int)pg, b);
    memset(&s, 0xff, sizeof s);
    posix_spawnattr_getsigdefault(&at, &s);
    siglist(b, sizeof b, &s);
    posix_spawnattr_getschedpolicy(&at, &pol);
    posix_spawnattr_getschedparam(&at, &prm);
    posix_spawnattr_getcgroup_np(&at, &cg);
    out(" sigdefault=%s policy=%d priority=%d cgroup=%d\n", b, pol, prm.sched_priority, cg);
    out("setflags 0x1ff: %d\n", posix_spawnattr_setflags(&at, 0x1ff));
    posix_spawnattr_getflags(&at, &fl);
    out("getflags: 0x%x\n", (unsigned)fl);
    out("setflags 0x200: %d\n", posix_spawnattr_setflags(&at, 0x200));
    out("setflags -1: %d\n", posix_spawnattr_setflags(&at, -1));
    posix_spawnattr_getflags(&at, &fl);
    out("getflags after refused: 0x%x\n", (unsigned)fl);
    out("setpgroup -5: %d\n", posix_spawnattr_setpgroup(&at, -5));
    posix_spawnattr_getpgroup(&at, &pg);
    out("getpgroup: %d\n", (int)pg);
    out("setschedpolicy 99: %d\n", posix_spawnattr_setschedpolicy(&at, 99));
    out("setschedpolicy SCHED_BATCH: %d\n", posix_spawnattr_setschedpolicy(&at, SCHED_BATCH));
    prm.sched_priority = 1000;
    out("setschedparam 1000: %d\n", posix_spawnattr_setschedparam(&at, &prm));
    out("setcgroup -1: %d\n", posix_spawnattr_setcgroup_np(&at, -1));
    sigemptyset(&s);
    sigaddset(&s, SIGKILL);
    out("setsigmask {KILL}: %d\n", posix_spawnattr_setsigmask(&at, &s));
    out("setsigdefault {KILL}: %d\n", posix_spawnattr_setsigdefault(&at, &s));
    out("attr destroy: %d\n", posix_spawnattr_destroy(&at));
}


static void attrs(void)
{
    posix_spawnattr_t at;
    sigset_t s, old;

    /* signals: parent blocks HUP, ignores USR1 and TERM, catches USR2 */
    signal(SIGUSR1, SIG_IGN);
    signal(SIGTERM, SIG_IGN);
    signal(SIGUSR2, count_sig);
    sigemptyset(&s);
    sigaddset(&s, SIGHUP);
    sigprocmask(SIG_BLOCK, &s, &old);
    sp("attr none (parent: blocks 1, ignores 10 15, catches 12)", 0, self, NULL, NULL, rep_argv, NULL);
    posix_spawnattr_init(&at);
    sp("attr empty", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGMASK);
    sigemptyset(&s);
    sigaddset(&s, SIGUSR2);
    posix_spawnattr_setsigmask(&at, &s);
    sp("SETSIGMASK {12}", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGDEF);
    sigemptyset(&s);
    sigaddset(&s, SIGUSR1);
    posix_spawnattr_setsigdefault(&at, &s);
    sp("SETSIGDEF {10}", 0, self, NULL, &at, rep_argv, NULL);
    sigfillset(&s);
    posix_spawnattr_setsigdefault(&at, &s);
    sp("SETSIGDEF all", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setsigmask(&at, &s);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGMASK);
    sp("SETSIGMASK all", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_destroy(&at);
    sigprocmask(SIG_SETMASK, &old, NULL);
    signal(SIGUSR1, SIG_DFL);
    signal(SIGTERM, SIG_DFL);
    signal(SIGUSR2, SIG_DFL);

    /* process groups and sessions */
    posix_spawnattr_init(&at);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETPGROUP);
    posix_spawnattr_setpgroup(&at, 0);
    sp("SETPGROUP 0", 0, self, NULL, &at, rep_argv, NULL);
    {
        char *sargv[] = {"s", "@sleep", "3000", NULL};
        pid_t a = -1;
        fflush(stdout);
        int r = posix_spawn(&a, self, NULL, &at, sargv, environ);
        char b[32];
        snprintf(b, sizeof b, "%d", (int)a);
        setenv("PROBE_XPGID", b, 1);
        posix_spawnattr_t at2;
        posix_spawnattr_init(&at2);
        posix_spawnattr_setflags(&at2, POSIX_SPAWN_SETPGROUP);
        posix_spawnattr_setpgroup(&at2, a);
        out("spawn group leader: r=%d\n", r);
        sp("SETPGROUP existing group (pgid=x)", 0, self, NULL, &at2, rep_argv, NULL);
        kill(a, SIGKILL);
        waitpid(a, NULL, 0);
        unsetenv("PROBE_XPGID");
        posix_spawnattr_destroy(&at2);
    }
    {
        pid_t gone = 4194303;
        while (gone > 300 && (kill(gone, 0) == 0 || errno != ESRCH || getpgid(gone) >= 0))
            gone--;
        posix_spawnattr_setpgroup(&at, gone);
        sp("SETPGROUP nonexistent group", 0, self, NULL, &at, rep_argv, NULL);
    }
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSID);
    sp("SETSID", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSID | POSIX_SPAWN_SETPGROUP);
    posix_spawnattr_setpgroup(&at, 0);
    sp("SETSID|SETPGROUP 0", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setpgroup(&at, getpgrp());
    sp("SETSID|SETPGROUP parent group", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_destroy(&at);

    /* scheduling */
    struct sched_param prm = {0};
    posix_spawnattr_init(&at);
    out("setschedpolicy: OTHER=%d FIFO=%d RR=%d BATCH=%d IDLE=%d DEADLINE=%d -1=%d\n", posix_spawnattr_setschedpolicy(&at, SCHED_OTHER), posix_spawnattr_setschedpolicy(&at, SCHED_FIFO),
        posix_spawnattr_setschedpolicy(&at, SCHED_RR), posix_spawnattr_setschedpolicy(&at, SCHED_BATCH), posix_spawnattr_setschedpolicy(&at, SCHED_IDLE),
        posix_spawnattr_setschedpolicy(&at, 6), posix_spawnattr_setschedpolicy(&at, -1));
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSCHEDULER);
    posix_spawnattr_setschedpolicy(&at, SCHED_OTHER);
    posix_spawnattr_setschedparam(&at, &prm);
    sp("SETSCHEDULER OTHER/0", 0, self, NULL, &at, rep_argv, NULL);
    prm.sched_priority = 5;
    posix_spawnattr_setschedparam(&at, &prm);
    sp("SETSCHEDULER OTHER/5", 0, self, NULL, &at, rep_argv, NULL);
    if (geteuid() != 0) {
        prm.sched_priority = 1;
        posix_spawnattr_setschedpolicy(&at, SCHED_FIFO);
        posix_spawnattr_setschedparam(&at, &prm);
        sp("SETSCHEDULER FIFO/1 unprivileged", 0, self, NULL, &at, rep_argv, NULL);
        posix_spawnattr_setschedpolicy(&at, SCHED_RR);
        sp("SETSCHEDULER RR/1 unprivileged", 0, self, NULL, &at, rep_argv, NULL);
    }
    prm.sched_priority = 0;
    posix_spawnattr_setschedpolicy(&at, SCHED_FIFO);
    posix_spawnattr_setschedparam(&at, &prm);
    sp("SETSCHEDULER FIFO/0", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSCHEDPARAM);
    prm.sched_priority = 5;
    posix_spawnattr_setschedparam(&at, &prm);
    sp("SETSCHEDPARAM 5 (caller OTHER)", 0, self, NULL, &at, rep_argv, NULL);
    prm.sched_priority = 0;
    posix_spawnattr_setschedparam(&at, &prm);
    sp("SETSCHEDPARAM 0 (caller OTHER)", 0, self, NULL, &at, rep_argv, NULL);
    /* the caller's own policy (BATCH, set directly) is inherited, and SETSCHEDPARAM keeps it */
    fflush(stdout);
    pid_t b = fork();
    if (b == 0) {
        child_guard();
        struct sched_param z = {0};
        out("caller sched_setscheduler BATCH: %d\n", sched_setscheduler(0, SCHED_BATCH, &z));
        sp("  spawn without attributes (caller BATCH)", 0, self, NULL, NULL, rep_argv, NULL);
        sp("  SETSCHEDPARAM 0 (caller BATCH)", 0, self, NULL, &at, rep_argv, NULL);
        posix_spawnattr_t a2;
        posix_spawnattr_init(&a2);
        posix_spawnattr_setflags(&a2, POSIX_SPAWN_SETSCHEDULER);
        posix_spawnattr_setschedpolicy(&a2, SCHED_OTHER);
        posix_spawnattr_setschedparam(&a2, &z);
        sp("  SETSCHEDULER OTHER/0 (caller BATCH)", 0, self, NULL, &a2, rep_argv, NULL);
        posix_spawnattr_destroy(&a2);
        _exit(0);
    }
    waitpid(b, NULL, 0);
    posix_spawnattr_destroy(&at);

    /* USEVFORK, RESETIDS unprivileged, SETCGROUP errors */
    posix_spawnattr_init(&at);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_USEVFORK);
    sp("USEVFORK", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_RESETIDS);
    sp("RESETIDS (real == effective)", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETCGROUP);
    posix_spawnattr_setcgroup_np(&at, -1);
    sp("SETCGROUP fd -1", 0, self, NULL, &at, rep_argv, NULL);
    int dfd = open(dir, O_RDONLY | O_DIRECTORY);
    posix_spawnattr_setcgroup_np(&at, dfd);
    sp("SETCGROUP fd not a cgroup", 0, self, NULL, &at, rep_argv, NULL);
    close(dfd);
    posix_spawnattr_setcgroup_np(&at, 40);
    sp("SETCGROUP fd closed", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_destroy(&at);
}

/* A signal sent to the child while it is between clone and exec (blocked in an open of a FIFO
 * by a file action) must not run the parent's handler in the child. */
static volatile int rec_fd = -1;
static void rec_handler(int s)
{
    (void)s;
    int me = (int)getpid();
    if (write(rec_fd, &me, sizeof me) < 0) {
    }
}

struct win_arg {
    pid_t group;
};

static void *win_thread(void *p)
{
    struct win_arg *w = p;
    sleep_ms(300);
    kill(-w->group, SIGUSR2);
    sleep_ms(200);
    int fd = open(P("fifo"), O_WRONLY);
    if (fd >= 0)
        close(fd);
    return NULL;
}

static void spawn_window(int mask_usr2)
{
    unlink(P("fifo"));
    mkfifo(P("fifo"), 0600);
    int rp[2];
    if (pipe(rp) != 0)
        return;
    fflush(stdout);
    pid_t s = fork();
    if (s == 0) {
        child_guard();
        setpgid(0, 0);
        rec_fd = rp[1];
        signal(SIGUSR2, rec_handler);
        struct win_arg w = {getpgrp()};
        pthread_t t;
        pthread_create(&t, NULL, win_thread, &w);
        posix_spawn_file_actions_t fa;
        posix_spawn_file_actions_init(&fa);
        posix_spawn_file_actions_addopen(&fa, 5, P("fifo"), O_RDONLY, 0);
        posix_spawnattr_t at;
        posix_spawnattr_init(&at);
        if (mask_usr2) {
            sigset_t m;
            sigemptyset(&m);
            sigaddset(&m, SIGUSR2);
            posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSIGMASK);
            posix_spawnattr_setsigmask(&at, &m);
        }
        pid_t c = -1;
        int r = posix_spawn(&c, self, &fa, &at, rep_argv, environ);
        int st = 0;
        if (r == 0)
            waitpid(c, &st, 0);
        pthread_join(t, NULL);
        int me = (int)getpid(), v, in_self = 0, in_other = 0;
        close(rp[1]);
        rec_fd = -1;
        while (read(rp[0], &v, sizeof v) == sizeof v) {
            if (v == me)
                in_self++;
            else
                in_other++;
        }
        out("window%s: r=%d child %s handler_in_parent=%s handler_in_other_process=%d\n", mask_usr2 ? " (SETSIGMASK {12})" : "", r, stat_str(st), in_self ? "yes" : "no", in_other);
        _exit(0);
    }
    close(rp[1]);
    close(rp[0]);
    int sst = 0;
    waitpid(s, &sst, 0);
    if (sst != 0)
        out("window: helper process %s\n", stat_str(sst));
}

struct thr {
    int ok;
};

static void *spawn_thread(void *p)
{
    struct thr *t = p;
    char *argv[] = {"t", "@exit", "3", NULL};
    for (int i = 0; i < 25; i++) {
        pid_t pid;
        if (posix_spawn(&pid, self, NULL, NULL, argv, environ) != 0)
            continue;
        int st;
        if (waitpid(pid, &st, 0) == pid && WIFEXITED(st) && WEXITSTATUS(st) == 3)
            t->ok++;
    }
    return NULL;
}

static void spawn_threads(void)
{
    pthread_t th[4];
    struct thr t[4] = {{0}};
    for (int i = 0; i < 4; i++)
        pthread_create(&th[i], NULL, spawn_thread, &t[i]);
    int ok = 0;
    for (int i = 0; i < 4; i++) {
        pthread_join(th[i], NULL);
        ok += t[i].ok;
    }
    out("spawn from 4 threads x 25: %d of 100 exited 3\n", ok);
}

static int count_fds(void)
{
    int n = 0;
    for (int fd = 0; fd < 1024; fd++)
        if (fcntl(fd, F_GETFD) >= 0)
            n++;
    return n;
}

/* Close descriptors that refer to processes (a pidfd left behind by a failed call). */
static void close_extra_pidfds(void)
{
    int n = 0;
    for (int fd = 3; fd < 1024; fd++)
        if (fcntl(fd, F_GETFD) >= 0) {
            errno = 0;
            if (pidfd_getpid(fd) != -1 || errno == ESRCH) {
                close(fd);
                n++;
            }
        }
    out("  of which pidfds: %d\n", n);
}

static void pidfd_spawns(void)
{
    int pfd = -1;
    fflush(stdout);
    int low = lowest_free_fd();
    int r = pidfd_spawn(&pfd, self, NULL, NULL, rep_argv, environ);
    int lowest = pfd == low, cloexec = fcntl(pfd, F_GETFD) == FD_CLOEXEC;
    pid_t pp = pidfd_getpid(pfd);
    siginfo_t si;
    memset(&si, 0, sizeof si);
    /* The child is reaped before this result is printed, so its report line always comes
     * first, whatever the scheduling (it raced with this line under load). */
    int w = waitid(P_PIDFD, (id_t)pfd, &si, WEXITED);
    out("pidfd_spawn: r=%d pidfd_is_lowest_free=%s cloexec=%s\n", r, lowest ? "yes" : "no", cloexec ? "yes" : "no");
    out("  waitid P_PIDFD: %d code=%d status=%d si_pid==pidfd_getpid=%s\n", w, si.si_code, si.si_status, si.si_pid == pp && pp > 0 ? "yes" : "no");
    SHOW("  pidfd_getpid after reap", pidfd_getpid(pfd));
    errno = 0;
    SHOW("  pidfd_send_signal after reap", pidfd_send_signal(pfd, 0, NULL, 0));
    close(pfd);

    pfd = -77;
    int nfd = count_fds();
    r = pidfd_spawn(&pfd, P("missing"), NULL, NULL, rep_argv, environ);
    int st;
    errno = 0;
    pid_t left = waitpid(-1, &st, WNOHANG);
    int extra = count_fds() - nfd;
    out("pidfd_spawn missing: r=%d pidfd_untouched=%s extra_open_fds=%d child_left=%s\n", r, pfd == -77 ? "yes" : "no", extra, left > 0 ? "yes" : "no");
    close_extra_pidfds();

    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 5, P("missing"), O_RDONLY, 0);
    pfd = -77;
    nfd = count_fds();
    r = pidfd_spawn(&pfd, self, &fa, NULL, rep_argv, environ);
    extra = count_fds() - nfd;
    out("pidfd_spawn failing action: r=%d pidfd_untouched=%s extra_open_fds=%d\n", r, pfd == -77 ? "yes" : "no", extra);
    close_extra_pidfds();
    posix_spawn_file_actions_destroy(&fa);

    setenv("PATH", mkpath("bin_noexec:bin_ok"), 1);
    char *argv[] = {"prog", "viapidfd", NULL};
    fflush(stdout);
    r = pidfd_spawnp(&pfd, "prog", NULL, NULL, argv, environ);
    memset(&si, 0, sizeof si);
    w = waitid(P_PIDFD, (id_t)pfd, &si, WEXITED);
    out("pidfd_spawnp EACCES then ok: r=%d waitid=%d code=%d status=%d\n", r, w, si.si_code, si.si_status);
    close(pfd);
    setenv("PATH", mkpath("bin_noexec"), 1);
    pfd = -77;
    r = pidfd_spawnp(&pfd, "prog", NULL, NULL, argv, environ);
    out("pidfd_spawnp EACCES only: r=%d pidfd_untouched=%s\n", r, pfd == -77 ? "yes" : "no");
    setenv("PATH", "/usr/bin:/bin", 1);

    /* pidfd_spawn with SETSID and a signal through the pidfd */
    posix_spawnattr_t at;
    posix_spawnattr_init(&at);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSID);
    char *sargv[] = {"s", "@sleep", "5000", NULL};
    r = pidfd_spawn(&pfd, self, NULL, &at, sargv, environ);
    errno = 0;
    int ks = pidfd_send_signal(pfd, SIGTERM, NULL, 0);
    memset(&si, 0, sizeof si);
    w = waitid(P_PIDFD, (id_t)pfd, &si, WEXITED);
    out("pidfd_spawn SETSID then pidfd_send_signal TERM: r=%d send=%d waitid=%d code=%d status=%d\n", r, ks, w, si.si_code, si.si_status);
    close(pfd);
    posix_spawnattr_destroy(&at);
}

/* ------------------------------------------------------------------ §18.5 pidfd */

static pid_t fork_ready(int *ready_rd, int hold_fd)
{
    int rp[2];
    if (pipe(rp) != 0)
        return -1;
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        close(rp[0]);
        if (hold_fd >= 0) {
            int f = open(P("afile"), O_RDONLY);
            dup2(f, hold_fd);
            close(f);
        }
        if (write(rp[1], "r", 1) < 0) {
        }
        close(rp[1]);
        for (;;)
            pause();
    }
    close(rp[1]);
    char b;
    if (read(rp[0], &b, 1) < 0) {
    }
    if (ready_rd)
        *ready_rd = rp[0];
    else
        close(rp[0]);
    return c;
}

static volatile pid_t ttid;
static volatile int tid_stop;

static void *tid_thread(void *p)
{
    (void)p;
    ttid = gettid();
    while (!tid_stop)
        sleep_ms(5);
    return NULL;
}

static void pidfds(void)
{
    pid_t c = fork_ready(NULL, 9);
    int pfd = pidfd_open(c, 0);
    struct pollfd pf = {pfd, POLLIN, 0};
    out("pidfd_open child: ok=%s cloexec=%s\n", pfd >= 0 ? "yes" : "no", fcntl(pfd, F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    out("  poll running: %d\n", poll(&pf, 1, 0));
    out("  pidfd_getpid == child: %s\n", pidfd_getpid(pfd) == c ? "yes" : "no");
    int g = pidfd_getfd(pfd, 9, 0);
    struct stat a, b;
    stat(P("afile"), &a);
    fstat(g, &b);
    out("  pidfd_getfd 9: ok=%s same_file=%s cloexec=%s\n", g >= 0 ? "yes" : "no", g >= 0 && a.st_ino == b.st_ino ? "yes" : "no", fcntl(g, F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    close(g);
    errno = 0;
    SHOW("  pidfd_getfd not open", pidfd_getfd(pfd, 50, 0));
    errno = 0;
    SHOW("  pidfd_getfd flags 1", pidfd_getfd(pfd, 9, 1));
    errno = 0;
    SHOW("  pidfd_getfd bad pidfd", pidfd_getfd(50, 9, 0));
    errno = 0;
    SHOW("  pidfd_send_signal 0", pidfd_send_signal(pfd, 0, NULL, 0));
    errno = 0;
    SHOW("  pidfd_send_signal flags 1", pidfd_send_signal(pfd, SIGTERM, NULL, 1));
    errno = 0;
    SHOW("  pidfd_send_signal sig 99", pidfd_send_signal(pfd, 99, NULL, 0));
    {
        siginfo_t si;
        memset(&si, 0, sizeof si);
        si.si_signo = SIGTERM;
        si.si_code = SI_USER;
        si.si_pid = getpid();
        si.si_uid = getuid();
        errno = 0;
        SHOW("  pidfd_send_signal info SI_USER to other process", pidfd_send_signal(pfd, SIGTERM, &si, 0));
        if (errno == 0) {
            int st;
            waitpid(c, &st, 0);
            out("  child %s\n", stat_str(st));
            close(pfd);
            c = fork_ready(NULL, -1);
            pfd = pidfd_open(c, 0);
        }
        memset(&si, 0, sizeof si);
        si.si_signo = SIGTERM;
        si.si_code = SI_QUEUE;
        si.si_pid = getpid();
        si.si_uid = getuid();
        errno = 0;
        SHOW("  pidfd_send_signal info SI_QUEUE to other process", pidfd_send_signal(pfd, SIGTERM, &si, 0));
        if (errno == 0) {
            int st;
            waitpid(c, &st, 0);
            out("  child %s\n", stat_str(st));
            close(pfd);
            c = fork_ready(NULL, -1);
            pfd = pidfd_open(c, 0);
        }
    }
    errno = 0;
    SHOW("  pidfd_send_signal KILL", pidfd_send_signal(pfd, SIGKILL, NULL, 0));
    pf.fd = pfd;
    pf.revents = 0;
    {
        int pr = poll(&pf, 1, 2000);
        out("  poll after exit (unreaped): %d revents_in=%s\n", pr, pf.revents & POLLIN ? "yes" : "no");
    }
    errno = 0;
    SHOW("  pidfd_send_signal zombie", pidfd_send_signal(pfd, 0, NULL, 0));
    errno = 0;
    pid_t zp = pidfd_getpid(pfd);
    out("  pidfd_getpid zombie: %s errno=%d\n", zp == c ? "pid" : zp < 0 ? "-1" : "other", errno);
    int st;
    waitpid(c, &st, 0);
    out("  child %s\n", stat_str(st));
    errno = 0;
    SHOW("  pidfd_send_signal reaped", pidfd_send_signal(pfd, 0, NULL, 0));
    errno = 0;
    SHOW("  pidfd_getpid reaped", (int)pidfd_getpid(pfd));
    errno = 0;
    siginfo_t si;
    SHOW("  waitid P_PIDFD reaped", waitid(P_PIDFD, (id_t)pfd, &si, WEXITED));
    close(pfd);

    pfd = pidfd_open(getpid(), 0);
    out("pidfd_open self: ok=%s getpid_match=%s\n", pfd >= 0 ? "yes" : "no", pidfd_getpid(pfd) == getpid() ? "yes" : "no");
    {
        sigset_t s, old;
        sigemptyset(&s);
        sigaddset(&s, SIGUSR1);
        sigprocmask(SIG_BLOCK, &s, &old);
        errno = 0;
        int r = pidfd_send_signal(pfd, SIGUSR1, NULL, 0);
        sigset_t p;
        sigpending(&p);
        out("  pidfd_send_signal self USR1 (blocked): %d errno=%d pending=%d\n", r, errno, sigismember(&p, SIGUSR1));
        int sig;
        sigwait(&s, &sig);
        sigprocmask(SIG_SETMASK, &old, NULL);
    }
    close(pfd);
    errno = 0;
    SHOW("pidfd_open flags 1", pidfd_open(getpid(), 1));
    errno = 0;
    SHOW("pidfd_open pid 0", pidfd_open(0, 0));
    errno = 0;
    SHOW("pidfd_open pid -1", pidfd_open(-1, 0));
    {
        pid_t gone = 4194303;
        while (gone > 300 && (kill(gone, 0) == 0 || errno != ESRCH))
            gone--;
        errno = 0;
        SHOW("pidfd_open nonexistent", pidfd_open(gone, 0));
    }
    {
        int p2[2];
        if (pipe(p2) == 0) {
            errno = 0;
            SHOW("pidfd_getpid of a pipe", (int)pidfd_getpid(p2[0]));
            errno = 0;
            SHOW("pidfd_send_signal on a pipe", pidfd_send_signal(p2[0], 0, NULL, 0));
            errno = 0;
            SHOW("waitid P_PIDFD on a pipe", waitid(P_PIDFD, (id_t)p2[0], &si, WEXITED));
            close(p2[0]);
            close(p2[1]);
        }
        errno = 0;
        SHOW("pidfd_getpid -1", (int)pidfd_getpid(-1));
        errno = 0;
        SHOW("pidfd_getpid closed", (int)pidfd_getpid(50));
    }
    /* PIDFD_NONBLOCK: waitid on a running child */
    c = fork_ready(NULL, -1);
    pfd = pidfd_open(c, PIDFD_NONBLOCK);
    errno = 0;
    out("pidfd_open PIDFD_NONBLOCK: ok=%s nonblock=%s\n", pfd >= 0 ? "yes" : "no", (fcntl(pfd, F_GETFL) & O_NONBLOCK) ? "yes" : "no");
    errno = 0;
    SHOW("  waitid P_PIDFD WEXITED running", waitid(P_PIDFD, (id_t)pfd, &si, WEXITED));
    memset(&si, 0xff, sizeof si);
    errno = 0;
    int r = waitid(P_PIDFD, (id_t)pfd, &si, WEXITED | WNOHANG);
    out("  waitid P_PIDFD WNOHANG running: %d errno=%d si_pid=%d\n", r, errno, (int)si.si_pid);
    kill(c, SIGKILL);
    memset(&si, 0, sizeof si);
    pf.fd = pfd;
    poll(&pf, 1, 2000);
    r = waitid(P_PIDFD, (id_t)pfd, &si, WEXITED);
    out("  waitid after kill: %d code=%d status=%d\n", r, si.si_code, si.si_status);
    close(pfd);
    /* a thread id that is not a process */
    {
        pid_t tid = -1;
        pthread_t t;
        pthread_create(&t, NULL, tid_thread, NULL);
        while (!ttid)
            sleep_ms(1);
        tid = ttid;
        errno = 0;
        int f = pidfd_open(tid, 0);
        out("pidfd_open of a non-leader thread: %s errno=%d\n", f >= 0 ? "ok" : "-1", errno);
        if (f >= 0)
            close(f);
        tid_stop = 1;
        pthread_join(t, NULL);
    }
}

/* ------------------------------------------------------------------ §18.6 wait */

static pid_t fork_exit(int code)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        _exit(code);
    }
    return c;
}

/* A child that makes 2 MiB resident before it exits. Since Linux 6.2 the per-process RSS counters
   are per-CPU and read approximately: a child of a small process (a static program linked with
   --gc-sections) that only exits reads ru_maxrss 0 on a many-CPU host, so a maxrss check needs a
   child with a resident size well above the counters' batch. */
static char touch_buf[2 << 20];

static pid_t fork_touch_exit(int code)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        memset(touch_buf, 1, sizeof touch_buf);
        __asm__ volatile("" ::: "memory");
        _exit(code);
    }
    return c;
}

static pid_t fork_sig(int sig)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        prctl(PR_SET_DUMPABLE, 0); /* no core file whatever the core pattern */
        signal(sig, SIG_DFL);
        sigset_t s;
        sigemptyset(&s);
        sigaddset(&s, sig);
        sigprocmask(SIG_UNBLOCK, &s, NULL);
        raise(sig);
        _exit(77);
    }
    return c;
}

static void waits(void)
{
    int st;
    int codes[] = {0, 1, 3, 127, 255, 256, 300, -1};
    for (size_t i = 0; i < sizeof codes / sizeof *codes; i++) {
        pid_t c = fork_exit(codes[i]);
        waitpid(c, &st, 0);
        out("waitpid _exit(%d): %s\n", codes[i], stat_str(st));
    }
    int sigs[] = {SIGKILL, SIGTERM, SIGUSR1, SIGABRT, SIGSEGV, SIGQUIT, SIGRTMIN, SIGRTMAX};
    for (size_t i = 0; i < sizeof sigs / sizeof *sigs; i++) {
        pid_t c = fork_sig(sigs[i]);
        waitpid(c, &st, 0);
        out("waitpid killed by %d%s: %s\n", sigs[i] >= SIGRTMIN ? sigs[i] - SIGRTMIN : sigs[i], sigs[i] >= SIGRTMIN ? " (+SIGRTMIN)" : "", stat_str(st));
    }
    /* waitid: the full code and si_status */
    siginfo_t si;
    int vals[] = {0, 3, 255, 256, 300, 0x12345};
    for (size_t i = 0; i < sizeof vals / sizeof *vals; i++) {
        pid_t c = fork_exit(vals[i]);
        memset(&si, 0, sizeof si);
        int r = waitid(P_PID, (id_t)c, &si, WEXITED);
        out("waitid _exit(0x%x): %d signo=%d code=%d status=%d pid_match=%s uid_match=%s\n", vals[i], r, si.si_signo, si.si_code, si.si_status, si.si_pid == c ? "yes" : "no", si.si_uid == getuid() ? "yes" : "no");
    }
    {
        pid_t c = fork_sig(SIGTERM);
        memset(&si, 0, sizeof si);
        waitid(P_PID, (id_t)c, &si, WEXITED);
        out("waitid killed by TERM: code=%d status=%d\n", si.si_code, si.si_status);
        c = fork_sig(SIGABRT);
        memset(&si, 0, sizeof si);
        waitid(P_PID, (id_t)c, &si, WEXITED);
        out("waitid killed by ABRT (not dumpable): code=%d status=%d\n", si.si_code, si.si_status);
    }

    /* stopped and continued; after it is continued the child waits on a pipe until the parent
       has seen the continued report, so that its exit cannot overtake that report */
    int hold[2];
    if (pipe(hold) != 0) { out("pipe failed\n"); return; }
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        close(hold[1]);
        raise(SIGSTOP);
        char b;
        while (read(hold[0], &b, 1) < 0 && errno == EINTR) {}
        _exit(5);
    }
    close(hold[0]);
    memset(&si, 0, sizeof si);
    int r = waitid(P_PID, (id_t)c, &si, WSTOPPED | WNOWAIT);
    out("waitid WSTOPPED|WNOWAIT: %d code=%d status=%d\n", r, si.si_code, si.si_status);
    r = waitpid(c, &st, WNOHANG);
    out("waitpid WNOHANG on stopped child without WUNTRACED: %d\n", r);
    r = waitpid(c, &st, WUNTRACED | WNOHANG);
    out("waitpid WUNTRACED: %s %s\n", r == c ? "child" : "other", stat_str(st));
    r = waitpid(c, &st, WUNTRACED | WNOHANG);
    out("waitpid WUNTRACED again: %d\n", r);
    kill(c, SIGCONT);
    memset(&si, 0, sizeof si);
    r = waitid(P_PID, (id_t)c, &si, WCONTINUED | WNOWAIT);
    out("waitid WCONTINUED|WNOWAIT: %d code=%d status=%d\n", r, si.si_code, si.si_status);
    r = waitpid(c, &st, WCONTINUED);
    out("waitpid WCONTINUED: %s %s\n", r == c ? "child" : "other", stat_str(st));
    close(hold[1]);
    waitpid(c, &st, 0);
    out("then: %s\n", stat_str(st));

    /* WNOWAIT leaves the child waitable */
    c = fork_exit(9);
    memset(&si, 0, sizeof si);
    waitid(P_PID, (id_t)c, &si, WEXITED | WNOWAIT);
    out("waitid WNOWAIT: code=%d status=%d\n", si.si_code, si.si_status);
    r = waitpid(c, &st, 0);
    out("  then waitpid: %s %s\n", r == c ? "child" : "other", stat_str(st));
    errno = 0;
    SHOW("  kill(pid, 0) after reap", kill(c, 0));

    /* WNOHANG with a running child */
    c = fork_ready(NULL, -1);
    out("waitpid WNOHANG running: %d\n", waitpid(c, &st, WNOHANG));
    memset(&si, 0xff, sizeof si);
    r = waitid(P_PID, (id_t)c, &si, WEXITED | WNOHANG);
    out("waitid WNOHANG running: %d si_pid=%d si_signo=%d\n", r, (int)si.si_pid, si.si_signo);
    errno = 0;
    SHOW("waitpid options 0x1000", waitpid(c, &st, 0x1000));
    errno = 0;
    SHOW("waitid options 0", waitid(P_PID, (id_t)c, &si, 0));
    errno = 0;
    SHOW("waitid idtype 99", waitid((idtype_t)99, (id_t)c, &si, WEXITED));
    errno = 0;
    SHOW("waitid WNOHANG only", waitid(P_PID, (id_t)c, &si, WNOHANG));
    kill(c, SIGKILL);
    waitpid(c, &st, 0);

    /* process-group selection */
    c = fork_ready(NULL, -1);
    pid_t c2 = fork_ready(NULL, -1);
    setpgid(c2, c2);
    kill(c2, SIGKILL);
    kill(c, SIGKILL);
    sleep_ms(100);
    r = waitpid(0, &st, 0);
    out("waitpid(0): %s\n", r == c ? "same-group child" : r == c2 ? "other-group child" : "?");
    r = waitpid(-c2, &st, 0);
    out("waitpid(-pgid): %s\n", r == c2 ? "that group's child" : "?");
    c = fork_exit(4);
    setpgid(c, c);
    memset(&si, 0, sizeof si);
    r = waitid(P_PGID, (id_t)c, &si, WEXITED);
    out("waitid P_PGID: %d status=%d pid_match=%s\n", r, si.si_status, si.si_pid == c ? "yes" : "no");
    c = fork_exit(6);
    memset(&si, 0, sizeof si);
    r = waitid(P_PGID, 0, &si, WEXITED);
    out("waitid P_PGID 0 (own group): %d status=%d\n", r, si.si_status);
    c = fork_exit(8);
    memset(&si, 0, sizeof si);
    r = waitid(P_ALL, 12345, &si, WEXITED);
    out("waitid P_ALL (id ignored): %d status=%d\n", r, si.si_status);
    errno = 0;
    SHOW("waitpid(-1) no children", waitpid(-1, &st, 0));
    errno = 0;
    SHOW("waitpid(1) not a child", waitpid(1, &st, 0));
    errno = 0;
    SHOW("waitpid(-pgid) no such group", waitpid(-4194000, &st, 0));
    errno = 0;
    SHOW("wait(NULL) no children", wait(NULL));
    errno = 0;
    SHOW("waitid P_ALL no children", waitid(P_ALL, 0, &si, WEXITED));
    errno = 0;
    SHOW("waitid P_ALL WNOHANG no children", waitid(P_ALL, 0, &si, WEXITED | WNOHANG));
    errno = 0;
    SHOW("wait4(-1) no children", wait4(-1, &st, 0, NULL));

    /* wait(NULL), status NULL */
    c = fork_exit(2);
    out("wait(NULL): %s\n", wait(NULL) == c ? "child" : "?");
    c = fork_exit(2);
    out("waitpid status NULL: %s\n", waitpid(c, NULL, 0) == c ? "child" : "?");

    /* wait3/wait4 resource usage */
    fflush(stdout);
    c = fork();
    if (c == 0) {
        child_guard();
        struct timespec t0, t1;
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &t0);
        do
            clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &t1);
        while ((t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000 < 120);
        _exit(1);
    }
    struct rusage ru;
    memset(&ru, 0, sizeof ru);
    r = wait4(c, &st, 0, &ru);
    long ms = (ru.ru_utime.tv_sec + ru.ru_stime.tv_sec) * 1000 + (ru.ru_utime.tv_usec + ru.ru_stime.tv_usec) / 1000;
    out("wait4: %s %s cpu>=100ms=%s\n", r == c ? "child" : "?", stat_str(st), ms >= 100 ? "yes" : "no");
    c = fork_touch_exit(2);
    memset(&ru, 0xff, sizeof ru);
    r = wait3(&st, 0, &ru);
    out("wait3: %s %s maxrss>0=%s\n", r == c ? "child" : "?", stat_str(st), ru.ru_maxrss > 0 ? "yes" : "no");
    c = fork_exit(2);
    r = wait4(c, &st, 0, NULL);
    out("wait4 rusage NULL: %s %s\n", r == c ? "child" : "?", stat_str(st));

    /* EINTR and restart */
    struct sigaction sa, old;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = count_sig;
    sigaction(SIGALRM, &sa, &old);
    fflush(stdout);
    c = fork();
    if (c == 0) {
        child_guard();
        sleep_ms(600);
        _exit(0);
    }
    struct itimerval it = {{0, 0}, {0, 100000}};
    setitimer(ITIMER_REAL, &it, NULL);
    errno = 0;
    r = waitpid(c, &st, 0);
    SHOWV("waitpid interrupted (no SA_RESTART)", r);
    sa.sa_flags = SA_RESTART;
    sigaction(SIGALRM, &sa, NULL);
    counts[SIGALRM] = 0;
    setitimer(ITIMER_REAL, &it, NULL);
    errno = 0;
    r = waitpid(c, &st, 0);
    out("waitpid with SA_RESTART: %s errno=%d handler_ran=%s\n", r == c ? "child" : "?", errno, counts[SIGALRM] ? "yes" : "no");
    fflush(stdout);
    c = fork();
    if (c == 0) {
        child_guard();
        sleep_ms(600);
        _exit(0);
    }
    sa.sa_flags = 0;
    sigaction(SIGALRM, &sa, NULL);
    setitimer(ITIMER_REAL, &it, NULL);
    errno = 0;
    r = waitid(P_PID, (id_t)c, &si, WEXITED);
    SHOWV("waitid interrupted (no SA_RESTART)", r);
    waitpid(c, &st, 0);
    sigaction(SIGALRM, &old, NULL);

    /* SIGCHLD ignored: children are reaped automatically */
    signal(SIGCHLD, SIG_IGN);
    c = fork_exit(3);
    errno = 0;
    r = waitpid(c, &st, 0);
    out("SIGCHLD ignored: waitpid %d errno=%d\n", r, errno);
    c = fork_exit(3);
    errno = 0;
    r = wait(&st);
    out("SIGCHLD ignored: wait %d errno=%d\n", r, errno);
    signal(SIGCHLD, SIG_DFL);
}

/* ------------------------------------------------------------------ §18.7 system, popen */

struct sysarg {
    const char *cmd;
    int st;
};

static void *system_thread(void *p)
{
    struct sysarg *a = p;
    a->st = system(a->cmd);
    return NULL;
}

static const char *disp(int sig)
{
    struct sigaction sa;
    sigaction(sig, NULL, &sa);
    return sa.sa_handler == SIG_IGN ? "ignored" : sa.sa_handler == SIG_DFL ? "default" : sa.sa_handler == count_sig ? "handler" : "other";
}

static void systems(void)
{
    int r;
    r = system(NULL);
    out("system(NULL): %d\n", r);
    r = system("exit 3");
    out("system exit 3: %s\n", stat_str(r));
    r = system("");
    out("system empty: %s\n", stat_str(r));
    r = system("kill -TERM $$");
    out("system shell killed by TERM: %s\n", stat_str(r));
    r = system("no_such_command_xyz 2>/dev/null");
    out("system missing command: %s\n", stat_str(r));
    r = system("echo \"  argv0=$0 argc=$#\"");
    out("system echo $0: %s\n", stat_str(r));
    {
        char *oldpath = getenv("PATH") ? strdup(getenv("PATH")) : NULL;
        setenv("PATH", "", 1);
        setenv("SHELL", "/bin/false", 1);
        r = system("echo '  shell found without PATH'");
        out("system with PATH empty, SHELL=/bin/false: %s\n", stat_str(r));
        unsetenv("SHELL");
        if (oldpath)
            setenv("PATH", oldpath, 1);
        free(oldpath);
    }

    /* the caller's state while system waits, as the shell sees it */
    signal(SIGINT, count_sig);
    signal(SIGQUIT, count_sig);
    signal(SIGCHLD, count_sig);
    counts[SIGINT] = counts[SIGQUIT] = counts[SIGCHLD] = 0;
    /* The caller blocks every signal around the spawn itself and restores its mask only after the child has exec'd, so a
     * shell that is fast enough (a loaded machine) reads the transient mask of either library. The state the test is
     * about is the one while the caller waits: poll until the caller sleeps in its wait (State S; during the spawn it is
     * suspended, State D, or running), for up to 20 s. */
    r = system("w=0; while [ $w -lt 2000 ] && ! grep -q '^State:.S' /proc/$PPID/status; do sleep 0.01; w=$((w+1)); done; echo '  caller:'; grep -E '^Sig(Blk|Ign)' /proc/$PPID/status | sed 's/^/    /'; echo '  shell:'; grep -E '^Sig(Blk|Ign)' /proc/$$/status | sed 's/^/    /'");
    out("system /proc status: %s\n", stat_str(r));
    counts[SIGINT] = counts[SIGQUIT] = counts[SIGCHLD] = 0;
    r = system("kill -INT $PPID; kill -QUIT $PPID; kill -CHLD $PPID; exit 0");
    out("system signals to caller: %s int=%d quit=%d chld=%d dispositions after: int=%s quit=%s chld=%s\n", stat_str(r), counts[SIGINT], counts[SIGQUIT], counts[SIGCHLD], disp(SIGINT), disp(SIGQUIT),
        disp(SIGCHLD));
    counts[SIGCHLD] = 0;
    r = system("exit 0");
    out("system: SIGCHLD delivered to caller after return: %d\n", counts[SIGCHLD]);
    {
        sigset_t m;
        sigprocmask(SIG_BLOCK, NULL, &m);
        out("system: caller mask after return has CHLD=%d\n", sigismember(&m, SIGCHLD));
    }
    signal(SIGINT, SIG_DFL);
    signal(SIGQUIT, SIG_DFL);
    signal(SIGCHLD, SIG_DFL);

    /* caller already ignoring SIGINT, blocking USR1 */
    signal(SIGINT, SIG_IGN);
    sigset_t s, old;
    sigemptyset(&s);
    sigaddset(&s, SIGUSR1);
    sigprocmask(SIG_BLOCK, &s, &old);
    r = system("echo '  shell (caller ignores INT, blocks USR1):'; grep -E '^Sig(Blk|Ign)' /proc/$$/status | sed 's/^/    /'");
    out("system: %s int after=%s\n", stat_str(r), disp(SIGINT));
    sigprocmask(SIG_SETMASK, &old, NULL);
    signal(SIGINT, SIG_DFL);

    /* SIGCHLD ignored by the caller */
    signal(SIGCHLD, SIG_IGN);
    errno = 0;
    r = system("exit 5");
    out("system with SIGCHLD ignored: %d (%s) errno=%d\n", r, r == -1 ? "-1" : stat_str(r), errno);
    signal(SIGCHLD, SIG_DFL);

    /* system does not reap other children */
    pid_t c = fork_exit(7);
    sleep_ms(50);
    r = system("exit 1");
    int st;
    pid_t w = waitpid(c, &st, 0);
    out("system leaves other children: system=%s other=%s %s\n", stat_str(r), w == c ? "still-waitable" : "gone", stat_str(st));

    /* concurrent callers: the dispositions stay ignored until the last returns */
    signal(SIGINT, count_sig);
    signal(SIGQUIT, count_sig);
    pthread_t ta, tb;
    struct sysarg sa_ = {"sleep 0.7", -1}, sb_ = {"true", -1};
    pthread_create(&ta, NULL, system_thread, &sa_);
    sleep_ms(100);
    pthread_create(&tb, NULL, system_thread, &sb_);
    sleep_ms(50);
    out("concurrent: during first system, main sees int=%s quit=%s\n", disp(SIGINT), disp(SIGQUIT));
    pthread_join(tb, NULL);
    out("concurrent: second (short) system returned %s; first still running: int=%s quit=%s\n", stat_str(sb_.st), disp(SIGINT), disp(SIGQUIT));
    {
        sigset_t m;
        pthread_sigmask(SIG_BLOCK, NULL, &m);
        out("concurrent: main thread mask has CHLD=%d while another thread is in system\n", sigismember(&m, SIGCHLD));
    }
    pthread_join(ta, NULL);
    out("concurrent: first returned %s; after both: int=%s quit=%s\n", stat_str(sa_.st), disp(SIGINT), disp(SIGQUIT));
    signal(SIGINT, SIG_DFL);
    signal(SIGQUIT, SIG_DFL);
}

static void popens(void)
{
    char line[512];
    FILE *f = popen("echo hi; exit 4", "r");
    out("popen r: %s line=%s", f ? "stream" : "NULL", f && fgets(line, sizeof line, f) ? line : "(none)\n");
    out("  fd cloexec=%s\n", f && fcntl(fileno(f), F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    out("  pclose: %s\n", stat_str(pclose(f)));

    char cmd[1200];
    snprintf(cmd, sizeof cmd, "cat > '%s'", P("popen_w"));
    f = popen(cmd, "w");
    fputs("written through popen\n", f);
    out("popen w: %s fd cloexec=%s\n", f ? "stream" : "NULL", fcntl(fileno(f), F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    out("  pclose: %s\n", stat_str(pclose(f)));
    FILE *g = fopen(P("popen_w"), "r");
    out("  file: %s", g && fgets(line, sizeof line, g) ? line : "(none)\n");
    if (g)
        fclose(g);

    f = popen("exit 0", "re");
    out("popen re: %s fd cloexec=%s\n", f ? "stream" : "NULL", f && fcntl(fileno(f), F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    if (f)
        pclose(f);
    f = popen("cat >/dev/null", "we");
    out("popen we: %s fd cloexec=%s\n", f ? "stream" : "NULL", f && fcntl(fileno(f), F_GETFD) == FD_CLOEXEC ? "yes" : "no");
    if (f)
        pclose(f);
    const char *modes[] = {"", "x", "rw", "wr", "r+", "w+", "rb", "wb", "er", "ee", "rx", "rew", "a"};
    for (size_t i = 0; i < sizeof modes / sizeof *modes; i++) {
        errno = 0;
        f = popen("exit 0", modes[i]);
        int e = errno;
        int fl = f ? fcntl(fileno(f), F_GETFL) & O_ACCMODE : -1;
        out("popen mode \"%s\": %s errno=%d%s\n", modes[i], f ? "stream" : "NULL", e, f ? (fl == O_RDONLY ? " reads" : fl == O_WRONLY ? " writes" : " other") : "");
        if (f)
            pclose(f);
    }
    f = popen("exit 3", "r");
    const char *rd = fgets(line, sizeof line, f) ? "data" : "EOF";
    out("popen exit 3: read=%s pclose=%s\n", rd, stat_str(pclose(f)));
    f = popen("kill -TERM $$", "r");
    out("popen killed: pclose=%s\n", stat_str(pclose(f)));
    f = popen("echo \"argv0=$0\"", "r");
    out("popen $0: %s", fgets(line, sizeof line, f) ? line : "(none)\n");
    pclose(f);

    /* a second popen's child does not inherit the first stream's pipe */
    FILE *p1 = popen("cat >/dev/null", "w");
    FILE *p2 = popen("exit 0", "r");
    FILE *p3 = popen(FDS_OF_SHELL, "r");
    out("concurrent popen: third child's shell fds: %s\n", fgets(line, sizeof line, p3) ? line : "(none)");
    pclose(p3);
    pclose(p2);
    out("concurrent popen: first pclose %s\n", stat_str(pclose(p1)));
    /* other descriptors of the caller are inherited */
    int a = open(P("afile"), O_RDONLY);
    dup2(a, 7);
    close(a);
    p3 = popen(FDS_OF_SHELL, "r");
    out("popen with caller fd 7 open: child fds: %s\n", fgets(line, sizeof line, p3) ? line : "(none)");
    pclose(p3);
    close(7);

    /* pclose of a stream that popen did not return */
    g = fopen(P("afile"), "r");
    errno = 0;
    FILE *volatile gv = g; /* deliberately not from popen */
    int r = pclose(gv);
    SHOWV("pclose of an fopen stream", r);

    /* pclose when the child was reaped elsewhere */
    signal(SIGCHLD, SIG_IGN);
    f = popen("exit 2", "r");
    errno = 0;
    r = pclose(f);
    SHOWV("pclose with SIGCHLD ignored", r);
    signal(SIGCHLD, SIG_DFL);

    /* popen with descriptors 0 and 1 closed: the pipe gets a low number */
    fflush(stdout);
    int p[2];
    if (pipe(p) != 0)
        return;
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        dup2(p[1], 2);
        close(p[1]);
        close(p[0]);
        close(0);
        close(1);
        FILE *q = popen("echo low-fd-read", "r");
        char l[128];
        dprintf(2, "popen r with 0,1 closed: fd=%d read=%s", q ? fileno(q) : -1, q && fgets(l, sizeof l, q) ? l : "(none)\n");
        if (q)
            dprintf(2, "  pclose %s\n", stat_str(pclose(q)));
        q = popen("cat >&2", "w");
        if (q) {
            fputs("low-fd-write\n", q);
            dprintf(2, "popen w with 0,1 closed: fd=%d\n", fileno(q));
            int s2 = pclose(q);
            dprintf(2, "  pclose %s\n", stat_str(s2));
        }
        _exit(0);
    }
    close(p[1]);
    char buf[1024];
    ssize_t n = 0, k;
    while ((k = read(p[0], buf + n, sizeof buf - 1 - (size_t)n)) > 0)
        n += k;
    buf[n > 0 ? n : 0] = 0;
    close(p[0]);
    waitpid(c, NULL, 0);
    out("%s", buf);
}

/* ------------------------------------------------------------------ §18.8 groups, sessions */

static void groups(void)
{
    out("getpgid(0)==getpgrp(): %s\n", getpgid(0) == getpgrp() ? "yes" : "no");
    out("getsid(getpid())==getsid(0): %s\n", getsid(getpid()) == getsid(0) ? "yes" : "no");
    pid_t gone = 4194303;
    while (gone > 300 && (kill(gone, 0) == 0 || errno != ESRCH || getpgid(gone) >= 0))
        gone--;
    errno = 0;
    SHOW("getpgid(nonexistent)", getpgid(gone));
    errno = 0;
    SHOW("getsid(nonexistent)", getsid(gone));
    errno = 0;
    {
        pid_t v = getsid(1);
        int e = errno;
        out("getsid(1) other session: ok=%s errno=%d\n", v >= 0 ? "yes" : "no", e);
        errno = 0;
        v = getpgid(1);
        e = errno;
        out("getpgid(1): ok=%s errno=%d\n", v >= 0 ? "yes" : "no", e);
    }

    pid_t c = fork_ready(NULL, -1);
    SHOW("setpgid(child, child)", setpgid(c, c));
    out("  now own group: %s\n", getpgid(c) == c ? "yes" : "no");
    SHOW("setpgid(child, our group)", setpgid(c, getpgrp()));
    out("  back in our group: %s\n", getpgid(c) == getpgrp() ? "yes" : "no");
    SHOW("setpgid(child, 0)", setpgid(c, 0));
    out("  now own group: %s\n", getpgid(c) == c ? "yes" : "no");
    errno = 0;
    SHOW("setpgid(child, -1)", setpgid(c, -1));
    errno = 0;
    SHOW("setpgid(child, nonexistent group)", setpgid(c, gone));
    errno = 0;
    SHOW("setpgid(1, 1) not a child", setpgid(1, 1));
    errno = 0;
    SHOW("setpgid(nonexistent, 0)", setpgid(gone, 0));
    errno = 0;
    SHOW("setpgid(-1, 0)", setpgid(-1, 0));
    kill(c, SIGKILL);
    waitpid(c, NULL, 0);

    /* a child that has exec'd */
    {
        int rp[2];
        if (pipe(rp) != 0)
            return;
        char fdb[16];
        snprintf(fdb, sizeof fdb, "%d", rp[1]);
        char *argv[] = {"ready", "@ready", fdb, "3000", NULL};
        pid_t e = -1;
        fflush(stdout);
        posix_spawn(&e, self, NULL, NULL, argv, environ);
        close(rp[1]);
        char b;
        if (read(rp[0], &b, 1) < 0) {
        }
        close(rp[0]);
        errno = 0;
        SHOW("setpgid(exec'd child, 0)", setpgid(e, 0));
        kill(e, SIGKILL);
        waitpid(e, NULL, 0);
    }

    /* setsid and friends in a child */
    fflush(stdout);
    int rp[2];
    if (pipe(rp) != 0)
        return;
    c = fork();
    if (c == 0) {
        child_guard();
        close(rp[0]);
        pid_t ppg = getpgid(getppid());
        pid_t me = getpid();
        errno = 0;
        pid_t s = setsid();
        out("child setsid: returns_own_pid=%s errno=%d sid_own=%s pgrp_own=%s\n", s == me ? "yes" : "no", errno, getsid(0) == me ? "yes" : "no", getpgrp() == me ? "yes" : "no");
        errno = 0;
        SHOW("child setsid again", (int)setsid());
        errno = 0;
        SHOW("child setpgid(0,0) as session leader", setpgid(0, 0));
        errno = 0;
        SHOW("child setpgid(0, parent's group) other session", setpgid(0, ppg));
        errno = 0;
        SHOW("child setpgrp as session leader", setpgrp());
        if (write(rp[1], "r", 1) < 0) {
        }
        for (;;)
            pause();
    }
    close(rp[1]);
    char b;
    if (read(rp[0], &b, 1) < 0) {
    }
    close(rp[0]);
    errno = 0;
    SHOW("setpgid(session-leader child, ...)", setpgid(c, getpgrp()));
    errno = 0;
    SHOW("setpgid(child in other session, 0)", setpgid(c, 0));
    kill(c, SIGKILL);
    waitpid(c, NULL, 0);

    fflush(stdout);
    c = fork();
    if (c == 0) {
        child_guard();
        errno = 0;
        int r = setpgrp();
        out("child setpgrp: %d errno=%d own=%s\n", r, errno, getpgrp() == getpid() ? "yes" : "no");
        errno = 0;
        SHOW("child setsid as group leader", (int)setsid());
        errno = 0;
        SHOW("child setpgid(0, parent's group) same session", setpgid(0, getpgid(getppid())));
        errno = 0;
        out("child back in parent's group: %s\n", getpgrp() == getpgid(getppid()) ? "yes" : "no");
        _exit(0);
    }
    waitpid(c, NULL, 0);
}

/* daemon(nochdir, noclose) in a child; this probe is a child subreaper so the daemon is
 * reparented here and can be waited for. */
static void daemon_atexit(void) { dprintf(9, "atexit-ran "); }

static void daemons(void)
{
    for (int nochdir = 0; nochdir < 2; nochdir++)
        for (int noclose = 0; noclose < 2; noclose++) {
            int rp[2];
            if (pipe(rp) != 0)
                return;
            fflush(stdout);
            pid_t d = fork();
            if (d == 0) {
                child_guard();
                close(rp[0]);
                dup2(rp[1], 9);
                close(rp[1]);
                if (chdir(P("sub")) != 0)
                    _exit(90);
                int a = open(P("afile"), O_RDWR);
                dup2(a, 0);
                dup2(a, 1);
                dup2(a, 2);
                close(a);
                pid_t me = getpid();
                errno = 0;
                int r = daemon(nochdir, noclose);
                int e = errno;
                pid_t now = getpid();
                for (int i = 0; i < 200 && getppid() == me; i++)
                    sleep_ms(5);
                char cwd[1024], ncwd[1024];
                if (!getcwd(cwd, sizeof cwd))
                    strcpy(cwd, "?");
                norm(ncwd, sizeof ncwd, cwd, dir);
                struct stat dn, f;
                stat("/dev/null", &dn);
                char fds[64] = "";
                for (int fd = 0; fd < 3; fd++) {
                    const char *w = fstat(fd, &f) != 0 ? "closed" : (S_ISCHR(f.st_mode) && f.st_rdev == dn.st_rdev) ? "null" : "kept";
                    strcat(fds, w);
                    if (fd < 2)
                        strcat(fds, ",");
                }
                int nullfd_flags = fcntl(0, F_GETFL) & O_ACCMODE;
                dprintf(9, "daemon(%d,%d): r=%d errno=%d new_process=%s reparented_to_probe=%s sid_own=%s pgrp_own=%s cwd=%s fds=%s fd0_mode=%s\n", nochdir, noclose, r, e,
                        now != me ? "yes" : "no", getppid() == (pid_t)atol(getenv("PROBE_PID")) ? "yes" : "no", getsid(0) == now ? "yes" : "no", getpgrp() == now ? "yes" : "no", ncwd, fds,
                        nullfd_flags == O_RDWR ? "rw" : nullfd_flags == O_RDONLY ? "r" : "w");
                _exit(0);
            }
            close(rp[1]);
            int st;
            waitpid(d, &st, 0);
            char buf[1024];
            ssize_t n = 0, k;
            while ((k = read(rp[0], buf + n, sizeof buf - 1 - (size_t)n)) > 0)
                n += k;
            buf[n > 0 ? n : 0] = 0;
            close(rp[0]);
            out("%s", buf);
            out("  daemon's original process: %s\n", stat_str(st));
            int st2;
            pid_t w = waitpid(-1, &st2, 0);
            out("  daemon process reaped by the probe: %s %s\n", w > 0 ? "yes" : "no", w > 0 ? stat_str(st2) : "");
        }

    /* stdio buffers at daemon(): does the exiting parent flush them? */
    int rp[2];
    if (pipe(rp) != 0)
        return;
    fflush(stdout);
    pid_t d = fork();
    if (d == 0) {
        child_guard();
        close(rp[0]);
        dup2(rp[1], 1);
        close(rp[1]);
        static char vb[256];
        setvbuf(stdout, vb, _IOFBF, sizeof vb);
        printf("X");
        if (daemon(1, 1) != 0)
            _exit(1);
        printf("Y");
        fflush(stdout);
        _exit(0);
    }
    close(rp[1]);
    waitpid(d, NULL, 0);
    char buf[64];
    ssize_t n = 0, k;
    while ((k = read(rp[0], buf + n, sizeof buf - 1 - (size_t)n)) > 0)
        n += k;
    buf[n > 0 ? n : 0] = 0;
    close(rp[0]);
    waitpid(-1, NULL, 0);
    out("daemon with unflushed stdout \"X\", child writes \"Y\": pipe got \"%s\"\n", buf);

    /* atexit handlers in the exiting parent */
    if (pipe(rp) != 0)
        return;
    fflush(stdout);
    d = fork();
    if (d == 0) {
        child_guard();
        close(rp[0]);
        dup2(rp[1], 9);
        close(rp[1]);
        atexit(daemon_atexit);
        if (daemon(1, 1) != 0)
            _exit(1);
        _exit(0);
    }
    close(rp[1]);
    waitpid(d, NULL, 0);
    n = 0;
    while ((k = read(rp[0], buf + n, sizeof buf - 1 - (size_t)n)) > 0)
        n += k;
    buf[n > 0 ? n : 0] = 0;
    close(rp[0]);
    waitpid(-1, NULL, 0);
    out("daemon: atexit handlers in the exiting parent: \"%s\"\n", buf);
}

/* ------------------------------------------------------------------ §18.9 */

static long peek_word = -1;

static void attributes(void)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        char name[32] = "";
        errno = 0;
        int r = prctl(PR_SET_NAME, "abcdefghijklmnopqrstuvwxyz");
        prctl(PR_GET_NAME, name);
        out("prctl PR_SET_NAME long: %d errno=%d get=%s\n", r, errno, name);
        out("prctl PR_GET_DUMPABLE: %d\n", prctl(PR_GET_DUMPABLE));
        SHOW("prctl PR_SET_DUMPABLE 0", prctl(PR_SET_DUMPABLE, 0));
        out("prctl PR_GET_DUMPABLE after: %d\n", prctl(PR_GET_DUMPABLE));
        errno = 0;
        SHOW("prctl PR_SET_DUMPABLE 2", prctl(PR_SET_DUMPABLE, 2));
        int v = -1;
        errno = 0;
        r = prctl(PR_SET_PDEATHSIG, SIGUSR1);
        prctl(PR_GET_PDEATHSIG, &v);
        out("prctl PR_SET_PDEATHSIG USR1: %d errno=%d get=%d\n", r, errno, v);
        errno = 0;
        SHOW("prctl PR_SET_PDEATHSIG 99", prctl(PR_SET_PDEATHSIG, 99));
        errno = 0;
        SHOW("prctl unknown option", prctl(0x7fff1234, 0, 0, 0, 0));
        SHOW("prctl PR_SET_NO_NEW_PRIVS 1", prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0));
        out("prctl PR_GET_NO_NEW_PRIVS after: %d\n", prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0));
        errno = 0;
        SHOW("prctl PR_SET_NO_NEW_PRIVS with nonzero arg3", prctl(PR_SET_NO_NEW_PRIVS, 1, 1, 0, 0));
        errno = 0;
        r = prctl(PR_GET_CHILD_SUBREAPER, &v);
        out("prctl PR_GET_CHILD_SUBREAPER (inherited? no): %d errno=%d value=%d\n", r, errno, v);
        int p0 = personality(0xffffffff);
        errno = 0;
        int p1 = personality(ADDR_NO_RANDOMIZE);
        int p2 = personality(0xffffffff);
        out("personality: query=0x%x set ADDR_NO_RANDOMIZE returns=0x%x errno=%d query=0x%x\n", p0, p1, errno, p2);
        errno = 0;
        p1 = personality(PER_LINUX32);
        p2 = personality(0xffffffff);
        out("personality PER_LINUX32: returns=0x%x errno=%d query=0x%x\n", p1, errno, p2);
        errno = 0;
        p1 = personality(0x7fffffff);
        int e = errno;
        p2 = personality(0xffffffff);
        out("personality 0x7fffffff: returns=0x%x errno=%d query=0x%x\n", p1, e, p2);
        _exit(0);
    }
    waitpid(c, NULL, 0);

    /* ptrace PEEK: the word is the return value; -1 needs errno to tell */
    fflush(stdout);
    c = fork();
    if (c == 0) {
        child_guard();
        ptrace(PTRACE_TRACEME, 0, NULL, NULL);
        raise(SIGSTOP);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
    errno = 0;
    long w = ptrace(PTRACE_PEEKDATA, c, &peek_word, NULL);
    int e1 = errno;
    errno = 0;
    long w2 = ptrace(PTRACE_PEEKDATA, c, (void *)8, NULL);
    int e2 = errno;
    errno = 0;
    long w3 = ptrace(PTRACE_PEEKUSER, c, (void *)1, NULL);
    int e3 = errno;
    out("ptrace PEEKDATA of a -1 word: %ld errno=%d; bad address: %ld errno=%d; PEEKUSER misaligned: %ld errno=%d\n", w, e1, w2, e2, w3, e3);
    ptrace(PTRACE_CONT, c, NULL, NULL);
    waitpid(c, &st, 0);
    out("  traced child: %s\n", stat_str(st));
}

/* ------------------------------------------------------------------ tty (addtcsetpgrp) */

static void tty_actions(void)
{
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        setsid();
        int m = posix_openpt(O_RDWR | O_NOCTTY);
        if (m < 0 || grantpt(m) != 0 || unlockpt(m) != 0) {
            out("tty: no pty\n");
            _exit(0);
        }
        int s = open(ptsname(m), O_RDWR); /* becomes the controlling terminal */
        dup2(s, 10);
        close(s);
        s = 10;
        out("tty: controlling, our group foreground: %s\n", tcgetpgrp(s) == getpgrp() ? "yes" : "no");
        char *fargv[] = {"fg", "@fg", "10", NULL};
        posix_spawn_file_actions_t fa;
        posix_spawnattr_t at;
        posix_spawn_file_actions_init(&fa);
        posix_spawn_file_actions_addtcsetpgrp_np(&fa, s);
        posix_spawnattr_init(&at);
        posix_spawnattr_setflags(&at, POSIX_SPAWN_SETPGROUP);
        posix_spawnattr_setpgroup(&at, 0);
        sp("tty: SETPGROUP 0 + addtcsetpgrp", 0, self, &fa, &at, fargv, NULL);
        out("  after: foreground is our group: %s\n", tcgetpgrp(s) == getpgrp() ? "yes" : "no");
        signal(SIGTTOU, SIG_IGN);
        tcsetpgrp(s, getpgrp());
        signal(SIGTTOU, SIG_DFL);
        sp("tty: addtcsetpgrp without SETPGROUP", 0, self, &fa, NULL, fargv, NULL);
        out("  after: foreground is our group: %s\n", tcgetpgrp(s) == getpgrp() ? "yes" : "no");
        posix_spawnattr_destroy(&at);
        posix_spawn_file_actions_destroy(&fa);
        /* a terminal that is not the controlling one */
        int m2 = posix_openpt(O_RDWR | O_NOCTTY);
        grantpt(m2);
        unlockpt(m2);
        int s2 = open(ptsname(m2), O_RDWR | O_NOCTTY);
        posix_spawn_file_actions_init(&fa);
        posix_spawn_file_actions_addtcsetpgrp_np(&fa, s2);
        sp("tty: addtcsetpgrp on a non-controlling tty", 0, self, &fa, NULL, rep_argv, NULL);
        posix_spawn_file_actions_destroy(&fa);
        posix_spawn_file_actions_init(&fa);
        posix_spawn_file_actions_addtcsetpgrp_np(&fa, m);
        sp("tty: addtcsetpgrp on the master", 0, self, &fa, NULL, rep_argv, NULL);
        posix_spawn_file_actions_destroy(&fa);
        close(s2);
        close(m2);
        close(s);
        close(m);
        _exit(0);
    }
    waitpid(c, NULL, 0);
}

/* ------------------------------------------------------------------ privileged phases */

static void root_phase(void)
{
    set_rel_env();
    setenv("PROBE_SHOWIDS", "1", 1);
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        child_guard();
        if (setresgid(65534, 0, 0) != 0 || setresuid(65534, 0, 0) != 0) {
            out("root: setres*id failed %d\n", errno);
            _exit(0);
        }
        posix_spawnattr_t at;
        posix_spawnattr_init(&at);
        sp("root: real 65534 effective 0, no RESETIDS", 0, self, NULL, &at, rep_argv, NULL);
        posix_spawnattr_setflags(&at, POSIX_SPAWN_RESETIDS);
        sp("root: RESETIDS", 0, self, NULL, &at, rep_argv, NULL);
        posix_spawnattr_destroy(&at);
        _exit(0);
    }
    waitpid(c, NULL, 0);
    unsetenv("PROBE_SHOWIDS");
    posix_spawnattr_t at;
    posix_spawnattr_init(&at);
    struct sched_param prm = {1};
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSCHEDULER);
    posix_spawnattr_setschedpolicy(&at, SCHED_FIFO);
    posix_spawnattr_setschedparam(&at, &prm);
    sp("root: SETSCHEDULER FIFO/1", 0, self, NULL, &at, rep_argv, NULL);
    prm.sched_priority = 0;
    posix_spawnattr_setschedparam(&at, &prm);
    sp("root: SETSCHEDULER FIFO/0", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSCHEDPARAM);
    prm.sched_priority = 1;
    posix_spawnattr_setschedparam(&at, &prm);
    sp("root: SETSCHEDPARAM 1 under OTHER", 0, self, NULL, &at, rep_argv, NULL);
    posix_spawnattr_destroy(&at);
}

static void noshell_phase(void)
{
    set_rel_env();
    errno = 0;
    int r = system(NULL);
    SHOWV("noshell: system(NULL)", r);
    errno = 0;
    r = system("exit 3");
    out("noshell: system exit 3: %d (%s) errno=%d\n", r, r == -1 ? "-1" : stat_str(r), errno);
    errno = 0;
    FILE *f = popen("echo hi", "r");
    int e = errno;
    char line[64];
    out("noshell: popen: %s errno=%d read=%s\n", f ? "stream" : "NULL", e, f && fgets(line, sizeof line, f) ? "data" : "EOF");
    if (f) {
        errno = 0;
        r = pclose(f);
        out("noshell: pclose: %d (%s) errno=%d\n", r, r == -1 ? "-1" : stat_str(r), errno);
    }
    struct ex x = {"noshell: execvp ENOEXEC fallback", K_EXECVP, "plain", "bin_plain", 0, NULL, {"plain", "a", NULL}, NULL, NULL};
    capture(x.label, ex_child, &x);
    char *argv[] = {"sh", "-c", "exit 0", NULL};
    sp("noshell: posix_spawn /bin/sh", 0, "/bin/sh", NULL, NULL, argv, NULL);
}

int main(int argc, char **argv)
{
    if (argc > 1 && argv[1][0] == '@')
        return helper(argc, argv);
    snprintf(dir, sizeof dir, "%s", argc > 1 ? argv[1] : "/tmp/process_diff");
    ssize_t n = readlink("/proc/self/exe", self, sizeof self - 1);
    self[n > 0 ? n : 0] = 0;
    /* a clean start: no inherited descriptors above 2, default dispositions, empty mask */
    for (int fd = 3; fd < 1024; fd++)
        close(fd);
    for (int s = 1; s < 65; s++)
        if (s != SIGKILL && s != SIGSTOP)
            signal(s, SIG_DFL);
    sigset_t none;
    sigemptyset(&none);
    sigprocmask(SIG_SETMASK, &none, NULL);
    umask(022);
    setenv("PATH", "/usr/bin:/bin", 1);
    unsetenv("X");
    char pidb[32];
    snprintf(pidb, sizeof pidb, "%d", (int)getpid());
    setenv("PROBE_PID", pidb, 1);
    prctl(PR_SET_CHILD_SUBREAPER, 1);
    const char *phase = argc > 2 ? argv[2] : "";
    mkdir(dir, 0755);
    {
        char abs[PATH_MAX];
        if (realpath(dir, abs))
            snprintf(dir, sizeof dir, "%s", abs);
    }
    if (strcmp(phase, "root") != 0 && chdir(dir) != 0)
        out("chdir to the scratch directory failed\n");
    if (strcmp(phase, "root") == 0) {
        root_phase();
    } else if (strcmp(phase, "noshell") == 0) {
        noshell_phase();
    } else {
        mkdir(dir, 0755);
        fixture();
        set_rel_env();
        out("== exec search\n");
        exec_search();
        capture("execvp ETXTBSY then ok", busy_child, NULL);
        capture("execv ETXTBSY", busy_only_child, NULL);
        out("== exec limits\n");
        exec_limits();
        out("== fexecve, execveat\n");
        exec_fd();
        out("== exec keeps\n");
        capture("exec keeps (parent: ignores 10, catches 12, blocks 15 with 15 pending, fd 7, fd 8 cloexec, cwd sub)", keep_child, NULL);
        out("== spawn file actions\n");
        fa_errors();
        fa_fail();
        fa_ok();
        out("== spawn exec failures\n");
        exec_fail_spawn();
        out("== spawn attributes\n");
        show_attr_defaults();
        attrs();
        spawn_window(0);
        spawn_window(1);
        spawn_threads();
        out("== pidfd_spawn\n");
        pidfd_spawns();
        out("== tty actions\n");
        tty_actions();
        out("== pidfd\n");
        pidfds();
        out("== wait\n");
        waits();
        out("== system\n");
        systems();
        out("== popen\n");
        popens();
        out("== groups\n");
        groups();
        out("== daemon\n");
        daemons();
        out("== prctl, personality, ptrace\n");
        attributes();
    }
    out("done\n");
    return 0;
}
