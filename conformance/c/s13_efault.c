/* Spec C0.1 (clauses s0_6_c0_1_1, s0_6_c0_1_3): a path-taking call given a pointer the process
 * cannot read (PROT_NONE page, address 1, text that runs into an unmapped page) fails with -1 and
 * EFAULT, as the kernel answers, and does not crash: the library hands the pointer to the kernel
 * before reading it. Each trial runs in a forked child, so a crash only costs that line.
 * No argument: check, print the lines that do not say EFAULT (a crash is SIGnn), exit 1 if any.
 * Argument "all": print every line, including the calls recorded in spec/behaviour/efault.md as
 * different (those are skipped by the check) and exit 0. Public interfaces only. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/statfs.h>
#include <sys/statvfs.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <utime.h>
#include <sys/time.h>
#include "expect_crash.h"

static char *none;      /* start of a PROT_NONE page */
static char *runoff;    /* 'a' bytes up to the end of a page, PROT_NONE after: never terminated */
static const char *good = "/nonexistent-efault-probe";

static char *bad(int kind) { return kind == 0 ? none : kind == 1 ? (char *)1 : runoff; }
static const char *kname[] = {"PROT_NONE", "ptr1", "unterminated"};

#define R(x) (int)(x)
/* Trials: expression using P (bad path pointer) and G (good path). */
#define PATHCALLS(X) \
    X(access, R(access(P, F_OK))) \
    X(euidaccess, R(euidaccess(P, F_OK))) \
    X(faccessat, R(faccessat(AT_FDCWD, P, F_OK, 0))) \
    X(open, R(open(P, O_RDONLY))) \
    X(open_creat, R(open(P, O_RDONLY | O_CREAT | O_EXCL, 0600))) \
    X(openat, R(openat(AT_FDCWD, P, O_RDONLY))) \
    X(creat, R(creat(P, 0600))) \
    X(stat, R(stat(P, &sb))) \
    X(lstat, R(lstat(P, &sb))) \
    X(fstatat, R(fstatat(AT_FDCWD, P, &sb, 0))) \
    X(unlink, R(unlink(P))) \
    X(unlinkat, R(unlinkat(AT_FDCWD, P, 0))) \
    X(mkdir, R(mkdir(P, 0700))) \
    X(rmdir, R(rmdir(P))) \
    X(rename_old, R(rename(P, G))) \
    X(rename_new, R(rename(G, P))) \
    X(chdir, R(chdir(P))) \
    X(chmod, R(chmod(P, 0600))) \
    X(chown, R(chown(P, 0, 0))) \
    X(lchown, R(lchown(P, 0, 0))) \
    X(readlink, R(readlink(P, lb, sizeof lb))) \
    X(symlink_target, R(symlink(P, G))) \
    X(symlink_link, R(symlink(G, P))) \
    X(link_old, R(link(P, G))) \
    X(link_new, R(link("/etc/passwd", P))) \
    X(truncate, R(truncate(P, 0))) \
    X(execve, R(execve(P, av, ev))) \
    X(statfs, R(statfs(P, &sf))) \
    X(statvfs, R(statvfs(P, &sv))) \
    X(utimensat, R(utimensat(AT_FDCWD, P, NULL, 0))) \
    X(utime, R(utime(P, NULL))) \
    X(utimes, R(utimes(P, NULL))) \
    X(mknod, R(mknod(P, S_IFIFO | 0600, 0))) \
    X(mkfifo, R(mkfifo(P, 0600))) \
    X(opendir, opendir(P) ? 0 : -1) \
    X(fopen, fopen(P, "r") ? 0 : -1) \
    X(realpath, realpath(P, rp) ? 0 : -1) \
    X(chroot, R(chroot(P))) \
    X(acct, R(acct(P)))

static int trial(int which, int kind) {
    struct stat sb; struct statfs sf; struct statvfs sv; char lb[64]; char rp[PATH_MAX];
    char *av[] = {"x", NULL}, *ev[] = {NULL};
    const char *G = good;
    char *P = bad(kind);
    int i = 0;
    (void)sb; (void)sf; (void)sv; (void)lb; (void)rp; (void)av; (void)ev; (void)G;
#define X(name, expr) if (which == i++) return (expr);
    PATHCALLS(X)
#undef X
    /* Buffer-taking calls: P is a bad output or input buffer. */
    if (which == i++) return (int)(getcwd(P, 64) ? 0 : -1);
    if (which == i++) return R(gethostname(P, 64));
    if (which == i++) return R(getdomainname(P, 64));
    if (which == i++) return R(read(0, P, 8));
    if (which == i++) return R(write(1, P, 0) < 0 ? -1 : write(-1, P, 8));
    if (which == i++) return R(stat(".", (struct stat *)P));
    if (which == i++) return R(readlink("/proc/self/exe", P, 64));
    return -2;
}

static int count(void) {
    int n = 0;
#define X(name, expr) n++;
    PATHCALLS(X)
#undef X
    return n + 7;
}
static const char *name(int which) {
    int i = 0;
#define X(nm, expr) if (which == i++) return #nm;
    PATHCALLS(X)
#undef X
    static const char *more[] = {"getcwd_buf", "gethostname_buf", "getdomainname_buf", "read_buf",
                                 "write_buf", "stat_statbuf", "readlink_buf"};
    return more[which - i];
}

int main(int argc, char **argv) {
    int check = !(argc > 1 && !strcmp(argv[1], "all"));
    long pg = sysconf(_SC_PAGESIZE);
    char *m = mmap(NULL, 4 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED) return 90;
    none = m;
    mprotect(m, pg, PROT_NONE);
    runoff = m + 3 * pg - 8;
    memset(m + 2 * pg, 'a', pg);   /* page 2: readable */
    mprotect(m + 3 * pg, pg, PROT_NONE);
    int fails = 0, total = 0;
    for (int w = 0; w < count(); w++) {
        const char *nm = name(w);
        /* Recorded differences (efault.md): the reference crashes too (realpath, gethostname,
           getdomainname) or answers EFAULT only because the kernel fills the buffer (stat). */
        if (check && (!strcmp(nm, "realpath") || !strcmp(nm, "gethostname_buf") || !strcmp(nm, "getdomainname_buf")
                      || !strcmp(nm, "stat_statbuf")))
            continue;
        if (!strcmp(nm, "acct") || !strcmp(nm, "chroot") || !strcmp(nm, "read_buf") || !strcmp(nm, "write_buf"))
            continue; /* privileged or blocking: not probed */
        for (int k = 0; k < 3; k++) {
            const int buffer_call = w >= count() - 7;
            if (buffer_call && k == 2) continue;
            if (!strcmp(nm, "stat_statbuf") && k != 0) continue;
            fflush(stdout);
            pid_t pid = fork();
            if (pid == 0) {
                expect_crash();
                alarm(5); errno = 0;
                int r = trial(w, k);
                int e = errno;
                _exit(r >= 0 ? 99 : (e > 0 && e < 99 ? e : 98));
            }
            int st;
            waitpid(pid, &st, 0);
            char res[32];
            if (WIFSIGNALED(st)) snprintf(res, sizeof res, "SIG%d", WTERMSIG(st));
            else if (WEXITSTATUS(st) == 99) strcpy(res, "OK");
            else if (WEXITSTATUS(st) == EFAULT) strcpy(res, "EFAULT");
            else snprintf(res, sizeof res, "errno%d", WEXITSTATUS(st));
            if (strcmp(res, "EFAULT")) fails++;
            if (!check || strcmp(res, "EFAULT")) printf("%s %s: %s\n", nm, kname[k], res);
            total++;
        }
    }
    if (check) printf("cases %d, not EFAULT %d\n", total, fails);
    return check ? (fails != 0) : 0;
}
