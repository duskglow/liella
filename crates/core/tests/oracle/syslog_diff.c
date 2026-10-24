/* System logging end to end (spec §26.1; D-26.1; phase 13): built against the host C library and
 * against Liella's products, the outputs must match line for line. Black-box: every line is a
 * result, an errno, or bytes the probe itself received on its own fixture /dev/log socket, read
 * from its own fixture /dev/console file, or read from its own standard-error pipe.
 *
 * The program must never reach the system log. It refuses to run unless /dev holds the marker file
 * `.liella-syslog-fixture`, which syslog_run.sh creates in a private tmpfs mounted over /dev inside
 * a private mount namespace: /dev/log, /dev/console and everything else under /dev there belong to
 * the probe and vanish with the namespace.
 *
 * Output conventions: bytes are printed in brackets with every byte outside printable ASCII as
 * \xNN and a backslash as \\; a run of more than 60 equal bytes is printed as <c>*<n>. In received
 * text, a timestamp of the form "Mmm dd hh:mm:ss" that equals the local time of the last ten
 * seconds (month names in English, the day as "%2d", the rest as "%02d") is printed as <TS>; one
 * that equals only the UTC time as <TS-UTC>; any other text of that shape as <TS?>. "[n]" where n
 * is the probe's process ID is printed as [<pid>], a child's as [<cpid>]. The program's short name
 * (program_invocation_short_name) is printed as <prog> where it starts the text or follows a
 * blank or '>' and is followed by '[' or ':'. Nothing prints a descriptor number, an address or a
 * time. Risky cases run in forked children with an alarm; the line "... child exited N" or
 * "... child killed by SIG" follows the child's own output.
 *
 * Usage: syslog_diff, normally through syslog_run.sh. */
#define _GNU_SOURCE
#undef _FORTIFY_SOURCE
#define SYSLOG_NAMES
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <printf.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <syslog.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>
#include "tscale.h"

/* The probe passes a registered conversion (%W) and non-literal formats on purpose. */
#pragma GCC diagnostic ignored "-Wformat"
#pragma GCC diagnostic ignored "-Wformat-security"

/* Declared by <bits/syslog-decl.h>, which <syslog.h> includes only when _FORTIFY_SOURCE is
 * active; the probe undefines _FORTIFY_SOURCE and repeats the declarations. */
extern void __syslog_chk(int pri, int flag, const char *fmt, ...);
extern void __vsyslog_chk(int pri, int flag, const char *fmt, va_list ap);

#define MARKER "/dev/.liella-syslog-fixture"
#define LOGPATH "/dev/log"
#define CONSOLE "/dev/console"

static const char *prog;
static pid_t mypid, cpid = -1;
static int rx = -1;          /* the probe's receiving socket at /dev/log */
static int rx_type;          /* SOCK_DGRAM or SOCK_STREAM */
static unsigned char big[2 << 20];

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
    case EINVAL: return "EINVAL";
    case EDOM: return "EDOM";
    case ERANGE: return "ERANGE";
    case EAGAIN: return "EAGAIN";
    case EPIPE: return "EPIPE";
    case EMSGSIZE: return "EMSGSIZE";
    case ENOBUFS: return "ENOBUFS";
    case ECONNREFUSED: return "ECONNREFUSED";
    case EPROTOTYPE: return "EPROTOTYPE";
    case ENOTCONN: return "ENOTCONN";
    case EISCONN: return "EISCONN";
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
    case SIGPIPE: return "SIGPIPE";
    case SIGKILL: return "SIGKILL";
    case SIGILL: return "SIGILL";
    case SIGTRAP: return "SIGTRAP";
    default:
        snprintf(b, sizeof b, "signal%d", s);
        return b;
    }
}

static const char *const mon[12] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun",
                                    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};

static void fmt_tm(char *o, const struct tm *tm)
{
    snprintf(o, 16, "%s %2d %02d:%02d:%02d", mon[tm->tm_mon % 12], tm->tm_mday, tm->tm_hour,
             tm->tm_min, tm->tm_sec);
}

static int is_d(unsigned char c) { return c >= '0' && c <= '9'; }

/* 0: not a timestamp; 1: local time of the last ten seconds; 2: UTC only; 3: shape only. */
static int ts_kind(const unsigned char *s, size_t n)
{
    if (n < 15)
        return 0;
    if (!(s[0] >= 'A' && s[0] <= 'Z' && s[1] >= 'a' && s[1] <= 'z' && s[2] >= 'a' && s[2] <= 'z'
          && s[3] == ' ' && (s[4] == ' ' || is_d(s[4])) && is_d(s[5]) && s[6] == ' '
          && is_d(s[7]) && is_d(s[8]) && s[9] == ':' && is_d(s[10]) && is_d(s[11])
          && s[12] == ':' && is_d(s[13]) && is_d(s[14])))
        return 0;
    tzset();
    time_t now = time(NULL);
    char c[16];
    struct tm tm;
    for (time_t t = now - 10; t <= now + 1; t++) {
        localtime_r(&t, &tm);
        fmt_tm(c, &tm);
        if (memcmp(c, s, 15) == 0)
            return 1;
    }
    for (time_t t = now - 10; t <= now + 1; t++) {
        gmtime_r(&t, &tm);
        fmt_tm(c, &tm);
        if (memcmp(c, s, 15) == 0)
            return 2;
    }
    return 3;
}

/* Print bytes with the conventions above (no newline). */
static void vis(const unsigned char *s, size_t n)
{
    size_t pl = strlen(prog);
    putchar('[');
    for (size_t i = 0; i < n;) {
        int k = ts_kind(s + i, n - i);
        if (k) {
            fputs(k == 1 ? "<TS>" : k == 2 ? "<TS-UTC>" : "<TS?>", stdout);
            i += 15;
            continue;
        }
        if (s[i] == '[' && i + 1 < n && is_d(s[i + 1])) {
            size_t j = i + 1;
            long v = 0;
            while (j < n && is_d(s[j]) && v < 100000000)
                v = v * 10 + (s[j++] - '0');
            if (j < n && s[j] == ']' && (v == mypid || v == cpid)) {
                fputs(v == mypid ? "[<pid>]" : "[<cpid>]", stdout);
                i = j + 1;
                continue;
            }
        }
        if (pl && n - i > pl && memcmp(s + i, prog, pl) == 0 && (s[i + pl] == '[' || s[i + pl] == ':')
            && (i == 0 || s[i - 1] == ' ' || s[i - 1] == '>')) {
            fputs("<prog>", stdout);
            i += pl;
            continue;
        }
        size_t run = 1;
        while (i + run < n && s[i + run] == s[i])
            run++;
        unsigned char c = s[i];
        if (run > 60) {
            if (c >= 0x20 && c < 0x7f && c != '\\')
                printf("<%c>*%zu", c, run);
            else
                printf("<\\x%02x>*%zu", c, run);
            i += run;
            continue;
        }
        if (c == '\\')
            fputs("\\\\", stdout);
        else if (c >= 0x20 && c < 0x7f)
            putchar(c);
        else
            printf("\\x%02x", c);
        i++;
    }
    putchar(']');
}

static void vis_s(const char *s) { vis((const unsigned char *)s, strlen(s)); }

/* The length, then the bytes. The length does not depend on the program's name: "N" when the text
 * holds no <prog>, "N+kP" when it holds k of them (N bytes besides the k copies of the name). */
static void show(const unsigned char *s, size_t shown, size_t total)
{
    size_t pl = strlen(prog), k = 0;
    for (size_t i = 0; i < shown;) {
        if (ts_kind(s + i, shown - i)) {
            i += 15;
            continue;
        }
        if (pl && shown - i > pl && memcmp(s + i, prog, pl) == 0
            && (s[i + pl] == '[' || s[i + pl] == ':')
            && (i == 0 || s[i - 1] == ' ' || s[i - 1] == '>')) {
            k++;
            i += pl;
            continue;
        }
        i++;
    }
    if (k)
        printf("%zu+%zuP ", total - k * pl, k);
    else
        printf("%zu ", total);
    vis(s, shown);
}

/* ---------------------------------------------------------------- the fixture /dev/log */

static void rx_close(void)
{
    if (rx >= 0)
        close(rx);
    rx = -1;
}

static void rx_bind(int type)
{
    rx_close();
    unlink(LOGPATH);
    rx = socket(AF_UNIX, type | SOCK_CLOEXEC, 0);
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    strcpy(a.sun_path, LOGPATH);
    if (rx < 0 || bind(rx, (struct sockaddr *)&a, sizeof a) != 0) {
        printf("FATAL: cannot bind %s: %s\n", LOGPATH, en(errno));
        exit(1);
    }
    int sz = 1 << 22;
    setsockopt(rx, SOL_SOCKET, SO_RCVBUF, &sz, sizeof sz);
    if (type == SOCK_STREAM)
        listen(rx, 8);
    rx_type = type;
}

/* Print every datagram waiting on rx. */
static int drain(const char *label)
{
    int got = 0;
    for (;;) {
        ssize_t n = recv(rx, big, sizeof big, MSG_DONTWAIT | MSG_TRUNC);
        if (n < 0)
            break;
        printf("%s: ", label);
        show(big, (size_t)n > sizeof big ? sizeof big : (size_t)n, (size_t)n);
        putchar('\n');
        got++;
    }
    if (!got)
        printf("%s: nothing\n", label);
    return got;
}

/* Count and discard datagrams waiting on rx. */
static int drain_quiet(void)
{
    int got = 0;
    while (recv(rx, big, sizeof big, MSG_DONTWAIT) >= 0)
        got++;
    return got;
}

/* The header "<n>" of the next datagram, or "none". */
static const char *hdr_next(void)
{
    static char b[8][48];
    static int k;
    k = (k + 1) % 8;
    ssize_t n = recv(rx, big, sizeof big, MSG_DONTWAIT);
    if (n < 0) {
        snprintf(b[k], sizeof b[k], "none");
        return b[k];
    }
    size_t j = 0;
    while (j < (size_t)n && j < 20 && big[j] != '>')
        j++;
    snprintf(b[k], sizeof b[k], "%.*s", (int)(j + 1 < (size_t)n ? j + 1 : (size_t)n), (char *)big);
    int extra = drain_quiet();
    if (extra) {
        size_t l = strlen(b[k]);
        snprintf(b[k] + l, sizeof b[k] - l, "+%d", extra);
    }
    return b[k];
}

/* Accept a pending stream connection without waiting; -1 if there is none. */
static int accept_now(void)
{
    struct pollfd p = {.fd = rx, .events = POLLIN};
    if (poll(&p, 1, 0) != 1)
        return -1;
    return accept4(rx, NULL, NULL, SOCK_CLOEXEC);
}

/* Read a stream connection until EOF or 300 ms of silence, and print it. */
static void stream_show(const char *label, int c)
{
    size_t n = 0;
    for (;;) {
        struct pollfd p = {.fd = c, .events = POLLIN};
        if (poll(&p, 1, 300) != 1)
            break;
        ssize_t r = read(c, big + n, sizeof big - n);
        if (r <= 0)
            break;
        n += (size_t)r;
    }
    printf("%s: ", label);
    show(big, n, n);
    putchar('\n');
}

/* ---------------------------------------------------------------- descriptors */

#define MAXFD 256
static void fd_snap(char *set)
{
    memset(set, 0, MAXFD);
    DIR *d = opendir("/proc/self/fd");
    if (!d)
        return;
    int self = dirfd(d);
    struct dirent *e;
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.')
            continue;
        int fd = atoi(e->d_name);
        if (fd != self && fd >= 0 && fd < MAXFD)
            set[fd] = 1;
    }
    closedir(d);
}

static void fd_describe(int fd)
{
    struct stat st;
    if (fstat(fd, &st) != 0) {
        printf(" (fstat %s)", en(errno));
        return;
    }
    if (S_ISSOCK(st.st_mode)) {
        int t = 0, dom = 0;
        socklen_t l = sizeof t;
        getsockopt(fd, SOL_SOCKET, SO_TYPE, &t, &l);
        l = sizeof dom;
        getsockopt(fd, SOL_SOCKET, SO_DOMAIN, &dom, &l);
        printf(" socket %s %s", dom == AF_UNIX ? "unix" : "other",
               t == SOCK_DGRAM ? "dgram" : t == SOCK_STREAM ? "stream" : "other");
        struct sockaddr_un a;
        l = sizeof a;
        memset(&a, 0, sizeof a);
        if (getpeername(fd, (struct sockaddr *)&a, &l) == 0) {
            printf(" peer=");
            vis_s(l > sizeof(sa_family_t) ? a.sun_path : "");
        } else {
            printf(" unconnected(%s)", en(errno));
        }
    } else {
        printf(" %s", S_ISREG(st.st_mode) ? "file" : S_ISCHR(st.st_mode) ? "chr"
                      : S_ISFIFO(st.st_mode) ? "fifo" : "other");
    }
    int fdf = fcntl(fd, F_GETFD), fl = fcntl(fd, F_GETFL);
    printf(" cloexec=%s nonblock=%s", (fdf & FD_CLOEXEC) ? "yes" : "no",
           (fl & O_NONBLOCK) ? "yes" : "no");
}

static void fd_diff(const char *label, const char *before)
{
    char now[MAXFD];
    fd_snap(now);
    int add = 0, del = 0;
    for (int i = 0; i < MAXFD; i++) {
        add += now[i] && !before[i];
        del += !now[i] && before[i];
    }
    printf("%s: fds +%d -%d", label, add, del);
    for (int i = 0; i < MAXFD; i++)
        if (now[i] && !before[i]) {
            printf(";");
            fd_describe(i);
        }
    putchar('\n');
}

/* ---------------------------------------------------------------- stderr and console capture */

static int err_saved = -1, err_pipe = -1;

static void err_begin(void)
{
    int p[2];
    fflush(stderr);
    pipe2(p, O_CLOEXEC);
    err_saved = fcntl(2, F_DUPFD_CLOEXEC, 10);
    dup2(p[1], 2);
    close(p[1]);
    err_pipe = p[0];
}

static void err_end(const char *label)
{
    fflush(stderr);
    dup2(err_saved, 2);
    close(err_saved);
    size_t n = 0;
    fcntl(err_pipe, F_SETFL, O_NONBLOCK);
    for (;;) {
        ssize_t r = read(err_pipe, big + n, sizeof big - n);
        if (r <= 0)
            break;
        n += (size_t)r;
    }
    close(err_pipe);
    printf("%s: stderr ", label);
    show(big, n, n);
    putchar('\n');
}

static void console_reset(void)
{
    unlink(CONSOLE);
    int fd = open(CONSOLE, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0666);
    if (fd >= 0)
        close(fd);
}

static void console_show(const char *label)
{
    int fd = open(CONSOLE, O_RDONLY | O_CLOEXEC | O_NONBLOCK);
    if (fd < 0) {
        printf("%s: console %s\n", label, en(errno));
        return;
    }
    struct stat st;
    fstat(fd, &st);
    if (!S_ISREG(st.st_mode)) {
        printf("%s: console not a file\n", label);
        close(fd);
        return;
    }
    ssize_t n = read(fd, big, sizeof big);
    close(fd);
    printf("%s: console ", label);
    show(big, n > 0 ? (size_t)n : 0, n > 0 ? (size_t)n : 0);
    putchar('\n');
    console_reset();
}

/* ---------------------------------------------------------------- children */

static void child_status(const char *label, int st)
{
    if (WIFEXITED(st))
        printf("%s: child exited %d\n", label, WEXITSTATUS(st));
    else if (WIFSIGNALED(st))
        printf("%s: child killed by %s\n", label, sig_name(WTERMSIG(st)));
}

/* Run fn in a child with an alarm; the child's standard error goes to a pipe, printed after the
 * child's status when it is not empty. */
static void run_child(const char *label, void (*fn)(void), unsigned secs)
{
    int ep[2];
    fflush(stdout);
    fflush(stderr);
    pipe2(ep, O_CLOEXEC);
    pid_t p = fork();
    if (p == 0) {
        tw_alarm(secs);
        cpid = getpid();
        dup2(ep[1], 2);
        fn();
        fflush(stdout);
        _exit(0);
    }
    close(ep[1]);
    size_t n = 0;
    ssize_t r;
    while (n < sizeof big && (r = read(ep[0], big + n, sizeof big - n)) > 0)
        n += (size_t)r;
    close(ep[0]);
    int st;
    waitpid(p, &st, 0);
    child_status(label, st);
    if (n) {
        cpid = p;
        printf("%s: child stderr ", label);
        show(big, n, n);
        putchar('\n');
        cpid = -1;
    }
}

/* ---------------------------------------------------------------- a counting printf specifier */

static int w_count;

static int w_print(FILE *f, const struct printf_info *info, const void *const *args)
{
    (void)info;
    (void)args;
    w_count++;
    fputc('W', f);
    return 1;
}

static int w_arginfo(const struct printf_info *info, size_t n, int *types, int *size)
{
    (void)info;
    (void)n;
    (void)types;
    (void)size;
    return 0;
}

/* ---------------------------------------------------------------- sections */

static void s_constants(void)
{
    printf("== constants\n");
    printf("priorities: EMERG %d ALERT %d CRIT %d ERR %d WARNING %d NOTICE %d INFO %d DEBUG %d\n",
           LOG_EMERG, LOG_ALERT, LOG_CRIT, LOG_ERR, LOG_WARNING, LOG_NOTICE, LOG_INFO, LOG_DEBUG);
    printf("facilities: KERN %d USER %d MAIL %d DAEMON %d AUTH %d SYSLOG %d LPR %d NEWS %d UUCP %d "
           "CRON %d AUTHPRIV %d FTP %d LOCAL0 %d LOCAL7 %d\n",
           LOG_KERN, LOG_USER, LOG_MAIL, LOG_DAEMON, LOG_AUTH, LOG_SYSLOG, LOG_LPR, LOG_NEWS,
           LOG_UUCP, LOG_CRON, LOG_AUTHPRIV, LOG_FTP, LOG_LOCAL0, LOG_LOCAL7);
    printf("options: PID %#x CONS %#x ODELAY %#x NDELAY %#x NOWAIT %#x PERROR %#x\n", LOG_PID,
           LOG_CONS, LOG_ODELAY, LOG_NDELAY, LOG_NOWAIT, LOG_PERROR);
    printf("masks: PRIMASK %#x FACMASK %#x NFACILITIES %d LOG_MASK(ERR) %#x LOG_UPTO(ERR) %#x "
           "LOG_PRI(0x9b) %d LOG_FAC(0x9b) %d LOG_MAKEPRI(LOCAL0,ERR) %d\n",
           LOG_PRIMASK, LOG_FACMASK, LOG_NFACILITIES, LOG_MASK(LOG_ERR), LOG_UPTO(LOG_ERR),
           LOG_PRI(0x9b), LOG_FAC(0x9b), LOG_MAKEPRI(LOG_LOCAL0, LOG_ERR));
    printf("_PATH_LOG ");
    vis_s(_PATH_LOG);
    printf(" INTERNAL_NOPRI %#x INTERNAL_MARK %#x\n", INTERNAL_NOPRI, INTERNAL_MARK);
    printf("prioritynames:");
    for (CODE *c = prioritynames; c->c_name; c++)
        printf(" %s=%d", c->c_name, c->c_val);
    printf("\nfacilitynames:");
    for (CODE *c = facilitynames; c->c_name; c++)
        printf(" %s=%d", c->c_name, c->c_val);
    printf("\n");
}

static void s_default(void)
{
    char b[MAXFD];
    printf("== default connection (no openlog)\n");
    fd_snap(b);
    syslog(LOG_INFO, "first message");
    fd_diff("after first syslog", b);
    drain("first");
    fd_snap(b);
    syslog(LOG_INFO, "second message");
    fd_diff("after second syslog", b);
    drain("second");
    fd_snap(b);
    closelog();
    fd_diff("after closelog", b);
    fd_snap(b);
    closelog();
    fd_diff("after second closelog", b);
    syslog(LOG_INFO, "after closelog");
    drain("after closelog");
    closelog();
}

static void s_priorities(void)
{
    static const struct { const char *n; int v; } fac[] = {
        {"KERN", LOG_KERN}, {"USER", LOG_USER}, {"MAIL", LOG_MAIL}, {"DAEMON", LOG_DAEMON},
        {"AUTH", LOG_AUTH}, {"SYSLOG", LOG_SYSLOG}, {"LPR", LOG_LPR}, {"NEWS", LOG_NEWS},
        {"UUCP", LOG_UUCP}, {"CRON", LOG_CRON}, {"AUTHPRIV", LOG_AUTHPRIV}, {"FTP", LOG_FTP},
        {"12<<3", 12 << 3}, {"15<<3", 15 << 3}, {"LOCAL0", LOG_LOCAL0}, {"LOCAL1", LOG_LOCAL1},
        {"LOCAL2", LOG_LOCAL2}, {"LOCAL3", LOG_LOCAL3}, {"LOCAL4", LOG_LOCAL4},
        {"LOCAL5", LOG_LOCAL5}, {"LOCAL6", LOG_LOCAL6}, {"LOCAL7", LOG_LOCAL7},
    };
    printf("== priorities and facilities (no openlog)\n");
    for (size_t f = 0; f < sizeof fac / sizeof fac[0]; f++) {
        printf("fac %s:", fac[f].n);
        for (int p = 0; p < 8; p++) {
            syslog(fac[f].v | p, "m");
            printf(" %s", hdr_next());
        }
        printf("\n");
    }
    static const int bad[] = {-1, 0x3f8 | 3, (LOG_NFACILITIES << 3) | 3,
                              ((LOG_NFACILITIES + 1) << 3) | 3, 0x400, 0x400 | 3, 0x1000 | 3,
                              0x10000 | LOG_ERR, INT_MIN, INT_MIN | LOG_LOCAL0 | LOG_ERR, INT_MAX};
    for (size_t i = 0; i < sizeof bad / sizeof bad[0]; i++) {
        char l[64];
        snprintf(l, sizeof l, "pri %#x", (unsigned)bad[i]);
        errno = EDOM;
        syslog(bad[i], "bad pri");
        int e = errno;
        drain(l);
        printf("pri %#x: errno after %s\n", (unsigned)bad[i], en(e));
    }
    closelog();
}

static void s_facility(void)
{
    printf("== openlog facility\n");
    struct { const char *n; int fac; } cases[] = {
        {"LOCAL3", LOG_LOCAL3}, {"0 after LOCAL3", 0}, {"MAIL", LOG_MAIL},
        {"LOCAL3|ERR", LOG_LOCAL3 | LOG_ERR}, {"0x400", 0x400}, {"0x3f8", 0x3f8},
        {"-1", -1}, {"LOCAL5", LOG_LOCAL5}, {"7", 7}, {"0x10000|LOCAL1", 0x10000 | LOG_LOCAL1},
    };
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        openlog("fac", 0, cases[i].fac);
        syslog(LOG_ERR, "e");
        const char *h1 = hdr_next();
        syslog(LOG_KERN | LOG_ERR, "e");
        const char *h2 = hdr_next();
        syslog(LOG_DAEMON | LOG_ERR, "e");
        const char *h3 = hdr_next();
        printf("openlog facility %s: ERR %s KERN|ERR %s DAEMON|ERR %s\n", cases[i].n, h1, h2, h3);
    }
    openlog("fac", 0, LOG_LOCAL2);
    closelog();
    syslog(LOG_ERR, "e");
    printf("after openlog LOCAL2 and closelog: ERR %s\n", hdr_next());
    openlog(NULL, 0, LOG_LOCAL2);
    syslog(LOG_ERR, "e");
    printf("openlog(NULL, 0, LOCAL2): ERR %s\n", hdr_next());
    closelog();
}

static void s_ident(void)
{
    printf("== ident\n");
    char buf[64] = "first";
    openlog(buf, 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident first");
    strcpy(buf, "second-changed");
    syslog(LOG_INFO, "x");
    drain("ident buffer changed after openlog");
    buf[0] = 0;
    syslog(LOG_INFO, "x");
    drain("ident buffer emptied after openlog");
    closelog();
    syslog(LOG_INFO, "x");
    drain("after closelog");
    openlog("", 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident empty");
    openlog("", LOG_PID, 0);
    syslog(LOG_INFO, "x");
    drain("ident empty, LOG_PID");
    openlog(NULL, 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident NULL");
    openlog(NULL, LOG_PID, 0);
    syslog(LOG_INFO, "x");
    drain("ident NULL, LOG_PID");
    openlog("a b:c[d]", 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident with blank, colon, brackets");
    openlog("nl\nid\x01\xc3\xa9", 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident with newline, control, UTF-8");
    openlog("%s%n%m", 0, 0);
    syslog(LOG_INFO, "x");
    drain("ident with conversions");
    static char longid[3000];
    static const size_t lens[] = {31, 32, 33, 64, 255, 256, 1000, 1024, 2000, 2999};
    for (size_t i = 0; i < sizeof lens / sizeof lens[0]; i++) {
        memset(longid, 'i', lens[i]);
        longid[lens[i]] = 0;
        openlog(longid, LOG_PID, 0);
        syslog(LOG_INFO, "x");
        char l[48];
        snprintf(l, sizeof l, "ident %zu bytes, LOG_PID", lens[i]);
        drain(l);
    }
    openlog("one", 0, 0);
    openlog(NULL, 0, 0);
    syslog(LOG_INFO, "x");
    drain("openlog one then openlog NULL");
    closelog();
}

static void s_options(void)
{
    char b[MAXFD];
    printf("== options\n");
    openlog("opt", LOG_PID, 0);
    syslog(LOG_INFO, "x");
    drain("LOG_PID");
    closelog();

    fd_snap(b);
    openlog("opt", LOG_NDELAY, 0);
    fd_diff("after openlog LOG_NDELAY", b);
    fd_snap(b);
    syslog(LOG_INFO, "x");
    fd_diff("after syslog", b);
    drain("LOG_NDELAY");
    fd_snap(b);
    openlog("opt", LOG_NDELAY, 0);
    fd_diff("after a second openlog LOG_NDELAY", b);
    fd_snap(b);
    openlog("opt", 0, 0);
    fd_diff("after openlog 0 while connected", b);
    syslog(LOG_INFO, "x");
    drain("openlog 0 while connected");
    fd_snap(b);
    closelog();
    fd_diff("after closelog", b);

    fd_snap(b);
    openlog("opt", LOG_ODELAY, 0);
    fd_diff("after openlog LOG_ODELAY", b);
    fd_snap(b);
    syslog(LOG_INFO, "x");
    fd_diff("after syslog", b);
    drain("LOG_ODELAY");
    closelog();

    fd_snap(b);
    openlog("opt", 0, 0);
    fd_diff("after openlog 0", b);
    closelog();

    openlog("opt", LOG_NOWAIT, 0);
    syslog(LOG_INFO, "x");
    drain("LOG_NOWAIT");
    closelog();
    openlog("opt", 0x1000 | 0x4000, 0);
    syslog(LOG_INFO, "x");
    drain("unknown option bits 0x5000");
    closelog();
    openlog("opt", LOG_PID | LOG_PID, 0);
    openlog("opt2", 0, 0);
    syslog(LOG_INFO, "x");
    drain("LOG_PID then openlog without it");
    closelog();
    openlog("opt", LOG_PID, LOG_LOCAL1);
    closelog();
    syslog(LOG_INFO, "x");
    drain("LOG_PID, LOCAL1, closelog, syslog");
    closelog();
}

static void perror_closed_child(void)
{
    close(2);
    openlog("closed2", LOG_PERROR | LOG_NDELAY, 0);
    syslog(LOG_ERR, "with stderr closed");
    drain("stderr closed");
    closelog();
}

static void s_perror(void)
{
    printf("== LOG_PERROR\n");
    openlog("perr", LOG_PERROR, 0);
    err_begin();
    syslog(LOG_ERR, "to both");
    err_end("perr");
    drain("perr");
    openlog("perr", LOG_PERROR | LOG_PID, 0);
    err_begin();
    syslog(LOG_ERR, "with pid");
    err_end("perr pid");
    drain("perr pid");
    err_begin();
    syslog(LOG_ERR, "trailing newline\n");
    err_end("perr trailing newline");
    drain("perr trailing newline");
    err_begin();
    syslog(LOG_ERR, "two newlines\n\n");
    err_end("perr two newlines");
    drain("perr two newlines");
    err_begin();
    syslog(LOG_ERR, "in\nside");
    err_end("perr embedded newline");
    drain("perr embedded newline");
    err_begin();
    syslog(LOG_ERR, "%s", "");
    err_end("perr empty");
    drain("perr empty");
    err_begin();
    syslog(LOG_ERR, "a%cb", 0);
    err_end("perr NUL");
    drain("perr NUL");
    static char longm[5001];
    memset(longm, 'y', 5000);
    err_begin();
    syslog(LOG_ERR, "%s", longm);
    err_end("perr 5000");
    drain("perr 5000");
    openlog(NULL, LOG_PERROR, 0);
    err_begin();
    syslog(LOG_ERR, "ident NULL");
    err_end("perr ident NULL");
    drain("perr ident NULL");
    openlog("", LOG_PERROR, 0);
    err_begin();
    syslog(LOG_ERR, "ident empty");
    err_end("perr ident empty");
    drain("perr ident empty");
    openlog("perr", LOG_PERROR, 0);
    int old = setlogmask(LOG_MASK(LOG_ERR));
    err_begin();
    syslog(LOG_INFO, "masked");
    err_end("perr masked priority");
    drain("perr masked priority");
    setlogmask(old);
    printf("stderr orientation after LOG_PERROR: %d\n", fwide(stderr, 0));
    closelog();
    err_begin();
    syslog(LOG_ERR, "after closelog");
    err_end("perr after closelog");
    drain("perr after closelog");
    openlog(NULL, 0, LOG_USER);
    err_begin();
    syslog(LOG_ERR, "after openlog(NULL, 0, LOG_USER)");
    err_end("perr after openlog 0");
    drain("perr after openlog 0");
    closelog();
    run_child("perr with fd 2 closed", perror_closed_child, 5);
    drain("perr with fd 2 closed, parent");
}

static void s_mask(void)
{
    printf("== setlogmask\n");
    printf("setlogmask(0) initially: %#x\n", (unsigned)setlogmask(0));
    printf("setlogmask(0) again: %#x\n", (unsigned)setlogmask(0));
    printf("setlogmask(LOG_MASK(ERR)): %#x\n", (unsigned)setlogmask(LOG_MASK(LOG_ERR)));
    printf("setlogmask(0): %#x\n", (unsigned)setlogmask(0));
    for (int p = 0; p < 8; p++) {
        syslog(LOG_LOCAL0 | p, "m");
        printf("mask ERR, LOCAL0|%d: %s\n", p, hdr_next());
    }
    printf("setlogmask(LOG_UPTO(WARNING)): %#x\n", (unsigned)setlogmask(LOG_UPTO(LOG_WARNING)));
    for (int p = 0; p < 8; p++) {
        syslog(p, "m");
        printf("mask UPTO(WARNING), %d: %s\n", p, hdr_next());
    }
    printf("setlogmask(-1): %#x\n", (unsigned)setlogmask(-1));
    printf("setlogmask(0): %#x\n", (unsigned)setlogmask(0));
    syslog(LOG_DEBUG, "m");
    printf("mask -1, DEBUG: %s\n", hdr_next());
    printf("setlogmask(0x100): %#x\n", (unsigned)setlogmask(0x100));
    printf("setlogmask(0): %#x\n", (unsigned)setlogmask(0));
    for (int p = 0; p < 8; p++) {
        syslog(p, "m");
        printf("mask 0x100, %d: %s\n", p, hdr_next());
    }
    syslog(0x10000 | LOG_ERR, "m");
    drain("mask 0x100, invalid pri 0x10003");
    printf("setlogmask(0x1ff): %#x\n", (unsigned)setlogmask(0x1ff));
    printf("setlogmask(LOG_MASK(INFO)): %#x\n", (unsigned)setlogmask(LOG_MASK(LOG_INFO)));
    syslog(0x10000 | LOG_ERR, "m");
    drain("mask INFO, invalid pri 0x10003");
    syslog(-1, "m");
    drain("mask INFO, pri -1");
    /* Masked messages and formatting. */
    register_printf_specifier('W', w_print, w_arginfo);
    w_count = 0;
    syslog(LOG_DEBUG, "masked %W");
    printf("masked syslog with a counting specifier: count %d, %s\n", w_count, hdr_next());
    syslog(LOG_INFO, "passed %W");
    printf("passing syslog with a counting specifier: count %d\n", w_count);
    drain("passed");
    char b[MAXFD];
    closelog();
    fd_snap(b);
    syslog(LOG_DEBUG, "masked, not connected");
    fd_diff("masked message while not connected", b);
    closelog();
    printf("setlogmask(0) after closelog: %#x\n", (unsigned)setlogmask(0));
    printf("restore 0xff: %#x\n", (unsigned)setlogmask(0xff));
}

static void vs(int pri, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsyslog(pri, fmt, ap);
    va_end(ap);
}

static void s_format(void)
{
    printf("== format\n");
    openlog("fmt", 0, 0);
    errno = EACCES;
    syslog(LOG_INFO, "m=%m");
    drain("%m EACCES");
    errno = 0;
    syslog(LOG_INFO, "m=%m");
    drain("%m 0");
    errno = 99999;
    syslog(LOG_INFO, "m=%m");
    drain("%m 99999");
    errno = -5;
    syslog(LOG_INFO, "m=%m");
    drain("%m -5");
    errno = ENOENT;
    syslog(LOG_INFO, "%s %m %d %m", "a", 7);
    drain("%m twice with arguments");
    errno = ENOENT;
    syslog(LOG_INFO, "[%-30m] [%30m] [%.5m]");
    drain("%m with width and precision");
    errno = ENOENT;
    syslog(LOG_INFO, "%%m %%%m");
    drain("%%m");
    errno = ENOENT;
    vs(LOG_INFO, "v %m %s", "arg");
    drain("vsyslog %m");
    errno = EDOM;
    syslog(LOG_INFO, "errno preserved?");
    printf("errno after a delivered syslog (EDOM before): %s\n", en(errno));
    drain("errno");
    syslog(LOG_INFO, "%s", "");
    drain("empty message");
    syslog(LOG_INFO, "line\n");
    drain("trailing newline");
    syslog(LOG_INFO, "line\n\n");
    drain("two trailing newlines");
    syslog(LOG_INFO, "\n");
    drain("only a newline");
    syslog(LOG_INFO, "a\nb\r\nc");
    drain("embedded newline and CRLF");
    syslog(LOG_INFO, "a%cb%cc", 0, 0);
    drain("embedded NUL");
    syslog(LOG_INFO, "tab\there \x01\x7f\xc3\xa9\xff");
    drain("control and high bytes");
    syslog(LOG_INFO, "%d %5.2f %x %s %c %p", -42, 3.14159, 255, "str", 'q', (void *)0);
    drain("printf conversions");
    syslog(LOG_INFO, "%ls", L"wide");
    drain("%ls");
    closelog();
}

static void null_fmt_child(void)
{
    openlog("nf", 0, 0);
    syslog(LOG_INFO, NULL);
    printf("returned\n");
}

static void s_long(void)
{
    printf("== long messages (datagram)\n");
    static const size_t sizes[] = {900,   1000,  1023,   1024,   1025,   2000,   2048,
                                   4000,  8192,  16384,  65536,  100000, 200000, 212000,
                                   213000, 300000, 1000000};
    char *m = malloc(1000001);
    openlog("long", 0, 0);
    for (size_t i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        memset(m, 'x', sizes[i]);
        m[sizes[i]] = 0;
        char l[48];
        snprintf(l, sizeof l, "body %zu", sizes[i]);
        errno = EDOM;
        syslog(LOG_INFO, "%s", m);
        int e = errno;
        drain(l);
        printf("%s: errno after %s\n", l, en(e));
    }
    /* Too long for a datagram, with LOG_CONS. */
    openlog("long", LOG_CONS, 0);
    console_reset();
    memset(m, 'x', 300000);
    m[300000] = 0;
    char b[MAXFD];
    fd_snap(b);
    errno = EDOM;
    syslog(LOG_INFO, "%s", m);
    printf("body 300000, LOG_CONS: errno after %s\n", en(errno));
    fd_diff("body 300000, LOG_CONS", b);
    drain("body 300000, LOG_CONS");
    console_show("body 300000, LOG_CONS");
    syslog(LOG_INFO, "short after");
    drain("short after the long one");
    openlog("long", 0, 0);
    /* A long literal format rather than an argument. */
    memset(m, 'z', 3000);
    m[3000] = 0;
    syslog(LOG_INFO, m);
    drain("format of 3000 bytes");
    free(m);
    closelog();
    run_child("NULL format", null_fmt_child, 5);
    drain("NULL format, parent");
}

static void tz_child(void)
{
    setenv("TZ", "XST-5:30", 1);
    tzset();
    openlog("tz", 0, 0);
    syslog(LOG_INFO, "x");
    drain("TZ=XST-5:30");
    setenv("TZ", "UTC0", 1);
    syslog(LOG_INFO, "x");
    drain("TZ changed to UTC0 without tzset");
    tzset();
    syslog(LOG_INFO, "x");
    drain("after tzset");
    closelog();
}

static void s_time(void)
{
    printf("== timestamp time zone\n");
    run_child("tz", tz_child, 5);
}

static void s_absent(void)
{
    printf("== /dev/log absent\n");
    rx_close();
    unlink(LOGPATH);
    console_reset();
    char b[MAXFD];
    fd_snap(b);
    errno = EDOM;
    syslog(LOG_INFO, "nowhere");
    printf("no LOG_CONS: errno after %s\n", en(errno));
    fd_diff("after syslog with no /dev/log", b);
    console_show("no LOG_CONS");
    fd_snap(b);
    openlog("absent", LOG_NDELAY, 0);
    fd_diff("after openlog LOG_NDELAY with no /dev/log", b);
    closelog();
    openlog("cons", LOG_CONS, 0);
    errno = EDOM;
    syslog(LOG_ERR, "to the console");
    printf("LOG_CONS: errno after %s\n", en(errno));
    console_show("LOG_CONS");
    openlog("cons", LOG_CONS | LOG_PID, LOG_LOCAL0);
    syslog(LOG_ERR, "with pid");
    console_show("LOG_CONS|LOG_PID");
    syslog(LOG_ERR, "trailing newline\n");
    console_show("LOG_CONS trailing newline");
    syslog(LOG_ERR, "a%cb", 0);
    console_show("LOG_CONS NUL");
    static char longm[5001];
    memset(longm, 'c', 5000);
    syslog(LOG_ERR, "%s", longm);
    console_show("LOG_CONS 5000");
    syslog(LOG_ERR, "first, longer");
    syslog(LOG_ERR, "2");
    console_show("LOG_CONS two messages");
    fd_snap(b);
    syslog(LOG_ERR, "fds");
    fd_diff("after a LOG_CONS message", b);
    console_show("LOG_CONS fds");
    openlog("cons", LOG_CONS | LOG_PERROR, 0);
    err_begin();
    syslog(LOG_ERR, "both");
    err_end("LOG_CONS|LOG_PERROR");
    console_show("LOG_CONS|LOG_PERROR");
    openlog("cons", LOG_PERROR, 0);
    err_begin();
    syslog(LOG_ERR, "stderr only");
    err_end("LOG_PERROR without LOG_CONS");
    console_show("LOG_PERROR without LOG_CONS");
    unlink(CONSOLE);
    openlog("cons", LOG_CONS, 0);
    syslog(LOG_ERR, "no console");
    console_show("LOG_CONS, no /dev/console");
    printf("still alive\n");
    /* A file at /dev/log that is not a socket. */
    int fd = open(LOGPATH, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0666);
    close(fd);
    console_reset();
    syslog(LOG_ERR, "log is a file");
    console_show("/dev/log a regular file, LOG_CONS");
    struct stat st;
    stat(LOGPATH, &st);
    printf("/dev/log file size after: %lld\n", (long long)st.st_size);
    unlink(LOGPATH);
    closelog();
}

static void fifo_console_child(void)
{
    openlog("fifo", LOG_CONS, 0);
    syslog(LOG_ERR, "to a FIFO console without a reader");
    printf("returned\n");
}

static void s_fifo_console(void)
{
    printf("== /dev/console a FIFO without a reader (no /dev/log)\n");
    unlink(CONSOLE);
    mkfifo(CONSOLE, 0666);
    run_child("fifo console", fifo_console_child, 3);
    unlink(CONSOLE);
    console_reset();
}

static void s_gone(void)
{
    printf("== receiver gone and reconnection\n");
    char b[MAXFD];
    rx_bind(SOCK_DGRAM);
    console_reset();
    openlog("gone", LOG_NDELAY | LOG_CONS, 0);
    syslog(LOG_INFO, "before");
    drain("connected");
    /* The receiver closes; the socket file stays. */
    rx_close();
    fd_snap(b);
    errno = EDOM;
    syslog(LOG_INFO, "receiver closed");
    printf("receiver closed: errno after %s\n", en(errno));
    fd_diff("after syslog to a closed receiver", b);
    console_show("receiver closed, LOG_CONS");
    errno = EDOM;
    syslog(LOG_INFO, "receiver still closed");
    console_show("receiver still closed, LOG_CONS");
    /* A new receiver at the same path, while the old connection is dead. */
    rx_bind(SOCK_DGRAM);
    syslog(LOG_INFO, "new receiver");
    drain("new receiver");
    console_show("new receiver, LOG_CONS");
    closelog();
    /* Connected, then the receiver is replaced without any failure in between. */
    openlog("gone", LOG_NDELAY, 0);
    syslog(LOG_INFO, "one");
    drain("first receiver");
    rx_close();
    rx_bind(SOCK_DGRAM);
    syslog(LOG_INFO, "after replacement");
    drain("replaced receiver");
    syslog(LOG_INFO, "again");
    drain("replaced receiver, next");
    closelog();
    /* No LOG_CONS, receiver closed. */
    openlog("gone", LOG_NDELAY, 0);
    rx_close();
    errno = EDOM;
    syslog(LOG_INFO, "lost");
    printf("receiver closed, no LOG_CONS: errno after %s\n", en(errno));
    console_show("receiver closed, no LOG_CONS");
    closelog();
    rx_bind(SOCK_DGRAM);
}

static int qpipe = -1;

static void full_child(void)
{
    closelog();
    openlog("full", 0, 0);
    for (int i = 0; i < 4000; i++) {
        syslog(LOG_INFO, "fill %d", i);
        (void)!write(qpipe, "x", 1);
    }
}

static void s_full(void)
{
    printf("== receiver queue full (datagram)\n");
    int p[2];
    pipe2(p, O_CLOEXEC);
    fcntl(p[0], F_SETFL, O_NONBLOCK);
    fcntl(p[1], F_SETFL, O_NONBLOCK);
    qpipe = p[1];
    run_child("4000 messages, receiver not reading", full_child, 3);
    close(p[1]);
    int n = 0;
    char c[4096];
    ssize_t r;
    while ((r = read(p[0], c, sizeof c)) > 0)
        n += (int)r;
    close(p[0]);
    int q = drain_quiet();
    printf("calls returned: %s; datagrams queued: %s; calls returned == datagrams queued: %s\n",
           n == 4000 ? "all 4000" : n > 10 ? "more than 10, fewer than 4000" : "10 or fewer",
           q == 4000 ? "4000" : q > 10 ? "more than 10, fewer than 4000" : "10 or fewer",
           n == q ? "yes" : "no");
    syslog(LOG_INFO, "after the queue was drained");
    drain("after drain");
    closelog();
}

static void fork_child(void)
{
    syslog(LOG_INFO, "from the child");
    closelog();
}

static void s_fork(void)
{
    printf("== fork\n");
    openlog("fork", LOG_PID | LOG_NDELAY, 0);
    syslog(LOG_INFO, "parent before");
    drain("parent before");
    fflush(stdout);
    pid_t p = fork();
    if (p == 0) {
        fork_child();
        _exit(0);
    }
    cpid = p;
    int st;
    waitpid(p, &st, 0);
    child_status("child", st);
    drain("child's message");
    syslog(LOG_INFO, "parent after the child's closelog");
    drain("parent after");
    cpid = -1;
    closelog();
}

static void stream_peer_closed_child(void)
{
    openlog("pipe", LOG_NDELAY | LOG_CONS, 0);
    int c = accept_now();
    if (c >= 0)
        close(c);
    syslog(LOG_INFO, "to a closed stream peer");
    printf("returned after a closed peer\n");
    console_show("stream peer closed, listener still there, LOG_CONS");
}

static void stream_peer_gone_child(void)
{
    openlog("pipe", LOG_NDELAY | LOG_CONS, 0);
    int c = accept_now();
    if (c >= 0)
        close(c);
    unlink(LOGPATH);
    errno = EDOM;
    syslog(LOG_INFO, "to a closed stream peer, no listener");
    printf("returned, errno %s\n", en(errno));
    console_show("stream peer closed, /dev/log removed, LOG_CONS");
}

static void stream_peer_gone_nocons_child(void)
{
    openlog("pipe", LOG_NDELAY, 0);
    int c = accept_now();
    if (c >= 0)
        close(c);
    unlink(LOGPATH);
    errno = EDOM;
    syslog(LOG_INFO, "to a closed stream peer, no listener, no LOG_CONS");
    printf("returned, errno %s\n", en(errno));
}

static void accept_all(const char *label)
{
    int c, k = 0;
    while ((c = accept_now()) >= 0) {
        stream_show(label, c);
        close(c);
        k++;
    }
    if (!k)
        printf("%s: no connection\n", label);
}

static void s_stream(void)
{
    printf("== stream socket at /dev/log\n");
    char b[MAXFD];
    rx_bind(SOCK_STREAM);
    fd_snap(b);
    openlog("stream", LOG_NDELAY, 0);
    fd_diff("after openlog LOG_NDELAY", b);
    int c = accept_now();
    printf("pending connection after openlog: %s\n", c >= 0 ? "yes" : "no");
    syslog(LOG_INFO, "one");
    syslog(LOG_INFO, "two\n");
    syslog(LOG_INFO, "%s", "");
    syslog(LOG_INFO, "a%cb", 0);
    static char m[5001];
    memset(m, 's', 5000);
    syslog(LOG_INFO, "%s", m);
    if (c < 0)
        c = accept_now();
    closelog();
    if (c >= 0) {
        stream_show("stream bytes", c);
        close(c);
    } else {
        printf("stream bytes: no connection\n");
    }
    c = accept_now();
    printf("further connection: %s\n", c >= 0 ? "yes" : "no");
    if (c >= 0)
        close(c);
    /* Without LOG_NDELAY: the connection is made by the first syslog. */
    openlog("stream", LOG_PID, 0);
    c = accept_now();
    printf("pending connection after openlog without LOG_NDELAY: %s\n", c >= 0 ? "yes" : "no");
    syslog(LOG_INFO, "lazy");
    if (c < 0)
        c = accept_now();
    closelog();
    if (c >= 0) {
        stream_show("stream lazy", c);
        close(c);
    }
    /* A long message over a stream (fits the socket buffer). */
    openlog("stream", 0, 0);
    char *l = malloc(100001);
    memset(l, 'L', 100000);
    l[100000] = 0;
    syslog(LOG_INFO, "%s", l);
    free(l);
    c = accept_now();
    closelog();
    if (c >= 0) {
        stream_show("stream 100000", c);
        close(c);
    }
    console_reset();
    run_child("stream peer closed", stream_peer_closed_child, 5);
    accept_all("after the child, further connections");
    rx_bind(SOCK_STREAM);
    run_child("stream peer gone", stream_peer_gone_child, 5);
    rx_bind(SOCK_STREAM);
    run_child("stream peer gone, no LOG_CONS", stream_peer_gone_nocons_child, 5);
    rx_bind(SOCK_DGRAM);
}

static void chk_ro_child(void)
{
    int n = -1;
    openlog("chk", 0, 0);
    __syslog_chk(LOG_INFO, 1, "ro %n done", &n);
    printf("returned, n=%d\n", n);
    drain("chk flag 1, read-only format with %n");
}

static void chk_rw_child(void)
{
    static char fmt[] = "rw %n done";
    int n = -1;
    openlog("chk", 0, 0);
    __syslog_chk(LOG_INFO, 1, fmt, &n);
    printf("returned, n=%d\n", n);
    drain("chk flag 1, writable format with %n");
}

static void chk_rw0_child(void)
{
    static char fmt[] = "rw0 %n done";
    int n = -1;
    openlog("chk", 0, 0);
    __syslog_chk(LOG_INFO, 0, fmt, &n);
    printf("returned, n=%d\n", n);
    drain("chk flag 0, writable format with %n");
}

static void vchk(int pri, int flag, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    __vsyslog_chk(pri, flag, fmt, ap);
    va_end(ap);
}

static void vchk_rw_child(void)
{
    static char fmt[] = "vrw %n done";
    int n = -1;
    openlog("chk", 0, 0);
    vchk(LOG_INFO, 1, fmt, &n);
    printf("returned, n=%d\n", n);
    drain("vchk flag 1, writable format with %n");
}

static void chk_masked_child(void)
{
    static char fmt[] = "masked %n done";
    int n = -1;
    openlog("chk", 0, 0);
    setlogmask(LOG_MASK(LOG_ERR));
    __syslog_chk(LOG_INFO, 1, fmt, &n);
    printf("returned, n=%d\n", n);
    drain("chk flag 1, writable %n, masked");
}

static void chk_plain_n_child(void)
{
    int n = -1;
    openlog("chk", 0, 0);
    syslog(LOG_INFO, "plain %n done", &n);
    printf("returned, n=%d\n", n);
    drain("syslog with %n");
}

static void s_chk(void)
{
    printf("== __syslog_chk and __vsyslog_chk\n");
    openlog("chk", LOG_PID, LOG_LOCAL4);
    errno = EACCES;
    __syslog_chk(LOG_ERR, 1, "chk %d %s %m", 5, "s");
    drain("__syslog_chk flag 1");
    errno = EACCES;
    vchk(LOG_ERR, 1, "vchk %d %s %m", 6, "t");
    drain("__vsyslog_chk flag 1");
    __syslog_chk(LOG_ERR, -1, "flag -1");
    drain("__syslog_chk flag -1");
    closelog();
    run_child("chk read-only %n", chk_ro_child, 5);
    run_child("chk writable %n", chk_rw_child, 5);
    run_child("chk flag 0 writable %n", chk_rw0_child, 5);
    run_child("vchk writable %n", vchk_rw_child, 5);
    run_child("chk masked writable %n", chk_masked_child, 5);
    run_child("syslog %n", chk_plain_n_child, 5);
    drain("parent after the children");
}

int main(void)
{
    tw_alarm(120);
    struct stat st;
    if (stat(MARKER, &st) != 0) {
        printf("refusing to run: %s is missing (run through syslog_run.sh)\n", MARKER);
        return 2;
    }
    setvbuf(stdout, NULL, _IOFBF, 1 << 16);
    prog = program_invocation_short_name;
    mypid = getpid();
    umask(022);
    rx_bind(SOCK_DGRAM);
    console_reset();
    printf("running as root: %s\n", geteuid() == 0 ? "yes" : "no");
    s_constants();
    s_default();
    s_priorities();
    s_facility();
    s_ident();
    s_options();
    s_perror();
    s_mask();
    s_format();
    s_long();
    s_time();
    s_fork();
    s_chk();
    s_full();
    s_gone();
    s_stream();
    s_absent();
    s_fifo_console();
    printf("done\n");
    rx_close();
    unlink(LOGPATH);
    unlink(CONSOLE);
    return 0;
}
