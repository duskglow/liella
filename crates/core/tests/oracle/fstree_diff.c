/* The working directory, directory streams, scandir, the tree walkers, path canonicalization and
 * temporary names end to end (spec §13.1-13.3, §13.8-13.9; phase 12). Built against the host C
 * library and against Liella's products, the outputs must match line for line. Black-box: every
 * line is a result, an errno, or a fact read back through a public function. Scratch paths are
 * printed relative to the scratch directory as <dir>; a 200-byte component of the deep tree is
 * printed as {d200}; nothing prints an inode, a descriptor number, a directory cookie or a time.
 *
 * Directory order: wherever a line lists entries in the order the directory produced them (readdir,
 * unsorted scandir, ftw/nftw, fts without a comparison function) the line names the file system of
 * the scratch directory (statfs f_type), as the owner's rule of 2026-10-03 requires. Sorted views
 * are labelled "sorted".
 *
 * Usage: fstree_diff DIR [root], normally through fstree_run.sh. DIR must be an absolute path
 * without symbolic links (the harness passes `pwd -P`). Without a second argument the unprivileged
 * cases run (the permission cases need a non-root user) and the fixture is written into DIR and
 * removed at the end. `root` runs the cases that need privileges (chroot, a lazy unmount, nested
 * mounts for FTW_MOUNT/FTS_XDEV, device nodes); the harness runs it as root in a private mount
 * namespace with a tmpfs mounted over DIR, so nothing it creates reaches a disk. `exhaust FUNC`
 * (outside the record) makes one temporary-name call in DIR, for running under strace's fault
 * injection (spec/behaviour/fstree.md, "Exhaustion under fault injection"). */
#define _GNU_SOURCE
/* The plain functions are probed, not the __*_chk forms that _FORTIFY_SOURCE (on by default in
 * Ubuntu's compiler at -O1) would substitute. */
#undef _FORTIFY_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <fts.h>
#include <ftw.h>
#include <limits.h>
#include <locale.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/statfs.h>
#include <sys/sysmacros.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>
#include "tscale.h"
#include "../../../../conformance/c/expect_crash.h" /* the expected-crash helper (CORE-STORM) */

#define DEPTH 25 /* deep tree: DEPTH components of 200 bytes (about 5 KB, over PATH_MAX) */
#define WALK_CAP 400 /* most lines one walk prints */

static char top[PATH_MAX];
static size_t toplen;
static char fsname[96];
static char LONGN[201];

/* ---------------------------------------------------------------- formatting helpers */

static const char *en(int e)
{
    static char b[4][24];
    static int k;
    switch (e) {
    case 0: return "0";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case EBADF: return "EBADF";
    case ENOMEM: return "ENOMEM";
    case EACCES: return "EACCES";
    case EEXIST: return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EISDIR: return "EISDIR";
    case EINVAL: return "EINVAL";
    case ERANGE: return "ERANGE";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case ELOOP: return "ELOOP";
    case EOVERFLOW: return "EOVERFLOW";
    case EINTR: return "EINTR";
    case ENOTEMPTY: return "ENOTEMPTY";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    case ENXIO: return "ENXIO";
    case EFAULT: return "EFAULT";
    case ESRCH: return "ESRCH";
    }
    k = (k + 1) % 4;
    snprintf(b[k], sizeof b[k], "errno%d", e);
    return b[k];
}

/* A path for printing: the scratch prefix as <dir>, each 200-byte deep component as {d200}, and
 * names longer than 60 bytes otherwise as <c>*<n>. */
static const char *rel(const char *p)
{
    static char bufs[6][4096];
    static int k;
    if (!p)
        return "NULL";
    k = (k + 1) % 6;
    char *o = bufs[k];
    size_t n = 0;
    const char *s = p;
    if (strncmp(s, "(unreachable)", 13) == 0) {
        n = (size_t)sprintf(o, "(unreachable)");
        s += 13;
    }
    if (toplen && strncmp(s, top, toplen) == 0 && (s[toplen] == 0 || s[toplen] == '/')) {
        n += (size_t)sprintf(o + n, "<dir>");
        s += toplen;
    }
    while (*s && n < sizeof bufs[0] - 40) {
        if (strncmp(s, LONGN, 200) == 0) {
            n += (size_t)sprintf(o + n, "{d200}");
            s += 200;
            continue;
        }
        size_t run = 1;
        while (s[run] == s[0])
            run++;
        if (run > 60) {
            n += (size_t)sprintf(o + n, "<%c>*%zu", s[0], run);
            s += run;
            continue;
        }
        o[n++] = *s++;
    }
    o[n] = 0;
    if (*s)
        strcat(o, "...");
    return o;
}

static const char *kind(mode_t m)
{
    switch (m & S_IFMT) {
    case S_IFREG: return "reg";
    case S_IFDIR: return "dir";
    case S_IFLNK: return "lnk";
    case S_IFIFO: return "fifo";
    case S_IFSOCK: return "sock";
    case S_IFCHR: return "chr";
    case S_IFBLK: return "blk";
    }
    return "?";
}

static const char *dtname(unsigned char t)
{
    switch (t) {
    case DT_UNKNOWN: return "UNKNOWN";
    case DT_REG: return "REG";
    case DT_DIR: return "DIR";
    case DT_LNK: return "LNK";
    case DT_FIFO: return "FIFO";
    case DT_SOCK: return "SOCK";
    case DT_CHR: return "CHR";
    case DT_BLK: return "BLK";
    }
    return "?";
}

static const char *fsname_of(const char *p)
{
    static char b[96];
    struct statfs s;
    if (statfs(p, &s) != 0) {
        snprintf(b, sizeof b, "statfs failed %s", en(errno));
        return b;
    }
    const char *n = "unknown";
    switch ((unsigned long)s.f_type) {
    case 0xEF53UL: n = "ext2/ext3"; break; /* the ext2/3/4 family; `stat -f -c %T` prints ext2/ext3 */
    case 0x01021994UL: n = "tmpfs"; break;
    case 0x9123683EUL: n = "btrfs"; break;
    case 0x58465342UL: n = "xfs"; break;
    case 0x2FC12FC1UL: n = "zfs"; break;
    case 0x794C7630UL: n = "overlayfs"; break;
    case 0xF2F52010UL: n = "f2fs"; break;
    case 0x6969UL: n = "nfs"; break;
    }
    snprintf(b, sizeof b, "%s (f_type 0x%lx)", n, (unsigned long)s.f_type);
    return b;
}

static void mkf(const char *p)
{
    int fd = open(p, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0)
        printf("fixture: cannot create %s: %s\n", rel(p), en(errno));
    else
        close(fd);
}

static void mkd(const char *p, mode_t m)
{
    if (mkdir(p, m) != 0)
        printf("fixture: cannot mkdir %s: %s\n", rel(p), en(errno));
}

static void sln(const char *target, const char *p)
{
    if (symlink(target, p) != 0)
        printf("fixture: cannot symlink %s: %s\n", rel(p), en(errno));
}

static void mksock(const char *p)
{
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a;
    memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX;
    snprintf(a.sun_path, sizeof a.sun_path, "%s", p);
    if (bind(s, (struct sockaddr *)&a, sizeof a) != 0)
        printf("fixture: cannot bind %s: %s\n", p, en(errno));
    close(s);
}

static void back_to_top(void)
{
    if (chdir(top) != 0)
        printf("cannot return to <dir>: %s\n", en(errno));
}

static const char *cwd_now(void)
{
    static char c[8192];
    errno = 0;
    char *r = getcwd(c, sizeof c);
    return r ? rel(r) : en(errno);
}

static int count_fds(void)
{
    int n = 0;
    for (int fd = 0; fd < 1024; fd++)
        if (fcntl(fd, F_GETFD) != -1)
            n++;
    return n;
}

/* ---------------------------------------------------------------- fixture */

/* t: the walk tree.
 *   t/a/{f1, sub/x, s_up -> ..}, t/b, t/e/ (empty), t/p (FIFO), t/so (socket),
 *   t/s_file -> b, t/s_dir -> a, t/s_dangling -> nowhere, and lp/s_loop -> s_loop (a self loop, outside t),
 *   t/noread/ (0333, holds nr_f), t/nosearch/ (0600, holds ns_f).
 * u: the same as t without noread, nosearch, p and so (for FTW_CHDIR, which stops at nosearch).
 * lp: lp/f and lp/s_loop -> s_loop. */
static void make_t(void)
{
    mkd("t", 0755);
    mkd("t/a", 0755);
    mkf("t/a/f1");
    mkd("t/a/sub", 0755);
    mkf("t/a/sub/x");
    sln("..", "t/a/s_up");
    mkf("t/b");
    mkd("t/e", 0755);
    if (mkfifo("t/p", 0644) != 0)
        printf("fixture: mkfifo %s\n", en(errno));
    mksock("t/so");
    sln("b", "t/s_file");
    sln("a", "t/s_dir");
    sln("nowhere", "t/s_dangling");
    mkd("lp", 0755);
    sln("s_loop", "lp/s_loop");
    mkf("lp/f");
    mkd("u", 0755);
    mkd("u/a", 0755);
    mkf("u/a/f1");
    mkd("u/a/sub", 0755);
    mkf("u/a/sub/x");
    sln("..", "u/a/s_up");
    mkf("u/b");
    mkd("u/e", 0755);
    sln("b", "u/s_file");
    sln("a", "u/s_dir");
    sln("nowhere", "u/s_dangling");
    mkd("t/noread", 0755);
    mkf("t/noread/nr_f");
    mkd("t/nosearch", 0755);
    mkf("t/nosearch/ns_f");
    chmod("t/noread", 0333);
    chmod("t/nosearch", 0600);
}

static void build_deep(void)
{
    mkd("deep", 0755);
    int fd = open("deep", O_RDONLY | O_DIRECTORY);
    for (int i = 0; i < DEPTH && fd >= 0; i++) {
        if (mkdirat(fd, LONGN, 0755) != 0)
            printf("fixture: deep mkdirat %d: %s\n", i, en(errno));
        int n = openat(fd, LONGN, O_RDONLY | O_DIRECTORY);
        close(fd);
        fd = n;
    }
    if (fd >= 0) {
        int f = openat(fd, "bottom", O_WRONLY | O_CREAT, 0644);
        if (f >= 0)
            close(f);
        close(fd);
    }
}

/* Removes the deep tree through descriptors (no path ever exceeds one component). */
static void remove_deep(void)
{
    int fds[DEPTH + 1];
    fds[0] = open("deep", O_RDONLY | O_DIRECTORY);
    int got = fds[0] >= 0 ? 1 : 0;
    for (int i = 1; i <= DEPTH && got == i; i++) {
        fds[i] = openat(fds[i - 1], LONGN, O_RDONLY | O_DIRECTORY);
        if (fds[i] >= 0)
            got++;
    }
    if (got == DEPTH + 1)
        unlinkat(fds[DEPTH], "bottom", 0);
    for (int i = got - 1; i >= 1; i--) {
        close(fds[i]);
        unlinkat(fds[i - 1], LONGN, AT_REMOVEDIR);
    }
    if (got)
        close(fds[0]);
    if (rmdir("deep") != 0)
        printf("cleanup: deep tree left behind: %s\n", en(errno));
}

/* Opens deep/ plus n components, returning the descriptor. */
static int open_deep_level(int n)
{
    int fd = open("deep", O_RDONLY | O_DIRECTORY);
    for (int i = 0; i < n && fd >= 0; i++) {
        int x = openat(fd, LONGN, O_RDONLY | O_DIRECTORY);
        close(fd);
        fd = x;
    }
    return fd;
}

/* ---------------------------------------------------------------- §13.1 working directory */

static void show_getcwd(const char *label, char *buf, size_t size, size_t bufsize)
{
    if (buf) {
        memset(buf, 'Z', bufsize);
        buf[bufsize - 1] = 0;
    }
    errno = 0;
    char *r = getcwd(buf, size);
    int e = errno;
    if (r) {
        printf("%s: %s%s\n", label, rel(r), buf ? (r == buf ? " ret==buf" : " ret!=buf") : "");
        if (!buf)
            free(r);
    } else {
        printf("%s: NULL errno=%s", label, en(e));
        if (buf)
            printf(" buf=%s", buf[0] == 'Z' ? "untouched" : rel(buf));
        printf("\n");
    }
}

static void show_gcdn(const char *label)
{
    errno = 0;
    char *p = get_current_dir_name();
    int e = errno;
    if (p)
        printf("%s: %s\n", label, rel(p));
    else
        printf("%s: NULL errno=%s\n", label, en(e));
    free(p);
}

static void show_getwd(const char *label)
{
    char wb[PATH_MAX];
    memset(wb, 'Z', sizeof wb);
    wb[sizeof wb - 1] = 0;
    errno = 0;
    char *r = getwd(wb);
    int e = errno;
    if (r)
        printf("%s: %s%s\n", label, rel(r), r == wb ? " ret==buf" : " ret!=buf");
    else
        printf("%s: NULL errno=%s buf=[%s]\n", label, en(e), wb[0] == 'Z' ? "untouched" : rel(wb));
}

static void sec_getcwd(void)
{
    printf("== getcwd\n");
    char buf[PATH_MAX + 64];
    mkd("c", 0755);
    mkd("c/sub", 0755);
    sln("c/sub", "c_link");
    if (chdir("c/sub") != 0)
        printf("chdir c/sub: %s\n", en(errno));
    show_getcwd("getcwd(buf, PATH_MAX+64)", buf, sizeof buf, sizeof buf);
    getcwd(buf, sizeof buf);
    size_t n = strlen(buf);
    show_getcwd("getcwd(buf, len+1)", buf, n + 1, sizeof buf);
    show_getcwd("getcwd(buf, len)", buf, n, sizeof buf);
    show_getcwd("getcwd(buf, 2)", buf, 2, sizeof buf);
    show_getcwd("getcwd(buf, 1)", buf, 1, sizeof buf);
    show_getcwd("getcwd(buf, 0)", buf, 0, sizeof buf);
    show_getcwd("getcwd(NULL, 0)", NULL, 0, 0);
    show_getcwd("getcwd(NULL, len+1)", NULL, n + 1, 0);
    show_getcwd("getcwd(NULL, len)", NULL, n, 0);
    show_getcwd("getcwd(NULL, 1)", NULL, 1, 0);
    show_getcwd("getcwd(NULL, 100000)", NULL, 100000, 0);
    show_getwd("getwd");

    printf("-- get_current_dir_name with cwd <dir>/c/sub (<dir>/c_link -> c/sub)\n");
    char pw[PATH_MAX * 2];
    struct {
        const char *label;
        const char *suffix; /* appended to top; NULL: literal */
        const char *literal;
    } pc[] = {
        {"PWD unset", NULL, NULL},
        {"PWD=<dir>/c/sub (physical)", "/c/sub", NULL},
        {"PWD=<dir>/c_link (symlink to cwd)", "/c_link", NULL},
        {"PWD=<dir>/c_link/ (trailing slash)", "/c_link/", NULL},
        {"PWD=<dir>/c/./sub", "/c/./sub", NULL},
        {"PWD=<dir>/c/../c/sub", "/c/../c/sub", NULL},
        {"PWD=<dir>//c/sub", "//c/sub", NULL},
        {"PWD=<dir>/c (another directory)", "/c", NULL},
        {"PWD=<dir>/missing", "/missing", NULL},
        {"PWD=. (relative, names cwd)", NULL, "."},
        {"PWD=c/sub (relative)", NULL, "c/sub"},
        {"PWD=\"\"", NULL, ""},
    };
    for (size_t i = 0; i < sizeof pc / sizeof pc[0]; i++) {
        if (pc[i].suffix) {
            snprintf(pw, sizeof pw, "%s%s", top, pc[i].suffix);
            setenv("PWD", pw, 1);
        } else if (pc[i].literal) {
            setenv("PWD", pc[i].literal, 1);
        } else {
            unsetenv("PWD");
        }
        show_gcdn(pc[i].label);
    }
    unsetenv("PWD");
    back_to_top();

    printf("-- removed working directory\n");
    mkd("gone", 0755);
    if (chdir("gone") != 0)
        printf("chdir gone: %s\n", en(errno));
    {
        int rr0 = rmdir("../gone");
        printf("rmdir(../gone) from inside: %d %s\n", rr0, rr0 ? en(errno) : "-");
    }
    show_getcwd("getcwd(buf, PATH_MAX+64)", buf, sizeof buf, sizeof buf);
    show_getcwd("getcwd(NULL, 0)", NULL, 0, 0);
    show_getwd("getwd");
    show_gcdn("get_current_dir_name PWD unset");
    snprintf(pw, sizeof pw, "%s/gone", top);
    setenv("PWD", pw, 1);
    show_gcdn("get_current_dir_name PWD=<dir>/gone");
    unsetenv("PWD");
    errno = 0;
    DIR *d = opendir(".");
    if (!d) {
        printf("opendir(.): NULL errno=%s\n", en(errno));
    } else {
        int cnt = 0;
        errno = 0;
        while (readdir(d))
            cnt++;
        printf("opendir(.): ok, readdir entries=%d errno=%s\n", cnt, en(errno));
        closedir(d);
    }
    {
        errno = 0;
        char *r = realpath(".", NULL);
        printf("realpath(.): %s\n", r ? rel(r) : en(errno));
        free(r);
        errno = 0;
        r = realpath("..", NULL);
        printf("realpath(..): %s\n", r ? rel(r) : en(errno));
        free(r);
    }
    errno = 0;
    {
        int r0 = chdir("..");
        int e0 = errno;
        printf("chdir(..) from removed dir: %d %s;", r0, r0 ? en(e0) : "-");
        printf(" cwd now %s\n", cwd_now());
    }
    back_to_top();

    printf("-- chdir, fchdir, chroot (as euid %s)\n", geteuid() == 0 ? "0" : "non-zero");
    char longpath[5000];
    memset(longpath, 0, sizeof longpath);
    for (size_t i = 0; i + 2 < 4200; i += 2)
        memcpy(longpath + i, "./", 2);
    char longname[300];
    memset(longname, 'n', 256);
    longname[256] = 0;
    struct {
        const char *label;
        const char *p;
    } cc[] = {
        {"chdir(\"\")", ""},
        {"chdir(missing)", "missing"},
        {"chdir(t/b) file", "t/b"},
        {"chdir(t/b/) file with slash", "t/b/"},
        {"chdir(t/nosearch) mode 0600", "t/nosearch"},
        {"chdir(t/noread) mode 0333", "t/noread"},
        {"chdir(lp/s_loop)", "lp/s_loop"},
        {"chdir(t/s_dangling)", "t/s_dangling"},
        {"chdir(4200-byte ./././...)", longpath},
        {"chdir(256-byte component)", longname},
    };
    for (size_t i = 0; i < sizeof cc / sizeof cc[0]; i++) {
        errno = 0;
        int r = chdir(cc[i].p);
        int e = errno;
        printf("%s: %d %s", cc[i].label, r, r ? en(e) : "-");
        printf(" cwd=%s\n", cwd_now());
        back_to_top();
    }
    int r = chdir("t/s_dir");
    printf("chdir(t/s_dir): %d", r);
    printf(" cwd=%s\n", cwd_now());
    back_to_top();
    int fd = open("t/a", O_RDONLY | O_DIRECTORY);
    r = fchdir(fd);
    printf("fchdir(t/a O_RDONLY): %d", r);
    printf(" cwd=%s\n", cwd_now());
    back_to_top();
    close(fd);
    fd = open("t/a", O_PATH | O_DIRECTORY);
    errno = 0;
    r = fchdir(fd);
    printf("fchdir(t/a O_PATH): %d %s", r, r ? en(errno) : "-");
    printf(" cwd=%s\n", cwd_now());
    back_to_top();
    close(fd);
    fd = open("t/b", O_RDONLY);
    errno = 0;
    r = fchdir(fd);
    printf("fchdir(file): %d %s\n", r, en(errno));
    close(fd);
    errno = 0;
    r = fchdir(fd);
    printf("fchdir(closed fd): %d %s\n", r, en(errno));
    errno = 0;
    r = fchdir(-1);
    printf("fchdir(-1): %d %s\n", r, en(errno));
    if (geteuid() != 0) {
        errno = 0;
        r = chroot("t/a");
        printf("chroot(t/a) unprivileged: %d %s\n", r, en(errno));
        errno = 0;
        r = chroot("missing");
        printf("chroot(missing) unprivileged: %d %s\n", r, en(errno));
    }
}

static void sec_deep_cwd(void)
{
    printf("== working directory deeper than PATH_MAX (deep/ + %d x {d200}; PATH_MAX=%d)\n", DEPTH, PATH_MAX);
    int fd = open_deep_level(DEPTH);
    if (fd < 0 || fchdir(fd) != 0) {
        printf("cannot enter deep tree\n");
        if (fd >= 0)
            close(fd);
        return;
    }
    close(fd);
    size_t expect = toplen + strlen("/deep") + (size_t)DEPTH * 201;
    printf("expected length <dir>+%zu\n", expect - toplen);
    static char big[16384];
    char buf[PATH_MAX];
    show_getcwd("getcwd(buf, PATH_MAX)", buf, sizeof buf, sizeof buf);
    errno = 0;
    char *r = getcwd(big, sizeof big);
    printf("getcwd(buf, 16384): %s errno=%s len=<dir>+%zu\n", r ? rel(r) : "NULL", r ? "-" : en(errno), r ? strlen(r) - toplen : 0);
    errno = 0;
    r = getcwd(big, expect + 1);
    printf("getcwd(buf, len+1): %s\n", r ? "ok" : en(errno));
    errno = 0;
    r = getcwd(big, expect);
    printf("getcwd(buf, len): %s\n", r ? "ok" : en(errno));
    errno = 0;
    r = getcwd(NULL, 0);
    printf("getcwd(NULL, 0): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
    free(r);
    errno = 0;
    r = getcwd(NULL, PATH_MAX);
    printf("getcwd(NULL, PATH_MAX): %s\n", r ? "ok" : en(errno));
    free(r);
    show_getwd("getwd");
    show_gcdn("get_current_dir_name PWD unset");
    if (getcwd(big, sizeof big)) {
        setenv("PWD", big, 1);
        show_gcdn("get_current_dir_name PWD=physical path");
        unsetenv("PWD");
    }
    errno = 0;
    r = realpath(".", NULL);
    printf("realpath(., NULL): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
    free(r);
    memset(buf, 'Q', sizeof buf);
    buf[sizeof buf - 1] = 0;
    errno = 0;
    r = realpath(".", buf);
    printf("realpath(., buf): %s buf=%s\n", r ? rel(r) : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
    errno = 0;
    r = realpath("bottom", NULL);
    printf("realpath(bottom, NULL): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
    free(r);
    errno = 0;
    r = canonicalize_file_name("bottom");
    printf("canonicalize_file_name(bottom): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
    free(r);

    /* An ancestor without read permission (search only): level 10 of 25. */
    char up[128] = "";
    for (int i = 0; i < DEPTH - 10; i++)
        strcat(up, i ? "/.." : "..");
    errno = 0;
    int c = chmod(up, 0111);
    printf("chmod(level 10, 0111): %d %s\n", c, c ? en(errno) : "-");
    errno = 0;
    r = getcwd(big, sizeof big);
    printf("  getcwd(buf, 16384): %s\n", r ? "ok" : en(errno));
    errno = 0;
    r = getcwd(NULL, 0);
    printf("  getcwd(NULL, 0): %s\n", r ? "ok" : en(errno));
    free(r);
    show_gcdn("  get_current_dir_name PWD unset");
    errno = 0;
    r = realpath(".", NULL);
    printf("  realpath(., NULL): %s\n", r ? "ok" : en(errno));
    free(r);
    chmod(up, 0755);
    back_to_top();

    printf("-- realpath from level 15 of a 10-component relative path (result over PATH_MAX)\n");
    fd = open_deep_level(15);
    if (fd >= 0 && fchdir(fd) == 0) {
        char relp[2100] = "";
        for (int i = 0; i < 10; i++) {
            if (i)
                strcat(relp, "/");
            strcat(relp, LONGN);
        }
        printf("input length %zu\n", strlen(relp));
        memset(buf, 'Q', sizeof buf);
        buf[sizeof buf - 1] = 0;
        errno = 0;
        r = realpath(relp, buf);
        printf("realpath(rel, buf): %s buf=%s\n", r ? "ok" : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
        if (!r && buf[0] != 'Q')
            printf("  prefix length <dir>+%zu\n", strlen(buf) - toplen);
        errno = 0;
        r = realpath(relp, NULL);
        printf("realpath(rel, NULL): %s len=<dir>+%zu\n", r ? rel(r) : en(errno), r ? strlen(r) - toplen : 0);
        free(r);
        errno = 0;
        r = canonicalize_file_name(relp);
        printf("canonicalize_file_name(rel): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
        free(r);
    }
    if (fd >= 0)
        close(fd);
    back_to_top();
    printf("-- realpath of an input string over PATH_MAX (deep/ + 25 components, from <dir>)\n");
    static char in[8192];
    strcpy(in, "deep");
    for (int i = 0; i < DEPTH; i++) {
        strcat(in, "/");
        strcat(in, LONGN);
    }
    strcat(in, "/bottom");
    printf("input length %zu\n", strlen(in));
    memset(buf, 'Q', sizeof buf);
    buf[sizeof buf - 1] = 0;
    errno = 0;
    r = realpath(in, buf);
    printf("realpath(in, buf): %s buf=%s\n", r ? "ok" : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
    errno = 0;
    r = realpath(in, NULL);
    printf("realpath(in, NULL): %s len=<dir>+%zu\n", r ? "ok" : en(errno), r ? strlen(r) - toplen : 0);
    free(r);
    errno = 0;
    r = canonicalize_file_name(in);
    printf("canonicalize_file_name(in): %s\n", r ? "ok" : en(errno));
    free(r);
}

/* ---------------------------------------------------------------- §13.2 directory streams */

struct ent {
    char name[300];
    unsigned char type;
    unsigned short reclen;
};

static int read_all(DIR *d, struct ent *v, int max, int *err)
{
    int n = 0;
    struct dirent *e;
    errno = 0;
    while ((e = readdir(d))) {
        if (n < max) {
            snprintf(v[n].name, sizeof v[n].name, "%s", e->d_name);
            v[n].type = e->d_type;
            v[n].reclen = e->d_reclen;
        }
        n++;
        errno = 0;
    }
    *err = errno;
    return n;
}

static int ent_cmp(const void *a, const void *b)
{
    return strcmp(((const struct ent *)a)->name, ((const struct ent *)b)->name);
}

static void print_ents(const char *label, struct ent *v, int n, int types, int sort)
{
    if (sort)
        qsort(v, (size_t)n, sizeof *v, ent_cmp);
    printf("%s:", label);
    for (int i = 0; i < n; i++) {
        printf(" %s", rel(v[i].name));
        if (types)
            printf(":%s", dtname(v[i].type));
    }
    printf("\n");
}

static void sec_readdir(void)
{
    printf("== directory streams (fs %s)\n", fsname);
    mkd("r", 0755);
    mkf("r/f");
    mkd("r/d", 0755);
    sln("f", "r/l");
    mkfifo("r/p", 0644);
    mksock("r/s");
    char n255[300] = "r/";
    memset(n255 + 2, 'n', 255);
    n255[257] = 0;
    mkf(n255);

    static struct ent v[2048];
    int err;
    DIR *d = opendir("r");
    if (!d) {
        printf("opendir(r) failed %s\n", en(errno));
        return;
    }
    int fl = fcntl(dirfd(d), F_GETFD);
    printf("opendir(r): ok, dirfd close-on-exec=%s, O_NONBLOCK %s\n", (fl & FD_CLOEXEC) ? "yes" : "no",
           (fcntl(dirfd(d), F_GETFL) & O_NONBLOCK) ? "set" : "clear");
    printf("telldir at start: %ld\n", telldir(d));
    int n = read_all(d, v, 2048, &err);
    printf("entries=%d errno at end=%s\n", n, en(err));
    {
        char lab[160];
        snprintf(lab, sizeof lab, "r in directory order (fs %s)", fsname);
        print_ents(lab, v, n, 0, 0);
    }
    print_ents("r sorted, with d_type", v, n, 1, 1);
    for (int i = 0; i < n; i++)
        if (!strcmp(v[i].name, "f") || !strcmp(v[i].name, "d") || strlen(v[i].name) == 255 || !strcmp(v[i].name, "."))
            printf("d_reclen %s: %u\n", rel(v[i].name), v[i].reclen);
    errno = 0;
    struct dirent *e = readdir(d);
    printf("readdir after end: %s errno=%s\n", e ? "entry" : "NULL", en(errno));
    errno = EINTR;
    e = readdir(d);
    printf("readdir after end with errno=EINTR before: %s errno=%s\n", e ? "entry" : "NULL", en(errno));
    rewinddir(d);
    struct dirent *p1 = readdir(d);
    char first[300];
    snprintf(first, sizeof first, "%s", p1 ? p1->d_name : "");
    struct dirent *p2 = readdir(d);
    printf("two readdir calls return the same storage: %s; first entry name still intact in storage: %s\n", p1 == p2 ? "yes" : "no",
           (p1 && strcmp(p1->d_name, first) == 0) ? "yes" : "no");
    int fd = dirfd(d);
    errno = 0;
    printf("closedir: %d\n", closedir(d));
    errno = 0;
    int fr = fcntl(fd, F_GETFD);
    printf("fcntl(old dirfd) after closedir: %d %s\n", fr, en(errno));

    /* readdir_r, readdir64, readdir64_r give the same set. */
    d = opendir("r");
    struct dirent buf, *res;
    int cnt = 0, rr;
    while ((rr = readdir_r(d, &buf, &res)) == 0 && res)
        cnt++;
    printf("readdir_r: %d entries, last return %d result %s\n", cnt, rr, res ? "set" : "NULL");
    rr = readdir_r(d, &buf, &res);
    printf("readdir_r after end: %d result %s\n", rr, res ? "set" : "NULL");
    closedir(d);
    d = opendir("r");
    cnt = 0;
    struct dirent64 *e64;
    while ((e64 = readdir64(d)))
        cnt++;
    printf("readdir64: %d entries\n", cnt);
    rewinddir(d);
    struct dirent64 b64, *r64;
    cnt = 0;
    while (readdir64_r(d, &b64, &r64) == 0 && r64)
        cnt++;
    printf("readdir64_r: %d entries\n", cnt);
    closedir(d);

    /* Two streams on one directory read alternately. */
    DIR *d1 = opendir("r"), *d2 = opendir("r");
    int c1 = 0, c2 = 0, more = 1;
    while (more) {
        more = 0;
        if (readdir(d1)) {
            c1++;
            more = 1;
        }
        if (readdir(d2)) {
            c2++;
            more = 1;
        }
    }
    printf("two streams interleaved: %d and %d entries\n", c1, c2);
    closedir(d1);
    closedir(d2);

    printf("-- opendir errors\n");
    char longname[300];
    memset(longname, 'n', 256);
    longname[256] = 0;
    struct {
        const char *label;
        const char *p;
    } oc[] = {
        {"opendir(\"\")", ""},
        {"opendir(missing)", "missing"},
        {"opendir(r/f) file", "r/f"},
        {"opendir(r/f/) file with slash", "r/f/"},
        {"opendir(r/l) symlink to file", "r/l"},
        {"opendir(t/noread) mode 0333", "t/noread"},
        {"opendir(t/nosearch) mode 0600", "t/nosearch"},
        {"opendir(t/nosearch/x) below 0600", "t/nosearch/x"},
        {"opendir(lp/s_loop)", "lp/s_loop"},
        {"opendir(t/s_dir) symlink to dir", "t/s_dir"},
        {"opendir(r/) trailing slash", "r/"},
        {"opendir(256-byte name)", longname},
        {"opendir(r/p) FIFO", "r/p"},
    };
    for (size_t i = 0; i < sizeof oc / sizeof oc[0]; i++) {
        errno = 0;
        d = opendir(oc[i].p);
        if (d) {
            int k = read_all(d, v, 2048, &err);
            printf("%s: ok, %d entries\n", oc[i].label, k);
            closedir(d);
        } else {
            printf("%s: NULL %s\n", oc[i].label, en(errno));
        }
    }

    printf("-- fdopendir, dirfd\n");
    fd = open("r", O_RDONLY | O_DIRECTORY);
    printf("fd close-on-exec before fdopendir: %s\n", (fcntl(fd, F_GETFD) & FD_CLOEXEC) ? "yes" : "no");
    d = fdopendir(fd);
    printf("fdopendir(dir fd): %s, dirfd==fd: %s, close-on-exec after: %s\n", d ? "ok" : en(errno), d && dirfd(d) == fd ? "yes" : "no",
           (fcntl(fd, F_GETFD) & FD_CLOEXEC) ? "yes" : "no");
    if (d) {
        int k = read_all(d, v, 2048, &err);
        printf("  entries=%d\n", k);
        closedir(d);
        errno = 0;
        fr = fcntl(fd, F_GETFD);
        printf("  fd after closedir: %d %s\n", fr, en(errno));
    }
    /* A descriptor already read to the end with getdents64: fdopendir does not rewind. */
    fd = open("r", O_RDONLY | O_DIRECTORY);
    {
        char gb[65536];
        long gn;
        while ((gn = getdents64(fd, gb, sizeof gb)) > 0)
            ;
    }
    d = fdopendir(fd);
    if (d) {
        int k = read_all(d, v, 2048, &err);
        printf("fdopendir(fd at end of directory): %d entries; after rewinddir: ", k);
        rewinddir(d);
        k = read_all(d, v, 2048, &err);
        printf("%d entries\n", k);
        closedir(d);
    }
    fd = open("r/f", O_RDONLY);
    errno = 0;
    d = fdopendir(fd);
    fr = errno;
    printf("fdopendir(file fd): %s %s;", d ? "ok" : "NULL", en(fr));
    printf(" fd still open: %s\n", fcntl(fd, F_GETFD) != -1 ? "yes" : "no");
    if (d)
        closedir(d);
    close(fd);
    fd = open("r", O_PATH | O_DIRECTORY);
    errno = 0;
    d = fdopendir(fd);
    printf("fdopendir(O_PATH dir fd): %s %s\n", d ? "ok" : "NULL", en(errno));
    if (d) {
        int k = read_all(d, v, 2048, &err);
        printf("  entries=%d errno=%s\n", k, en(err));
        closedir(d);
    } else {
        close(fd);
    }
    errno = 0;
    d = fdopendir(-1);
    printf("fdopendir(-1): %s %s\n", d ? "ok" : "NULL", en(errno));
    errno = 0;
    d = fdopendir(1000);
    printf("fdopendir(closed 1000): %s %s\n", d ? "ok" : "NULL", en(errno));

    printf("-- errors while reading\n");
    d = opendir("r");
    close(dirfd(d));
    errno = 0;
    e = readdir(d);
    printf("readdir with its descriptor closed: %s errno=%s\n", e ? "entry" : "NULL", en(errno));
    rr = readdir_r(d, &buf, &res);
    printf("readdir_r with its descriptor closed: %d result %s\n", rr, res ? "set" : "NULL");
    errno = 0;
    fr = closedir(d);
    printf("closedir with its descriptor closed: %d %s\n", fr, en(errno));
    mkd("rm1", 0755);
    d = opendir("rm1");
    rmdir("rm1");
    errno = 0;
    cnt = 0;
    while (readdir(d))
        cnt++;
    printf("readdir of a directory removed after opendir: %d entries errno=%s\n", cnt, en(errno));
    closedir(d);

    printf("-- getdents64, getdirentries\n");
    fd = open("r", O_RDONLY | O_DIRECTORY);
    char gb[4096];
    errno = 0;
    long gn = getdents64(fd, gb, 1);
    printf("getdents64(dir, 1 byte): %ld %s\n", gn, en(errno));
    int total = 0;
    static struct ent gv[64];
    while ((gn = getdents64(fd, gb, sizeof gb)) > 0) {
        for (long off = 0; off < gn;) {
            struct dirent64 *g = (struct dirent64 *)(gb + off);
            if (total < 64)
                snprintf(gv[total].name, sizeof gv[total].name, "%s", g->d_name);
            total++;
            off += g->d_reclen;
        }
    }
    printf("getdents64 total entries=%d final %ld\n", total, gn);
    print_ents("getdents64 sorted", gv, total, 0, 1);
    close(fd);
    fd = open("r/f", O_RDONLY);
    errno = 0;
    gn = getdents64(fd, gb, sizeof gb);
    printf("getdents64(file): %ld %s\n", gn, en(errno));
    close(fd);
    errno = 0;
    gn = getdents64(-1, gb, sizeof gb);
    printf("getdents64(-1): %ld %s\n", gn, en(errno));
    fd = open("r", O_RDONLY | O_DIRECTORY);
    off_t base = 12345;
    errno = 0;
    ssize_t gd = getdirentries(fd, gb, sizeof gb, &base);
    printf("getdirentries first call: %s base=%ld\n", gd > 0 ? "bytes>0" : en(errno), (long)base);
    total = 0;
    if (gd > 0)
        for (long off = 0; off < gd;) {
            struct dirent *g = (struct dirent *)(gb + off);
            total++;
            off += g->d_reclen;
        }
    while ((gd = getdirentries(fd, gb, sizeof gb, &base)) > 0)
        for (long off = 0; off < gd;) {
            struct dirent *g = (struct dirent *)(gb + off);
            total++;
            off += g->d_reclen;
        }
    printf("getdirentries total entries=%d final %ld\n", total, (long)gd);
    close(fd);

    printf("-- rewinddir sees entries created after the stream reached its end\n");
    mkd("rw", 0755);
    mkf("rw/a");
    mkf("rw/b");
    d = opendir("rw");
    n = read_all(d, v, 2048, &err);
    mkf("rw/c");
    unlink("rw/a");
    errno = 0;
    e = readdir(d);
    printf("readdir at end after create/unlink: %s\n", e ? rel(e->d_name) : "NULL");
    rewinddir(d);
    n = read_all(d, v, 2048, &err);
    print_ents("after rewinddir, sorted", v, n, 0, 1);
    closedir(d);

    printf("-- telldir, seekdir (60 entries; positions are fs cookies, fs %s)\n", fsname);
    mkd("ts", 0755);
    for (int i = 0; i < 58; i++) {
        char p[32];
        snprintf(p, sizeof p, "ts/e%02d", i);
        mkf(p);
    }
    d = opendir("ts");
    long pos[80];
    char names[80][32];
    int k = 0;
    for (;;) {
        long t = telldir(d);
        e = readdir(d);
        if (!e)
            break;
        if (k < 80) {
            pos[k] = t;
            snprintf(names[k], sizeof names[k], "%s", e->d_name);
        }
        k++;
    }
    long endpos = telldir(d);
    printf("entries=%d telldir before first readdir=%ld\n", k, pos[0]);
    int distinct = 1;
    for (int i = 0; i < k && i < 80; i++)
        for (int j = i + 1; j < k && j < 80; j++)
            if (pos[i] == pos[j])
                distinct = 0;
    printf("positions distinct: %s; end position distinct from all: %s\n", distinct ? "yes" : "no", ({
               int de = 1;
               for (int i = 0; i < k && i < 80; i++)
                   if (pos[i] == endpos)
                       de = 0;
               de ? "yes" : "no";
           }));
    int idx[] = {0, 1, 2, 17, 30, 45, 59, 3};
    for (size_t i = 0; i < sizeof idx / sizeof idx[0]; i++) {
        int j = idx[i];
        if (j >= k)
            continue;
        seekdir(d, pos[j]);
        long t = telldir(d);
        e = readdir(d);
        printf("seekdir(pos[%d]): telldir==pos %s, readdir gives entry %d: %s\n", j, t == pos[j] ? "yes" : "no", j,
               e && strcmp(e->d_name, names[j]) == 0 ? "yes" : "no");
    }
    seekdir(d, endpos);
    e = readdir(d);
    printf("seekdir(end position): readdir %s\n", e ? "entry" : "NULL");
    rewinddir(d);
    printf("telldir after rewinddir == start: %s\n", telldir(d) == pos[0] ? "yes" : "no");
    /* Positions from one stream used in another stream on the same directory. */
    DIR *o = opendir("ts");
    seekdir(o, pos[30]);
    e = readdir(o);
    printf("seekdir in a second stream with the first stream's pos[30]: %s\n", e && strcmp(e->d_name, names[30]) == 0 ? "same entry" : "different");
    closedir(o);
    /* After seekdir to an earlier position, the rest of the stream follows in the same order. */
    seekdir(d, pos[10]);
    int same = 1;
    for (int j = 10; j < k; j++) {
        e = readdir(d);
        if (!e || strcmp(e->d_name, names[j]) != 0)
            same = 0;
    }
    printf("after seekdir(pos[10]) the remaining entries repeat in order: %s\n", same ? "yes" : "no");
    closedir(d);
}

/* §13.2 Failure lists ENOENT for a directory removed while it is being read. A directory is removed
 * (emptied first where it held files, since rmdir needs an empty directory) while a stream or a
 * descriptor is open on it, at several points of the reading, with each reading function; and
 * getdents64 is called directly on such a descriptor, to show what the kernel itself returns. This
 * section runs in both phases (the user phase on the scratch file system, the root phase on its
 * tmpfs), and every line starts with the file system's name, as the owner's rule of 2026-10-03
 * requires. Counts of entries returned after the removal come from the stream's buffer (read before
 * the removal) and so depend on its size. */
static char fstag[48];
#define RD_BIG 2000 /* files in the big directory: far more than one getdents64 buffer holds */

enum { RD_READDIR, RD_READDIR64, RD_READDIR_R, RD_READDIR64_R };
static const char *const rd_fn[] = {"readdir", "readdir64", "readdir_r", "readdir64_r"};

static void rd_fill(const char *dir, int n)
{
    char p[160];
    mkd(dir, 0755);
    for (int i = 0; i < n; i++) {
        snprintf(p, sizeof p, "%s/entry-%04d-padding-to-make-the-records-longer", dir, i);
        mkf(p);
    }
}

static void rd_empty(const char *dir, int n)
{
    char p[160];
    for (int i = 0; i < n; i++) {
        snprintf(p, sizeof p, "%s/entry-%04d-padding-to-make-the-records-longer", dir, i);
        if (unlink(p) != 0)
            printf("[fs %s] cannot unlink %s: %s\n", fstag, rel(p), en(errno));
    }
}

static void rd_remove(const char *dir, int n)
{
    rd_empty(dir, n);
    if (rmdir(dir) != 0)
        printf("[fs %s] cannot rmdir %s: %s\n", fstag, dir, en(errno));
}

/* Reads up to max entries (all if max < 0) with the given function. Returns the number read; at the
 * end *endtxt describes how the stream ended: "NULL errno=E" for readdir/readdir64 (errno cleared
 * before each call), "ret=R result=NULL" for the _r forms; "." and ".." are counted in *dots, the
 * other names in *files. */
static int rd_read(DIR *d, int how, int max, char *endtxt, size_t endsz, int *dots, int *files)
{
    int n = 0;
    struct dirent db, *dr;
    struct dirent64 db64, *dr64;
    endtxt[0] = 0;
    while (max < 0 || n < max) {
        const char *name = NULL;
        int rv = 0;
        errno = 0;
        switch (how) {
        case RD_READDIR: {
            struct dirent *e = readdir(d);
            rv = errno;
            name = e ? e->d_name : NULL;
            break;
        }
        case RD_READDIR64: {
            struct dirent64 *e = readdir64(d);
            rv = errno;
            name = e ? e->d_name : NULL;
            break;
        }
        case RD_READDIR_R:
            rv = readdir_r(d, &db, &dr);
            name = rv == 0 && dr ? dr->d_name : NULL;
            break;
        case RD_READDIR64_R:
            rv = readdir64_r(d, &db64, &dr64);
            name = rv == 0 && dr64 ? dr64->d_name : NULL;
            break;
        }
        if (!name) {
            if (how == RD_READDIR || how == RD_READDIR64)
                snprintf(endtxt, endsz, "NULL errno=%s", en(rv));
            else
                snprintf(endtxt, endsz, "ret=%s result=NULL", en(rv));
            return n;
        }
        n++;
        if (!strcmp(name, ".") || !strcmp(name, ".."))
            ++*dots;
        else
            ++*files;
    }
    snprintf(endtxt, endsz, "(stopped)");
    return n;
}

static void sec_removed_dir(void)
{
    char end[64], end2[64];
    int dots, files;
    printf("-- directory removed while open (fs %s; every line names it)\n", fsname);
    for (int how = 0; how < 4; how++) {
        /* Removed after opendir, before the first read. */
        mkd("rmd", 0755);
        DIR *d = opendir("rmd");
        rmdir("rmd");
        dots = files = 0;
        int n = rd_read(d, how, -1, end, sizeof end, &dots, &files);
        rd_read(d, how, -1, end2, sizeof end2, &dots, &files);
        printf("[fs %s] %s, empty directory removed before the first read: %d entries; end: %s; again: %s\n", fstag, rd_fn[how], n, end, end2);
        closedir(d);

        /* An empty directory removed after its first entry was read. */
        mkd("rmd", 0755);
        d = opendir("rmd");
        dots = files = 0;
        int n0 = rd_read(d, how, 1, end, sizeof end, &dots, &files);
        rmdir("rmd");
        dots = files = 0;
        n = rd_read(d, how, -1, end, sizeof end, &dots, &files);
        printf("[fs %s] %s, empty directory removed after %d entry read: %d more (dot entries %d); end: %s\n", fstag, rd_fn[how], n0, n, dots, end);
        closedir(d);

        /* A directory of 3 files, read 3 entries, emptied and removed. */
        rd_fill("rmd", 3);
        d = opendir("rmd");
        dots = files = 0;
        n0 = rd_read(d, how, 3, end, sizeof end, &dots, &files);
        rd_remove("rmd", 3);
        dots = files = 0;
        n = rd_read(d, how, -1, end, sizeof end, &dots, &files);
        printf("[fs %s] %s, 3 files, emptied and removed after %d entries read: %d more; end: %s\n", fstag, rd_fn[how], n0, n, end);
        closedir(d);

        /* A directory of RD_BIG files (several getdents64 buffers), read 50 entries, emptied and
         * removed: once the stream's buffer is used up it must ask the kernel again. */
        rd_fill("rmd", RD_BIG);
        d = opendir("rmd");
        dots = files = 0;
        n0 = rd_read(d, how, 50, end, sizeof end, &dots, &files);
        rd_remove("rmd", RD_BIG);
        dots = files = 0;
        n = rd_read(d, how, -1, end, sizeof end, &dots, &files);
        printf("[fs %s] %s, %d files, emptied and removed after %d entries read: more entries returned: %s (fewer than the %d left: %s); end: %s\n", fstag,
               rd_fn[how], RD_BIG, n0, n > 0 ? "some" : "none", RD_BIG + 2 - n0, n < RD_BIG + 2 - n0 ? "yes" : "no", end);
        closedir(d);
    }
    /* The same big directory emptied but not removed (the comparison: is it the removal or the
     * emptying that ends the stream?). */
    rd_fill("rmd", RD_BIG);
    DIR *d = opendir("rmd");
    dots = files = 0;
    int n0 = rd_read(d, RD_READDIR, 50, end, sizeof end, &dots, &files);
    rd_empty("rmd", RD_BIG);
    dots = files = 0;
    int n = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
    printf("[fs %s] readdir, %d files, emptied but NOT removed after %d entries read: more entries returned: %s (fewer than the %d left: %s); end: %s\n", fstag,
           RD_BIG, n0, n > 0 ? "some" : "none", RD_BIG + 2 - n0, n < RD_BIG + 2 - n0 ? "yes" : "no", end);
    closedir(d);
    rmdir("rmd");

    /* rewinddir after the removal: the whole directory was read first, then removed. */
    rd_fill("rmd", 3);
    d = opendir("rmd");
    dots = files = 0;
    n0 = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
    rd_remove("rmd", 3);
    rewinddir(d);
    dots = files = 0;
    n = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
    printf("[fs %s] rewinddir after removal (%d entries read before it): %d entries; end: %s\n", fstag, n0, n, end);
    /* ... and telldir/seekdir back to the start. */
    seekdir(d, 0);
    dots = files = 0;
    n = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
    printf("[fs %s] seekdir(0) after that: %d entries; end: %s\n", fstag, n, end);
    closedir(d);
    /* rewinddir after a removal that came before any read. */
    mkd("rmd", 0755);
    d = opendir("rmd");
    rmdir("rmd");
    rewinddir(d);
    dots = files = 0;
    n = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
    printf("[fs %s] rewinddir on a stream whose directory was removed before the first read: %d entries; end: %s\n", fstag, n, end);
    closedir(d);

    /* fdopendir on a removed directory's descriptor. */
    mkd("rmd", 0755);
    int fd = open("rmd", O_RDONLY | O_DIRECTORY);
    rmdir("rmd");
    errno = 0;
    d = fdopendir(fd);
    printf("[fs %s] fdopendir(descriptor of a removed directory): %s %s", fstag, d ? "ok" : "NULL", en(errno));
    if (d) {
        dots = files = 0;
        n = rd_read(d, RD_READDIR, -1, end, sizeof end, &dots, &files);
        printf("; readdir: %d entries; end: %s", n, end);
        closedir(d);
    } else {
        close(fd);
    }
    printf("\n");
    /* fdopendir first, then the removal. */
    mkd("rmd", 0755);
    fd = open("rmd", O_RDONLY | O_DIRECTORY);
    d = fdopendir(fd);
    rmdir("rmd");
    dots = files = 0;
    n = rd_read(d, RD_READDIR_R, -1, end, sizeof end, &dots, &files);
    printf("[fs %s] fdopendir, then the directory removed: readdir_r %d entries; end: %s\n", fstag, n, end);
    closedir(d);

    /* getdents64 itself, without the library's stream. */
    char gb[1024];
    long gr;
    int ge;
#define GD(fd_, sz_)                                     \
    do {                                                 \
        errno = 0;                                       \
        gr = getdents64((fd_), gb, (sz_));               \
        ge = errno;                                      \
    } while (0)
#define GDTXT(r_, e_) ((r_) > 0 ? "bytes>0" : (r_) == 0 ? "0 (end)" : en(e_))
    mkd("rmd", 0755);
    fd = open("rmd", O_RDONLY | O_DIRECTORY);
    rmdir("rmd");
    GD(fd, sizeof gb);
    long g1 = gr;
    int e1 = ge;
    GD(fd, sizeof gb);
    printf("[fs %s] getdents64 on a descriptor of an empty directory removed before the first call: %s; again: %s\n", fstag, GDTXT(g1, e1),
           GDTXT(gr, ge));
    errno = 0;
    off_t lr = lseek(fd, 0, SEEK_SET);
    int le = errno;
    GD(fd, sizeof gb);
    printf("[fs %s]   lseek(0): %s; getdents64 after it: %s\n", fstag, lr == 0 ? "0" : en(le), GDTXT(gr, ge));
    close(fd);

    rd_fill("rmd", RD_BIG);
    fd = open("rmd", O_RDONLY | O_DIRECTORY);
    GD(fd, sizeof gb);
    g1 = gr;
    e1 = ge;
    rd_remove("rmd", RD_BIG);
    GD(fd, sizeof gb);
    long g2 = gr;
    int e2 = ge;
    GD(fd, sizeof gb);
    printf("[fs %s] getdents64, %d files, one 1024-byte call (%s), then emptied and removed: %s; again: %s\n", fstag, RD_BIG, GDTXT(g1, e1),
           GDTXT(g2, e2), GDTXT(gr, ge));
    errno = 0;
    lr = lseek(fd, 0, SEEK_SET);
    le = errno;
    GD(fd, sizeof gb);
    printf("[fs %s]   lseek(0): %s; getdents64 after it: %s\n", fstag, lr == 0 ? "0" : en(le), GDTXT(gr, ge));
    close(fd);

    rd_fill("rmd", RD_BIG);
    fd = open("rmd", O_RDONLY | O_DIRECTORY);
    GD(fd, sizeof gb);
    g1 = gr;
    e1 = ge;
    rd_empty("rmd", RD_BIG);
    GD(fd, sizeof gb);
    printf("[fs %s] getdents64, %d files, one 1024-byte call (%s), then emptied but NOT removed: %s\n", fstag, RD_BIG, GDTXT(g1, e1), GDTXT(gr, ge));
    close(fd);
    rmdir("rmd");

    /* A removed working directory opened as ".". */
    mkd("rmd", 0755);
    if (chdir("rmd") == 0) {
        rmdir("../rmd");
        errno = 0;
        fd = open(".", O_RDONLY | O_DIRECTORY);
        int oe = errno;
        if (fd >= 0) {
            GD(fd, sizeof gb);
            printf("[fs %s] removed working directory: open(\".\"): ok; getdents64: %s\n", fstag, GDTXT(gr, ge));
            close(fd);
        } else {
            printf("[fs %s] removed working directory: open(\".\"): %s\n", fstag, en(oe));
        }
        back_to_top();
    }
#undef GD
#undef GDTXT
}

/* The "exactly once" guarantee: entries that exist throughout a scan, while others are created
 * and removed. */
static void sec_exactly_once(void)
{
    printf("== readdir while entries are created and removed (fs %s)\n", fsname);
    enum { NS = 1500, NV = 300, NN = 600 };
    mkd("big", 0755);
    char p[128];
    for (int i = 0; i < NS; i++) {
        snprintf(p, sizeof p, "big/stable-%04d-padding-to-make-records-longer", i);
        mkf(p);
    }
    for (int i = 0; i < NV; i++) {
        snprintf(p, sizeof p, "big/victim-%04d-padding-to-make-records-longer", i);
        mkf(p);
    }
    static int seen_s[NS], seen_v[NV], seen_n[NN];
    int dots = 0, dotdots = 0, other = 0, reads = 0, created = 0, removed = 0;
    DIR *d = opendir("big");
    struct dirent *e;
    errno = 0;
    while ((e = readdir(d))) {
        int k;
        if (!strcmp(e->d_name, "."))
            dots++;
        else if (!strcmp(e->d_name, ".."))
            dotdots++;
        else if (sscanf(e->d_name, "stable-%4d", &k) == 1 && k >= 0 && k < NS)
            seen_s[k]++;
        else if (sscanf(e->d_name, "victim-%4d", &k) == 1 && k >= 0 && k < NV)
            seen_v[k]++;
        else if (sscanf(e->d_name, "new-%4d", &k) == 1 && k >= 0 && k < NN)
            seen_n[k]++;
        else
            other++;
        reads++;
        if (reads % 50 == 0) {
            for (int j = 0; j < 15 && created < NN; j++) {
                snprintf(p, sizeof p, "big/new-%04d-padding-to-make-records-longer", created++);
                mkf(p);
            }
            for (int j = 0; j < 8 && removed < NV; j++) {
                snprintf(p, sizeof p, "big/victim-%04d-padding-to-make-records-longer", removed++);
                unlink(p);
            }
        }
        errno = 0;
    }
    int err = errno;
    closedir(d);
    int once = 0, never = 0, twice = 0, vdup = 0, ndup = 0, vseen = 0, nseen = 0;
    for (int i = 0; i < NS; i++) {
        if (seen_s[i] == 1)
            once++;
        else if (seen_s[i] == 0)
            never++;
        else
            twice++;
    }
    for (int i = 0; i < NV; i++) {
        if (seen_v[i])
            vseen++;
        if (seen_v[i] > 1)
            vdup++;
    }
    for (int i = 0; i < NN; i++) {
        if (seen_n[i])
            nseen++;
        if (seen_n[i] > 1)
            ndup++;
    }
    int survivors = NV - removed;
    printf("stable entries: %d, seen exactly once %d, never %d, more than once %d; '.' %d '..' %d; unexpected names %d; end errno=%s\n",
           NS, once, never, twice, dots, dotdots, other, en(err));
    printf("victims never removed (%d) all seen: %s\n", survivors, ({
               int ok = 1;
               for (int i = removed; i < NV; i++)
                   if (seen_v[i] != 1)
                       ok = 0;
               ok ? "yes, once each" : "no";
           }));
    printf("entries returned more than once among removed/created: %d/%d\n", vdup, ndup);
    printf("created during the scan: %d, removed during the scan: %d\n", created, removed);
    printf("(directory order, fs %s) removed entries seen before removal: %d of %d; created entries seen: %d of %d\n", fsname,
           vseen - survivors, removed, nseen, created);
}

/* ---------------------------------------------------------------- §13.3 scandir */

static int filter_calls, filter_saw_dot, filter_saw_dotdot;
static int filt_nodot(const struct dirent *e)
{
    filter_calls++;
    if (!strcmp(e->d_name, "."))
        filter_saw_dot = 1;
    if (!strcmp(e->d_name, ".."))
        filter_saw_dotdot = 1;
    return e->d_name[0] != '.';
}
static int filt_ret;
static int filt_value(const struct dirent *e)
{
    return strcmp(e->d_name, "a") == 0 ? filt_ret : 0;
}

static void print_list(const char *label, struct dirent **l, int n)
{
    printf("%s: %d:", label, n);
    for (int i = 0; i < n; i++)
        printf(" %s", l[i]->d_name);
    printf("\n");
}

static void free_list(struct dirent **l, int n)
{
    for (int i = 0; i < n; i++)
        free(l[i]);
    free(l);
}

static const char *sc_names[] = {"a", "B", "b", "A", "_c", "a10", "a2", "a1", "a01", "a001", "A1", ".h", "\xc3\xa9", "e", "E", "z", "Z", "a1.10", "a1.9",
                                 "a-b", "ab", "a b", "\xc3\xa4", "1", "10", "9", "x~", "\xc3\x85", "ch", "h", "c", "0a", "a0", "a00", "~"};

static void sec_scandir(void)
{
    printf("== scandir\n");
    mkd("sc", 0755);
    for (size_t i = 0; i < sizeof sc_names / sizeof sc_names[0]; i++) {
        char p[64];
        snprintf(p, sizeof p, "sc/%s", sc_names[i]);
        mkf(p);
    }
    struct dirent **l;
    int n = scandir("sc", &l, NULL, NULL);
    {
        char lab[160];
        snprintf(lab, sizeof lab, "scandir(sc, NULL, NULL) in directory order (fs %s)", fsname);
        print_list(lab, l, n);
    }
    free_list(l, n);
    n = scandir("sc", &l, NULL, alphasort);
    print_list("alphasort C", l, n);
    free_list(l, n);
    n = scandir("sc", &l, NULL, versionsort);
    print_list("versionsort C", l, n);
    free_list(l, n);
    const char *locs[] = {"en_US.UTF-8", "cs_CZ.UTF-8"};
    for (size_t i = 0; i < 2; i++) {
        if (!setlocale(LC_ALL, locs[i])) {
            printf("%s unavailable\n", locs[i]);
            continue;
        }
        char lab[64];
        n = scandir("sc", &l, NULL, alphasort);
        snprintf(lab, sizeof lab, "alphasort %s", locs[i]);
        print_list(lab, l, n);
        free_list(l, n);
        n = scandir("sc", &l, NULL, versionsort);
        snprintf(lab, sizeof lab, "versionsort %s", locs[i]);
        print_list(lab, l, n);
        free_list(l, n);
        struct dirent **l64;
        n = scandir64("sc", (struct dirent64 ***)&l64, NULL, alphasort64);
        snprintf(lab, sizeof lab, "scandir64+alphasort64 %s", locs[i]);
        print_list(lab, l64, n);
        free_list(l64, n);
    }
    setlocale(LC_ALL, "C");
    struct dirent **l64;
    n = scandir64("sc", (struct dirent64 ***)&l64, NULL, versionsort64);
    print_list("scandir64+versionsort64 C", l64, n);
    free_list(l64, n);

    filter_calls = filter_saw_dot = filter_saw_dotdot = 0;
    n = scandir("sc", &l, filt_nodot, alphasort);
    printf("filter (no dot names): count %d; filter called %d times; saw . %s, .. %s\n", n, filter_calls, filter_saw_dot ? "yes" : "no",
           filter_saw_dotdot ? "yes" : "no");
    free_list(l, n);
    int vals[] = {1, 2, -1, 0};
    for (size_t i = 0; i < 4; i++) {
        filt_ret = vals[i];
        n = scandir("sc", &l, filt_value, NULL);
        printf("filter returning %d for \"a\" (0 otherwise): count %d\n", vals[i], n);
        free_list(l, n);
    }
    mkd("sce", 0755);
    n = scandir("sce", &l, NULL, alphasort);
    print_list("empty directory, alphasort", l, n);
    free_list(l, n);

    struct {
        const char *label;
        const char *p;
    } ec[] = {
        {"scandir(missing)", "missing"},
        {"scandir(\"\")", ""},
        {"scandir(sc/a) file", "sc/a"},
        {"scandir(t/noread) 0333", "t/noread"},
        {"scandir(t/nosearch) 0600", "t/nosearch"},
        {"scandir(lp/s_loop)", "lp/s_loop"},
    };
    for (size_t i = 0; i < sizeof ec / sizeof ec[0]; i++) {
        l = (struct dirent **)&ec; /* sentinel */
        errno = 0;
        n = scandir(ec[i].p, &l, NULL, alphasort);
        printf("%s: %d %s list %s\n", ec[i].label, n, n < 0 ? en(errno) : "-", l == (struct dirent **)&ec ? "unchanged" : "written");
        if (n >= 0)
            free_list(l, n);
    }
    printf("t/nosearch with d_type (0600: names readable, entries not searchable):");
    n = scandir("t/nosearch", &l, NULL, alphasort);
    for (int i = 0; i < n; i++)
        printf(" %s:%s", l[i]->d_name, dtname(l[i]->d_type));
    printf("\n");
    if (n >= 0)
        free_list(l, n);

    int dfd = open(".", O_RDONLY | O_DIRECTORY);
    n = scandirat(dfd, "sc", &l, NULL, alphasort);
    printf("scandirat(<dir> fd, sc): %d\n", n);
    if (n >= 0)
        free_list(l, n);
    close(dfd);
    n = scandirat(AT_FDCWD, "sc", &l, NULL, alphasort);
    printf("scandirat(AT_FDCWD, sc): %d\n", n);
    if (n >= 0)
        free_list(l, n);
    errno = 0;
    n = scandirat(-5, "sc", &l, NULL, alphasort);
    printf("scandirat(-5, sc): %d %s\n", n, n < 0 ? en(errno) : "-");
    if (n >= 0)
        free_list(l, n);
    char ab[PATH_MAX + 8];
    snprintf(ab, sizeof ab, "%s/sc", top);
    errno = 0;
    n = scandirat(-5, ab, &l, NULL, alphasort);
    printf("scandirat(-5, <dir>/sc absolute): %d\n", n);
    if (n >= 0)
        free_list(l, n);
    dfd = open("sc/a", O_RDONLY);
    errno = 0;
    n = scandirat(dfd, "x", &l, NULL, alphasort);
    printf("scandirat(file fd, x): %d %s\n", n, n < 0 ? en(errno) : "-");
    close(dfd);
    n = scandirat64(AT_FDCWD, "sc", (struct dirent64 ***)&l64, NULL, versionsort64);
    printf("scandirat64(AT_FDCWD, sc, versionsort64): %d\n", n);
    if (n >= 0)
        free_list(l64, n);

    printf("-- alphasort, versionsort called directly (sign)\n");
    const char *pairs[][2] = {{"a", "b"}, {"b", "a"}, {"a", "a"}, {"a", "B"}, {"A", "b"}, {"a2", "a10"}, {"a01", "a1"}, {"a001", "a01"}, {"a0", "a00"},
                              {"a1.9", "a1.10"}, {"", "a"}, {"\xc3\xa9", "f"}, {"_c", "a"}};
    for (size_t i = 0; i < sizeof pairs / sizeof pairs[0]; i++) {
        struct dirent x, y;
        memset(&x, 0, sizeof x);
        memset(&y, 0, sizeof y);
        strcpy(x.d_name, pairs[i][0]);
        strcpy(y.d_name, pairs[i][1]);
        const struct dirent *px = &x, *py = &y;
        int ac = alphasort(&px, &py), vc = versionsort(&px, &py);
        setlocale(LC_ALL, "en_US.UTF-8");
        int au = alphasort(&px, &py);
        setlocale(LC_ALL, "C");
        printf("[%s] vs [%s]: alphasort C %s, en_US.UTF-8 %s; versionsort %s\n", pairs[i][0], pairs[i][1], ac < 0 ? "<" : ac > 0 ? ">" : "=",
               au < 0 ? "<" : au > 0 ? ">" : "=", vc < 0 ? "<" : vc > 0 ? ">" : "=");
    }
}

/* ---------------------------------------------------------------- §13.3 ftw, nftw */

static int nlines;
static int print_cwd_in_cb;
static const char *act_path;
static int act_flag = -1; /* typeflag the action applies to, -1 any */
static int act_ret;
static int act_once;
static int quiet;

static const char *tfname(int f)
{
    switch (f) {
    case FTW_F: return "F";
    case FTW_D: return "D";
    case FTW_DNR: return "DNR";
    case FTW_NS: return "NS";
    case FTW_SL: return "SL";
    case FTW_DP: return "DP";
    case FTW_SLN: return "SLN";
    }
    return "?";
}

static int cb(const char *p, const struct stat *st, int f, struct FTW *w)
{
    if (nlines++ < WALK_CAP && !quiet) {
        if (strncmp(p, top, toplen) == 0)
            printf("  %s level=%d base=<dir>+%d %s", tfname(f), w->level, w->base - (int)toplen, rel(p));
        else
            printf("  %s level=%d base=%d %s", tfname(f), w->level, w->base, rel(p));
        if (f != FTW_NS)
            printf(" [%s]", kind(st->st_mode));
        if (print_cwd_in_cb)
            printf(" cwd=%s", cwd_now());
        printf("\n");
    }
    if (act_path && strcmp(p, act_path) == 0 && (act_flag < 0 || act_flag == f)) {
        if (act_once)
            act_path = NULL;
        return act_ret;
    }
    return 0;
}

static int cb_ftw(const char *p, const struct stat *st, int f)
{
    if (nlines++ < WALK_CAP) {
        printf("  %s %s", tfname(f), rel(p));
        if (f != FTW_NS)
            printf(" [%s]", kind(st->st_mode));
        printf("\n");
    }
    if (act_path && strcmp(p, act_path) == 0)
        return act_ret;
    return 0;
}

static void walk_end(int r, int e)
{
    if (nlines > WALK_CAP)
        printf("  ... %d more callbacks\n", nlines - WALK_CAP);
    printf("  -> %d", r);
    if (r == -1)
        printf(" errno=%s", en(e));
    printf("; cwd after=%s\n", cwd_now());
    back_to_top();
}

static void run_nftw(const char *label, const char *root, int nfd, int flags)
{
    printf("nftw(%s, nopenfd=%d, %s) (directory order, fs %s):\n", rel(root), nfd, label, fsname);
    nlines = 0;
    errno = 0;
    int r = nftw(root, cb, nfd, flags);
    walk_end(r, errno);
    act_path = NULL;
    act_flag = -1;
    act_once = 0;
}

static void run_ftw(const char *root, int nfd)
{
    printf("ftw(%s, nopenfd=%d) (directory order, fs %s):\n", rel(root), nfd, fsname);
    nlines = 0;
    errno = 0;
    int r = ftw(root, cb_ftw, nfd);
    walk_end(r, errno);
    act_path = NULL;
}

static void sec_nftw(void)
{
    printf("== nftw, ftw over t (fs %s)\n", fsname);
    run_nftw("0", "t", 10, 0);
    run_nftw("FTW_PHYS", "t", 10, FTW_PHYS);
    run_nftw("FTW_DEPTH", "t", 10, FTW_DEPTH);
    run_nftw("FTW_PHYS|FTW_DEPTH", "t", 10, FTW_PHYS | FTW_DEPTH);
    print_cwd_in_cb = 1;
    run_nftw("FTW_PHYS|FTW_CHDIR", "t", 10, FTW_PHYS | FTW_CHDIR);
    run_nftw("FTW_PHYS|FTW_CHDIR|FTW_DEPTH", "t", 10, FTW_PHYS | FTW_CHDIR | FTW_DEPTH);
    print_cwd_in_cb = 0;
    run_nftw("FTW_MOUNT|FTW_PHYS (one file system)", "t", 10, FTW_MOUNT | FTW_PHYS);
    printf("-- the same over u (t without the unreadable and unsearchable directories, the FIFO and the socket)\n");
    run_nftw("0", "u", 10, 0);
    run_nftw("FTW_DEPTH", "u", 10, FTW_DEPTH);
    print_cwd_in_cb = 1;
    run_nftw("FTW_PHYS|FTW_CHDIR", "u", 10, FTW_PHYS | FTW_CHDIR);
    run_nftw("FTW_PHYS|FTW_CHDIR|FTW_DEPTH", "u", 10, FTW_PHYS | FTW_CHDIR | FTW_DEPTH);
    run_nftw("FTW_CHDIR (following links)", "u", 10, FTW_CHDIR);
    act_path = "u/a/sub/x";
    act_ret = 5;
    run_nftw("FTW_PHYS|FTW_CHDIR, 5 at u/a/sub/x", "u", 10, FTW_PHYS | FTW_CHDIR);
    print_cwd_in_cb = 0;
    run_nftw("0", "lp", 10, 0);
    run_nftw("FTW_PHYS", "lp", 10, FTW_PHYS);

    printf("-- FTW_ACTIONRETVAL\n");
    act_path = "t/a";
    act_ret = FTW_SKIP_SUBTREE;
    run_nftw("ACTIONRETVAL|PHYS, SKIP_SUBTREE at t/a", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t/a";
    act_ret = FTW_SKIP_SUBTREE;
    run_nftw("ACTIONRETVAL|PHYS|DEPTH, SKIP_SUBTREE at t/a (FTW_DP)", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS | FTW_DEPTH);
    act_path = "t/a/f1";
    act_ret = FTW_SKIP_SIBLINGS;
    run_nftw("ACTIONRETVAL|PHYS, SKIP_SIBLINGS at t/a/f1", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t/a/f1";
    act_ret = FTW_SKIP_SIBLINGS;
    run_nftw("ACTIONRETVAL|PHYS|DEPTH, SKIP_SIBLINGS at t/a/f1", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS | FTW_DEPTH);
    act_path = "t/b";
    act_ret = FTW_SKIP_SUBTREE;
    run_nftw("ACTIONRETVAL|PHYS, SKIP_SUBTREE at file t/b", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t/b";
    act_ret = FTW_STOP;
    run_nftw("ACTIONRETVAL|PHYS, STOP at t/b", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t/b";
    act_ret = 99;
    run_nftw("ACTIONRETVAL|PHYS, 99 at t/b", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t";
    act_ret = FTW_SKIP_SIBLINGS;
    run_nftw("ACTIONRETVAL|PHYS, SKIP_SIBLINGS at the root", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    act_path = "t";
    act_ret = FTW_SKIP_SUBTREE;
    run_nftw("ACTIONRETVAL|PHYS, SKIP_SUBTREE at the root", "t", 10, FTW_ACTIONRETVAL | FTW_PHYS);
    printf("constants: FTW_CONTINUE=%d FTW_STOP=%d FTW_SKIP_SUBTREE=%d FTW_SKIP_SIBLINGS=%d\n", FTW_CONTINUE, FTW_STOP, FTW_SKIP_SUBTREE,
           FTW_SKIP_SIBLINGS);

    printf("-- a non-zero return stops the walk\n");
    act_path = "t/b";
    act_ret = 42;
    run_nftw("PHYS, 42 at t/b", "t", 10, FTW_PHYS);
    act_path = "t/a/sub/x";
    act_ret = -7;
    run_nftw("PHYS, -7 at t/a/sub/x", "t", 10, FTW_PHYS);
    act_path = "t/a/sub/x";
    act_ret = -7;
    print_cwd_in_cb = 1;
    run_nftw("PHYS|CHDIR, -7 at t/a/sub/x", "t", 10, FTW_PHYS | FTW_CHDIR);
    print_cwd_in_cb = 0;
    act_path = "t/a/sub/x";
    act_ret = FTW_STOP;
    act_flag = -1;
    run_nftw("PHYS without ACTIONRETVAL, FTW_STOP (1) at t/a/sub/x", "t", 10, FTW_PHYS);

    printf("-- roots\n");
    run_nftw("0", "t/b", 10, 0);
    run_nftw("0", "t/s_dir", 10, 0);
    run_nftw("FTW_PHYS", "t/s_dir", 10, FTW_PHYS);
    run_nftw("FTW_PHYS", "t/s_dir/", 10, FTW_PHYS);
    run_nftw("0", "t/s_dangling", 10, 0);
    run_nftw("FTW_PHYS", "t/s_dangling", 10, FTW_PHYS);
    run_nftw("0", "lp/s_loop", 10, 0);
    run_nftw("0", "missing", 10, 0);
    run_nftw("0", "", 10, 0);
    run_nftw("0", "t/b/x", 10, 0);
    run_nftw("0", "t/nosearch/ns_f", 10, 0);
    run_nftw("0", "t/nosearch", 10, 0);
    run_nftw("FTW_CHDIR", "t/nosearch", 10, FTW_CHDIR);
    run_nftw("0", "t/noread", 10, 0);
    run_nftw("FTW_DEPTH", "t/noread", 10, FTW_DEPTH);
    run_nftw("FTW_PHYS", "t/a/", 10, FTW_PHYS);
    run_nftw("FTW_PHYS", "t//a", 10, FTW_PHYS);
    run_nftw("FTW_PHYS", "./t/a/sub", 10, FTW_PHYS);
    {
        char ab[PATH_MAX + 8];
        snprintf(ab, sizeof ab, "%s/t/a/sub", top);
        run_nftw("FTW_PHYS", ab, 10, FTW_PHYS);
    }
    if (chdir("t/e") == 0) {
        run_nftw("FTW_PHYS (cwd t/e, empty)", ".", 10, FTW_PHYS);
        if (chdir("t/e") == 0)
            run_nftw("FTW_PHYS (cwd t/e)", "..", 10, FTW_PHYS);
    }

    printf("-- nopenfd\n");
    run_nftw("FTW_PHYS", "t/a", 0, FTW_PHYS);
    run_nftw("FTW_PHYS", "t/a", -1, FTW_PHYS);
    run_nftw("FTW_PHYS", "t/a", 1, FTW_PHYS);

    printf("-- ftw\n");
    run_ftw("t", 10);
    act_path = "t/b";
    act_ret = 5;
    printf("(the callback returns 5 at t/b)\n");
    run_ftw("t", 10);
    run_ftw("t/s_dangling", 10);
    run_ftw("missing", 10);
    run_ftw("t/a", 0);
}

/* Walk over a single root "/" would be huge: the callback above stops it at once. */
static int stop_root(const char *p, const struct stat *st, int f, struct FTW *w)
{
    (void)st;
    (void)w;
    printf("  %s %s -> stop\n", tfname(f), p);
    return FTW_STOP;
}

/* Counters for walks whose callbacks are not printed one by one. */
static int c_n, c_maxlevel, c_maxlen, c_byflag[8], c_maxfds, c_basefds, c_bad, c_badlevel, c_badlen, c_badbase;
static int cb_count(const char *p, const struct stat *st, int f, struct FTW *w)
{
    (void)st;
    c_n++;
    /* In the deep tree a path at level L (L <= DEPTH) is "deep" plus L components of 201 bytes; the
     * file "bottom" is at level DEPTH+1. */
    size_t want = w->level <= DEPTH ? 4 + 201 * (size_t)w->level : 4 + 201 * (size_t)DEPTH + 7;
    const char *last = w->level == 0 ? "deep" : w->level <= DEPTH ? LONGN : "bottom";
    if (strlen(p) != want || strcmp(p + w->base, last) != 0) {
        if (!c_bad) {
            c_badlevel = w->level;
            c_badlen = (int)strlen(p);
            c_badbase = w->base;
        }
        c_bad++;
    }
    if (w->level > c_maxlevel)
        c_maxlevel = w->level;
    if ((int)strlen(p) > c_maxlen)
        c_maxlen = (int)strlen(p);
    if (f >= 0 && f < 8)
        c_byflag[f]++;
    int fds = count_fds() - c_basefds;
    if (fds > c_maxfds)
        c_maxfds = fds;
    return 0;
}
static int cb_count_ftw(const char *p, const struct stat *st, int f)
{
    struct FTW w = {0, 0};
    (void)w;
    c_n++;
    if ((int)strlen(p) > c_maxlen)
        c_maxlen = (int)strlen(p);
    if (f >= 0 && f < 8)
        c_byflag[f]++;
    (void)st;
    return 0;
}

static void count_reset(void)
{
    c_n = c_maxlevel = c_maxlen = c_maxfds = c_bad = 0;
    memset(c_byflag, 0, sizeof c_byflag);
    c_basefds = count_fds();
}

static void count_print(int r, int e)
{
    printf("  callbacks=%d maxlevel=%d maxpathlen=%d F=%d D=%d DNR=%d NS=%d SL=%d DP=%d SLN=%d -> %d", c_n, c_maxlevel, c_maxlen, c_byflag[FTW_F],
           c_byflag[FTW_D], c_byflag[FTW_DNR], c_byflag[FTW_NS], c_byflag[FTW_SL], c_byflag[FTW_DP], c_byflag[FTW_SLN], r);
    if (r == -1)
        printf(" errno=%s", en(e));
    printf("; cwd after=%s\n", cwd_now());
    if (c_bad)
        printf("  %d callbacks had a path that does not name the entry: first at level %d, length %d, base %d\n", c_bad, c_badlevel, c_badlen,
               c_badbase);
    back_to_top();
}

/* Runs fn(arg) in a child so that a walk that kills the process is recorded as such. */
static void in_child(void (*fn)(int), int arg)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        tw_alarm(120);
        expect_crash(); /* a walk may kill the process: that is what is recorded */
        fn(arg);
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (WIFSIGNALED(st))
        printf("  the process was killed by signal %d%s\n", WTERMSIG(st), WTERMSIG(st) == SIGABRT ? " (SIGABRT)" : "");
    else if (WEXITSTATUS(st) != 0)
        printf("  the process exited with status %d\n", WEXITSTATUS(st));
}

static int cut_level; /* FTW_ACTIONRETVAL walks: SKIP_SUBTREE at this level */
static int cb_cut(const char *p, const struct stat *st, int f, struct FTW *w)
{
    cb_count(p, st, f, w);
    return w->level >= cut_level && f == FTW_D ? FTW_SKIP_SUBTREE : FTW_CONTINUE;
}

static const struct {
    const char *label;
    int flags;
    int nfd;
} deep_cases[] = {
    {"FTW_PHYS", FTW_PHYS, 20},
    {"FTW_PHYS|FTW_DEPTH", FTW_PHYS | FTW_DEPTH, 20},
    {"0", 0, 20},
    {"FTW_PHYS|FTW_CHDIR", FTW_PHYS | FTW_CHDIR, 20},
    {"FTW_PHYS|FTW_CHDIR|FTW_DEPTH", FTW_PHYS | FTW_CHDIR | FTW_DEPTH, 20},
    {"FTW_PHYS|FTW_CHDIR", FTW_PHYS | FTW_CHDIR, 1},
};

static void deep_nftw(int i)
{
    printf("nftw(deep, %d, %s):\n", deep_cases[i].nfd, deep_cases[i].label);
    fflush(stdout);
    count_reset();
    errno = 0;
    int r = nftw("deep", cb_count, deep_cases[i].nfd, deep_cases[i].flags);
    count_print(r, errno);
}

static void deep_ftw(int unused)
{
    (void)unused;
    printf("ftw(deep, 20) (ftw has no level: maxlevel stays 0):\n");
    fflush(stdout);
    count_reset();
    errno = 0;
    int r = ftw("deep", cb_count_ftw, 20);
    count_print(r, errno);
}

static void deep_cut(int level)
{
    cut_level = level;
    printf("nftw(deep, 20, FTW_PHYS|FTW_CHDIR|FTW_ACTIONRETVAL), not descending below level %d (longest path %zu bytes):\n", level,
           strlen("deep") + (size_t)level * 201);
    fflush(stdout);
    count_reset();
    errno = 0;
    int r = nftw("deep", cb_cut, 20, FTW_PHYS | FTW_CHDIR | FTW_ACTIONRETVAL);
    count_print(r, errno);
}

static void deep_nfd(int nfd)
{
    count_reset();
    errno = 0;
    int r = nftw("deep", cb_count, nfd, FTW_PHYS);
    int e = errno;
    printf("nftw(deep, nopenfd=%d, FTW_PHYS): callbacks=%d -> %d%s%s; most descriptors open beyond the caller's during a callback: %d\n", nfd, c_n,
           r, r == -1 ? " errno=" : "", r == -1 ? en(e) : "", c_maxfds);
    back_to_top();
}

static void sec_deep_walks(void)
{
    printf("== walks over the deep tree (paths up to %zu bytes; PATH_MAX=%d; each walk runs in a child process)\n",
           strlen("deep/bottom") + (size_t)DEPTH * 201, PATH_MAX);
    for (int i = 0; i < (int)(sizeof deep_cases / sizeof deep_cases[0]); i++)
        in_child(deep_nftw, i);
    in_child(deep_ftw, 0);
    int levels[] = {18, 19, 20, 21, 22};
    for (int i = 0; i < 5; i++)
        in_child(deep_cut, levels[i]);
    int nfds[] = {1, 2, 5, 100};
    for (int i = 0; i < 4; i++)
        in_child(deep_nfd, nfds[i]);
}

/* ---------------------------------------------------------------- §13.3 fts */

static const char *finame(int i)
{
    switch (i) {
    case FTS_D: return "D";
    case FTS_DC: return "DC";
    case FTS_DEFAULT: return "DEFAULT";
    case FTS_DNR: return "DNR";
    case FTS_DOT: return "DOT";
    case FTS_DP: return "DP";
    case FTS_ERR: return "ERR";
    case FTS_F: return "F";
    case FTS_INIT: return "INIT";
    case FTS_NS: return "NS";
    case FTS_NSOK: return "NSOK";
    case FTS_SL: return "SL";
    case FTS_SLNONE: return "SLNONE";
    case FTS_W: return "W";
    }
    return "?";
}

static int by_name(const FTSENT **a, const FTSENT **b)
{
    return strcmp((*a)->fts_name, (*b)->fts_name);
}

/* statok: the walk does not use FTS_NOSTAT, so fts_statp is meaningful for the infos below (with
 * FTS_NOSTAT its contents are unspecified and are not read). */
static void show_ent(const char *pre, FTSENT *e, int showcwd, int statok)
{
    printf("%s%s level=%d name=%s path=%s acc=%s", pre, finame(e->fts_info), e->fts_level, rel(e->fts_name), rel(e->fts_path), rel(e->fts_accpath));
    if (e->fts_errno)
        printf(" errno=%s", en(e->fts_errno));
    if (e->fts_info == FTS_DC && e->fts_cycle)
        /* By name and level: the cycle entry's fts_path is the stream's shared buffer, which
         * holds the current entry's path. */
        printf(" cycle=%s@%d", rel(e->fts_cycle->fts_name), e->fts_cycle->fts_level);
    switch (e->fts_info) {
    case FTS_D:
    case FTS_DP:
    case FTS_DC:
    case FTS_F:
    case FTS_SL:
    case FTS_SLNONE:
    case FTS_DEFAULT:
    case FTS_DOT:
    case FTS_DNR:
        if (statok && e->fts_statp)
            printf(" [%s]", kind(e->fts_statp->st_mode));
        break;
    }
    if (e->fts_namelen != strlen(e->fts_name) || e->fts_pathlen != strlen(e->fts_path))
        printf(" LENGTHS-DIFFER");
    if (showcwd)
        printf(" cwd=%s", cwd_now());
    printf("\n");
}

/* fts_set actions applied while walking. */
struct fact {
    const char *path;
    int info; /* -1 any */
    int instr;
    int once;
    int done;
};

/* Set: the actions write fts_instr into the entry themselves (no fts_set), and every entry's
 * fts_instr is shown as fts_read returns it. */
static int fact_direct;

static void run_fts(const char *label, char *const *argv, int opts, int (*cmp)(const FTSENT **, const FTSENT **), struct fact *acts, int nacts)
{
    printf("fts_open(");
    for (int i = 0; argv[i]; i++)
        printf("%s%s", i ? " " : "", argv[i][0] ? rel(argv[i]) : "\"\"");
    printf("; %s; %s)%s:\n", label, cmp ? "sorted by name" : "no comparison", cmp ? "" : "");
    if (!cmp)
        printf("  (directory order, fs %s)\n", fsname);
    errno = 0;
    FTS *f = fts_open(argv, opts, cmp);
    if (!f) {
        printf("  fts_open: NULL %s\n", en(errno));
        return;
    }
    int showcwd = !(opts & FTS_NOCHDIR);
    FTSENT *e;
    int n = 0;
    errno = 0;
    while ((e = fts_read(f))) {
        if (n++ < WALK_CAP) {
            show_ent("  ", e, showcwd, !(opts & FTS_NOSTAT));
            if (fact_direct)
                printf("    fts_instr=%d\n", e->fts_instr);
        }
        for (int i = 0; i < nacts; i++) {
            if (acts[i].done && acts[i].once)
                continue;
            if (strcmp(e->fts_path, acts[i].path) == 0 && (acts[i].info < 0 || acts[i].info == e->fts_info)) {
                if (fact_direct) {
                    e->fts_instr = (unsigned short)acts[i].instr;
                    printf("    fts_instr=%d written directly\n", acts[i].instr);
                } else {
                    errno = 0;
                    int r = fts_set(f, e, acts[i].instr);
                    printf("    fts_set(%d): %d %s\n", acts[i].instr, r, r ? en(errno) : "-");
                }
                acts[i].done = 1;
            }
        }
        errno = 0;
    }
    int err = errno;
    if (n > WALK_CAP)
        printf("  ... %d more entries\n", n - WALK_CAP);
    printf("  fts_read end: NULL errno=%s; again: ", en(err));
    errno = 0;
    e = fts_read(f);
    printf("%s errno=%s\n", e ? "entry" : "NULL", en(errno));
    errno = 0;
    int r = fts_close(f);
    printf("  fts_close: %d; cwd after=%s\n", r, cwd_now());
    back_to_top();
}

static void deep_fts(int m);
static void run_fts(const char *label, char *const *argv, int opts, int (*cmp)(const FTSENT **, const FTSENT **), struct fact *acts, int nacts);

static void fts_empty_argv(int unused)
{
    (void)unused;
    char *none[] = {NULL};
    run_fts("FTS_PHYSICAL, empty argv (in a child process)", none, FTS_PHYSICAL, NULL, NULL, 0);
}

/* Instructions given to the entries fts_children lists (FTS-INSTR-DIRECT): written into
 * fts_instr directly or through fts_set; the walk that follows, each entry's fts_instr as
 * fts_read returns it. roots: the instructions go to the roots fts_children lists before the
 * first fts_read. Only what fts(3) defines is printed (FTS-INSTR-DIRECT-2): FTS_SKIP on the
 * listed directories t/a and t/e means none of their descendants is visited, so the walk is
 * shown without the two marked entries themselves (whether a marked listed entry is returned
 * is not defined); FTS_FOLLOW goes only to the listed root t/s_dir. A listed FTS_FOLLOW below
 * a root, FTS_AGAIN on a listed entry and the marked entries' own visits are Liella's own
 * behaviour, tested in s13_3_c_4 (D-13.4). */
static int list_marked(const char *path, int roots)
{
    return !strcmp(path, "t/a") || (!roots && !strcmp(path, "t/e"));
}

static void fts_list_instr(int direct, int opts, const char *label, int roots)
{
    char *t[] = {"t", NULL};
    char *rl[] = {"t/s_dir", "t/a", "t/b", NULL};
    printf("fts_children then instructions %s (%s, %s):\n", direct ? "written into fts_instr directly" : "through fts_set",
           roots ? "roots t/s_dir t/a t/b, before the first fts_read" : "the entries of t", label);
    FTS *f = fts_open(roots ? rl : t, opts, by_name);
    if (!f) {
        printf("  fts_open: NULL %s\n", en(errno));
        return;
    }
    FTSENT *e;
    if (!roots) {
        e = fts_read(f);
        printf("  fts_read: %s:%s\n", rel(e->fts_path), finame(e->fts_info));
    }
    for (FTSENT *c = fts_children(f, 0); c; c = c->fts_link) {
        const char *nm = roots ? c->fts_path : c->fts_name;
        int v = -1;
        if (!strcmp(nm, "a") || !strcmp(nm, "t/a") || !strcmp(nm, "e"))
            v = FTS_SKIP;
        else if (!strcmp(nm, "t/s_dir"))
            v = FTS_FOLLOW;
        if (v < 0)
            continue;
        if (direct) {
            c->fts_instr = (unsigned short)v;
            printf("  %s: fts_instr=%d written directly\n", rel(nm), v);
        } else {
            errno = 0;
            int r = fts_set(f, c, v);
            printf("  %s: fts_set(%d): %d %s\n", rel(nm), v, r, r ? en(errno) : "-");
        }
    }
    int n = 0;
    errno = 0;
    while ((e = fts_read(f))) {
        if (list_marked(e->fts_path, roots)) {
            errno = 0;
            continue;
        }
        if (n++ < WALK_CAP) {
            show_ent("  ", e, !(opts & FTS_NOCHDIR), 1);
            printf("    fts_instr=%d\n", e->fts_instr);
        }
        errno = 0;
    }
    if (n > WALK_CAP)
        printf("  ... %d more entries\n", n - WALK_CAP);
    printf("  fts_read end: NULL errno=%s\n", en(errno));
    int r = fts_close(f);
    printf("  fts_close: %d; cwd after=%s\n", r, cwd_now());
    back_to_top();
}

static void sec_fts(void)
{
    printf("== fts over t (fs %s)\n", fsname);
    char *t[] = {"t", NULL};
    run_fts("FTS_PHYSICAL", t, FTS_PHYSICAL, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_LOGICAL", t, FTS_LOGICAL, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_SEEDOT|FTS_NOCHDIR", t, FTS_PHYSICAL | FTS_SEEDOT | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_NOSTAT|FTS_NOCHDIR", t, FTS_PHYSICAL | FTS_NOSTAT | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_LOGICAL|FTS_NOSTAT", t, FTS_LOGICAL | FTS_NOSTAT, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_XDEV|FTS_NOCHDIR", t, FTS_PHYSICAL | FTS_XDEV | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", t, FTS_PHYSICAL | FTS_NOCHDIR, NULL, NULL, 0);
    run_fts("FTS_LOGICAL|FTS_NOCHDIR", t, FTS_LOGICAL | FTS_NOCHDIR, NULL, NULL, 0);
    char *sd[] = {"t/s_dir", NULL};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", sd, FTS_PHYSICAL | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_COMFOLLOW|FTS_NOCHDIR", sd, FTS_PHYSICAL | FTS_COMFOLLOW | FTS_NOCHDIR, by_name, NULL, 0);
    char *sdl[] = {"t/s_dangling", "lp/s_loop", "t/s_file", NULL};
    run_fts("FTS_PHYSICAL|FTS_COMFOLLOW|FTS_NOCHDIR", sdl, FTS_PHYSICAL | FTS_COMFOLLOW | FTS_NOCHDIR, NULL, NULL, 0);
    run_fts("FTS_LOGICAL|FTS_NOCHDIR", sdl, FTS_LOGICAL | FTS_NOCHDIR, NULL, NULL, 0);

    printf("-- roots\n");
    char *roots[] = {"t/b", "t/a/sub", "missing", "t/e", NULL};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", roots, FTS_PHYSICAL | FTS_NOCHDIR, by_name, NULL, 0);
    printf("  (roots given in argument order; no comparison function, so no directory order is involved)\n");
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, roots unsorted", roots, FTS_PHYSICAL | FTS_NOCHDIR, NULL, NULL, 0);
    char *bad[] = {"t/b/x", "t/nosearch/ns_f", "t/nosearch", "t/noread", NULL};
    run_fts("FTS_PHYSICAL", bad, FTS_PHYSICAL, NULL, NULL, 0);
    char *empty[] = {"", NULL};
    run_fts("FTS_PHYSICAL", empty, FTS_PHYSICAL, NULL, NULL, 0);
    char *emptyl[] = {"t/b", "", NULL};
    run_fts("FTS_PHYSICAL", emptyl, FTS_PHYSICAL, NULL, NULL, 0);
    in_child(fts_empty_argv, 0);
    char *slash[] = {"t/a/", "t//e", "./t/a/sub", NULL};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", slash, FTS_PHYSICAL | FTS_NOCHDIR, by_name, NULL, 0);
    char abs_a[PATH_MAX + 8];
    snprintf(abs_a, sizeof abs_a, "%s/t/a/sub", top);
    char *absr[] = {abs_a, NULL};
    run_fts("FTS_PHYSICAL", absr, FTS_PHYSICAL, by_name, NULL, 0);

    printf("-- options\n");
    struct {
        const char *l;
        int o;
    } oo[] = {
        {"0", 0},
        {"FTS_PHYSICAL|FTS_LOGICAL", FTS_PHYSICAL | FTS_LOGICAL},
        {"FTS_PHYSICAL|0x1000", FTS_PHYSICAL | 0x1000},
        {"FTS_PHYSICAL|FTS_NAMEONLY", FTS_PHYSICAL | FTS_NAMEONLY},
        {"FTS_PHYSICAL|FTS_STOP", FTS_PHYSICAL | FTS_STOP},
        {"FTS_NOCHDIR alone", FTS_NOCHDIR},
    };
    for (size_t i = 0; i < sizeof oo / sizeof oo[0]; i++) {
        errno = 0;
        FTS *f = fts_open(t, oo[i].o, NULL);
        printf("fts_open(t, %s): %s %s\n", oo[i].l, f ? "ok" : "NULL", f ? "-" : en(errno));
        if (f)
            fts_close(f);
    }
    printf("constants: FTS_COMFOLLOW=%d FTS_LOGICAL=%d FTS_NOCHDIR=%d FTS_NOSTAT=%d FTS_PHYSICAL=%d FTS_SEEDOT=%d FTS_XDEV=%d FTS_WHITEOUT=%d "
           "FTS_OPTIONMASK=%d FTS_NAMEONLY=%d FTS_STOP=%d\n",
           FTS_COMFOLLOW, FTS_LOGICAL, FTS_NOCHDIR, FTS_NOSTAT, FTS_PHYSICAL, FTS_SEEDOT, FTS_XDEV, FTS_WHITEOUT, FTS_OPTIONMASK, FTS_NAMEONLY,
           FTS_STOP);
    printf("info values: D=%d DC=%d DEFAULT=%d DNR=%d DOT=%d DP=%d ERR=%d F=%d INIT=%d NS=%d NSOK=%d SL=%d SLNONE=%d W=%d; AGAIN=%d FOLLOW=%d "
           "NOINSTR=%d SKIP=%d; ROOTLEVEL=%d ROOTPARENTLEVEL=%d\n",
           FTS_D, FTS_DC, FTS_DEFAULT, FTS_DNR, FTS_DOT, FTS_DP, FTS_ERR, FTS_F, FTS_INIT, FTS_NS, FTS_NSOK, FTS_SL, FTS_SLNONE, FTS_W, FTS_AGAIN,
           FTS_FOLLOW, FTS_NOINSTR, FTS_SKIP, FTS_ROOTLEVEL, FTS_ROOTPARENTLEVEL);

    printf("-- fts_set\n");
    struct fact skip[] = {{"t/a", FTS_D, FTS_SKIP, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, SKIP at t/a", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, skip, 1);
    struct fact skipdp[] = {{"t/a", FTS_DP, FTS_SKIP, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, SKIP at t/a postorder", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, skipdp, 1);
    struct fact again[] = {{"t/b", -1, FTS_AGAIN, 1, 0}, {"t/a/sub", FTS_D, FTS_AGAIN, 1, 0}, {"t/e", FTS_DP, FTS_AGAIN, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, AGAIN at t/b, t/a/sub (preorder), t/e (postorder), once each", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, again,
            3);
    struct fact fol[] = {{"t/s_dir", -1, FTS_FOLLOW, 1, 0},
                         {"t/s_dangling", -1, FTS_FOLLOW, 1, 0},
                         {"t/s_file", -1, FTS_FOLLOW, 1, 0},
                         {"t/b", -1, FTS_FOLLOW, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, FOLLOW at s_dir, s_dangling, s_file and file b", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, fol, 4);
    char *lp[] = {"lp", NULL};
    struct fact folloop[] = {{"lp/s_loop", -1, FTS_FOLLOW, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, FOLLOW at the self loop lp/s_loop", lp, FTS_PHYSICAL | FTS_NOCHDIR, by_name, folloop, 1);
    run_fts("FTS_LOGICAL", lp, FTS_LOGICAL, by_name, NULL, 0);
    struct fact fol2[] = {{"t/s_dir", -1, FTS_FOLLOW, 1, 0}};
    run_fts("FTS_PHYSICAL (chdir), FOLLOW at s_dir", t, FTS_PHYSICAL, by_name, fol2, 1);
    struct fact badi[] = {{"t/b", -1, 7, 1, 0}, {"t/e", FTS_D, -1, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, instr 7 at t/b and -1 at t/e", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, badi, 2);

    printf("-- fts_instr written directly (FTS-INSTR-DIRECT: fts acts on the field, not only on fts_set)\n");
    fact_direct = 1;
    struct fact dskip[] = {{"t/a", FTS_D, FTS_SKIP, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, SKIP at t/a", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, dskip, 1);
    struct fact dagain[] = {{"t/b", -1, FTS_AGAIN, 1, 0}, {"t/a/sub", FTS_D, FTS_AGAIN, 1, 0}, {"t/e", FTS_DP, FTS_AGAIN, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, AGAIN at t/b, t/a/sub (preorder), t/e (postorder), once each", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name,
            dagain, 3);
    struct fact dfol[] = {{"t/s_dir", -1, FTS_FOLLOW, 1, 0},
                          {"t/s_dangling", -1, FTS_FOLLOW, 1, 0},
                          {"t/s_file", -1, FTS_FOLLOW, 1, 0},
                          {"t/b", -1, FTS_FOLLOW, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, FOLLOW at s_dir, s_dangling, s_file and file b", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, dfol, 4);
    struct fact dfol2[] = {{"t/s_dir", -1, FTS_FOLLOW, 1, 0}};
    run_fts("FTS_PHYSICAL (chdir), FOLLOW at s_dir", t, FTS_PHYSICAL, by_name, dfol2, 1);
    struct fact dother[] = {{"t/b", -1, 7, 1, 0}, {"t/e", FTS_D, 0, 1, 0}, {"t/a", FTS_D, FTS_NOINSTR, 1, 0}, {"t/a/sub", FTS_D, 0xffff, 1, 0}};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR, 7 at t/b, 0 at t/e, NOINSTR at t/a, 0xffff at t/a/sub", t, FTS_PHYSICAL | FTS_NOCHDIR, by_name, dother, 4);
    fact_direct = 0;
    fts_list_instr(1, FTS_PHYSICAL | FTS_NOCHDIR, "FTS_PHYSICAL|FTS_NOCHDIR", 0);
    fts_list_instr(1, FTS_PHYSICAL, "FTS_PHYSICAL (chdir)", 0);
    fts_list_instr(1, FTS_PHYSICAL | FTS_NOCHDIR, "FTS_PHYSICAL|FTS_NOCHDIR", 1);
    printf("-- fts_set on the entries fts_children lists (only what fts(3) defines)\n");
    fts_list_instr(0, FTS_PHYSICAL | FTS_NOCHDIR, "FTS_PHYSICAL|FTS_NOCHDIR", 0);
    fts_list_instr(0, FTS_PHYSICAL, "FTS_PHYSICAL (chdir)", 0);
    fts_list_instr(0, FTS_PHYSICAL | FTS_NOCHDIR, "FTS_PHYSICAL|FTS_NOCHDIR", 1);

    printf("-- fts_children\n");
    {
        char *two[] = {"t/e", "t/b", NULL};
        FTS *f = fts_open(two, FTS_PHYSICAL | FTS_NOCHDIR, by_name);
        errno = 0;
        FTSENT *c = fts_children(f, 0);
        printf("fts_children before the first fts_read (roots t/e t/b, sorted by name):");
        for (; c; c = c->fts_link)
            printf(" %s:%s(level %d)", rel(c->fts_name), finame(c->fts_info), c->fts_level);
        printf("\n");
        FTSENT *e;
        printf("then fts_read:");
        while ((e = fts_read(f)))
            printf(" %s:%s", rel(e->fts_path), finame(e->fts_info));
        printf("\n");
        fts_close(f);
    }
    int modes[] = {FTS_PHYSICAL | FTS_NOCHDIR, FTS_PHYSICAL};
    for (int m = 0; m < 2; m++) {
        FTS *f = fts_open(t, modes[m], by_name);
        FTSENT *e = fts_read(f);
        printf("[%s] after fts_read returns %s:%s\n", m ? "chdir" : "FTS_NOCHDIR", rel(e->fts_path), finame(e->fts_info));
        errno = 0;
        FTSENT *c = fts_children(f, 0);
        printf("  fts_children(0):");
        for (; c; c = c->fts_link) {
            printf(" %s:%s", rel(c->fts_name), finame(c->fts_info));
            if (c->fts_errno)
                printf("(%s)", en(c->fts_errno));
        }
        printf(" cwd=%s\n", cwd_now());
        errno = 0;
        c = fts_children(f, FTS_NAMEONLY);
        printf("  fts_children(FTS_NAMEONLY):");
        for (; c; c = c->fts_link)
            printf(" %s:%s", rel(c->fts_name), finame(c->fts_info));
        printf("\n");
        errno = 0;
        c = fts_children(f, 5);
        printf("  fts_children(5): %s %s\n", c ? "list" : "NULL", en(errno));
        int n = 0;
        printf("  remaining fts_read:");
        while ((e = fts_read(f))) {
            if (n++ < 60)
                printf(" %s:%s", rel(e->fts_path), finame(e->fts_info));
            if (strcmp(e->fts_path, "t/b") == 0) {
                errno = 0;
                c = fts_children(f, 0);
                printf(" [fts_children at file: %s errno=%s]", c ? "list" : "NULL", en(errno));
            }
            if (strcmp(e->fts_path, "t/noread") == 0 && e->fts_info == FTS_D) {
                errno = 0;
                c = fts_children(f, 0);
                printf(" [fts_children at t/noread: %s errno=%s]", c ? "list" : "NULL", en(errno));
            }
            if (strcmp(e->fts_path, "t/e") == 0 && e->fts_info == FTS_D) {
                errno = 0;
                c = fts_children(f, 0);
                printf(" [fts_children at empty t/e: %s errno=%s]", c ? "list" : "NULL", en(errno));
            }
            if (strcmp(e->fts_path, "t/nosearch") == 0 && e->fts_info == FTS_D) {
                errno = 0;
                c = fts_children(f, 0);
                printf(" [fts_children at t/nosearch:");
                for (; c; c = c->fts_link)
                    printf(" %s:%s(%s)", c->fts_name, finame(c->fts_info), en(c->fts_errno));
                printf(" errno=%s]", en(errno));
            }
        }
        printf("\n");
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
            tw_alarm(30);
            errno = 0;
            FTSENT *c2 = fts_children(f, 0);
            printf("  fts_children after the end (in a child process): %s errno=%s\n", c2 ? "list" : "NULL", en(errno));
            fflush(stdout);
            _exit(0);
        }
        int st = 0;
        waitpid(pid, &st, 0);
        if (WIFSIGNALED(st))
            printf("  fts_children after the end (in a child process): the process was killed by signal %d\n", WTERMSIG(st));
        fts_close(f);
        printf("  cwd after fts_close=%s\n", cwd_now());
        back_to_top();
    }

    printf("-- FTSENT fields\n");
    {
        FTS *f = fts_open(t, FTS_PHYSICAL | FTS_NOCHDIR, by_name);
        FTSENT *e;
        while ((e = fts_read(f))) {
            if (e->fts_level == 0 && e->fts_info == FTS_D)
                printf("root: level %d, fts_parent level %d, fts_number %ld\n", e->fts_level, e->fts_parent ? e->fts_parent->fts_level : 99,
                       e->fts_number);
            if (!strcmp(e->fts_path, "t/a") && e->fts_info == FTS_D) {
                e->fts_number = 7;
                e->fts_pointer = (void *)e;
            }
            if (!strcmp(e->fts_path, "t/a") && e->fts_info == FTS_DP)
                printf("t/a postorder: fts_number %ld, fts_pointer is the preorder entry: %s\n", e->fts_number,
                       e->fts_pointer == (void *)e ? "yes" : "no");
            if (!strcmp(e->fts_path, "t/a/f1"))
                printf("t/a/f1: fts_parent is the entry named a: %s, parent fts_number %ld\n", e->fts_parent && !strcmp(e->fts_parent->fts_name, "a") ? "yes" : "no",
                       e->fts_parent ? e->fts_parent->fts_number : -1);
        }
        fts_close(f);
    }

    printf("-- fts_close in the middle of a chdir walk\n");
    {
        FTS *f = fts_open(t, FTS_PHYSICAL, by_name);
        FTSENT *e;
        while ((e = fts_read(f)))
            if (!strcmp(e->fts_path, "t/a/sub/x"))
                break;
        printf("stopped at %s, cwd=%s\n", e ? rel(e->fts_path) : "end", cwd_now());
        errno = 0;
        int r = fts_close(f);
        printf("fts_close: %d %s cwd after=%s\n", r, en(errno), cwd_now());
        back_to_top();
    }

    printf("-- fts64\n");
    {
        FTS64 *f = fts64_open(t, FTS_PHYSICAL | FTS_NOCHDIR, NULL);
        FTSENT64 *e;
        int n = 0, d = 0;
        while ((e = fts64_read(f))) {
            n++;
            if (e->fts_info == FTS_D)
                d++;
        }
        printf("fts64 over t: %d entries, %d FTS_D; fts64_close %d\n", n, d, fts64_close(f));
    }

    /* fts(3)'s validity rules, which a shared path buffer must keep: the returned entry's path is
     * NUL-terminated at fts_pathlen; an ancestor (valid while the walk is inside it) is readable
     * to its fts_pathlen and names it; fts_accpath is the path (FTS_NOCHDIR, roots) or the name
     * (chdir mode); fts_children's entries have their own fts_pathlen. Only what fts(3) defines is
     * printed. */
    printf("-- fts path validity\n");
    {
        mkd("sp", 0755);
        mkd("sp/aaaa", 0755);
        mkd("sp/aaaa/bbbb", 0755);
        mkf("sp/aaaa/bbbb/f");
        mkf("sp/aaaa/g");
        mkf("sp/z");
        char *spv[] = {"sp", NULL};
        int sm[] = {FTS_PHYSICAL | FTS_NOCHDIR, FTS_PHYSICAL};
        for (int m = 0; m < 2; m++) {
            FTS *f = fts_open(spv, sm[m], by_name);
            FTSENT *e;
            int n = 0, nul = 0, anc = 0, acc = 0, kids = 0, kidlen = 0;
            while ((e = fts_read(f))) {
                n++;
                if (strlen(e->fts_path) == e->fts_pathlen)
                    nul++;
                int ok = 1;
                for (FTSENT *a = e->fts_parent; a && a->fts_level >= 0; a = a->fts_parent) {
                    size_t l = a->fts_pathlen;
                    if (l >= e->fts_pathlen || memcmp(a->fts_path, e->fts_path, l) != 0 || e->fts_path[l] != '/' || l < a->fts_namelen ||
                        memcmp(a->fts_path + l - a->fts_namelen, a->fts_name, a->fts_namelen) != 0)
                        ok = 0;
                }
                anc += ok;
                if (strcmp(e->fts_accpath, (m == 1 && e->fts_level > 0) ? e->fts_name : e->fts_path) == 0)
                    acc++;
                if (e->fts_info == FTS_D) {
                    for (FTSENT *c = fts_children(f, 0); c; c = c->fts_link) {
                        kids++;
                        if (c->fts_pathlen == e->fts_pathlen + 1 + c->fts_namelen)
                            kidlen++;
                    }
                }
            }
            printf("[%s] %d entries: NUL-terminated %d, ancestors readable to fts_pathlen %d, fts_accpath %d; "
                   "fts_children entries %d, fts_pathlen parent's + 1 + name's %d\n",
                   m ? "chdir" : "FTS_NOCHDIR", n, nul, anc, acc, kids, kidlen);
            fts_close(f);
            back_to_top();
        }
        unlink("sp/aaaa/bbbb/f");
        unlink("sp/aaaa/g");
        unlink("sp/z");
        rmdir("sp/aaaa/bbbb");
        rmdir("sp/aaaa");
        rmdir("sp");
    }

    printf("-- fts over the deep tree (each walk in a child process)\n");
    for (int m = 0; m < 4; m++)
        in_child(deep_fts, m);
}

static void deep_fts(int m)
{
    int dm[] = {FTS_PHYSICAL, FTS_PHYSICAL | FTS_NOCHDIR, FTS_LOGICAL, FTS_LOGICAL | FTS_NOCHDIR};
    const char *dl[] = {"FTS_PHYSICAL", "FTS_PHYSICAL|FTS_NOCHDIR", "FTS_LOGICAL", "FTS_LOGICAL|FTS_NOCHDIR"};
    char *deep[] = {"deep", NULL};
    {
        printf("fts(deep, %s):\n", dl[m]);
        fflush(stdout);
        int byinfo[16] = {0};
        int n = 0, maxlevel = 0, maxlen = 0;
        int firsterr = 0;
        char firsterrinfo[16] = "";
        int base = count_fds();
        int maxfds = 0;
        FTS *f = fts_open(deep, dm[m], NULL);
        FTSENT *e;
        errno = 0;
        while ((e = fts_read(f))) {
            n++;
            if (e->fts_info < 16)
                byinfo[e->fts_info]++;
            if (e->fts_level > maxlevel)
                maxlevel = e->fts_level;
            if ((int)e->fts_pathlen > maxlen)
                maxlen = (int)e->fts_pathlen;
            if (e->fts_errno && !firsterr) {
                firsterr = e->fts_errno;
                snprintf(firsterrinfo, sizeof firsterrinfo, "%s@%d", finame(e->fts_info), e->fts_level);
            }
            int k = count_fds() - base;
            if (k > maxfds)
                maxfds = k;
            errno = 0;
        }
        int err = errno;
        printf("  %s: entries=%d maxlevel=%d maxpathlen=%d D=%d DP=%d F=%d NS=%d DNR=%d ERR=%d first error %s %s; end errno=%s; most extra "
               "descriptors %d\n",
               dl[m], n, maxlevel, maxlen, byinfo[FTS_D], byinfo[FTS_DP], byinfo[FTS_F], byinfo[FTS_NS], byinfo[FTS_DNR], byinfo[FTS_ERR],
               firsterr ? en(firsterr) : "none", firsterrinfo, en(err), maxfds);
        fts_close(f);
        back_to_top();
    }
}

/* ---------------------------------------------------------------- §13.8 realpath */

static void rp_case(const char *in)
{
    static char b[PATH_MAX];
    memset(b, 'Q', sizeof b);
    b[sizeof b - 1] = 0;
    errno = 0;
    char *r = realpath(in, b);
    int e = errno;
    printf("realpath(%s): ", in ? (strlen(in) > 300 ? "<long>" : in[0] ? rel(in) : "\"\"") : "NULL");
    if (r)
        printf("%s%s", rel(r), r == b ? "" : " ret!=buf");
    else
        printf("NULL %s buf=%s", en(e), b[0] == 'Q' ? "untouched" : rel(b));
    errno = 0;
    char *a = realpath(in, NULL);
    e = errno;
    if (!r || !a || strcmp(a, r) != 0)
        printf(" | alloc: %s", a ? rel(a) : en(e));
    free(a);
    errno = 0;
    a = canonicalize_file_name(in);
    e = errno;
    if (!r || !a || strcmp(a, r) != 0)
        printf(" | canonicalize_file_name: %s", a ? rel(a) : en(e));
    free(a);
    printf("\n");
}

static void sec_realpath(void)
{
    printf("== realpath, canonicalize_file_name (lines show the caller-buffer result; alloc and canonicalize_file_name are shown only when they "
           "differ)\n");
    mkd("rp", 0755);
    mkd("rp/a", 0755);
    mkf("rp/a/f");
    sln("a", "rp/l_rel");
    char ab[PATH_MAX + 16];
    snprintf(ab, sizeof ab, "%s/rp/a", top);
    sln(ab, "rp/l_abs");
    sln("a/f", "rp/l_file");
    sln("missing", "rp/l_dang");
    sln("missing/x", "rp/l_dang2");
    sln("l_loop", "rp/l_loop");
    sln("l_pong", "rp/l_ping");
    sln("l_ping", "rp/l_pong");
    sln("/", "rp/l_root");
    sln("..", "rp/l_up");
    sln("a/../a/./f", "rp/l_dots");
    mkd("rp/ns", 0755);
    mkf("rp/ns/f");
    mkd("rp/ns/d", 0755);
    chmod("rp/ns", 0600);
    mkd("rp/nr", 0755);
    mkd("rp/nr/d", 0755);
    chmod("rp/nr", 0311);
    const char *cases[] = {"rp",
                           "rp/a/f",
                           "rp/./a//f",
                           "rp/a/../a/f",
                           "./rp/a/",
                           "rp/a/.",
                           "rp/l_rel",
                           "rp/l_rel/f",
                           "rp/l_abs/f",
                           "rp/l_file",
                           "rp/l_dots",
                           "rp/l_up/rp/a",
                           "rp/l_file/",
                           "rp/a/f/",
                           "rp/a/f/.",
                           "rp/a/f/..",
                           "rp/a/f/x",
                           "rp/missing",
                           "rp/missing/",
                           "rp/missing/x",
                           "rp/a/missing",
                           "rp/missing/../a",
                           "rp/l_dang",
                           "rp/l_dang/x",
                           "rp/l_dang2",
                           "rp/l_loop",
                           "rp/l_loop/x",
                           "rp/l_ping",
                           "rp/ns/f",
                           "rp/ns",
                           "rp/ns/d/..",
                           "rp/ns/..",
                           "rp/nr/d",
                           "rp/nr",
                           "rp/l_root",
                           "rp/l_root/..",
                           "/",
                           "//",
                           "///",
                           "/..",
                           "/../..",
                           "//.",
                           "",
                           ".",
                           "./",
                           NULL};
    for (int i = 0; cases[i]; i++)
        rp_case(cases[i]);
    snprintf(ab, sizeof ab, "%s/rp/l_rel/f", top);
    rp_case(ab);
    snprintf(ab, sizeof ab, "%s/rp/missing/x", top);
    rp_case(ab);
    char longname[300] = "rp/";
    memset(longname + 3, 'n', 256);
    longname[259] = 0;
    rp_case(longname);
    char longname2[300] = "rp/";
    memset(longname2 + 3, 'n', 255);
    longname2[258] = 0;
    rp_case(longname2);
    static char longdots[6000];
    strcpy(longdots, "rp");
    while (strlen(longdots) < 4200)
        strcat(longdots, "/.");
    strcat(longdots, "/a/f");
    printf("(<long> = rp + /. x %zu + /a/f, %zu bytes)\n", (strlen(longdots) - 6) / 2, strlen(longdots));
    rp_case(longdots);
    {
        volatile char *np = NULL;
        static char b[PATH_MAX];
        memset(b, 'Q', sizeof b);
        b[sizeof b - 1] = 0;
        errno = 0;
        char *r = realpath((const char *)np, b);
        printf("realpath(NULL, buf): %s %s buf=%s\n", r ? "ok" : "NULL", en(errno), b[0] == 'Q' ? "untouched" : rel(b));
        errno = 0;
        r = canonicalize_file_name((const char *)np);
        printf("canonicalize_file_name(NULL): %s %s\n", r ? "ok" : "NULL", en(errno));
    }

    printf("-- symbolic-link chains rp/ch/l<k> -> l<k-1>, l0 -> ../a/f\n");
    mkd("rp/ch", 0755);
    sln("../a/f", "rp/ch/l0");
    for (int k = 1; k <= 45; k++) {
        char tg[16], p[32];
        snprintf(tg, sizeof tg, "l%d", k - 1);
        snprintf(p, sizeof p, "rp/ch/l%d", k);
        sln(tg, p);
    }
    int lastok = -1, firstfail = -1, ferr = 0;
    for (int k = 0; k <= 45; k++) {
        char p[32];
        snprintf(p, sizeof p, "rp/ch/l%d", k);
        errno = 0;
        char *r = realpath(p, NULL);
        if (r && lastok == k - 1)
            lastok = k;
        if (!r && firstfail < 0) {
            firstfail = k;
            ferr = errno;
        }
        free(r);
    }
    printf("realpath: chains l0..l%d resolve (l%d is %d links); first failure l%d %s\n", lastok, lastok, lastok + 1, firstfail, en(ferr));
    int slast = -1;
    for (int k = 0; k <= 45; k++) {
        char p[32];
        struct stat st;
        snprintf(p, sizeof p, "rp/ch/l%d", k);
        if (stat(p, &st) == 0 && slast == k - 1)
            slast = k;
    }
    printf("stat (the kernel's limit, for comparison): chains l0..l%d resolve\n", slast);
    /* A chain within one path: l<k> as an intermediate directory component. */
    mkd("rp/dch", 0755);
    sln("../a", "rp/dch/m0");
    for (int k = 1; k <= 45; k++) {
        char tg[16], p[32];
        snprintf(tg, sizeof tg, "m%d", k - 1);
        snprintf(p, sizeof p, "rp/dch/m%d", k);
        sln(tg, p);
    }
    lastok = -1;
    firstfail = -1;
    for (int k = 0; k <= 45; k++) {
        char p[48];
        snprintf(p, sizeof p, "rp/dch/m%d/f", k);
        errno = 0;
        char *r = realpath(p, NULL);
        if (r && lastok == k - 1)
            lastok = k;
        if (!r && firstfail < 0) {
            firstfail = k;
            ferr = errno;
        }
        free(r);
    }
    printf("realpath m<k>/f (chain as a directory component): m0..m%d resolve; first failure m%d %s\n", lastok, firstfail, en(ferr));
    /* Many distinct links in one path, each a single hop: rp/hop/h<k> -> . */
    mkd("rp/hop", 0755);
    sln(".", "rp/hop/h");
    static char hp[4096];
    int lasth = 0;
    for (int k = 1; k <= 45; k++) {
        strcpy(hp, "rp/hop");
        for (int j = 0; j < k; j++)
            strcat(hp, "/h");
        strcat(hp, "/.");
        errno = 0;
        char *r = realpath(hp, NULL);
        if (r && lasth == k - 1)
            lasth = k;
        if (!r && lasth == k - 1)
            printf("realpath rp/hop/h/h/... (%d separate one-hop links): %s\n", k, en(errno));
        free(r);
    }
    printf("realpath through k separate one-hop links h -> .: up to k=%d resolve\n", lasth);
    chmod("rp/ns", 0755);
    chmod("rp/nr", 0755);
}

/* ---------------------------------------------------------------- §13.9 temporary names */

enum { K_MKSTEMP, K_MKSTEMPS, K_MKOSTEMP, K_MKOSTEMPS, K_MKSTEMP64, K_MKOSTEMP64, K_MKSTEMPS64, K_MKOSTEMPS64, K_MKDTEMP, K_MKTEMP };

static char tbuf[PATH_MAX + 64];
static char *tret; /* mkdtemp/mktemp return value */

/* One call; returns the descriptor (file kinds), 0 (mkdtemp/mktemp success) or -1. */
static int tcall(int k, int s, int f)
{
    tret = NULL;
    switch (k) {
    case K_MKSTEMP: return mkstemp(tbuf);
    case K_MKSTEMPS: return mkstemps(tbuf, s);
    case K_MKOSTEMP: return mkostemp(tbuf, f);
    case K_MKOSTEMPS: return mkostemps(tbuf, s, f);
    case K_MKSTEMP64: return mkstemp64(tbuf);
    case K_MKOSTEMP64: return mkostemp64(tbuf, f);
    case K_MKSTEMPS64: return mkstemps64(tbuf, s);
    case K_MKOSTEMPS64: return mkostemps64(tbuf, s, f);
    case K_MKDTEMP:
        tret = mkdtemp(tbuf);
        return tret ? 0 : -1;
    case K_MKTEMP:
        tret = mktemp(tbuf);
        return tbuf[0] ? 0 : -1;
    }
    return -1;
}

static void tundo(int k, int r)
{
    if (r < 0)
        return;
    if (k == K_MKDTEMP)
        rmdir(tbuf);
    else if (k != K_MKTEMP) {
        close(r);
        unlink(tbuf);
    }
}

static void fd_report(int fd, const char *name, char *out, size_t outsz)
{
    struct stat st, sn;
    fstat(fd, &st);
    int fl = fcntl(fd, F_GETFL);
    int fdf = fcntl(fd, F_GETFD);
    int acc = fl & O_ACCMODE;
    size_t o = (size_t)snprintf(out, outsz, " mode=%04o %s size=%ld acc=%s%s%s%s%s%s%s cloexec=%s", (unsigned)(st.st_mode & 07777), kind(st.st_mode), (long)st.st_size,
           acc == O_RDWR ? "RDWR" : acc == O_WRONLY ? "WRONLY" : "RDONLY", fl & O_APPEND ? "|APPEND" : "", (fl & O_SYNC) == O_SYNC ? "|SYNC" : "",
           (fl & O_DSYNC) && (fl & O_SYNC) != O_SYNC ? "|DSYNC" : "", fl & O_NONBLOCK ? "|NONBLOCK" : "", fl & O_NOATIME ? "|NOATIME" : "",
           fl & O_DIRECT ? "|DIRECT" : "", fdf & FD_CLOEXEC ? "yes" : "no");
    if (lstat(name, &sn) == 0)
        snprintf(out + o, outsz - o, " name-is-fd=%s", sn.st_ino == st.st_ino && sn.st_dev == st.st_dev ? "yes" : "no");
    else
        snprintf(out + o, outsz - o, " name: %s", en(errno));
}

/* Runs the call eight times on fresh copies of the template. The template is printed with every
 * position that changed in any run as '?' (always a letter or digit) or '!' (something else seen),
 * and the first run's result is described. Eight runs make a varying position that keeps its
 * template character every time (probability 62^-8) practically impossible. */
static void tcase(const char *label, int k, const char *orig, int s, int f)
{
    size_t n = strlen(orig);
    char mark[PATH_MAX];
    memset(mark, 0, sizeof mark);
    int r0 = 0, e0 = 0;
    char first[PATH_MAX + 64];
    char *ret0 = NULL;
    struct stat st0;
    int st0ok = 0;
    char fdline[512] = "";
    for (int run = 0; run < 8; run++) {
        snprintf(tbuf, sizeof tbuf, "%s", orig);
        errno = 0;
        int r = tcall(k, s, f);
        int e = errno;
        for (size_t i = 0; i < n; i++) {
            char c = tbuf[i];
            if (c != orig[i]) {
                int alnum = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9');
                if (!alnum || mark[i] == '!')
                    mark[i] = '!';
                else
                    mark[i] = '?';
            }
        }
        if (run == 0) {
            r0 = r;
            e0 = e;
            ret0 = tret;
            snprintf(first, sizeof first, "%s", tbuf);
            if (r >= 0 && k != K_MKDTEMP && k != K_MKTEMP)
                fd_report(r, tbuf, fdline, sizeof fdline);
            if (r >= 0 && (k == K_MKDTEMP || k == K_MKTEMP))
                st0ok = lstat(tbuf, &st0) == 0;
        }
        tundo(k, r);
        if (run == 0 && r < 0 && strcmp(tbuf, orig) == 0)
            break; /* the template was not touched: nothing varies */
    }
    char shown[PATH_MAX + 64];
    for (size_t i = 0; i <= n; i++)
        shown[i] = mark[i] ? mark[i] : orig[i];
    shown[n] = 0;
    if (strlen(first) != n)
        snprintf(shown, sizeof shown, "LENGTH-CHANGED(%zu)", strlen(first));
    printf("%s: ", label);
    if (k == K_MKTEMP) {
        printf("%s %s%s%s\n", ret0 == tbuf ? "ret==template" : ret0 ? "ret!=template" : "NULL", first[0] ? rel(shown) : "template[0]=NUL errno=",
               first[0] ? "" : en(e0), first[0] ? (st0ok ? " exists" : " not created") : "");
        return;
    }
    if (r0 >= 0) {
        if (k == K_MKDTEMP)
            printf("%s %s mode=%04o %s\n", ret0 == tbuf ? "ret==template" : "ret!=template", rel(shown), st0ok ? (unsigned)(st0.st_mode & 07777) : 0u,
                   st0ok ? kind(st0.st_mode) : "?");
        else
            printf("fd %s%s\n", rel(shown), fdline);
    } else {
        printf("%s %s template %s\n", k == K_MKDTEMP ? "NULL" : "-1", en(e0), strcmp(first, orig) == 0 ? "unchanged" : rel(shown));
    }
}

static void sec_mkstemp(void)
{
    printf("== temporary names (umask 022 unless stated; each case runs 8 times, '?' marks a position that varied)\n");
    mkd("mk", 0755);
    mkd("mk_ro", 0555);
    mkf("mk/file");
    const char *st[] = {"mk/aXXXXXX", "mk/XXXXXX", "XXXXXX", "mk/aXXXXXXXX", "mk/aXXXXX", "mk/a", "mk/axxxxxx", "mk/aXXXXXXb", "XXXXX", "",
                        "mk/aXXXXXX/", "mk_ro/aXXXXXX", "missing/aXXXXXX", "mk/file/aXXXXXX", "mk/a.XXXXXX.XXXXXX", "mk/\xc3\xa9XXXXXX", NULL};
    for (int i = 0; st[i]; i++) {
        char lab[128];
        snprintf(lab, sizeof lab, "mkstemp(%s)", st[i][0] ? rel(st[i]) : "\"\"");
        tcase(lab, K_MKSTEMP, st[i], 0, 0);
    }
    mode_t masks[] = {077, 0277, 0, 0777};
    for (int i = 0; i < 4; i++) {
        umask(masks[i]);
        char lab[64];
        snprintf(lab, sizeof lab, "mkstemp umask %03o", (unsigned)masks[i]);
        tcase(lab, K_MKSTEMP, "mk/uXXXXXX", 0, 0);
    }
    umask(022);
    {
        char names[20][32];
        int ok = 1;
        for (int i = 0; i < 20; i++) {
            snprintf(names[i], sizeof names[i], "mk/qXXXXXX");
            int fd = mkstemp(names[i]);
            if (fd < 0)
                ok = 0;
            else
                close(fd);
        }
        int distinct = 1;
        for (int i = 0; i < 20; i++)
            for (int j = i + 1; j < 20; j++)
                if (!strcmp(names[i], names[j]))
                    distinct = 0;
        printf("20 mkstemp calls on one template, files kept: all created %s, names distinct %s\n", ok ? "yes" : "no", distinct ? "yes" : "no");
        for (int i = 0; i < 20; i++)
            unlink(names[i]);
    }
    printf("-- mkstemps\n");
    struct {
        const char *t;
        int s;
    } ss[] = {{"mk/bXXXXXX.txt", 4}, {"mk/bXXXXXX.txt", 5}, {"mk/bXXXXXX.txt", 3}, {"mk/bXXXXXX", 0},  {"mk/bXXXXXX.txt", -1},
              {"mk/bXXXXXX.txt", 100}, {"mk/bXXXXXXXX", 2}, {"XXXXXX.t", 2},     {"mk/bXXXXXX.txt", 14}, {"mk/bXXXXXX.txt", 8}};
    for (size_t i = 0; i < sizeof ss / sizeof ss[0]; i++) {
        char lab[128];
        snprintf(lab, sizeof lab, "mkstemps(%s, %d)", ss[i].t, ss[i].s);
        tcase(lab, K_MKSTEMPS, ss[i].t, ss[i].s, 0);
    }
    printf("-- mkostemp flags\n");
    struct {
        const char *l;
        int f;
    } fs[] = {{"0", 0},
              {"O_CLOEXEC", O_CLOEXEC},
              {"O_APPEND", O_APPEND},
              {"O_SYNC", O_SYNC},
              {"O_DSYNC", O_DSYNC},
              {"O_NONBLOCK", O_NONBLOCK},
              {"O_NOATIME", O_NOATIME},
              {"O_WRONLY", O_WRONLY},
              {"O_RDWR", O_RDWR},
              {"O_ACCMODE", O_ACCMODE},
              {"O_TRUNC", O_TRUNC},
              {"O_CREAT|O_EXCL", O_CREAT | O_EXCL},
              {"O_NOFOLLOW", O_NOFOLLOW},
              {"O_NOCTTY", O_NOCTTY},
              {"O_DIRECTORY", O_DIRECTORY},
              {"O_PATH", O_PATH},
              {"O_TMPFILE", O_TMPFILE},
              {"O_DIRECT", O_DIRECT},
              {"O_CLOEXEC|O_APPEND|O_SYNC", O_CLOEXEC | O_APPEND | O_SYNC},
              {"0x40000000", 0x40000000}};
    for (size_t i = 0; i < sizeof fs / sizeof fs[0]; i++) {
        char lab[96];
        snprintf(lab, sizeof lab, "mkostemp(%s)", fs[i].l);
        tcase(lab, K_MKOSTEMP, "mk/oXXXXXX", 0, fs[i].f);
    }
    tcase("mkostemps(mk/pXXXXXX.s, 2, O_CLOEXEC|O_APPEND)", K_MKOSTEMPS, "mk/pXXXXXX.s", 2, O_CLOEXEC | O_APPEND);
    tcase("mkostemps(mk/pXXXXXX.s, 3, O_CLOEXEC)", K_MKOSTEMPS, "mk/pXXXXXX.s", 3, O_CLOEXEC);
    tcase("mkstemp64(mk/rXXXXXX)", K_MKSTEMP64, "mk/rXXXXXX", 0, 0);
    tcase("mkostemp64(mk/rXXXXXX, O_CLOEXEC)", K_MKOSTEMP64, "mk/rXXXXXX", 0, O_CLOEXEC);
    tcase("mkstemps64(mk/rXXXXXX.x, 2)", K_MKSTEMPS64, "mk/rXXXXXX.x", 2, 0);
    tcase("mkostemps64(mk/rXXXXXX.x, 2, O_APPEND)", K_MKOSTEMPS64, "mk/rXXXXXX.x", 2, O_APPEND);

    printf("-- mkdtemp\n");
    const char *dt[] = {"mk/dXXXXXX", "mk/dXXXXX", "mk/dXXXXXXXX", "mk/dXXXXXX/", "mk_ro/dXXXXXX", "missing/dXXXXXX", "mk/file/dXXXXXX", "", NULL};
    for (int i = 0; dt[i]; i++) {
        char lab[96];
        snprintf(lab, sizeof lab, "mkdtemp(%s)", dt[i][0] ? rel(dt[i]) : "\"\"");
        tcase(lab, K_MKDTEMP, dt[i], 0, 0);
    }
    for (int i = 0; i < 4; i++) {
        umask(masks[i]);
        char lab[64];
        snprintf(lab, sizeof lab, "mkdtemp umask %03o", (unsigned)masks[i]);
        tcase(lab, K_MKDTEMP, "mk/vXXXXXX", 0, 0);
    }
    umask(022);

    printf("-- mktemp\n");
    const char *mt[] = {"mk/gXXXXXX", "mk/gXXXXX", "mk/g", "missing/gXXXXXX", "mk_ro/gXXXXXX", "mk/file/gXXXXXX", "", NULL};
    for (int i = 0; mt[i]; i++) {
        char lab[96];
        snprintf(lab, sizeof lab, "mktemp(%s)", mt[i][0] ? rel(mt[i]) : "\"\"");
        tcase(lab, K_MKTEMP, mt[i], 0, 0);
    }
    chmod("mk_ro", 0755);
}

/* ---------------------------------------------------------------- privileged cases */

static void root_phase(void)
{
    printf("== root phase: euid 0 %s; scratch fs %s\n", geteuid() == 0 ? "yes" : "no", fsname);
    char buf[PATH_MAX];
    mkd("c", 0755);
    mkd("c/in", 0755);
    mkf("file");
    printf("-- chroot (in a child)\n");
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        tw_alarm(30);
        errno = 0;
        int r = chroot("c");
        printf("chroot(c) with cwd <dir>: %d %s\n", r, r ? en(errno) : "-");
        show_getcwd("  getcwd(buf, PATH_MAX)", buf, sizeof buf, sizeof buf);
        show_getcwd("  getcwd(NULL, 0)", NULL, 0, 0);
        show_getwd("  getwd");
        unsetenv("PWD");
        show_gcdn("  get_current_dir_name PWD unset");
        setenv("PWD", "/", 1);
        show_gcdn("  get_current_dir_name PWD=/");
        unsetenv("PWD");
        memset(buf, 'Q', sizeof buf);
        buf[sizeof buf - 1] = 0;
        errno = 0;
        char *rp = realpath(".", buf);
        printf("  realpath(., buf): %s buf=%s\n", rp ? rel(rp) : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
        memset(buf, 'Q', sizeof buf);
        buf[sizeof buf - 1] = 0;
        errno = 0;
        rp = realpath("c/in", buf);
        printf("  realpath(c/in, buf) (relative to the old cwd): %s buf=%s\n", rp ? rel(rp) : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
        errno = 0;
        rp = realpath("/in/..", NULL);
        printf("  realpath(/in/..): %s\n", rp ? rp : en(errno));
        free(rp);
        errno = 0;
        r = chdir("/in");
        printf("  chdir(/in): %d; getcwd=%s\n", r, cwd_now());
        errno = 0;
        r = chdir("/..");
        printf("  chdir(/..): %d; getcwd=%s\n", r, cwd_now());
        fflush(stdout);
        _exit(0);
    }
    waitpid(pid, NULL, 0);
    fflush(stdout);
    pid = fork();
    if (pid == 0) {
        tw_alarm(30);
        errno = 0;
        int r = chroot("missing");
        printf("chroot(missing) as root: %d %s\n", r, en(errno));
        errno = 0;
        r = chroot("file");
        printf("chroot(file) as root: %d %s\n", r, en(errno));
        errno = 0;
        r = chroot("");
        printf("chroot(\"\") as root: %d %s\n", r, en(errno));
        fflush(stdout);
        _exit(0);
    }
    waitpid(pid, NULL, 0);

    printf("-- lazy unmount of the working directory's file system\n");
    mkd("lz", 0755);
    errno = 0;
    if (mount("tmpfs", "lz", "tmpfs", 0, "size=64k") != 0) {
        printf("mount lz: %s\n", en(errno));
    } else {
        mkd("lz/in", 0755);
        if (chdir("lz/in") == 0) {
            char p[PATH_MAX + 8];
            snprintf(p, sizeof p, "%s/lz", top);
            errno = 0;
            int ur = umount2(p, MNT_DETACH);
            printf("umount2(<dir>/lz, MNT_DETACH): %d %s\n", ur, en(errno));
            show_getcwd("  getcwd(buf, PATH_MAX)", buf, sizeof buf, sizeof buf);
            show_getcwd("  getcwd(NULL, 0)", NULL, 0, 0);
            show_getwd("  getwd");
            unsetenv("PWD");
            show_gcdn("  get_current_dir_name PWD unset");
            snprintf(p, sizeof p, "%s/lz/in", top);
            setenv("PWD", p, 1);
            show_gcdn("  get_current_dir_name PWD=<dir>/lz/in");
            unsetenv("PWD");
            memset(buf, 'Q', sizeof buf);
            buf[sizeof buf - 1] = 0;
            errno = 0;
            char *rp = realpath(".", buf);
            printf("  realpath(., buf): %s buf=%s\n", rp ? rel(rp) : en(errno), buf[0] == 'Q' ? "untouched" : rel(buf));
            errno = 0;
            rp = realpath("..", NULL);
            printf("  realpath(.., NULL): %s\n", rp ? rel(rp) : en(errno));
            free(rp);
            DIR *d = opendir(".");
            int n = 0;
            if (d) {
                while (readdir(d))
                    n++;
                closedir(d);
            }
            printf("  opendir(.) still reads: %d entries\n", n);
        }
        back_to_top();
        rmdir("lz");
    }

    printf("-- FTW_MOUNT, FTS_XDEV, device nodes: m/ (fs %s) holds f and mnt, a second tmpfs\n", fsname);
    mkd("m", 0755);
    mkf("m/f");
    mkd("m/mnt", 0755);
    errno = 0;
    if (mount("tmpfs", "m/mnt", "tmpfs", 0, "size=64k") != 0) {
        printf("mount m/mnt: %s\n", en(errno));
        return;
    }
    printf("m/mnt fs %s\n", fsname_of("m/mnt"));
    mkf("m/mnt/inner");
    mkd("m/mnt/sub", 0755);
    mkf("m/mnt/sub/g");
    if (mknod("m/mnt/chr", S_IFCHR | 0600, makedev(1, 3)) != 0)
        printf("mknod chr: %s\n", en(errno));
    if (mknod("m/mnt/blk", S_IFBLK | 0600, makedev(7, 250)) != 0)
        printf("mknod blk: %s\n", en(errno));
    mkd("m/zero", 0000);
    {
        static struct ent v[32];
        int err;
        DIR *d = opendir("m/mnt");
        int n = read_all(d, v, 32, &err);
        closedir(d);
        print_ents("m/mnt sorted, with d_type", v, n, 1, 1);
    }
    run_nftw("FTW_PHYS", "m", 10, FTW_PHYS);
    run_nftw("FTW_PHYS|FTW_MOUNT", "m", 10, FTW_PHYS | FTW_MOUNT);
    run_nftw("FTW_PHYS|FTW_MOUNT|FTW_DEPTH", "m", 10, FTW_PHYS | FTW_MOUNT | FTW_DEPTH);
    run_nftw("FTW_MOUNT", "m/mnt", 10, FTW_MOUNT);
    char *mr[] = {"m", NULL};
    run_fts("FTS_PHYSICAL|FTS_NOCHDIR", mr, FTS_PHYSICAL | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_XDEV|FTS_NOCHDIR", mr, FTS_PHYSICAL | FTS_XDEV | FTS_NOCHDIR, by_name, NULL, 0);
    run_fts("FTS_PHYSICAL|FTS_XDEV", mr, FTS_PHYSICAL | FTS_XDEV, by_name, NULL, 0);
    char *mm[] = {"m/mnt", NULL};
    run_fts("FTS_PHYSICAL|FTS_XDEV|FTS_NOCHDIR", mm, FTS_PHYSICAL | FTS_XDEV | FTS_NOCHDIR, by_name, NULL, 0);
    umount2("m/mnt", 0);
    rmdir("m/zero");
    unlink("m/f");
    rmdir("m/mnt");
    rmdir("m");
    rmdir("c/in");
    rmdir("c");
    unlink("file");
    sec_removed_dir();
}

/* ---------------------------------------------------------------- exhaustion (outside the record) */

/* `fstree_diff DIR exhaust FUNC`: one call of FUNC on a template in DIR (the current directory),
 * for running under `strace -e inject=...` so that every create fails with EEXIST (or every name
 * appears to exist, for mktemp). Not part of the record; fstree.md's section "Exhaustion under fault
 * injection" gives the commands. Nothing before the call opens or creates a file. */
static int exhaust(const char *fn)
{
    char t[64];
    int r = -2;
    char *p = NULL;
    FILE *f = NULL;
    snprintf(t, sizeof t, "%s", !strcmp(fn, "mkstemps") || !strcmp(fn, "mkostemps") ? "aXXXXXX.sfx" : "aXXXXXX");
    errno = 0;
    if (!strcmp(fn, "mkstemp"))
        r = mkstemp(t);
    else if (!strcmp(fn, "mkostemp"))
        r = mkostemp(t, O_CLOEXEC);
    else if (!strcmp(fn, "mkstemps"))
        r = mkstemps(t, 4);
    else if (!strcmp(fn, "mkostemps"))
        r = mkostemps(t, 4, O_CLOEXEC);
    else if (!strcmp(fn, "mkstemp64"))
        r = mkstemp64(t);
    else if (!strcmp(fn, "mkdtemp"))
        p = mkdtemp(t);
    else if (!strcmp(fn, "mktemp"))
        p = mktemp(t);
    else if (!strcmp(fn, "tmpfile"))
        f = tmpfile();
    else {
        fprintf(stderr, "unknown function %s\n", fn);
        return 2;
    }
    int e = errno;
    if (!strcmp(fn, "tmpfile"))
        printf("%s: %s errno=%s\n", fn, f ? "FILE" : "NULL", en(e));
    else if (!strcmp(fn, "mkdtemp") || !strcmp(fn, "mktemp"))
        printf("%s: %s errno=%s template=[%s]%s%s%s\n", fn, p == t ? "ret==template" : p ? "ret!=template" : "NULL", en(e), t,
               t[0] ? "" : " (template[0]=NUL; the bytes after it: [", t[0] ? "" : t + 1, t[0] ? "" : "])");
    else
        printf("%s: %d errno=%s template=[%s]\n", fn, r, en(e), t);
    return 0;
}

/* ---------------------------------------------------------------- cleanup */

static int rm_cb(const char *p, const struct stat *st, int f, struct FTW *w)
{
    (void)st;
    (void)w;
    if (strcmp(p, ".") == 0)
        return 0;
    if (f == FTW_DP)
        rmdir(p);
    else
        unlink(p);
    return 0;
}

static void cleanup(void)
{
    back_to_top();
    chmod("t/noread", 0755);
    chmod("t/nosearch", 0755);
    chmod("rp/ns", 0755);
    chmod("rp/nr", 0755);
    chmod("mk_ro", 0755);
    remove_deep();
    nftw(".", rm_cb, 16, FTW_DEPTH | FTW_PHYS);
    DIR *d = opendir(".");
    int left = 0;
    struct dirent *e;
    while ((e = readdir(d)))
        if (strcmp(e->d_name, ".") && strcmp(e->d_name, ".."))
            left++;
    closedir(d);
    printf("cleanup: %d entries left in <dir>\n", left);
}

int main(int argc, char **argv)
{
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct rlimit nocore = {0, 0};
    setrlimit(RLIMIT_CORE, &nocore); /* a walk that aborts leaves no core file */
    tw_alarm(600);
    if (argc < 2 || argv[1][0] != '/') {
        fprintf(stderr, "usage: fstree_diff /ABSOLUTE/SCRATCH [root]\n");
        return 2;
    }
    snprintf(top, sizeof top, "%s", argv[1]);
    toplen = strlen(top);
    while (toplen > 1 && top[toplen - 1] == '/')
        top[--toplen] = 0;
    memset(LONGN, 'd', 200);
    LONGN[200] = 0;
    if (chdir(top) != 0) {
        printf("cannot enter scratch directory: %s\n", en(errno));
        return 1;
    }
    umask(022);
    unsetenv("PWD");
    setlocale(LC_ALL, "C");
    snprintf(fsname, sizeof fsname, "%s", fsname_of("."));
    snprintf(fstag, sizeof fstag, "%s", fsname);
    if (strchr(fstag, ' '))
        *strchr(fstag, ' ') = 0;
    if (argc > 3 && strcmp(argv[2], "exhaust") == 0)
        return exhaust(argv[3]);
    printf("scratch directory file system: %s\n", fsname);
    if (argc > 2 && strcmp(argv[2], "root") == 0) {
        root_phase();
        printf("done root\n");
        return 0;
    }
    printf("running as root: %s (the permission cases need a non-root user)\n", geteuid() == 0 ? "yes" : "no");
    make_t();
    build_deep();
    sec_getcwd();
    back_to_top();
    sec_deep_cwd();
    back_to_top();
    sec_readdir();
    back_to_top();
    sec_removed_dir();
    back_to_top();
    sec_exactly_once();
    back_to_top();
    sec_scandir();
    back_to_top();
    sec_nftw();
    {
        printf("nftw(/, FTW_PHYS|FTW_ACTIONRETVAL) with a callback that stops at once:\n");
        errno = 0;
        int r = nftw("/", stop_root, 10, FTW_PHYS | FTW_ACTIONRETVAL);
        printf("  -> %d\n", r);
    }
    back_to_top();
    sec_deep_walks();
    back_to_top();
    sec_fts();
    back_to_top();
    sec_realpath();
    back_to_top();
    sec_mkstemp();
    cleanup();
    printf("done\n");
    return 0;
}
