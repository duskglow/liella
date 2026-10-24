/* POSIX message queues and shared memory end to end (spec §25.3-25.4; D-25.2, D-25.3; phase 13):
 * built against the host C library and against Liella's products, the outputs must match line for
 * line. Black-box: every line is a result, an errno, or a fact read back through a public
 * function or the file system.
 *
 * The program never touches the system's queues or /dev/shm. posixipc_run.sh runs it as root in a
 * private IPC namespace (`unshare -i`, so every queue lives and dies with the namespace) and a
 * private mount namespace in which /dev/mqueue is a fresh mqueue mount of that IPC namespace and
 * /dev/shm a fresh tmpfs. Each phase refuses to run unless the fixture's marker is present
 * (/dev/shm/.liella-posixipc-fixture, or /dev/.liella-posixipc-nodev for the `nodev` phase).
 *
 * Output conventions: strings are printed in brackets with every byte outside printable ASCII as
 * \xNN and a backslash as \\; a run of more than 60 equal bytes is printed as <c>*<n>. Nothing
 * prints a descriptor number, an address, a process or thread ID or a time; IDs are compared and
 * printed as `self`, `other`, `main`. Risky cases run in forked children with an alarm; the line
 * "... child exited N" or "... child killed by SIG" follows the child's own output.
 *
 * Usage: posixipc_diff PHASE [DIR], normally through posixipc_run.sh:
 *  - setup (root): creates root-owned queues and /dev/shm entries for the permission cases;
 *  - user (an ordinary user): everything else, with /dev/shm a tmpfs;
 *  - ext4 (the same user): /dev/shm is a bind mount of an ext4 directory;
 *  - nodev (the same user): /dev is an empty tmpfs, so there is no /dev/shm. */
#define _GNU_SOURCE
#undef _FORTIFY_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <mqueue.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/statfs.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "tscale.h"

/* The probe passes NULL where the headers declare non-null parameters, on purpose. */
#pragma GCC diagnostic ignored "-Wnonnull"

/* Declared by <bits/mqueue2.h>, which <mqueue.h> includes only when _FORTIFY_SOURCE is active;
 * the probe undefines _FORTIFY_SOURCE and repeats the declaration. */
extern mqd_t __mq_open_2(const char *name, int oflag);

#define SHM_MARKER "/dev/shm/.liella-posixipc-fixture"
#define NODEV_MARKER "/dev/.liella-posixipc-nodev"

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
    case EACCES: return "EACCES";
    case EEXIST: return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EISDIR: return "EISDIR";
    case EINVAL: return "EINVAL";
    case ENFILE: return "ENFILE";
    case EMFILE: return "EMFILE";
    case ENOSPC: return "ENOSPC";
    case ENOMEM: return "ENOMEM";
    case EAGAIN: return "EAGAIN";
    case EINTR: return "EINTR";
    case EFAULT: return "EFAULT";
    case EBUSY: return "EBUSY";
    case EMSGSIZE: return "EMSGSIZE";
    case ETIMEDOUT: return "ETIMEDOUT";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case ELOOP: return "ELOOP";
    case ENOSYS: return "ENOSYS";
    case ENXIO: return "ENXIO";
    case EROFS: return "EROFS";
    case EOVERFLOW: return "EOVERFLOW";
    default:
        k = (k + 1) % 4;
        snprintf(b[k], sizeof b[k], "errno%d", e);
        return b[k];
    }
}

static const char *sig_name(int s)
{
    static char b[16];
    switch (s) {
    case SIGABRT: return "SIGABRT";
    case SIGSEGV: return "SIGSEGV";
    case SIGBUS: return "SIGBUS";
    case SIGALRM: return "SIGALRM";
    case SIGUSR1: return "SIGUSR1";
    case SIGUSR2: return "SIGUSR2";
    case SIGKILL: return "SIGKILL";
    default:
        snprintf(b, sizeof b, "signal%d", s);
        return b;
    }
}

static const char *vis(const char *s)
{
    static char b[6][1024];
    static int k;
    if (!s)
        return "(null)";
    k = (k + 1) % 6;
    char *o = b[k];
    size_t w = 0, n = strlen(s), cap = sizeof b[k] - 16;
    o[w++] = '[';
    for (size_t i = 0; i < n && w < cap;) {
        size_t run = 1;
        while (i + run < n && s[i + run] == s[i])
            run++;
        unsigned char c = (unsigned char)s[i];
        if (run > 60) {
            if (c >= 0x20 && c < 0x7f && c != '\\')
                w += (size_t)snprintf(o + w, sizeof b[k] - w, "<%c>*%zu", c, run);
            else
                w += (size_t)snprintf(o + w, sizeof b[k] - w, "<\\x%02x>*%zu", c, run);
            i += run;
            continue;
        }
        if (c == '\\')
            w += (size_t)snprintf(o + w, sizeof b[k] - w, "\\\\");
        else if (c >= 0x20 && c < 0x7f)
            o[w++] = (char)c;
        else
            w += (size_t)snprintf(o + w, sizeof b[k] - w, "\\x%02x", c);
        i++;
    }
    o[w++] = ']';
    o[w] = 0;
    return o;
}

/* "ok" or the errno of a -1 result. */
static const char *rc_(int r, int e)
{
    return r == -1 ? en(e) : r == 0 ? "0" : "ok";
}

/* rc(CALL, errno): the result of CALL, then the errno it left (the second argument is only a
 * reminder; errno is read after CALL returns). */
#define rc(call, ignored) ({ int r_ = (call); int e_ = errno; rc_(r_, e_); })

/* The result of a size-returning CALL and, when it is -1, the errno it left. */
static const char *szs(ssize_t s, int e)
{
    static char b[4][48];
    static int k;
    k = (k + 1) % 4;
    snprintf(b[k], sizeof b[k], "%zd %s", s, en(s < 0 ? e : 0));
    return b[k];
}
#define SZ(call) ({ ssize_t s_ = (call); int e_ = errno; szs(s_, e_); })

static void child_status(const char *label, int st)
{
    if (WIFEXITED(st))
        printf("%s: child exited %d\n", label, WEXITSTATUS(st));
    else if (WIFSIGNALED(st))
        printf("%s: child killed by %s\n", label, sig_name(WTERMSIG(st)));
}

/* Run fn in a child with an alarm; its standard error is printed after its status. */
static void run_child(const char *label, void (*fn)(void), unsigned secs)
{
    int ep[2];
    fflush(stdout);
    if (pipe2(ep, O_CLOEXEC) != 0)
        return;
    pid_t p = fork();
    if (p == 0) {
        tw_alarm(secs);
        dup2(ep[1], 2);
        fn();
        fflush(stdout);
        _exit(0);
    }
    close(ep[1]);
    char buf[512];
    size_t n = 0;
    ssize_t r;
    while (n < sizeof buf - 1 && (r = read(ep[0], buf + n, sizeof buf - 1 - n)) > 0)
        n += (size_t)r;
    buf[n] = 0;
    close(ep[0]);
    int st;
    waitpid(p, &st, 0);
    child_status(label, st);
    if (n)
        printf("%s: child stderr %s\n", label, vis(buf));
}

/* The entries of a directory except the marker, in strcmp order, as one line. */
static int cmpstr(const void *a, const void *b)
{
    return strcmp(*(char *const *)a, *(char *const *)b);
}

static void list_dir(const char *label, const char *path)
{
    DIR *d = opendir(path);
    if (!d) {
        printf("%s: %s\n", label, en(errno));
        return;
    }
    char *names[64];
    int n = 0;
    struct dirent *e;
    while ((e = readdir(d)) && n < 64) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")
            || !strcmp(e->d_name, ".liella-posixipc-fixture"))
            continue;
        names[n++] = strdup(e->d_name);
    }
    closedir(d);
    qsort(names, (size_t)n, sizeof names[0], cmpstr);
    printf("%s:", label);
    if (!n)
        printf(" (empty)");
    for (int i = 0; i < n; i++) {
        printf(" %s", vis(names[i]));
        free(names[i]);
    }
    printf("\n");
}

static void mode_of(const char *label, const char *path)
{
    struct stat st;
    if (lstat(path, &st) != 0) {
        printf("%s: lstat %s\n", label, en(errno));
        return;
    }
    printf("%s: mode %04o owner %s size %lld\n", label, (unsigned)(st.st_mode & 07777),
           st.st_uid == getuid() ? "self" : st.st_uid == 0 ? "root" : "other",
           (long long)st.st_size);
}

static void fd_flags(const char *label, int fd)
{
    int fdf = fcntl(fd, F_GETFD), fl = fcntl(fd, F_GETFL);
    if (fdf < 0 || fl < 0) {
        printf("%s: fcntl %s\n", label, en(errno));
        return;
    }
    int acc = fl & O_ACCMODE;
    printf("%s: cloexec=%s access=%s append=%s nonblock=%s\n", label,
           (fdf & FD_CLOEXEC) ? "yes" : "no",
           acc == O_RDONLY ? "RDONLY" : acc == O_WRONLY ? "WRONLY" : acc == O_RDWR ? "RDWR" : "3",
           (fl & O_APPEND) ? "yes" : "no", (fl & O_NONBLOCK) ? "yes" : "no");
}

static const char *fstype(long t)
{
    static char b[32];
    switch ((unsigned long)t) {
    case 0x01021994UL: return "tmpfs";
    case 0xEF53UL: return "ext2/3/4";
    case 0x19800202UL: return "mqueue";
    case 0x9fa0UL: return "proc";
    default:
        snprintf(b, sizeof b, "%#lx", (unsigned long)t);
        return b;
    }
}

static void fs_of(const char *label, const char *path)
{
    struct statfs s;
    if (statfs(path, &s) != 0)
        printf("%s: statfs %s\n", label, en(errno));
    else
        printf("%s: file system %s\n", label, fstype((long)s.f_type));
}

/* ---------------------------------------------------------------- message queues */

static void pattr(const char *label, mqd_t q)
{
    struct mq_attr a;
    memset(&a, 0x5a, sizeof a);
    int r = mq_getattr(q, &a);
    if (r != 0) {
        printf("%s: mq_getattr %s\n", label, en(errno));
        return;
    }
    printf("%s: flags %#lx maxmsg %ld msgsize %ld curmsgs %ld\n", label, (unsigned long)a.mq_flags,
           (long)a.mq_maxmsg, (long)a.mq_msgsize, (long)a.mq_curmsgs);
}

static mqd_t mk(const char *name, long maxmsg, long msgsize, int extra)
{
    struct mq_attr a = {.mq_maxmsg = maxmsg, .mq_msgsize = msgsize};
    mq_unlink(name);
    return mq_open(name, O_RDWR | O_CREAT | O_EXCL | extra, 0600, &a);
}

static void read_sysctl(const char *name)
{
    char p[128], b[64];
    snprintf(p, sizeof p, "/proc/sys/fs/mqueue/%s", name);
    FILE *f = fopen(p, "re");
    if (!f) {
        printf("%s: %s\n", name, en(errno));
        return;
    }
    if (!fgets(b, sizeof b, f))
        b[0] = 0;
    fclose(f);
    b[strcspn(b, "\n")] = 0;
    printf("%s: %s\n", name, b);
}

static void s_mq_names(void)
{
    printf("== mq_open names\n");
    static char n255[260], n256[260], bare255[260];
    n255[0] = n256[0] = '/';
    memset(n255 + 1, 'n', 255);
    memset(n256 + 1, 'n', 256);
    memset(bare255, 'b', 255);
    const char *names[] = {"/q", "q", "//q", "/a/b", "/", "", "/.", "/..", "/q/", "/sp ace",
                           "/nl\n", "/\xc3\xa9", n255, n256, bare255, "/a/", "///"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 8};
        errno = 0;
        mqd_t q = mq_open(names[i], O_RDWR | O_CREAT | O_EXCL, 0600, &a);
        int e = errno;
        printf("name %s: mq_open %s\n", vis(names[i]), q == (mqd_t)-1 ? en(e) : "ok");
        list_dir("  /dev/mqueue", "/dev/mqueue");
        if (q != (mqd_t)-1) {
            errno = 0;
            mqd_t q2 = mq_open(names[i], O_RDWR);
            printf("  reopen without O_CREAT: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
            if (q2 != (mqd_t)-1)
                mq_close(q2);
            mq_close(q);
        }
        errno = 0;
        int r = mq_unlink(names[i]);
        printf("  mq_unlink: %s\n", rc(r, errno));
    }
    list_dir("after the names: /dev/mqueue", "/dev/mqueue");
}

static void s_mq_attrs(void)
{
    printf("== mq_open attributes\n");
    read_sysctl("msg_default");
    read_sysctl("msg_max");
    read_sysctl("msgsize_default");
    read_sysctl("msgsize_max");
    read_sysctl("queues_max");
    printf("MQ_PRIO_MAX %d sysconf(_SC_MQ_PRIO_MAX) %ld sysconf(_SC_MQ_OPEN_MAX) %ld\n",
           MQ_PRIO_MAX, sysconf(_SC_MQ_PRIO_MAX), sysconf(_SC_MQ_OPEN_MAX));
    printf("sizeof(struct mq_attr) %zu sizeof(mqd_t) %zu\n", sizeof(struct mq_attr), sizeof(mqd_t));
    mq_unlink("/attr");
    mqd_t q = mq_open("/attr", O_RDWR | O_CREAT | O_EXCL, 0600, NULL);
    printf("attr NULL: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    if (q != (mqd_t)-1) {
        pattr("attr NULL", q);
        mq_close(q);
    }
    mq_unlink("/attr");
    struct { long maxmsg, msgsize, flags, cur; } c[] = {
        {1, 1, 0, 0},         {10, 8192, 0, 0},  {11, 8, 0, 0},       {1, 8193, 0, 0},
        {0, 8, 0, 0},         {1, 0, 0, 0},      {-1, 8, 0, 0},       {1, -1, 0, 0},
        {LONG_MAX, 8, 0, 0},  {1, LONG_MAX, 0, 0}, {2, 4, O_NONBLOCK, 0}, {2, 4, 0, 5},
        {2, 4, 0x7fff, 0},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++) {
        struct mq_attr a = {.mq_flags = c[i].flags, .mq_maxmsg = c[i].maxmsg,
                            .mq_msgsize = c[i].msgsize, .mq_curmsgs = c[i].cur};
        char l[96];
        snprintf(l, sizeof l, "attr maxmsg %ld msgsize %ld flags %#lx curmsgs %ld", c[i].maxmsg,
                 c[i].msgsize, (unsigned long)c[i].flags, c[i].cur);
        errno = 0;
        q = mq_open("/attr", O_RDWR | O_CREAT | O_EXCL, 0600, &a);
        printf("%s: %s\n", l, q == (mqd_t)-1 ? en(errno) : "ok");
        if (q != (mqd_t)-1) {
            pattr(l, q);
            mq_close(q);
        }
        mq_unlink("/attr");
    }
    /* Attributes are ignored when the queue exists or O_CREAT is absent. */
    q = mk("/attr", 2, 4, 0);
    struct mq_attr bad = {.mq_maxmsg = 0, .mq_msgsize = 0};
    errno = 0;
    mqd_t q2 = mq_open("/attr", O_RDWR | O_CREAT, 0600, &bad);
    printf("existing queue, O_CREAT, invalid attr: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1) {
        pattr("existing queue, O_CREAT, invalid attr", q2);
        mq_close(q2);
    }
    errno = 0;
    q2 = mq_open("/attr", O_RDWR, 0600, &bad);
    printf("existing queue, no O_CREAT, invalid attr: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1)
        mq_close(q2);
    mq_close(q);
    mq_unlink("/attr");
}

static void s_mq_flags(void)
{
    printf("== mq_open flags and modes\n");
    umask(022);
    mqd_t q = mk("/f", 2, 8, 0);
    fd_flags("O_RDWR|O_CREAT|O_EXCL descriptor", q);
    pattr("O_RDWR", q);
    errno = 0;
    mqd_t q2 = mq_open("/f", O_RDWR | O_CREAT | O_EXCL, 0600, NULL);
    printf("O_CREAT|O_EXCL on an existing queue: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    errno = 0;
    q2 = mq_open("/f", O_RDWR | O_EXCL);
    printf("O_EXCL without O_CREAT on an existing queue: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1)
        mq_close(q2);
    q2 = mq_open("/f", O_RDWR | O_NONBLOCK);
    pattr("O_RDWR|O_NONBLOCK", q2);
    fd_flags("O_RDWR|O_NONBLOCK descriptor", q2);
    mq_close(q2);
    q2 = mq_open("/f", O_WRONLY | O_CLOEXEC);
    fd_flags("O_WRONLY|O_CLOEXEC descriptor", q2);
    errno = 0;
    char buf[64];
    ssize_t r = mq_receive(q2, buf, sizeof buf, NULL);
    printf("mq_receive on O_WRONLY: %s\n", r < 0 ? en(errno) : "ok");
    mq_close(q2);
    q2 = mq_open("/f", O_RDONLY);
    errno = 0;
    printf("mq_send on O_RDONLY: %s\n", rc(mq_send(q2, "x", 1, 0), errno));
    mq_close(q2);
    errno = 0;
    q2 = mq_open("/f", O_ACCMODE);
    printf("access mode 3: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1)
        mq_close(q2);
    errno = 0;
    q2 = mq_open("/f", O_RDWR | O_TRUNC);
    printf("O_TRUNC: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1)
        mq_close(q2);
    mq_close(q);
    mq_unlink("/f");
    errno = 0;
    q = mq_open("/missing", O_RDWR);
    printf("missing queue without O_CREAT: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    /* Modes: the umask applies; the owner is checked like anyone else. */
    struct { int mode; const char *n; } m[] = {{0666, "0666"}, {0600, "0600"}, {0400, "0400"},
                                               {0200, "0200"}, {0, "0"}, {07777, "07777"}};
    for (size_t i = 0; i < sizeof m / sizeof m[0]; i++) {
        struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 8};
        mq_unlink("/m");
        q = mq_open("/m", O_RDWR | O_CREAT | O_EXCL, m[i].mode, &a);
        int e = errno;
        char l[64];
        snprintf(l, sizeof l, "mode %s, umask 022", m[i].n);
        if (q == (mqd_t)-1) {
            printf("%s: create %s\n", l, en(e));
            continue;
        }
        mode_of(l, "/dev/mqueue/m");
        mq_close(q);
        errno = 0;
        q = mq_open("/m", O_RDONLY);
        printf("%s: reopen O_RDONLY %s", l, q == (mqd_t)-1 ? en(errno) : "ok");
        if (q != (mqd_t)-1)
            mq_close(q);
        errno = 0;
        q = mq_open("/m", O_WRONLY);
        printf(", O_WRONLY %s", q == (mqd_t)-1 ? en(errno) : "ok");
        if (q != (mqd_t)-1)
            mq_close(q);
        errno = 0;
        q = mq_open("/m", O_RDWR);
        printf(", O_RDWR %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
        if (q != (mqd_t)-1)
            mq_close(q);
        mq_unlink("/m");
    }
    umask(0);
    struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 8};
    q = mq_open("/m", O_RDWR | O_CREAT | O_EXCL, 0666, &a);
    mode_of("mode 0666, umask 0", "/dev/mqueue/m");
    mq_close(q);
    mq_unlink("/m");
    umask(022);
    /* Queues created by root in the setup phase. */
    mode_of("root's /rootq", "/dev/mqueue/rootq");
    mode_of("root's /rootq-ro", "/dev/mqueue/rootq-ro");
    const struct { const char *n; int f; } rq[] = {
        {"/rootq", O_RDONLY}, {"/rootq", O_WRONLY}, {"/rootq-ro", O_RDONLY},
        {"/rootq-ro", O_WRONLY}, {"/rootq-ro", O_RDWR}};
    for (size_t i = 0; i < sizeof rq / sizeof rq[0]; i++) {
        errno = 0;
        q = mq_open(rq[i].n, rq[i].f);
        printf("root's %s %s: %s\n", rq[i].n,
               rq[i].f == O_RDONLY ? "O_RDONLY" : rq[i].f == O_WRONLY ? "O_WRONLY" : "O_RDWR",
               q == (mqd_t)-1 ? en(errno) : "ok");
        if (q != (mqd_t)-1)
            mq_close(q);
    }
    errno = 0;
    q = mq_open("/rootq", O_RDWR | O_CREAT, 0666, &a);
    printf("root's /rootq O_RDWR|O_CREAT: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    errno = 0;
    printf("mq_unlink root's /rootq-ro: %s\n", rc(mq_unlink("/rootq-ro"), errno));
    list_dir("/dev/mqueue", "/dev/mqueue");
}

static void s_mq_io(void)
{
    printf("== mq_send and mq_receive\n");
    mqd_t q = mk("/io", 8, 16, 0);
    const struct { const char *m; unsigned p; } snd[] = {
        {"a1", 1}, {"b5", 5}, {"c3", 3}, {"d5", 5}, {"e0", 0}, {"", 2}, {"f32767", 32767},
        {"g5", 5}};
    for (size_t i = 0; i < sizeof snd / sizeof snd[0]; i++) {
        errno = 0;
        int r = mq_send(q, snd[i].m, strlen(snd[i].m), snd[i].p);
        printf("send %s prio %u: %s\n", vis(snd[i].m), snd[i].p, rc(r, errno));
    }
    pattr("after 8 sends", q);
    struct timespec past = {0, 0};
    errno = 0;
    printf("mq_timedsend to the full queue, timeout in the past: %s\n",
           rc(mq_timedsend(q, "z", 1, 0, &past), errno));
    char buf[64];
    for (int i = 0; i < 8; i++) {
        unsigned p = 99;
        memset(buf, '#', sizeof buf);
        errno = 0;
        ssize_t r = mq_receive(q, buf, 16, &p);
        if (r < 0) {
            printf("receive: %s\n", en(errno));
            continue;
        }
        buf[r] = 0;
        printf("receive: %zd %s prio %u\n", r, vis(buf), p);
    }
    errno = 0;
    printf("mq_timedreceive from the empty queue, timeout in the past: %s\n",
           SZ(mq_timedreceive(q, buf, 16, NULL, &past)));
    /* Sizes. */
    char big[40];
    memset(big, 'x', sizeof big);
    errno = 0;
    printf("send 16 bytes (msgsize): %s\n", rc(mq_send(q, big, 16, 0), errno));
    errno = 0;
    printf("send 17 bytes: %s\n", rc(mq_send(q, big, 17, 0), errno));
    errno = 0;
    printf("receive with 15-byte buffer, a message waiting: %s\n",
           SZ(mq_receive(q, buf, 15, NULL)));
    errno = 0;
    printf("receive with 0-byte buffer: %s\n", SZ(mq_receive(q, buf, 0, NULL)));
    errno = 0;
    printf("receive with 40-byte buffer, NULL prio: %s\n", SZ(mq_receive(q, buf, 40, NULL)));
    errno = 0;
    printf("send prio MQ_PRIO_MAX: %s\n", rc(mq_send(q, "p", 1, MQ_PRIO_MAX), errno));
    errno = 0;
    printf("send prio UINT_MAX: %s\n", rc(mq_send(q, "p", 1, UINT_MAX), errno));
    errno = 0;
    printf("send 17 bytes with prio MQ_PRIO_MAX: %s\n",
           rc(mq_send(q, big, 17, MQ_PRIO_MAX), errno));
    pattr("after the size cases", q);
    mq_close(q);
    mq_unlink("/io");
    /* O_NONBLOCK. */
    q = mk("/nb", 2, 8, O_NONBLOCK);
    errno = 0;
    printf("nonblock receive on empty: %s\n", SZ(mq_receive(q, buf, 8, NULL)));
    mq_send(q, "1", 1, 0);
    mq_send(q, "2", 1, 0);
    errno = 0;
    printf("nonblock send on full: %s\n", rc(mq_send(q, "3", 1, 0), errno));
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += 5;
    errno = 0;
    printf("nonblock mq_timedsend on full (5 s timeout): %s\n",
           rc(mq_timedsend(q, "3", 1, 0, &ts), errno));
    mq_close(q);
    mq_unlink("/nb");
    /* Bad descriptors. */
    errno = 0;
    printf("mq_send on -1: %s\n", rc(mq_send((mqd_t)-1, "x", 1, 0), errno));
    errno = 0;
    printf("mq_receive on -1: %s\n", SZ(mq_receive((mqd_t)-1, buf, 8, NULL)));
    int p[2];
    pipe2(p, O_CLOEXEC);
    errno = 0;
    printf("mq_send on a pipe: %s\n", rc(mq_send((mqd_t)p[1], "x", 1, 0), errno));
    struct mq_attr a;
    errno = 0;
    printf("mq_getattr on a pipe: %s\n", rc(mq_getattr((mqd_t)p[0], &a), errno));
    struct sigevent sev = {.sigev_notify = SIGEV_NONE};
    errno = 0;
    printf("mq_notify on a pipe: %s\n", rc(mq_notify((mqd_t)p[0], &sev), errno));
    close(p[0]);
    close(p[1]);
    q = mk("/c", 1, 8, 0);
    errno = 0;
    printf("mq_close: %s\n", rc(mq_close(q), errno));
    errno = 0;
    printf("mq_close again: %s\n", rc(mq_close(q), errno));
    errno = 0;
    printf("mq_close(-1): %s\n", rc(mq_close((mqd_t)-1), errno));
    mq_unlink("/c");
}

static long ms_since(const struct timespec *a)
{
    struct timespec b;
    clock_gettime(CLOCK_MONOTONIC, &b);
    return (b.tv_sec - a->tv_sec) * 1000 + (b.tv_nsec - a->tv_nsec) / 1000000;
}

static void s_mq_timed(void)
{
    printf("== timed send and receive\n");
    mqd_t full = mk("/tfull", 1, 8, 0);
    mq_send(full, "f", 1, 0);
    mqd_t empty = mk("/tempty", 1, 8, 0);
    mqd_t room = mk("/troom", 4, 8, 0);
    char buf[16];
    const struct { const char *n; struct timespec t; } tt[] = {
        {"tv_nsec -1", {0, -1}},
        {"tv_nsec 1000000000", {0, 1000000000}},
        {"tv_sec -1 tv_nsec 0", {-1, 0}},
        {"tv_sec 0 tv_nsec 0", {0, 0}},
        {"tv_sec -1 tv_nsec -1", {-1, -1}},
    };
    for (size_t i = 0; i < sizeof tt / sizeof tt[0]; i++) {
        errno = 0;
        int r = mq_timedsend(full, "x", 1, 0, &tt[i].t);
        printf("timedsend full, %s: %s\n", tt[i].n, rc(r, errno));
        errno = 0;
        r = mq_timedsend(room, "x", 1, 0, &tt[i].t);
        printf("timedsend with room, %s: %s\n", tt[i].n, rc(r, errno));
        errno = 0;
        ssize_t s = mq_timedreceive(empty, buf, 8, NULL, &tt[i].t);
        printf("timedreceive empty, %s: %zd %s\n", tt[i].n, s, en(s < 0 ? errno : 0));
        errno = 0;
        s = mq_timedreceive(room, buf, 8, NULL, &tt[i].t);
        printf("timedreceive with a message, %s: %zd %s\n", tt[i].n, s, en(s < 0 ? errno : 0));
    }
    errno = 0;
    printf("timedsend with room, NULL timeout: %s\n",
           rc(mq_timedsend(room, "n", 1, 0, NULL), errno));
    errno = 0;
    ssize_t s = mq_timedreceive(room, buf, 8, NULL, NULL);
    printf("timedreceive with a message, NULL timeout: %zd %s\n", s, en(s < 0 ? errno : 0));
    struct timespec start, ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_nsec += 150000000;
    if (ts.tv_nsec >= 1000000000) {
        ts.tv_sec++;
        ts.tv_nsec -= 1000000000;
    }
    clock_gettime(CLOCK_MONOTONIC, &start);
    errno = 0;
    int r = mq_timedsend(full, "x", 1, 0, &ts);
    long ms = ms_since(&start);
    printf("timedsend full, now+150 ms: %s, waited at least 100 ms: %s\n", rc(r, errno),
           ms >= 100 ? "yes" : "no");
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_nsec += 150000000;
    if (ts.tv_nsec >= 1000000000) {
        ts.tv_sec++;
        ts.tv_nsec -= 1000000000;
    }
    clock_gettime(CLOCK_MONOTONIC, &start);
    errno = 0;
    s = mq_timedreceive(empty, buf, 8, NULL, &ts);
    ms = ms_since(&start);
    printf("timedreceive empty, now+150 ms: %zd %s, waited at least 100 ms: %s\n", s, en(errno),
           ms >= 100 ? "yes" : "no");
    mq_close(full);
    mq_close(empty);
    mq_close(room);
    mq_unlink("/tfull");
    mq_unlink("/tempty");
    mq_unlink("/troom");
}

static mqd_t setattr_q;
static void setattr_null_child(void)
{
    errno = 0;
    int r = mq_setattr(setattr_q, NULL, NULL);
    printf("mq_setattr(q, NULL, NULL): %s\n", rc(r, errno));
}
static void getattr_null_child(void)
{
    errno = 0;
    int r = mq_getattr(setattr_q, NULL);
    printf("mq_getattr(q, NULL): %s\n", rc(r, errno));
}

static void s_mq_setattr(void)
{
    printf("== mq_getattr and mq_setattr\n");
    mqd_t q = mk("/sa", 3, 8, 0);
    mq_send(q, "x", 1, 0);
    struct mq_attr n = {.mq_flags = O_NONBLOCK, .mq_maxmsg = 99, .mq_msgsize = 99,
                        .mq_curmsgs = 99};
    struct mq_attr o;
    memset(&o, 0x5a, sizeof o);
    errno = 0;
    int r = mq_setattr(q, &n, &o);
    printf("setattr O_NONBLOCK (other fields 99): %s; old flags %#lx maxmsg %ld msgsize %ld "
           "curmsgs %ld\n",
           rc(r, errno), (unsigned long)o.mq_flags, (long)o.mq_maxmsg, (long)o.mq_msgsize,
           (long)o.mq_curmsgs);
    pattr("after", q);
    fd_flags("descriptor after setattr O_NONBLOCK", q);
    char buf[16];
    mq_receive(q, buf, 8, NULL);
    errno = 0;
    printf("receive on empty after setattr O_NONBLOCK: %s\n", SZ(mq_receive(q, buf, 8, NULL)));
    const struct { const char *nm; long f; } fl[] = {
        {"O_NONBLOCK|O_APPEND", O_NONBLOCK | O_APPEND}, {"O_RDWR", O_RDWR},
        {"O_CREAT", O_CREAT}, {"-1", -1}, {"0", 0}};
    for (size_t i = 0; i < sizeof fl / sizeof fl[0]; i++) {
        n.mq_flags = fl[i].f;
        errno = 0;
        r = mq_setattr(q, &n, NULL);
        printf("setattr %s: %s\n", fl[i].nm, rc(r, errno));
        pattr("  now", q);
    }
    mqd_t q2 = mq_open("/sa", O_RDWR);
    n.mq_flags = O_NONBLOCK;
    mq_setattr(q, &n, NULL);
    pattr("setattr on one descriptor: that descriptor", q);
    pattr("setattr on one descriptor: another descriptor of the queue", q2);
    mqd_t q3 = (mqd_t)dup((int)q);
    pattr("setattr on one descriptor: a dup of it", q3);
    close((int)q3);
    mq_close(q2);
    errno = 0;
    printf("setattr on -1: %s\n", rc(mq_setattr((mqd_t)-1, &n, &o), errno));
    setattr_q = q;
    run_child("setattr NULL", setattr_null_child, 5);
    run_child("getattr NULL", getattr_null_child, 5);
    mq_close(q);
    mq_unlink("/sa");
}

/* ---------------------------------------------------------------- mq_notify */

static sigset_t usr1;

static void wait_sig(const char *label, int ms)
{
    struct timespec t = {ms / 1000, (ms % 1000) * 1000000L};
    siginfo_t si;
    memset(&si, 0, sizeof si);
    int s = sigtimedwait(&usr1, &si, &t);
    if (s < 0) {
        printf("%s: no signal (%s)\n", label, en(errno));
        return;
    }
    printf("%s: %s si_code %s si_value %d si_pid %s si_uid %s\n", label, sig_name(s),
           si.si_code == SI_MESGQ ? "SI_MESGQ" : si.si_code == SI_USER ? "SI_USER" : "other",
           si.si_value.sival_int, si.si_pid == getpid() ? "self" : "other",
           si.si_uid == getuid() ? "self" : "other");
}

static mqd_t nq;
static void notify_other_child(void)
{
    struct sigevent sev = {.sigev_notify = SIGEV_SIGNAL, .sigev_signo = SIGUSR2};
    errno = 0;
    printf("child registers too: %s\n", rc(mq_notify(nq, &sev), errno));
    errno = 0;
    printf("child mq_notify(NULL): %s\n", rc(mq_notify(nq, NULL), errno));
}

static void notify_sender_child(void)
{
    mq_send(nq, "c", 1, 0);
}

static void s_mq_notify_signal(void)
{
    printf("== mq_notify with SIGEV_SIGNAL and SIGEV_NONE\n");
    sigemptyset(&usr1);
    sigaddset(&usr1, SIGUSR1);
    sigprocmask(SIG_BLOCK, &usr1, NULL);
    nq = mk("/n", 4, 8, O_NONBLOCK);
    char buf[16];
    struct sigevent sev = {.sigev_notify = SIGEV_SIGNAL, .sigev_signo = SIGUSR1,
                           .sigev_value.sival_int = 42};
    errno = 0;
    printf("register: %s\n", rc(mq_notify(nq, &sev), errno));
    errno = 0;
    printf("register again (same process): %s\n", rc(mq_notify(nq, &sev), errno));
    run_child("another process", notify_other_child, 5);
    mq_send(nq, "m", 1, 0);
    wait_sig("message on the empty queue", 2000);
    mq_receive(nq, buf, 8, NULL);
    mq_send(nq, "m", 1, 0);
    wait_sig("second message (one-shot)", 200);
    errno = 0;
    printf("register after the notification: %s\n", rc(mq_notify(nq, &sev), errno));
    mq_send(nq, "m", 1, 0);
    wait_sig("message on a non-empty queue", 200);
    while (mq_receive(nq, buf, 8, NULL) >= 0)
        ;
    run_child("sender", notify_sender_child, 5);
    wait_sig("message from another process after the queue emptied", 2000);
    mq_receive(nq, buf, 8, NULL);
    errno = 0;
    printf("register: %s\n", rc(mq_notify(nq, &sev), errno));
    errno = 0;
    printf("mq_notify(NULL): %s\n", rc(mq_notify(nq, NULL), errno));
    mq_send(nq, "m", 1, 0);
    wait_sig("message after removal", 200);
    mq_receive(nq, buf, 8, NULL);
    errno = 0;
    printf("mq_notify(NULL) with nothing registered: %s\n", rc(mq_notify(nq, NULL), errno));
    /* Registration and the descriptor that made it. */
    mqd_t other = mq_open("/n", O_RDWR | O_NONBLOCK);
    errno = 0;
    printf("register through a second descriptor: %s\n", rc(mq_notify(other, &sev), errno));
    errno = 0;
    printf("register through the first descriptor: %s\n", rc(mq_notify(nq, &sev), errno));
    errno = 0;
    printf("mq_notify(NULL) through the first descriptor: %s\n", rc(mq_notify(nq, NULL), errno));
    errno = 0;
    printf("register through the first descriptor now: %s\n", rc(mq_notify(nq, &sev), errno));
    mq_close(other);
    mq_send(nq, "m", 1, 0);
    wait_sig("registered through the first descriptor, second descriptor closed, message", 200);
    mq_receive(nq, buf, 8, NULL);
    errno = 0;
    printf("register after that close: %s\n", rc(mq_notify(nq, &sev), errno));
    mq_notify(nq, NULL);
    other = mq_open("/n", O_RDWR | O_NONBLOCK);
    mq_notify(other, &sev);
    mq_close(other);
    errno = 0;
    printf("register after the registering descriptor was closed: %s\n",
           rc(mq_notify(nq, &sev), errno));
    mq_notify(nq, NULL);
    /* SIGEV_NONE. */
    struct sigevent none = {.sigev_notify = SIGEV_NONE};
    errno = 0;
    printf("SIGEV_NONE register: %s\n", rc(mq_notify(nq, &none), errno));
    run_child("another process while SIGEV_NONE is registered", notify_other_child, 5);
    mq_send(nq, "m", 1, 0);
    wait_sig("message with SIGEV_NONE", 200);
    mq_receive(nq, buf, 8, NULL);
    errno = 0;
    printf("register SIGEV_SIGNAL after a SIGEV_NONE arrival: %s\n",
           rc(mq_notify(nq, &sev), errno));
    mq_notify(nq, NULL);
    /* Invalid requests. */
    struct { const char *n; int notify, signo; } bad[] = {
        {"sigev_notify 99", 99, SIGUSR1}, {"SIGEV_SIGNAL signo 0", SIGEV_SIGNAL, 0},
        {"SIGEV_SIGNAL signo 65", SIGEV_SIGNAL, 65}, {"SIGEV_SIGNAL signo -1", SIGEV_SIGNAL, -1},
        {"SIGEV_THREAD_ID", SIGEV_THREAD_ID, SIGUSR1}};
    for (size_t i = 0; i < sizeof bad / sizeof bad[0]; i++) {
        struct sigevent b = {.sigev_notify = bad[i].notify, .sigev_signo = bad[i].signo};
        errno = 0;
        int r = mq_notify(nq, &b);
        printf("%s: %s\n", bad[i].n, rc(r, errno));
        mq_notify(nq, NULL);
    }
    errno = 0;
    printf("mq_notify on -1: %s\n", rc(mq_notify((mqd_t)-1, &sev), errno));
    mq_close(nq);
    mq_unlink("/n");
    sigprocmask(SIG_UNBLOCK, &usr1, NULL);
}

static sem_t tsem;
static pthread_t main_thread;
static int token, calls, rereg;
static size_t want_stack;
static mqd_t tq;
static char tinfo[512];

static void thread_fn(union sigval v)
{
    calls++;
    pthread_attr_t a;
    int det = -1;
    size_t st = 0;
    if (pthread_getattr_np(pthread_self(), &a) == 0) {
        pthread_attr_getdetachstate(&a, &det);
        pthread_attr_getstacksize(&a, &st);
        pthread_attr_destroy(&a);
    }
    sigset_t m;
    pthread_sigmask(SIG_BLOCK, NULL, &m);
    int nblocked = 0;
    for (int s = 1; s < 32; s++)
        nblocked += sigismember(&m, s) == 1;
    snprintf(tinfo, sizeof tinfo,
             "arg %s, thread %s, detached %s, stack %s, SIGUSR2 blocked %s, SIGINT blocked %s, "
             "signals 1-31 blocked %d",
             v.sival_ptr == &token ? "&token" : "other",
             pthread_equal(pthread_self(), main_thread) ? "main" : "new",
             det == PTHREAD_CREATE_DETACHED ? "yes" : det == PTHREAD_CREATE_JOINABLE ? "no" : "?",
             want_stack == 0 ? "-" : st == want_stack ? "as requested" : st > want_stack
                                                                             ? "larger"
                                                                             : "smaller",
             sigismember(&m, SIGUSR2) == 1 ? "yes" : "no", sigismember(&m, SIGINT) == 1 ? "yes" : "no",
             nblocked);
    if (rereg) {
        struct sigevent sev = {.sigev_notify = SIGEV_THREAD, .sigev_notify_function = thread_fn,
                               .sigev_value.sival_ptr = &token};
        errno = 0;
        int r = mq_notify(tq, &sev);
        size_t l = strlen(tinfo);
        snprintf(tinfo + l, sizeof tinfo - l, ", re-register inside: %s", rc(r, errno));
        rereg = 0;
    }
    sem_post(&tsem);
}

static int wait_thread(const char *label, int ms)
{
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    t.tv_sec += ms / 1000;
    t.tv_nsec += (ms % 1000) * 1000000L;
    if (t.tv_nsec >= 1000000000) {
        t.tv_sec++;
        t.tv_nsec -= 1000000000;
    }
    while (sem_timedwait(&tsem, &t) != 0) {
        if (errno != EINTR) {
            printf("%s: no call\n", label);
            return 0;
        }
    }
    printf("%s: called; %s\n", label, tinfo);
    return 1;
}

static void s_mq_notify_thread(void)
{
    printf("== mq_notify with SIGEV_THREAD\n");
    main_thread = pthread_self();
    sem_init(&tsem, 0, 0);
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &m, NULL);
    tq = mk("/t", 4, 8, O_NONBLOCK);
    nq = tq;
    char buf[16];
    struct sigevent sev = {.sigev_notify = SIGEV_THREAD, .sigev_notify_function = thread_fn,
                           .sigev_value.sival_ptr = &token};
    errno = 0;
    printf("register (attributes NULL): %s\n", rc(mq_notify(tq, &sev), errno));
    errno = 0;
    printf("register again: %s\n", rc(mq_notify(tq, &sev), errno));
    run_child("another process", notify_other_child, 5);
    mq_send(tq, "m", 1, 0);
    wait_thread("message on the empty queue", 5000);
    mq_receive(tq, buf, 8, NULL);
    mq_send(tq, "m", 1, 0);
    wait_thread("second message (one-shot)", 300);
    mq_receive(tq, buf, 8, NULL);
    /* Attributes: joinable (the default) with a chosen stack size. */
    pthread_attr_t a;
    pthread_attr_init(&a);
    want_stack = 256 * 1024;
    pthread_attr_setstacksize(&a, want_stack);
    pthread_attr_setdetachstate(&a, PTHREAD_CREATE_JOINABLE);
    sev.sigev_notify_attributes = &a;
    errno = 0;
    printf("register (joinable, stack 256 KiB): %s\n", rc(mq_notify(tq, &sev), errno));
    pthread_attr_destroy(&a);
    mq_send(tq, "m", 1, 0);
    wait_thread("with attributes (destroyed after mq_notify)", 5000);
    mq_receive(tq, buf, 8, NULL);
    want_stack = 0;
    sev.sigev_notify_attributes = NULL;
    /* Re-registration from the notification thread. */
    rereg = 1;
    mq_notify(tq, &sev);
    mq_send(tq, "m", 1, 0);
    wait_thread("re-registering", 5000);
    mq_receive(tq, buf, 8, NULL);
    mq_send(tq, "m", 1, 0);
    wait_thread("after re-registration", 5000);
    mq_receive(tq, buf, 8, NULL);
    /* Removal. */
    errno = 0;
    printf("register: %s\n", rc(mq_notify(tq, &sev), errno));
    errno = 0;
    printf("mq_notify(NULL): %s\n", rc(mq_notify(tq, NULL), errno));
    mq_send(tq, "m", 1, 0);
    wait_thread("after removal", 300);
    mq_receive(tq, buf, 8, NULL);
    /* Message from another process. */
    mq_notify(tq, &sev);
    run_child("sender", notify_sender_child, 5);
    wait_thread("message from another process", 5000);
    mq_receive(tq, buf, 8, NULL);
    /* Two queues registered at once. */
    mqd_t t2 = mk("/t2", 2, 8, O_NONBLOCK);
    mq_notify(tq, &sev);
    errno = 0;
    printf("second queue register: %s\n", rc(mq_notify(t2, &sev), errno));
    mq_send(t2, "m", 1, 0);
    wait_thread("second queue", 5000);
    mq_send(tq, "m", 1, 0);
    wait_thread("first queue", 5000);
    printf("calls so far: %d\n", calls);
    mq_close(t2);
    mq_unlink("/t2");
    mq_close(tq);
    mq_unlink("/t");
    pthread_sigmask(SIG_UNBLOCK, &m, NULL);
}

static void s_mq_unlink(void)
{
    printf("== mq_unlink\n");
    errno = 0;
    printf("missing: %s\n", rc(mq_unlink("/nope"), errno));
    errno = 0;
    printf("missing, no slash: %s\n", rc(mq_unlink("nope"), errno));
    static char n256[260];
    n256[0] = '/';
    memset(n256 + 1, 'n', 256);
    errno = 0;
    printf("name of 256 bytes after the slash: %s\n", rc(mq_unlink(n256), errno));
    mqd_t q = mk("/u", 2, 8, 0);
    mq_send(q, "kept", 4, 0);
    errno = 0;
    printf("unlink while open: %s\n", rc(mq_unlink("/u"), errno));
    list_dir("  /dev/mqueue", "/dev/mqueue");
    char buf[16];
    ssize_t r = mq_receive(q, buf, 8, NULL);
    printf("  receive through the open descriptor: %zd\n", r);
    errno = 0;
    mqd_t q2 = mq_open("/u", O_RDWR);
    printf("  reopen: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    q2 = mk("/u", 2, 8, 0);
    printf("  a new queue with the name: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    const char *sent = rc(mq_send(q, "x", 1, 0), errno);
    struct mq_attr na;
    mq_getattr(q2, &na);
    printf("  send to the old descriptor: %s; messages in the new queue: %ld\n", sent,
           (long)na.mq_curmsgs);
    mq_close(q);
    mq_close(q2);
    errno = 0;
    printf("unlink twice: %s", rc(mq_unlink("/u"), errno));
    errno = 0;
    printf(" %s\n", rc(mq_unlink("/u"), errno));
}

static void mq_open_2_creat_child(void)
{
    errno = 0;
    mqd_t q = __mq_open_2("/o2c", O_RDWR | O_CREAT);
    printf("__mq_open_2 with O_CREAT returned: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
}

static void s_mq_open_2(void)
{
    printf("== __mq_open_2\n");
    mqd_t q = mk("/o2", 2, 8, 0);
    errno = 0;
    mqd_t q2 = __mq_open_2("/o2", O_RDWR);
    printf("existing queue: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    if (q2 != (mqd_t)-1) {
        pattr("existing queue", q2);
        mq_close(q2);
    }
    errno = 0;
    q2 = __mq_open_2("/missing", O_RDWR);
    printf("missing queue: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    errno = 0;
    q2 = __mq_open_2("noslash", O_RDWR);
    printf("no slash: %s\n", q2 == (mqd_t)-1 ? en(errno) : "ok");
    run_child("O_CREAT", mq_open_2_creat_child, 5);
    list_dir("/dev/mqueue", "/dev/mqueue");
    mq_unlink("/o2c");
    mq_close(q);
    mq_unlink("/o2");
}

static void rlimit_child(void)
{
    struct rlimit rl = {4096, 4096};
    setrlimit(RLIMIT_MSGQUEUE, &rl);
    struct mq_attr a = {.mq_maxmsg = 10, .mq_msgsize = 8192};
    errno = 0;
    mqd_t q = mq_open("/rl", O_RDWR | O_CREAT | O_EXCL, 0600, &a);
    printf("RLIMIT_MSGQUEUE 4096, queue of 10 x 8192: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    struct mq_attr s = {.mq_maxmsg = 1, .mq_msgsize = 8};
    errno = 0;
    q = mq_open("/rl", O_RDWR | O_CREAT | O_EXCL, 0600, &s);
    printf("RLIMIT_MSGQUEUE 4096, queue of 1 x 8: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    mq_unlink("/rl");
}

static void s_mq_limits(void)
{
    printf("== limits\n");
    run_child("rlimit", rlimit_child, 5);
    int n = 0, e = 0;
    char name[32];
    for (n = 0; n < 400; n++) {
        snprintf(name, sizeof name, "/lim%d", n);
        struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 1};
        mqd_t q = mq_open(name, O_RDWR | O_CREAT | O_EXCL, 0600, &a);
        if (q == (mqd_t)-1) {
            e = errno;
            break;
        }
        mq_close(q);
    }
    printf("queues created before the first failure: %d (%s)\n", n, n < 400 ? en(e) : "none");
    for (int i = 0; i < n; i++) {
        snprintf(name, sizeof name, "/lim%d", i);
        mq_unlink(name);
    }
    list_dir("/dev/mqueue", "/dev/mqueue");
}

/* ---------------------------------------------------------------- shared memory */

static void s_shm_names(void)
{
    printf("== shm_open names\n");
    static char n255[260], n256[260], bare255[260];
    n255[0] = n256[0] = '/';
    memset(n255 + 1, 's', 255);
    memset(n256 + 1, 's', 256);
    memset(bare255, 'b', 255);
    const char *names[] = {"/s", "s", "//s", "/a/b", "/", "", "/.", "/..", ".", "..", "/s/",
                           "/sp ace", "/nl\n", "/\xc3\xa9", n255, n256, bare255, "///", "s/"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        errno = 0;
        int fd = shm_open(names[i], O_RDWR | O_CREAT | O_EXCL, 0600);
        int e = errno;
        printf("name %s: shm_open %s\n", vis(names[i]), fd < 0 ? en(e) : "ok");
        list_dir("  /dev/shm", "/dev/shm");
        if (fd >= 0) {
            errno = 0;
            int fd2 = shm_open(names[i], O_RDWR, 0);
            printf("  reopen without O_CREAT: %s\n", fd2 < 0 ? en(errno) : "ok");
            if (fd2 >= 0)
                close(fd2);
            close(fd);
        }
        errno = 0;
        int r = shm_unlink(names[i]);
        printf("  shm_unlink: %s\n", rc(r, errno));
    }
    list_dir("after the names: /dev/shm", "/dev/shm");
}

static void fifo_child(void)
{
    errno = 0;
    int fd = shm_open("/fifo", O_RDONLY, 0);
    printf("shm_open of a FIFO, O_RDONLY: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        fd_flags("  FIFO descriptor", fd);
}

static void s_shm_flags(void)
{
    printf("== shm_open flags and modes\n");
    umask(022);
    shm_unlink("/f");
    int fd = shm_open("/f", O_RDWR | O_CREAT | O_EXCL, 0666);
    fd_flags("O_RDWR|O_CREAT|O_EXCL descriptor", fd);
    mode_of("mode 0666, umask 022", "/dev/shm/f");
    errno = 0;
    printf("ftruncate 4096: %s\n", rc(ftruncate(fd, 4096), errno));
    close(fd);
    errno = 0;
    fd = shm_open("/f", O_RDWR | O_CREAT | O_EXCL, 0666);
    printf("O_CREAT|O_EXCL on an existing object: %s\n", fd < 0 ? en(errno) : "ok");
    fd = shm_open("/f", O_RDONLY, 0);
    fd_flags("O_RDONLY descriptor", fd);
    errno = 0;
    printf("ftruncate on O_RDONLY: %s\n", rc(ftruncate(fd, 8192), errno));
    close(fd);
    errno = 0;
    fd = shm_open("/f", O_WRONLY, 0);
    printf("O_WRONLY: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0) {
        fd_flags("O_WRONLY descriptor", fd);
        close(fd);
    }
    errno = 0;
    fd = shm_open("/f", O_ACCMODE, 0);
    printf("access mode 3: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    fd = shm_open("/f", O_RDWR | O_APPEND | O_NONBLOCK, 0);
    printf("O_RDWR|O_APPEND|O_NONBLOCK: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0) {
        fd_flags("O_RDWR|O_APPEND|O_NONBLOCK descriptor", fd);
        close(fd);
    }
    errno = 0;
    fd = shm_open("/f", O_RDWR | O_DIRECTORY, 0);
    printf("O_RDWR|O_DIRECTORY on a file: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    errno = 0;
    fd = shm_open("/f", O_RDWR | O_EXCL, 0);
    printf("O_EXCL without O_CREAT: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    mode_of("before O_TRUNC", "/dev/shm/f");
    fd = shm_open("/f", O_RDWR | O_TRUNC, 0);
    printf("O_RDWR|O_TRUNC: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    mode_of("after O_TRUNC", "/dev/shm/f");
    fd = shm_open("/f", O_RDWR, 0);
    ftruncate(fd, 4096);
    close(fd);
    errno = 0;
    fd = shm_open("/f", O_RDONLY | O_TRUNC, 0);
    printf("O_RDONLY|O_TRUNC on a 4096-byte object: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    mode_of("after O_RDONLY|O_TRUNC", "/dev/shm/f");
    errno = 0;
    fd = shm_open("/f", O_ACCMODE, 0);
    printf("access mode 3 again: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0) {
        fd_flags("access mode 3 descriptor", fd);
        close(fd);
    }
    errno = 0;
    fd = shm_open("/f", O_RDWR | O_CREAT, 0600);
    printf("O_CREAT on an existing object (mode ignored): %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    mode_of("  still", "/dev/shm/f");
    shm_unlink("/f");
    errno = 0;
    fd = shm_open("/missing", O_RDWR, 0);
    printf("missing object without O_CREAT: %s\n", fd < 0 ? en(errno) : "ok");
    errno = 0;
    fd = shm_open("/rc", O_RDONLY | O_CREAT | O_EXCL, 0600);
    printf("O_RDONLY|O_CREAT: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0) {
        errno = 0;
        printf("  ftruncate: %s\n", rc(ftruncate(fd, 4096), errno));
        close(fd);
    }
    shm_unlink("/rc");
    struct { int mode; const char *n; } m[] = {{0600, "0600"}, {0400, "0400"}, {0, "0"},
                                               {07777, "07777"}, {01777, "01777"}};
    for (size_t i = 0; i < sizeof m / sizeof m[0]; i++) {
        shm_unlink("/m");
        fd = shm_open("/m", O_RDWR | O_CREAT | O_EXCL, m[i].mode);
        char l[64];
        snprintf(l, sizeof l, "mode %s, umask 022", m[i].n);
        if (fd < 0) {
            printf("%s: create %s\n", l, en(errno));
            continue;
        }
        close(fd);
        mode_of(l, "/dev/shm/m");
        errno = 0;
        fd = shm_open("/m", O_RDONLY, 0);
        printf("%s: reopen O_RDONLY %s", l, fd < 0 ? en(errno) : "ok");
        if (fd >= 0)
            close(fd);
        errno = 0;
        fd = shm_open("/m", O_RDWR, 0);
        printf(", O_RDWR %s\n", fd < 0 ? en(errno) : "ok");
        if (fd >= 0)
            close(fd);
        shm_unlink("/m");
    }
    umask(0);
    fd = shm_open("/m", O_RDWR | O_CREAT | O_EXCL, 0666);
    close(fd);
    mode_of("mode 0666, umask 0", "/dev/shm/m");
    shm_unlink("/m");
    umask(022);
    /* Entries made by root in the setup phase. */
    mode_of("root's rootshm", "/dev/shm/rootshm");
    mode_of("root's rootshm-ro", "/dev/shm/rootshm-ro");
    const struct { const char *n; int f; } rq[] = {
        {"/rootshm", O_RDONLY}, {"/rootshm-ro", O_RDONLY}, {"/rootshm-ro", O_RDWR}};
    for (size_t i = 0; i < sizeof rq / sizeof rq[0]; i++) {
        errno = 0;
        fd = shm_open(rq[i].n, rq[i].f, 0);
        printf("root's %s %s: %s\n", rq[i].n, rq[i].f == O_RDONLY ? "O_RDONLY" : "O_RDWR",
               fd < 0 ? en(errno) : "ok");
        if (fd >= 0)
            close(fd);
    }
    errno = 0;
    fd = shm_open("/rootshm", O_RDWR | O_CREAT, 0666);
    printf("root's /rootshm O_RDWR|O_CREAT: %s\n", fd < 0 ? en(errno) : "ok");
    errno = 0;
    printf("shm_unlink root's /rootshm-ro: %s\n", rc(shm_unlink("/rootshm-ro"), errno));
    /* Other kinds of entries in /dev/shm. */
    errno = 0;
    fd = shm_open("/link", O_RDONLY, 0);
    printf("symbolic link to rootshm-ro, O_RDONLY: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    errno = 0;
    fd = shm_open("/dir", O_RDONLY, 0);
    printf("directory, O_RDONLY: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    errno = 0;
    fd = shm_open("/dir", O_RDWR, 0);
    printf("directory, O_RDWR: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    errno = 0;
    fd = shm_open("/dir/inner", O_RDWR | O_CREAT, 0600);
    printf("inside the directory: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0)
        close(fd);
    errno = 0;
    printf("shm_unlink of the directory: %s\n", rc(shm_unlink("/dir"), errno));
    errno = 0;
    printf("shm_unlink of the link: %s\n", rc(shm_unlink("/link"), errno));
    run_child("FIFO", fifo_child, 3);
    list_dir("/dev/shm", "/dev/shm");
}

static void s_shm_roundtrip(void)
{
    printf("== ftruncate, mmap and shm_unlink\n");
    shm_unlink("/rt");
    int fd = shm_open("/rt", O_RDWR | O_CREAT | O_EXCL, 0600);
    struct stat st;
    fstat(fd, &st);
    printf("new object size %lld\n", (long long)st.st_size);
    ftruncate(fd, 8192);
    fstat(fd, &st);
    printf("after ftruncate size %lld\n", (long long)st.st_size);
    unsigned char *p = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    printf("mmap: %s\n", p == MAP_FAILED ? en(errno) : "ok");
    if (p == MAP_FAILED)
        return;
    int zero = 1;
    for (int i = 0; i < 8192; i++)
        zero &= p[i] == 0;
    printf("new pages zero: %s\n", zero ? "yes" : "no");
    for (int i = 0; i < 8192; i++)
        p[i] = (unsigned char)(i * 7 + 3);
    close(fd);
    int fd2 = shm_open("/rt", O_RDONLY, 0);
    unsigned char *q = mmap(NULL, 8192, PROT_READ, MAP_SHARED, fd2, 0);
    printf("second mapping (O_RDONLY, PROT_READ) equal: %s\n",
           q != MAP_FAILED && memcmp(p, q, 8192) == 0 ? "yes" : "no");
    errno = 0;
    void *w = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd2, 0);
    printf("PROT_WRITE MAP_SHARED on O_RDONLY: %s\n", w == MAP_FAILED ? en(errno) : "ok");
    unsigned char buf[16];
    ssize_t r = pread(fd2, buf, 16, 100);
    printf("pread at 100: %zd, equal: %s\n", r, r == 16 && memcmp(buf, p + 100, 16) == 0 ? "yes" : "no");
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        int f = shm_open("/rt", O_RDWR, 0);
        unsigned char *x = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, f, 0);
        int ok = x != MAP_FAILED && x[5000] == (unsigned char)(5000 * 7 + 3);
        if (x != MAP_FAILED)
            x[0] = 0xEE;
        _exit(ok ? 0 : 1);
    }
    int sts;
    waitpid(c, &sts, 0);
    printf("child reads the data: %s, child's write visible: %s\n",
           WIFEXITED(sts) && WEXITSTATUS(sts) == 0 ? "yes" : "no", p[0] == 0xEE ? "yes" : "no");
    errno = 0;
    printf("shm_unlink: %s\n", rc(shm_unlink("/rt"), errno));
    p[1] = 0xAB;
    printf("mapping still usable after unlink: %s\n", q != MAP_FAILED && q[1] == 0xAB ? "yes" : "no");
    errno = 0;
    fd = shm_open("/rt", O_RDWR, 0);
    printf("reopen after unlink: %s\n", fd < 0 ? en(errno) : "ok");
    errno = 0;
    printf("shm_unlink again: %s\n", rc(shm_unlink("/rt"), errno));
    errno = 0;
    printf("shm_unlink missing, no slash: %s\n", rc(shm_unlink("nope"), errno));
    errno = 0;
    printf("shm_unlink \"\": %s\n", rc(shm_unlink(""), errno));
    errno = 0;
    printf("shm_unlink \"/\": %s\n", rc(shm_unlink("/"), errno));
    errno = 0;
    printf("shm_unlink \"/a/b\": %s\n", rc(shm_unlink("/a/b"), errno));
    munmap(p, 8192);
    if (q != MAP_FAILED)
        munmap(q, 8192);
    close(fd2);
}

/* ---------------------------------------------------------------- phases */

static int in_fixture(void)
{
    struct stat st;
    return stat(SHM_MARKER, &st) == 0;
}

static void phase_setup(void)
{
    umask(0);
    struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 8};
    mqd_t q = mq_open("/rootq", O_RDWR | O_CREAT | O_EXCL, 0600, &a);
    mqd_t q2 = mq_open("/rootq-ro", O_RDWR | O_CREAT | O_EXCL, 0644, &a);
    int f1 = shm_open("/rootshm", O_RDWR | O_CREAT | O_EXCL, 0600);
    int f2 = shm_open("/rootshm-ro", O_RDWR | O_CREAT | O_EXCL, 0644);
    int ok = q != (mqd_t)-1 && q2 != (mqd_t)-1 && f1 >= 0 && f2 >= 0
             && symlink("rootshm-ro", "/dev/shm/link") == 0 && mkdir("/dev/shm/dir", 0755) == 0
             && mkfifo("/dev/shm/fifo", 0666) == 0;
    printf("setup (root): %s\n", ok ? "done" : "FAILED");
}

static void phase_user(void)
{
    printf("running as root: %s\n", geteuid() == 0 ? "yes" : "no");
    fs_of("/dev/shm", "/dev/shm");
    fs_of("/dev/mqueue", "/dev/mqueue");
    mode_of("/dev/shm directory", "/dev/shm");
    list_dir("/dev/mqueue at start", "/dev/mqueue");
    list_dir("/dev/shm at start", "/dev/shm");
    s_mq_names();
    s_mq_attrs();
    s_mq_flags();
    s_mq_io();
    s_mq_timed();
    s_mq_setattr();
    s_mq_notify_signal();
    s_mq_notify_thread();
    s_mq_unlink();
    s_mq_open_2();
    s_mq_limits();
    s_shm_names();
    s_shm_flags();
    s_shm_roundtrip();
    list_dir("/dev/mqueue at the end", "/dev/mqueue");
    list_dir("/dev/shm at the end", "/dev/shm");
}

static void phase_ext4(void)
{
    printf("== /dev/shm on another file system\n");
    fs_of("/dev/shm", "/dev/shm");
    errno = 0;
    int fd = shm_open("/e", O_RDWR | O_CREAT | O_EXCL, 0600);
    printf("shm_open create: %s\n", fd < 0 ? en(errno) : "ok");
    if (fd >= 0) {
        errno = 0;
        printf("ftruncate: %s\n", rc(ftruncate(fd, 4096), errno));
        void *p = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        printf("mmap: %s\n", p == MAP_FAILED ? en(errno) : "ok");
        close(fd);
    }
    list_dir("/dev/shm", "/dev/shm");
    errno = 0;
    printf("shm_unlink: %s\n", rc(shm_unlink("/e"), errno));
}

static void phase_nodev(void)
{
    printf("== no /dev/shm\n");
    struct stat st;
    printf("/dev/shm exists: %s\n", stat("/dev/shm", &st) == 0 ? "yes" : "no");
    errno = 0;
    int fd = shm_open("/x", O_RDWR | O_CREAT | O_EXCL, 0600);
    printf("shm_open create: %s\n", fd < 0 ? en(errno) : "ok");
    errno = 0;
    fd = shm_open("/x", O_RDONLY, 0);
    printf("shm_open existing: %s\n", fd < 0 ? en(errno) : "ok");
    errno = 0;
    printf("shm_unlink: %s\n", rc(shm_unlink("/x"), errno));
    /* Message queues do not depend on /dev. */
    struct mq_attr a = {.mq_maxmsg = 1, .mq_msgsize = 8};
    errno = 0;
    mqd_t q = mq_open("/nodevq", O_RDWR | O_CREAT | O_EXCL, 0600, &a);
    printf("mq_open without /dev/mqueue: %s\n", q == (mqd_t)-1 ? en(errno) : "ok");
    errno = 0;
    printf("mq_unlink: %s\n", rc(mq_unlink("/nodevq"), errno));
}

int main(int argc, char **argv)
{
    tw_alarm(120);
    signal(SIGPIPE, SIG_IGN);
    setvbuf(stdout, NULL, _IOLBF, 0);
    const char *phase = argc > 1 ? argv[1] : "";
    struct stat st;
    if (!strcmp(phase, "nodev")) {
        if (stat(NODEV_MARKER, &st) != 0) {
            printf("refusing to run: %s is missing (run through posixipc_run.sh)\n", NODEV_MARKER);
            return 2;
        }
        phase_nodev();
    } else if (!in_fixture()) {
        printf("refusing to run: %s is missing (run through posixipc_run.sh)\n", SHM_MARKER);
        return 2;
    } else if (!strcmp(phase, "setup")) {
        phase_setup();
    } else if (!strcmp(phase, "user")) {
        phase_user();
    } else if (!strcmp(phase, "ext4")) {
        phase_ext4();
    } else {
        printf("unknown phase\n");
        return 2;
    }
    printf("done %s\n", phase);
    return 0;
}
