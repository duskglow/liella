/* Terminals, job control and pseudo-terminals end to end (spec §19.1-19.5; phase 11, D-19.1 to
 * D-19.3): built against the host C library and against Liella's products, the outputs must
 * match line for line. Black-box: every line is a result of a public function, an errno, or a
 * relative fact (a pty is shown as /dev/pts/<n>, process ids only as comparisons).
 *
 * Usage: terminals_diff DIR [ns]. DIR is a scratch directory. Without `ns`, the program runs the
 * unprivileged cases on ptys it creates itself (job control in children that start their own
 * session, so the caller's terminal is never touched) and writes the fixture DIR/etc/ttys. With
 * `ns`, it expects to run as root in a private mount namespace with DIR/etc bind-mounted over
 * /etc (terminals_run.sh does this) and runs the ttyent cases and the ttyname cases that need
 * /dev/pts or /proc replaced (mount(2), only inside that namespace).
 *
 * Termios values are shown as hex of the four flag fields, speeds by B* name, and the first 17
 * control characters; never the raw structure. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <pty.h>
#include <sgtty.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <termios.h>
#include <ttyent.h>
#include <unistd.h>
#include <utmp.h>
#include "tscale.h"

/* BOTHER is not in <termios.h> on glibc 2.39; this is the Linux value (<asm/termbits.h>). */
#define K_BOTHER 010000

static char dir[512];
static int out_fd = 1;

static void say(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vdprintf(out_fd, fmt, ap);
    va_end(ap);
}

static const char *en(int e)
{
    static char b[4][24];
    static int k;
    switch (e) {
    case 0: return "0";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case ESRCH: return "ESRCH";
    case EINTR: return "EINTR";
    case EIO: return "EIO";
    case ENXIO: return "ENXIO";
    case EBADF: return "EBADF";
    case EAGAIN: return "EAGAIN";
    case EACCES: return "EACCES";
    case EFAULT: return "EFAULT";
    case EBUSY: return "EBUSY";
    case EEXIST: return "EEXIST";
    case ENODEV: return "ENODEV";
    case ENOTDIR: return "ENOTDIR";
    case EINVAL: return "EINVAL";
    case ENOTTY: return "ENOTTY";
    case ERANGE: return "ERANGE";
    case ENOSYS: return "ENOSYS";
    case EOPNOTSUPP: return "EOPNOTSUPP";
    }
    k = (k + 1) % 4;
    snprintf(b[k], sizeof b[k], "errno%d", e);
    return b[k];
}

#define R(name, expr)                                          \
    do {                                                       \
        errno = 0;                                             \
        long r_ = (long)(expr);                                \
        int e_ = errno;                                        \
        say("%s: %ld errno=%s\n", name, r_, en(e_));           \
    } while (0)

static const char *path(const char *name)
{
    static char p[4][768];
    static int k;
    k = (k + 1) % 4;
    snprintf(p[k], sizeof p[k], "%s/%s", dir, name);
    return p[k];
}

/* A pty name with its number hidden: "/dev/pts/<n>" when it is one, else the text itself. */
static const char *pname(const char *s)
{
    static char b[4][256];
    static int k;
    if (!s)
        return "NULL";
    k = (k + 1) % 4;
    if (strncmp(s, "/dev/pts/", 9) == 0 && s[9] >= '0' && s[9] <= '9' && strspn(s + 9, "0123456789") == strlen(s + 9))
        snprintf(b[k], sizeof b[k], "/dev/pts/<n>");
    else
        snprintf(b[k], sizeof b[k], "%s", s);
    return b[k];
}

static const char *ns(const char *s) { return s ? s : "NULL"; }

/* ------------------------------------------------------------------ speeds and flags */

struct sp {
    const char *name;
    speed_t v;
};
static const struct sp SPEEDS[] = {
    {"B0", B0},           {"B50", B50},           {"B75", B75},           {"B110", B110},         {"B134", B134},
    {"B150", B150},       {"B200", B200},         {"B300", B300},         {"B600", B600},         {"B1200", B1200},
    {"B1800", B1800},     {"B2400", B2400},       {"B4800", B4800},       {"B9600", B9600},       {"B19200", B19200},
    {"B38400", B38400},   {"B57600", B57600},     {"B115200", B115200},   {"B230400", B230400},   {"B460800", B460800},
    {"B500000", B500000}, {"B576000", B576000},   {"B921600", B921600},   {"B1000000", B1000000}, {"B1152000", B1152000},
    {"B1500000", B1500000}, {"B2000000", B2000000}, {"B2500000", B2500000}, {"B3000000", B3000000}, {"B3500000", B3500000},
    {"B4000000", B4000000},
};
#define NSPEEDS (sizeof SPEEDS / sizeof SPEEDS[0])

static const char *spn(speed_t v)
{
    static char b[4][24];
    static int k;
    for (size_t i = 0; i < NSPEEDS; i++)
        if (SPEEDS[i].v == v)
            return SPEEDS[i].name;
    k = (k + 1) % 4;
    snprintf(b[k], sizeof b[k], "%#x", (unsigned)v);
    return b[k];
}

static void show_t(const char *what, const struct termios *t)
{
    say("%s: iflag=%#x oflag=%#x cflag=%#x lflag=%#x ispeed=%s ospeed=%s cc=", what, (unsigned)t->c_iflag, (unsigned)t->c_oflag, (unsigned)t->c_cflag,
        (unsigned)t->c_lflag, spn(cfgetispeed(t)), spn(cfgetospeed(t)));
    for (int i = 0; i < 17; i++)
        say("%s%02x", i ? "," : "", (unsigned char)t->c_cc[i]);
    say("\n");
}

static int same_t(const struct termios *a, const struct termios *b)
{
    return a->c_iflag == b->c_iflag && a->c_oflag == b->c_oflag && a->c_cflag == b->c_cflag && a->c_lflag == b->c_lflag && memcmp(a->c_cc, b->c_cc, 17) == 0 &&
           cfgetispeed(a) == cfgetispeed(b) && cfgetospeed(a) == cfgetospeed(b);
}

static void base_t(struct termios *t)
{
    memset(t, 0, sizeof *t);
    t->c_cflag = CS8 | CREAD;
}

/* ------------------------------------------------------------------ helpers */

static int pair(int *m, int *s)
{
    *m = posix_openpt(O_RDWR | O_NOCTTY);
    if (*m < 0)
        return -1;
    if (grantpt(*m) != 0 || unlockpt(*m) != 0)
        return -1;
    char *n = ptsname(*m);
    if (!n)
        return -1;
    *s = open(n, O_RDWR | O_NOCTTY);
    return *s < 0 ? -1 : 0;
}

static int readable(int fd, int ms)
{
    struct pollfd p = {fd, POLLIN, 0};
    return poll(&p, 1, ms) == 1 && (p.revents & POLLIN);
}

/* Read what is available without blocking; returns the count, bytes in out (escaped). */
static int take(int fd, char *out, size_t outsz)
{
    int fl = fcntl(fd, F_GETFL);
    fcntl(fd, F_SETFL, fl | O_NONBLOCK);
    unsigned char b[256];
    int total = 0;
    size_t o = 0;
    if (out && outsz)
        out[0] = 0;
    for (;;) {
        ssize_t n = read(fd, b, sizeof b);
        if (n <= 0)
            break;
        for (ssize_t i = 0; i < n; i++) {
            if (out && o + 5 < outsz) {
                if (b[i] >= 0x20 && b[i] < 0x7f && b[i] != '\\')
                    out[o++] = (char)b[i];
                else
                    o += (size_t)snprintf(out + o, outsz - o, "\\x%02x", b[i]);
                out[o] = 0;
            }
        }
        total += (int)n;
        if (!readable(fd, 50))
            break;
    }
    fcntl(fd, F_SETFL, fl);
    return total;
}

static void sleep_ms(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static void raw(int fd)
{
    struct termios t;
    tcgetattr(fd, &t);
    cfmakeraw(&t);
    tcsetattr(fd, TCSANOW, &t);
}

static int count_fds(void)
{
    int n = 0;
    for (int fd = 0; fd < 1024; fd++)
        if (fcntl(fd, F_GETFD) != -1)
            n++;
    return n;
}

static const char *signame(int s)
{
    static char b[16];
    switch (s) {
    case SIGTTOU: return "SIGTTOU";
    case SIGTTIN: return "SIGTTIN";
    case SIGHUP: return "SIGHUP";
    case SIGSEGV: return "SIGSEGV";
    case SIGBUS: return "SIGBUS";
    case SIGKILL: return "SIGKILL";
    case SIGALRM: return "SIGALRM";
    case SIGABRT: return "SIGABRT";
    case SIGSTOP: return "SIGSTOP";
    case SIGTSTP: return "SIGTSTP";
    }
    snprintf(b, sizeof b, "sig%d", s);
    return b;
}

static const char *wstat(int st)
{
    static char b[64];
    if (WIFEXITED(st))
        snprintf(b, sizeof b, "exit %d", WEXITSTATUS(st));
    else if (WIFSIGNALED(st))
        snprintf(b, sizeof b, "killed by %s", signame(WTERMSIG(st)));
    else if (WIFSTOPPED(st))
        snprintf(b, sizeof b, "stopped by %s", signame(WSTOPSIG(st)));
    else
        snprintf(b, sizeof b, "status %#x", st);
    return b;
}

/* Run fn in a child whose output goes through a pipe (fd 100 in the child), so that a child that
 * redirects 0-2 or crashes still reports in order. Prints the child's status unless it is exit 0. */
static void in_child(const char *label, void (*fn)(void))
{
    int p[2];
    if (pipe(p) != 0)
        return;
    pid_t pid = fork();
    if (pid == 0) {
        close(p[0]);
        dup2(p[1], 100);
        close(p[1]);
        out_fd = 100;
        tw_alarm(20);
        fn();
        _exit(0);
    }
    close(p[1]);
    char buf[4096];
    for (;;) {
        if (!readable(p[0], 30000))
            break;
        ssize_t n = read(p[0], buf, sizeof buf);
        if (n <= 0)
            break;
        write(out_fd, buf, (size_t)n);
    }
    close(p[0]);
    int st = 0;
    waitpid(pid, &st, 0);
    if (!(WIFEXITED(st) && WEXITSTATUS(st) == 0))
        say("%s: child %s\n", label, wstat(st));
}

/* ------------------------------------------------------------------ §19.1 speeds */

static void speeds(void)
{
    struct termios t;
    for (size_t i = 0; i < NSPEEDS; i++) {
        speed_t v = SPEEDS[i].v;
        base_t(&t);
        errno = 0;
        int r = cfsetospeed(&t, v);
        int e = errno;
        say("cfsetospeed %s(%#x): %d errno=%s ospeed=%s ispeed=%s cflag=%#x\n", SPEEDS[i].name, (unsigned)v, r, en(e), spn(cfgetospeed(&t)), spn(cfgetispeed(&t)),
            (unsigned)t.c_cflag);
        base_t(&t);
        errno = 0;
        r = cfsetispeed(&t, v);
        e = errno;
        say("cfsetispeed %s: %d errno=%s ospeed=%s ispeed=%s cflag=%#x\n", SPEEDS[i].name, r, en(e), spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
        base_t(&t);
        errno = 0;
        r = cfsetspeed(&t, v);
        e = errno;
        say("cfsetspeed %s: %d errno=%s ospeed=%s ispeed=%s cflag=%#x\n", SPEEDS[i].name, r, en(e), spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    }
    /* Numeric rates and values that are not B* codes. */
    static const speed_t odd[] = {50, 75, 110, 300, 1200, 9600, 19200, 38400, 57600, 115200, 4000000, 12345, 16, 17, 0x100f, 0x1010, 0x1020, K_BOTHER,
                                  0x2000, 0x10000, 0x100000, 0xffffffffu};
    for (size_t i = 0; i < sizeof odd / sizeof odd[0]; i++) {
        speed_t v = odd[i];
        const char *fn[3] = {"cfsetospeed", "cfsetispeed", "cfsetspeed"};
        for (int f = 0; f < 3; f++) {
            base_t(&t);
            cfsetspeed(&t, B2400);
            errno = 0;
            int r = f == 0 ? cfsetospeed(&t, v) : f == 1 ? cfsetispeed(&t, v) : cfsetspeed(&t, v);
            int e = errno;
            say("%s %u (%#x): %d errno=%s ospeed=%s ispeed=%s cflag=%#x\n", fn[f], (unsigned)v, (unsigned)v, r, en(e), spn(cfgetospeed(&t)), spn(cfgetispeed(&t)),
                (unsigned)t.c_cflag);
        }
    }
    /* Separate input and output speeds, and the input speed 0 rule. */
    base_t(&t);
    cfsetospeed(&t, B9600);
    say("ospeed B9600 only: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    cfsetispeed(&t, B2400);
    say("then ispeed B2400: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    R("then cfsetispeed(0)", cfsetispeed(&t, 0));
    say("after ispeed 0: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    cfsetospeed(&t, B1200);
    say("then ospeed B1200: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    base_t(&t);
    cfsetispeed(&t, B2400);
    cfsetospeed(&t, B9600);
    say("ispeed B2400 then ospeed B9600: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)), (unsigned)t.c_cflag);
    /* What the getters read from a structure whose c_cflag was written directly. */
    memset(&t, 0, sizeof t);
    t.c_cflag = B9600 | CS8;
    say("direct cflag B9600: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
    memset(&t, 0, sizeof t);
    t.c_cflag = B9600 | ((tcflag_t)B2400 << 16) | CS8;
    say("direct cflag B9600 CIBAUD=B2400: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
    memset(&t, 0, sizeof t);
    t.c_cflag = 0xffffffffu;
    say("direct cflag all ones: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
    memset(&t, 0, sizeof t);
    t.c_cflag = K_BOTHER;
    say("direct cflag BOTHER: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
}

/* ------------------------------------------------------------------ §19.1 cfmakeraw */

static void makeraw(void)
{
    struct termios t;
    memset(&t, 0, sizeof t);
    t.c_iflag = t.c_oflag = t.c_lflag = 0xffffffffu;
    t.c_cflag = 0xffffffffu;
    memset(t.c_cc, 0xaa, sizeof t.c_cc);
    cfmakeraw(&t);
    int changed = 0;
    for (int i = 0; i < NCCS; i++)
        if (i != VMIN && i != VTIME && (unsigned char)t.c_cc[i] != 0xaa)
            changed++;
    say("cfmakeraw all ones: iflag=%#x oflag=%#x cflag=%#x lflag=%#x VMIN=%u VTIME=%u other_cc_changed=%d\n", (unsigned)t.c_iflag, (unsigned)t.c_oflag, (unsigned)t.c_cflag,
        (unsigned)t.c_lflag, (unsigned char)t.c_cc[VMIN], (unsigned char)t.c_cc[VTIME], changed);
    memset(&t, 0, sizeof t);
    t.c_cflag = B9600;
    cfmakeraw(&t);
    say("cfmakeraw zero: iflag=%#x oflag=%#x cflag=%#x lflag=%#x VMIN=%u VTIME=%u ospeed=%s ispeed=%s\n", (unsigned)t.c_iflag, (unsigned)t.c_oflag, (unsigned)t.c_cflag,
        (unsigned)t.c_lflag, (unsigned char)t.c_cc[VMIN], (unsigned char)t.c_cc[VTIME], spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
    memset(&t, 0, sizeof t);
    cfsetospeed(&t, B115200);
    cfsetispeed(&t, B300);
    cfmakeraw(&t);
    say("cfmakeraw keeps speeds: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&t)), spn(cfgetispeed(&t)));
    /* The flags cfmakeraw clears, by name. */
    struct {
        const char *n;
        tcflag_t f;
        int field;
    } fl[] = {
        {"IGNBRK", IGNBRK, 0}, {"BRKINT", BRKINT, 0}, {"IGNPAR", IGNPAR, 0}, {"PARMRK", PARMRK, 0}, {"INPCK", INPCK, 0}, {"ISTRIP", ISTRIP, 0},
        {"INLCR", INLCR, 0},   {"IGNCR", IGNCR, 0},   {"ICRNL", ICRNL, 0},   {"IUCLC", IUCLC, 0},   {"IXON", IXON, 0},   {"IXANY", IXANY, 0},
        {"IXOFF", IXOFF, 0},   {"IMAXBEL", IMAXBEL, 0}, {"IUTF8", IUTF8, 0}, {"OPOST", OPOST, 1}, {"ONLCR", ONLCR, 1}, {"CSIZE", CSIZE, 2},
        {"CSTOPB", CSTOPB, 2}, {"CREAD", CREAD, 2},   {"PARENB", PARENB, 2}, {"PARODD", PARODD, 2}, {"HUPCL", HUPCL, 2}, {"CLOCAL", CLOCAL, 2},
        {"CRTSCTS", CRTSCTS, 2}, {"ISIG", ISIG, 3},   {"ICANON", ICANON, 3}, {"ECHO", ECHO, 3},   {"ECHOE", ECHOE, 3}, {"ECHOK", ECHOK, 3},
        {"ECHONL", ECHONL, 3}, {"NOFLSH", NOFLSH, 3}, {"TOSTOP", TOSTOP, 3}, {"ECHOCTL", ECHOCTL, 3}, {"ECHOKE", ECHOKE, 3}, {"IEXTEN", IEXTEN, 3},
    };
    memset(&t, 0, sizeof t);
    t.c_iflag = t.c_oflag = t.c_lflag = 0xffffffffu;
    t.c_cflag = 0xffffffffu & ~(tcflag_t)(CSIZE);
    cfmakeraw(&t);
    say("cfmakeraw clears:");
    for (size_t i = 0; i < sizeof fl / sizeof fl[0]; i++) {
        tcflag_t v = fl[i].field == 0 ? t.c_iflag : fl[i].field == 1 ? t.c_oflag : fl[i].field == 2 ? t.c_cflag : t.c_lflag;
        if (fl[i].f == CSIZE)
            say(" CSIZE=%s", (v & CSIZE) == CS8 ? "CS8" : "other");
        else if ((v & fl[i].f) == 0)
            say(" %s", fl[i].n);
    }
    say("\n");
}

/* ------------------------------------------------------------------ §19.1 tcgetattr/tcsetattr */

static void attrs(void)
{
    int m, s;
    if (pair(&m, &s) != 0) {
        say("attrs: no pty\n");
        return;
    }
    struct termios t, u, orig;
    R("tcgetattr slave", tcgetattr(s, &orig));
    show_t("slave default", &orig);
    R("tcgetattr master", tcgetattr(m, &u));
    say("master attrs equal slave: %d\n", same_t(&u, &orig));
    say("c_line default: %u\n", (unsigned)orig.c_line);

    /* tcgetattr into a structure prefilled with 0x77: what the bytes past the kernel's control
     * characters hold afterwards. */
    memset(&u, 0x77, sizeof u);
    tcgetattr(s, &u);
    say("tcgetattr prefilled: cc[17..31]=");
    for (int i = 17; i < NCCS; i++)
        say("%s%02x", i > 17 ? "," : "", (unsigned char)u.c_cc[i]);
    say("\n");

    int dn = open("/dev/null", O_RDWR);
    int fd = open(path("plain"), O_RDWR | O_CREAT | O_TRUNC, 0600);
    int pp[2];
    pipe(pp);
    R("tcgetattr /dev/null", tcgetattr(dn, &u));
    R("tcgetattr regular file", tcgetattr(fd, &u));
    R("tcgetattr pipe", tcgetattr(pp[0], &u));
    R("tcgetattr fd -1", tcgetattr(-1, &u));
    R("tcgetattr closed fd 900", tcgetattr(900, &u));
    R("tcsetattr /dev/null", tcsetattr(dn, TCSANOW, &orig));
    R("tcsetattr fd -1", tcsetattr(-1, TCSANOW, &orig));
    R("tcsetattr /dev/null bad action", tcsetattr(dn, 99, &orig));
    R("tcsetattr fd -1 bad action", tcsetattr(-1, 99, &orig));

    say("TCSANOW=%d TCSADRAIN=%d TCSAFLUSH=%d\n", TCSANOW, TCSADRAIN, TCSAFLUSH);
#ifdef TCSASOFT
    say("TCSASOFT defined: %d\n", TCSASOFT);
#else
    say("TCSASOFT not defined\n");
#endif
    const int acts[] = {TCSANOW, TCSADRAIN, TCSAFLUSH};
    const char *an[] = {"TCSANOW", "TCSADRAIN", "TCSAFLUSH"};
    for (int i = 0; i < 3; i++) {
        t = orig;
        t.c_lflag ^= ECHO;
        t.c_cc[VMIN] = (cc_t)(3 + i);
        char nm[64];
        snprintf(nm, sizeof nm, "tcsetattr %s", an[i]);
        R(nm, tcsetattr(s, acts[i], &t));
        tcgetattr(s, &u);
        say("%s applied: %d\n", an[i], same_t(&t, &u));
        tcsetattr(s, TCSANOW, &orig);
    }
    static const int bad[] = {3, 4, -1, 0x10, 0x7fffffff, TCSANOW | 0x100};
    for (size_t i = 0; i < sizeof bad / sizeof bad[0]; i++) {
        t = orig;
        t.c_lflag ^= ECHO;
        char nm[64];
        snprintf(nm, sizeof nm, "tcsetattr action %#x", (unsigned)bad[i]);
        R(nm, tcsetattr(s, bad[i], &t));
        tcgetattr(s, &u);
        say("action %#x left settings unchanged: %d\n", (unsigned)bad[i], same_t(&orig, &u));
    }

    /* Pending input across the three actions (raw mode so one byte is readable). */
    for (int i = 0; i < 3; i++) {
        raw(s);
        tcgetattr(s, &t);
        write(m, "abc", 3);
        int before = readable(s, 1000);
        tcsetattr(s, acts[i], &t);
        int after = readable(s, 200);
        say("%s pending input: before=%d after=%d\n", an[i], before, after);
        take(s, NULL, 0);
        take(m, NULL, 0);
    }
    tcsetattr(s, TCSANOW, &orig);

    /* Partial application: every flag bit requested; tcsetattr reports success, and the bits
     * that did not take effect show on re-reading. */
    t = orig;
    t.c_iflag = 0xffffffffu;
    t.c_oflag = 0xffffffffu;
    t.c_lflag = 0xffffffffu;
    t.c_cflag = (0xffffffffu & ~(tcflag_t)(CBAUD | CIBAUD)) | B9600;
    R("tcsetattr all flag bits", tcsetattr(s, TCSANOW, &t));
    tcgetattr(s, &u);
    say("all flag bits read back: iflag=%#x oflag=%#x cflag=%#x lflag=%#x ospeed=%s ispeed=%s\n", (unsigned)u.c_iflag, (unsigned)u.c_oflag, (unsigned)u.c_cflag,
        (unsigned)u.c_lflag, spn(cfgetospeed(&u)), spn(cfgetispeed(&u)));
    tcsetattr(s, TCSANOW, &orig);

    /* Character size and parity: requested, reported as success, and read back. */
    t = orig;
    t.c_cflag = (t.c_cflag & ~(tcflag_t)(CSIZE | PARENB | PARODD | CSTOPB)) | CS7 | PARENB | PARODD | CSTOPB;
    t.c_lflag ^= ECHO;
    R("tcsetattr CS7 PARENB PARODD CSTOPB, ECHO toggled", tcsetattr(s, TCSANOW, &t));
    tcgetattr(s, &u);
    say("read back: CSIZE=%s PARENB=%d PARODD=%d CSTOPB=%d ECHO toggled=%d\n", (u.c_cflag & CSIZE) == CS8 ? "CS8" : (u.c_cflag & CSIZE) == CS7 ? "CS7" : "other",
        !!(u.c_cflag & PARENB), !!(u.c_cflag & PARODD), !!(u.c_cflag & CSTOPB), (u.c_lflag & ECHO) != (orig.c_lflag & ECHO));
    tcsetattr(s, TCSANOW, &orig);

    /* Control characters, including those past the kernel's. */
    t = orig;
    t.c_cc[VINTR] = 1;
    t.c_cc[VMIN] = 5;
    t.c_cc[VTIME] = 7;
    t.c_cc[VEOL2] = 0x42;
    for (int i = 17; i < NCCS; i++)
        t.c_cc[i] = 0x55;
    R("tcsetattr cc", tcsetattr(s, TCSANOW, &t));
    memset(&u, 0, sizeof u);
    tcgetattr(s, &u);
    say("cc read back: VINTR=%u VMIN=%u VTIME=%u VEOL2=%#x cc[17..31]=", (unsigned char)u.c_cc[VINTR], (unsigned char)u.c_cc[VMIN], (unsigned char)u.c_cc[VTIME],
        (unsigned char)u.c_cc[VEOL2]);
    for (int i = 17; i < NCCS; i++)
        say("%s%02x", i > 17 ? "," : "", (unsigned char)u.c_cc[i]);
    say("\n");
    tcsetattr(s, TCSANOW, &orig);

    /* Every speed through the terminal. */
    for (size_t i = 1; i < NSPEEDS; i++) {
        t = orig;
        cfsetspeed(&t, SPEEDS[i].v);
        errno = 0;
        int r = tcsetattr(s, TCSANOW, &t);
        int e = errno;
        tcgetattr(s, &u);
        say("pty speed %s: %d errno=%s read back ospeed=%s ispeed=%s cflag=%#x\n", SPEEDS[i].name, r, en(e), spn(cfgetospeed(&u)), spn(cfgetispeed(&u)), (unsigned)u.c_cflag);
    }
    t = orig;
    cfsetospeed(&t, B9600);
    cfsetispeed(&t, B2400);
    R("tcsetattr ospeed B9600 ispeed B2400", tcsetattr(s, TCSANOW, &t));
    tcgetattr(s, &u);
    say("split speeds read back: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&u)), spn(cfgetispeed(&u)), (unsigned)u.c_cflag);
    t = orig;
    cfsetospeed(&t, B19200);
    cfsetispeed(&t, 0);
    R("tcsetattr ospeed B19200 ispeed 0", tcsetattr(s, TCSANOW, &t));
    tcgetattr(s, &u);
    say("ispeed 0 read back: ospeed=%s ispeed=%s cflag=%#x\n", spn(cfgetospeed(&u)), spn(cfgetispeed(&u)), (unsigned)u.c_cflag);
    t = orig;
    errno = 0;
    int r = cfsetospeed(&t, K_BOTHER);
    say("cfsetospeed BOTHER for the pty: %d errno=%s\n", r, en(errno));
    if (r == 0) {
        R("tcsetattr after cfsetospeed BOTHER", tcsetattr(s, TCSANOW, &t));
        tcgetattr(s, &u);
        say("BOTHER read back: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&u)), spn(cfgetispeed(&u)));
    }
    t = orig;
    t.c_cflag = (t.c_cflag & ~(tcflag_t)CBAUD) | K_BOTHER;
    R("tcsetattr direct cflag BOTHER", tcsetattr(s, TCSANOW, &t));
    tcgetattr(s, &u);
    say("direct BOTHER read back: ospeed=%s ispeed=%s\n", spn(cfgetospeed(&u)), spn(cfgetispeed(&u)));
    tcsetattr(s, TCSANOW, &orig);

    /* Settings written through the master are the slave's. */
    t = orig;
    t.c_lflag ^= ICANON;
    R("tcsetattr master", tcsetattr(m, TCSANOW, &t));
    tcgetattr(s, &u);
    say("master write seen on slave: %d\n", same_t(&t, &u));
    tcsetattr(s, TCSANOW, &orig);

    /* B0 (hang up) last, on this pair only. */
    t = orig;
    cfsetspeed(&t, B0);
    R("tcsetattr B0", tcsetattr(s, TCSANOW, &t));
    errno = 0;
    r = tcgetattr(s, &u);
    say("after B0: tcgetattr %d errno=%s ospeed=%s ispeed=%s\n", r, en(errno), spn(cfgetospeed(&u)), spn(cfgetispeed(&u)));
    errno = 0;
    ssize_t w = write(s, "x", 1);
    say("after B0: write slave %ld errno=%s\n", (long)w, en(errno));
    close(m);
    close(s);
    close(dn);
    close(fd);
    close(pp[0]);
    close(pp[1]);
}

/* ------------------------------------------------------------------ §19.1 line control */

static void control(void)
{
    int m, s;
    if (pair(&m, &s) != 0) {
        say("control: no pty\n");
        return;
    }
    int dn = open("/dev/null", O_RDWR);
    raw(s);
    R("tcsendbreak slave 0", tcsendbreak(s, 0));
    R("tcsendbreak slave 10", tcsendbreak(s, 10));
    R("tcsendbreak slave -1", tcsendbreak(s, -1));
    R("tcsendbreak master 0", tcsendbreak(m, 0));
    R("tcsendbreak /dev/null", tcsendbreak(dn, 0));
    R("tcsendbreak fd -1", tcsendbreak(-1, 0));
    char got[256];
    int n = take(m, got, sizeof got);
    say("master bytes after tcsendbreak: %d\n", n);
    n = take(s, got, sizeof got);
    say("slave bytes after tcsendbreak: %d\n", n);

    R("tcdrain slave", tcdrain(s));
    R("tcdrain master", tcdrain(m));
    R("tcdrain /dev/null", tcdrain(dn));
    R("tcdrain fd -1", tcdrain(-1));

    say("TCIFLUSH=%d TCOFLUSH=%d TCIOFLUSH=%d TCOOFF=%d TCOON=%d TCIOFF=%d TCION=%d\n", TCIFLUSH, TCOFLUSH, TCIOFLUSH, TCOOFF, TCOON, TCIOFF, TCION);
    R("tcflush slave TCIFLUSH", tcflush(s, TCIFLUSH));
    R("tcflush slave TCOFLUSH", tcflush(s, TCOFLUSH));
    R("tcflush slave TCIOFLUSH", tcflush(s, TCIOFLUSH));
    R("tcflush slave 3", tcflush(s, 3));
    R("tcflush slave -1", tcflush(s, -1));
    R("tcflush master TCIOFLUSH", tcflush(m, TCIOFLUSH));
    R("tcflush /dev/null", tcflush(dn, TCIFLUSH));
    R("tcflush /dev/null bad queue", tcflush(dn, 3));
    R("tcflush fd -1", tcflush(-1, TCIFLUSH));

    /* Effects. Each case waits until the data is readable before flushing. */
    struct {
        const char *what;
        int writer, flusher, q, reader;
    } fx[] = {
        {"input to slave, slave TCIFLUSH", 0, 1, TCIFLUSH, 1},
        {"input to slave, slave TCOFLUSH", 0, 1, TCOFLUSH, 1},
        {"input to slave, slave TCIOFLUSH", 0, 1, TCIOFLUSH, 1},
        {"output of slave, slave TCOFLUSH", 1, 1, TCOFLUSH, 0},
        {"output of slave, slave TCIFLUSH", 1, 1, TCIFLUSH, 0},
        {"output of slave, master TCIFLUSH", 1, 0, TCIFLUSH, 0},
        {"input to slave, master TCOFLUSH", 0, 0, TCOFLUSH, 1},
    };
    for (size_t i = 0; i < sizeof fx / sizeof fx[0]; i++) {
        int w = fx[i].writer ? s : m, f = fx[i].flusher ? s : m, rd = fx[i].reader ? s : m;
        write(w, "data", 4);
        int before = readable(rd, 1000);
        tcflush(f, fx[i].q);
        int after = readable(rd, 200);
        say("flush %s: readable before=%d after=%d\n", fx[i].what, before, after);
        take(s, NULL, 0);
        take(m, NULL, 0);
    }

    /* tcflow. */
    R("tcflow slave TCOOFF", tcflow(s, TCOOFF));
    int fl = fcntl(s, F_GETFL);
    fcntl(s, F_SETFL, fl | O_NONBLOCK);
    errno = 0;
    ssize_t w = write(s, "x", 1);
    say("write while TCOOFF: %ld errno=%s\n", (long)w, en(errno));
    say("master readable while TCOOFF: %d\n", readable(m, 200));
    R("tcflow slave TCOON", tcflow(s, TCOON));
    errno = 0;
    w = write(s, "y", 1);
    say("write after TCOON: %ld errno=%s\n", (long)w, en(errno));
    say("master readable after TCOON: %d\n", readable(m, 1000));
    n = take(m, got, sizeof got);
    say("master bytes after TCOON: %d [%s]\n", n, got);
    fcntl(s, F_SETFL, fl);
    R("tcflow slave TCOON again", tcflow(s, TCOON));
    R("tcflow slave TCIOFF", tcflow(s, TCIOFF));
    readable(m, 1000);
    n = take(m, got, sizeof got);
    say("master bytes after TCIOFF: %d [%s]\n", n, got);
    R("tcflow slave TCION", tcflow(s, TCION));
    readable(m, 1000);
    n = take(m, got, sizeof got);
    say("master bytes after TCION: %d [%s]\n", n, got);
    struct termios t;
    tcgetattr(s, &t);
    t.c_cc[VSTOP] = 0x01;
    t.c_cc[VSTART] = 0x02;
    tcsetattr(s, TCSANOW, &t);
    tcflow(s, TCIOFF);
    readable(m, 1000);
    n = take(m, got, sizeof got);
    say("TCIOFF with VSTOP=1: %d [%s]\n", n, got);
    tcflow(s, TCION);
    readable(m, 1000);
    n = take(m, got, sizeof got);
    say("TCION with VSTART=2: %d [%s]\n", n, got);
    t.c_cc[VSTOP] = _POSIX_VDISABLE;
    tcsetattr(s, TCSANOW, &t);
    tcflow(s, TCIOFF);
    n = readable(m, 200) ? take(m, got, sizeof got) : 0;
    say("TCIOFF with VSTOP disabled: %d\n", n);
    R("tcflow slave 4", tcflow(s, 4));
    R("tcflow slave -1", tcflow(s, -1));
    R("tcflow master TCOOFF", tcflow(m, TCOOFF));
    R("tcflow master TCOON", tcflow(m, TCOON));
    R("tcflow /dev/null", tcflow(dn, TCOON));
    R("tcflow /dev/null bad action", tcflow(dn, 4));
    R("tcflow fd -1", tcflow(-1, TCOON));
    close(m);
    close(s);
    close(dn);
}

/* ------------------------------------------------------------------ §19.1 isatty, §12.12 ctermid */

static void isatty_ctermid(void)
{
    int m, s;
    pair(&m, &s);
    int dn = open("/dev/null", O_RDWR);
    int fd = open(path("plain"), O_RDWR | O_CREAT, 0600);
    int dd = open(dir, O_RDONLY | O_DIRECTORY);
    int pp[2], sv[2];
    pipe(pp);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    R("isatty slave", isatty(s));
    R("isatty master", isatty(m));
    R("isatty /dev/null", isatty(dn));
    R("isatty regular file", isatty(fd));
    R("isatty directory", isatty(dd));
    R("isatty pipe", isatty(pp[0]));
    R("isatty socket", isatty(sv[0]));
    R("isatty fd -1", isatty(-1));
    R("isatty closed fd 900", isatty(900));
    errno = 1234;
    int r = isatty(s);
    say("isatty slave keeps errno: %d errno=%d\n", r, errno);

    say("L_ctermid=%d\n", L_ctermid);
    char *a = ctermid(NULL), *b = ctermid(NULL);
    say("ctermid(NULL): [%s] same pointer twice=%d\n", ns(a), a == b);
    char buf[L_ctermid + 8];
    memset(buf, 'x', sizeof buf);
    char *c = ctermid(buf);
    say("ctermid(buf): [%s] returns buf=%d\n", ns(c), c == buf);
    close(m);
    close(s);
    close(dn);
    close(fd);
    close(dd);
}

/* ------------------------------------------------------------------ §19.3 ttyname */

/* NULL buffers are outside the headers' contract (the parameters are declared nonnull); the
 * calls go through volatile pointers so that the compiler passes them as written, in a child. */
static int (*volatile v_ttyname_r)(int, char *, size_t) = ttyname_r;
static int (*volatile v_ptsname_r)(int, char *, size_t) = ptsname_r;
static char *volatile v_null;

static void ttyname_null_buf(void)
{
    int m, s;
    pair(&m, &s);
    R("ttyname_r(slave, NULL, 64)", v_ttyname_r(s, v_null, 64));
    R("ttyname_r(slave, NULL, 0)", v_ttyname_r(s, v_null, 0));
}

static void ptsname_null_buf(void)
{
    int m = posix_openpt(O_RDWR | O_NOCTTY);
    int r = v_ptsname_r(m, v_null, 64);
    say("ptsname_r NULL buffer: %d (%s) errno=%s\n", r, en(r), en(errno));
}

static void names(void)
{
    int m, s;
    pair(&m, &s);
    int dn = open("/dev/null", O_RDWR);
    char *pn = ptsname(m);
    char sname[128];
    snprintf(sname, sizeof sname, "%s", pn ? pn : "");
    char *a = ttyname(s);
    say("ttyname slave: [%s] equals ptsname=%d\n", pname(a), a && strcmp(a, sname) == 0);
    char *b = ttyname(m);
    say("ttyname master: [%s] same static buffer=%d\n", pname(b), a == b);
    errno = 0;
    char *c = ttyname(dn);
    say("ttyname /dev/null: %s errno=%s\n", ns(c), en(errno));
    errno = 0;
    c = ttyname(-1);
    say("ttyname fd -1: %s errno=%s\n", ns(c), en(errno));
    errno = 0;
    c = ttyname(900);
    say("ttyname closed fd 900: %s errno=%s\n", ns(c), en(errno));
    int pp[2];
    pipe(pp);
    errno = 0;
    c = ttyname(pp[0]);
    say("ttyname pipe: %s errno=%s\n", ns(c), en(errno));
    close(pp[0]);
    close(pp[1]);
    int fd = open(path("plain"), O_RDWR | O_CREAT, 0600);
    errno = 0;
    c = ttyname(fd);
    say("ttyname regular file: %s errno=%s\n", ns(c), en(errno));
    close(fd);
    errno = 0;
    c = ttyname(s);
    say("ttyname after a failure: [%s] errno=%s\n", pname(c), en(errno));

    size_t len = strlen(sname);
    say("slave name length: %s\n", len >= 10 && len <= 20 ? "10..20" : "other");
    char buf[256];
    size_t lens[] = {0, 1, len - 1, len, len + 1, sizeof buf};
    const char *ln[] = {"0", "1", "len-1", "len", "len+1", "256"};
    for (int i = 0; i < 6; i++) {
        memset(buf, 'z', sizeof buf);
        errno = 0;
        int r = ttyname_r(s, buf, lens[i]);
        int e = errno;
        say("ttyname_r size %s: %d (%s) errno=%s buf=[%s]\n", ln[i], r, en(r), en(e), r == 0 ? pname(buf) : buf[0] == 'z' ? "untouched" : "written");
    }
    errno = 0;
    int r = ttyname_r(dn, buf, sizeof buf);
    say("ttyname_r /dev/null: %d (%s) errno=%s\n", r, en(r), en(errno));
    errno = 0;
    r = ttyname_r(-1, buf, sizeof buf);
    say("ttyname_r fd -1: %d (%s) errno=%s\n", r, en(r), en(errno));
    errno = 0;
    r = ttyname_r(dn, buf, 0);
    say("ttyname_r /dev/null size 0: %d (%s) errno=%s\n", r, en(r), en(errno));
    in_child("ttyname_r NULL buffer", ttyname_null_buf);

    /* Opened through a symbolic link: the name is the device's, not the link's. */
    char lk[600];
    snprintf(lk, sizeof lk, "%s", path("ptslink"));
    unlink(lk);
    symlink(sname, lk);
    int s2 = open(lk, O_RDWR | O_NOCTTY);
    c = ttyname(s2);
    say("ttyname via symlink: [%s] equals ptsname=%d\n", pname(c), c && strcmp(c, sname) == 0);
    close(s2);
    unlink(lk);
    /* A second slave descriptor, and the slave opened through /dev/pts/ptmx is not tried. */
    int s3 = dup(s);
    c = ttyname(s3);
    say("ttyname dup of slave: equals ptsname=%d\n", c && strcmp(c, sname) == 0);
    close(s3);
    close(m);
    close(s);
    close(dn);
}

/* ------------------------------------------------------------------ §19.4 the four-step API */

static void fdinfo(const char *what, int fd)
{
    if (fd < 0)
        return;
    int fl = fcntl(fd, F_GETFL), fdf = fcntl(fd, F_GETFD);
    const char *acc = (fl & O_ACCMODE) == O_RDWR ? "O_RDWR" : (fl & O_ACCMODE) == O_RDONLY ? "O_RDONLY" : (fl & O_ACCMODE) == O_WRONLY ? "O_WRONLY" : "other";
    say("%s: %s nonblock=%d append=%d cloexec=%d isatty=%d ttyname=[%s]\n", what, acc, !!(fl & O_NONBLOCK), !!(fl & O_APPEND), !!(fdf & FD_CLOEXEC), isatty(fd),
        pname(ttyname(fd)));
}

static void ptyapi(void)
{
    struct {
        const char *n;
        int f;
    } fl[] = {
        {"O_RDWR", O_RDWR},
        {"O_RDWR|O_NOCTTY", O_RDWR | O_NOCTTY},
        {"O_RDWR|O_CLOEXEC", O_RDWR | O_CLOEXEC},
        {"O_RDWR|O_NONBLOCK", O_RDWR | O_NONBLOCK},
        {"O_RDONLY", O_RDONLY},
        {"O_WRONLY", O_WRONLY},
        {"O_ACCMODE", O_ACCMODE},
        {"O_RDWR|O_APPEND", O_RDWR | O_APPEND},
        {"O_RDWR|O_CREAT", O_RDWR | O_CREAT},
        {"O_RDWR|O_EXCL", O_RDWR | O_EXCL},
        {"O_RDWR|O_TRUNC", O_RDWR | O_TRUNC},
        {"O_RDWR|O_DIRECTORY", O_RDWR | O_DIRECTORY},
        {"O_RDWR|O_PATH", O_RDWR | O_PATH},
        {"O_RDWR|0x40000000", O_RDWR | 0x40000000},
        {"-1", -1},
    };
    for (size_t i = 0; i < sizeof fl / sizeof fl[0]; i++) {
        errno = 0;
        int m = posix_openpt(fl[i].f);
        int e = errno;
        say("posix_openpt %s: %s errno=%s\n", fl[i].n, m >= 0 ? "fd" : "-1", en(e));
        char nm[64];
        snprintf(nm, sizeof nm, "  master %s", fl[i].n);
        fdinfo(nm, m);
        if (m >= 0)
            close(m);
    }
    errno = 0;
    int m = getpt();
    say("getpt: %s errno=%s\n", m >= 0 ? "fd" : "-1", en(errno));
    fdinfo("  getpt master", m);
    if (m >= 0)
        close(m);

    m = posix_openpt(O_RDWR | O_NOCTTY);
    char *pn = ptsname(m);
    char sname[128];
    snprintf(sname, sizeof sname, "%s", pn ? pn : "");
    say("ptsname master: [%s]\n", pname(pn));
    errno = 0;
    int s = open(sname, O_RDWR | O_NOCTTY);
    say("open slave before unlockpt: %s errno=%s\n", s >= 0 ? "fd" : "-1", en(errno));
    if (s >= 0)
        close(s);
    struct stat st;
    stat(sname, &st);
    say("slave before grantpt: mode=%o owner_is_me=%d gid=%u\n", (unsigned)(st.st_mode & 07777), st.st_uid == getuid(), (unsigned)st.st_gid);
    R("grantpt master", grantpt(m));
    stat(sname, &st);
    say("slave after grantpt: mode=%o owner_is_me=%d gid=%u\n", (unsigned)(st.st_mode & 07777), st.st_uid == getuid(), (unsigned)st.st_gid);
    R("unlockpt master", unlockpt(m));
    R("unlockpt master again", unlockpt(m));
    R("grantpt master again", grantpt(m));
    errno = 0;
    s = open(sname, O_RDWR | O_NOCTTY);
    say("open slave after unlockpt: %s errno=%s\n", s >= 0 ? "fd" : "-1", en(errno));
    fdinfo("  slave", s);

    int dn = open("/dev/null", O_RDWR);
    int fd = open(path("plain"), O_RDWR | O_CREAT, 0600);
    R("grantpt slave", grantpt(s));
    R("grantpt /dev/null", grantpt(dn));
    R("grantpt regular file", grantpt(fd));
    R("grantpt fd -1", grantpt(-1));
    R("grantpt closed fd 900", grantpt(900));
    R("unlockpt slave", unlockpt(s));
    R("unlockpt /dev/null", unlockpt(dn));
    R("unlockpt regular file", unlockpt(fd));
    R("unlockpt fd -1", unlockpt(-1));

    char *a = ptsname(m), *b;
    errno = 0;
    b = ptsname(s);
    say("ptsname slave: %s errno=%s\n", pname(b), en(errno));
    errno = 0;
    b = ptsname(dn);
    say("ptsname /dev/null: %s errno=%s\n", pname(b), en(errno));
    errno = 0;
    b = ptsname(fd);
    say("ptsname regular file: %s errno=%s\n", pname(b), en(errno));
    errno = 0;
    b = ptsname(-1);
    say("ptsname fd -1: %s errno=%s\n", pname(b), en(errno));
    int m2 = posix_openpt(O_RDWR | O_NOCTTY);
    b = ptsname(m2);
    say("ptsname static buffer: same pointer=%d names differ=%d\n", a == b, strcmp(sname, b ? b : "") != 0);
    close(m2);

    size_t len = strlen(sname);
    char buf[256];
    size_t lens[] = {0, 1, len - 1, len, len + 1, sizeof buf};
    const char *ln[] = {"0", "1", "len-1", "len", "len+1", "256"};
    for (int i = 0; i < 6; i++) {
        memset(buf, 'z', sizeof buf);
        errno = 0;
        int r = ptsname_r(m, buf, lens[i]);
        int e = errno;
        say("ptsname_r size %s: %d (%s) errno=%s buf=[%s]\n", ln[i], r, en(r), en(e), r == 0 ? pname(buf) : buf[0] == 'z' ? "untouched" : "written");
    }
    in_child("ptsname_r NULL buffer", ptsname_null_buf);
    errno = 0;
    int r = ptsname_r(s, buf, sizeof buf);
    say("ptsname_r slave: %d (%s) errno=%s\n", r, en(r), en(errno));
    errno = 0;
    r = ptsname_r(dn, buf, sizeof buf);
    say("ptsname_r /dev/null: %d (%s) errno=%s\n", r, en(r), en(errno));
    errno = 0;
    r = ptsname_r(-1, buf, sizeof buf);
    say("ptsname_r fd -1: %d (%s) errno=%s\n", r, en(r), en(errno));

    /* A round trip through the pair. */
    raw(s);
    write(m, "ping", 4);
    readable(s, 1000);
    char got[64];
    int n = take(s, got, sizeof got);
    write(s, "pong", 4);
    readable(m, 1000);
    char got2[64];
    int n2 = take(m, got2, sizeof got2);
    say("round trip: slave read %d [%s], master read %d [%s]\n", n, got, n2, got2);
    close(s);
    errno = 0;
    ssize_t rr = read(m, got, sizeof got);
    say("master read after slave closed: %ld errno=%s\n", (long)rr, en(errno));
    close(m);
    close(dn);
    close(fd);
}

/* ------------------------------------------------------------------ §19.4 openpty */

static void openpty_null_master(void)
{
    int s;
    R("openpty(NULL master)", openpty(NULL, &s, NULL, NULL, NULL));
}

static void openpty_null_slave(void)
{
    int m;
    R("openpty(NULL slave)", openpty(&m, NULL, NULL, NULL, NULL));
}

static void openpty_cases(void)
{
    int before = count_fds();
    int m = -1, s = -1;
    R("openpty all NULL", openpty(&m, &s, NULL, NULL, NULL));
    say("openpty opened %d descriptors\n", count_fds() - before);
    fdinfo("  openpty master", m);
    fdinfo("  openpty slave", s);
    char *pn = ptsname(m);
    char *tn = ttyname(s);
    say("ptsname(master) equals ttyname(slave): %d\n", pn && tn && strcmp(pn, tn) == 0);
    struct termios t, u;
    tcgetattr(s, &t);
    show_t("openpty default", &t);
    struct winsize ws;
    memset(&ws, 0x5a, sizeof ws);
    R("TIOCGWINSZ default", ioctl(s, TIOCGWINSZ, &ws));
    say("default winsize: rows=%u cols=%u xpixel=%u ypixel=%u\n", ws.ws_row, ws.ws_col, ws.ws_xpixel, ws.ws_ypixel);
    close(m);
    close(s);

    char name[128];
    memset(name, 'q', sizeof name);
    struct termios want;
    memset(&want, 0, sizeof want);
    want.c_iflag = ICRNL | IXON;
    want.c_oflag = OPOST;
    want.c_cflag = CS8 | CREAD | HUPCL;
    want.c_lflag = ICANON | ISIG;
    want.c_cc[VINTR] = 3;
    want.c_cc[VMIN] = 4;
    want.c_cc[VTIME] = 9;
    want.c_cc[VEOF] = 4;
    cfsetspeed(&want, B115200);
    struct winsize w = {24, 80, 640, 480};
    R("openpty name termp winp", openpty(&m, &s, name, &want, &w));
    say("openpty name: [%s] equals ttyname(slave)=%d\n", pname(name), ttyname(s) && strcmp(ttyname(s), name) == 0);
    tcgetattr(s, &u);
    show_t("openpty termp read back", &u);
    say("openpty termp applied: %d\n", same_t(&want, &u));
    memset(&ws, 0, sizeof ws);
    ioctl(s, TIOCGWINSZ, &ws);
    say("openpty winp on slave: rows=%u cols=%u xpixel=%u ypixel=%u\n", ws.ws_row, ws.ws_col, ws.ws_xpixel, ws.ws_ypixel);
    memset(&ws, 0, sizeof ws);
    ioctl(m, TIOCGWINSZ, &ws);
    say("openpty winp on master: rows=%u cols=%u\n", ws.ws_row, ws.ws_col);
    close(m);
    close(s);

    /* A termios the terminal does not take in full: openpty's result. */
    memset(&want, 0, sizeof want);
    want.c_iflag = 0xffffffffu;
    want.c_cflag = CS8 | CREAD | B9600;
    R("openpty with all iflag bits", openpty(&m, &s, NULL, &want, NULL));
    if (m >= 0) {
        tcgetattr(s, &u);
        say("openpty all iflag bits read back: iflag=%#x\n", (unsigned)u.c_iflag);
        close(m);
        close(s);
    }
    in_child("openpty NULL master", openpty_null_master);
    in_child("openpty NULL slave", openpty_null_slave);
}

/* ------------------------------------------------------------------ §19.1-19.2 controlling terminal */

static int g_m, g_s;

/* A background job: run op in a child in its own process group with SIGTTOU set as asked;
 * report its result or the signal that stopped it. */
static int (*bg_op)(void);
static void bg(const char *what, int ttou)
{
    pid_t g = fork();
    if (g == 0) {
        setpgid(0, 0);
        if (ttou == 1)
            signal(SIGTTOU, SIG_IGN);
        if (ttou == 2) {
            sigset_t ss;
            sigemptyset(&ss);
            sigaddset(&ss, SIGTTOU);
            sigprocmask(SIG_BLOCK, &ss, NULL);
        }
        errno = 0;
        int r = bg_op();
        say("background %s (SIGTTOU %s): %d errno=%s\n", what, ttou == 0 ? "default" : ttou == 1 ? "ignored" : "blocked", r, en(errno));
        _exit(0);
    }
    setpgid(g, g);
    int st = 0;
    waitpid(g, &st, WUNTRACED);
    if (WIFSTOPPED(st)) {
        say("background %s (SIGTTOU %s): %s\n", what, ttou == 0 ? "default" : ttou == 1 ? "ignored" : "blocked", wstat(st));
        kill(g, SIGKILL);
        waitpid(g, &st, 0);
    }
    /* Take the foreground back if the job took it. */
    void (*old)(int) = signal(SIGTTOU, SIG_IGN);
    tcsetpgrp(g_s, getpgrp());
    signal(SIGTTOU, old);
}

static struct termios g_t;
static int op_tcsetpgrp(void) { return tcsetpgrp(g_s, getpgrp()); }
static int op_tcsetattr(void) { return tcsetattr(g_s, TCSANOW, &g_t); }
static int op_tcgetattr(void)
{
    struct termios t;
    return tcgetattr(g_s, &t);
}
static int op_tcgetpgrp_is_leader(void) { return tcgetpgrp(g_s) == getsid(0); }
static int op_tcdrain(void) { return tcdrain(g_s); }
static int op_tcflush(void) { return tcflush(g_s, TCIFLUSH); }
static int op_tcflow(void) { return tcflow(g_s, TCOON); }
static int op_tcsendbreak(void) { return tcsendbreak(g_s, 0); }
static int op_tcgetsid_is_leader(void) { return tcgetsid(g_s) == getsid(0); }

static void ctty(void)
{
    pid_t ppg = getpgid(getppid());
    R("setsid", setsid() == getpid());
    int m, s;
    pair(&m, &s);
    errno = 0;
    int t = open("/dev/tty", O_RDWR);
    say("new session, pty opened O_NOCTTY: open /dev/tty %s errno=%s\n", t >= 0 ? "fd" : "-1", en(errno));
    R("tcgetpgrp slave (not controlling)", tcgetpgrp(s));
    R("tcgetsid slave (not controlling)", tcgetsid(s));
    R("tcsetpgrp slave (not controlling)", tcsetpgrp(s, getpgrp()));
    R("tcgetpgrp master (not controlling)", tcgetpgrp(m));

    int m2 = posix_openpt(O_RDWR);
    errno = 0;
    t = open("/dev/tty", O_RDWR);
    say("posix_openpt(O_RDWR) in a session leader: open /dev/tty %s errno=%s\n", t >= 0 ? "fd" : "-1", en(errno));
    if (t >= 0)
        close(t);
    close(m2);
    m2 = getpt();
    errno = 0;
    t = open("/dev/tty", O_RDWR);
    say("getpt in a session leader: open /dev/tty %s errno=%s\n", t >= 0 ? "fd" : "-1", en(errno));
    if (t >= 0)
        close(t);
    close(m2);
    int m3, s3;
    openpty(&m3, &s3, NULL, NULL, NULL);
    errno = 0;
    t = open("/dev/tty", O_RDWR);
    say("openpty in a session leader: open /dev/tty %s errno=%s\n", t >= 0 ? "fd" : "-1", en(errno));
    if (t >= 0) {
        say("openpty slave became controlling: tcgetsid(slave)==getsid=%d\n", tcgetsid(s3) == getsid(0));
        close(t);
        say("openpty case stops here (the session has a terminal)\n");
        return;
    }
    close(m3);
    close(s3);

    char sname[128];
    snprintf(sname, sizeof sname, "%s", ptsname(m));
    int s2 = open(sname, O_RDWR);
    errno = 0;
    t = open("/dev/tty", O_RDWR);
    say("slave opened without O_NOCTTY: open /dev/tty %s errno=%s\n", t >= 0 ? "fd" : "-1", en(errno));
    close(s2);
    if (t < 0)
        return;
    say("tcgetpgrp(slave)==getpgrp: %d\n", tcgetpgrp(s) == getpgrp());
    say("tcgetpgrp(master)==getpgrp: %d\n", tcgetpgrp(m) == getpgrp());
    say("tcgetpgrp(/dev/tty)==getpgrp: %d\n", tcgetpgrp(t) == getpgrp());
    say("tcgetsid(slave)==getsid: %d\n", tcgetsid(s) == getsid(0));
    errno = 0;
    pid_t sid = tcgetsid(m);
    say("tcgetsid(master): %s errno=%s\n", sid == getsid(0) ? "session" : sid == -1 ? "-1" : "other", en(errno));
    say("ttyname(/dev/tty fd): [%s]\n", pname(ttyname(t)));
    say("ctermid: [%s]\n", ctermid(NULL));
    int dn = open("/dev/null", O_RDWR);
    R("tcgetsid /dev/null", tcgetsid(dn));
    R("tcgetsid fd -1", tcgetsid(-1));
    R("tcgetpgrp /dev/null", tcgetpgrp(dn));
    R("tcgetpgrp fd -1", tcgetpgrp(-1));
    R("tcsetpgrp own group", tcsetpgrp(s, getpgrp()));
    R("tcsetpgrp via master", tcsetpgrp(m, getpgrp()));
    R("tcsetpgrp -1", tcsetpgrp(s, -1));
    R("tcsetpgrp 0", tcsetpgrp(s, 0));
    R("tcsetpgrp INT_MIN", tcsetpgrp(s, (pid_t)0x80000000));
    R("tcsetpgrp unused id", tcsetpgrp(s, 0x3ffffff0));
    R("tcsetpgrp group of another session", tcsetpgrp(s, ppg));
    R("tcsetpgrp /dev/null", tcsetpgrp(dn, getpgrp()));
    R("tcsetpgrp fd -1", tcsetpgrp(-1, getpgrp()));
    say("foreground still own group: %d\n", tcgetpgrp(s) == getpgrp());

    /* Background jobs. */
    g_m = m;
    g_s = s;
    tcgetattr(s, &g_t);
    struct {
        const char *n;
        int (*op)(void);
    } ops[] = {
        {"tcsetpgrp", op_tcsetpgrp}, {"tcsetattr", op_tcsetattr}, {"tcdrain", op_tcdrain},       {"tcflush", op_tcflush},
        {"tcflow", op_tcflow},       {"tcsendbreak", op_tcsendbreak}, {"tcgetattr", op_tcgetattr},
    };
    for (size_t i = 0; i < sizeof ops / sizeof ops[0]; i++)
        for (int k = 0; k < 3; k++) {
            bg_op = ops[i].op;
            bg(ops[i].n, k);
            if (ops[i].op == op_tcsetpgrp && k > 0)
                say("  foreground restored to own group: %d\n", tcgetpgrp(s) == getpgrp());
        }
    bg_op = op_tcgetpgrp_is_leader;
    bg("tcgetpgrp==session leader's group", 0);
    bg_op = op_tcgetsid_is_leader;
    bg("tcgetsid==session", 0);

    /* An orphaned background group: its parent leaves, so no member has a parent in another
     * group of the session. */
    struct {
        const char *n;
        int (*op)(void);
    } oops[] = {{"tcsetpgrp", op_tcsetpgrp}, {"tcsetattr", op_tcsetattr}, {"tcdrain", op_tcdrain}, {"tcflush", op_tcflush}, {"tcgetattr", op_tcgetattr}};
    for (size_t i = 0; i < sizeof oops / sizeof oops[0]; i++) {
        int sync[2];
        pipe(sync);
        pid_t g = fork();
        if (g == 0) {
            setpgid(0, 0);
            pid_t me = getpid();
            pid_t h = fork();
            if (h == 0) {
                close(sync[0]);
                for (int k = 0; k < 200 && getppid() == me; k++)
                    sleep_ms(10);
                errno = 0;
                int r = oops[i].op();
                say("orphaned background %s (SIGTTOU default): %d errno=%s\n", oops[i].n, r, en(errno));
                write(sync[1], "x", 1);
                _exit(0);
            }
            _exit(0);
        }
        close(sync[1]);
        setpgid(g, g);
        int st;
        waitpid(g, &st, 0);
        char c;
        if (!readable(sync[0], 5000) || read(sync[0], &c, 1) != 1)
            say("orphaned background %s: no report\n", oops[i].n);
        close(sync[0]);
        void (*old)(int) = signal(SIGTTOU, SIG_IGN);
        tcsetpgrp(s, getpgrp());
        signal(SIGTTOU, old);
    }
    close(t);
}

/* ------------------------------------------------------------------ §19.4 login_tty, forkpty */

static void lt_basic(void)
{
    int m, s;
    pair(&m, &s);
    char sname[128];
    snprintf(sname, sizeof sname, "%s", ptsname(m));
    int sfd = s;
    R("login_tty slave", login_tty(s));
    say("session leader: %d\n", getsid(0) == getpid());
    say("isatty 0 1 2: %d %d %d\n", isatty(0), isatty(1), isatty(2));
    say("ttyname(0..2) is the slave: %d %d %d\n", ttyname(0) && strcmp(ttyname(0), sname) == 0, ttyname(1) && strcmp(ttyname(1), sname) == 0,
        ttyname(2) && strcmp(ttyname(2), sname) == 0);
    say("tcgetpgrp(0)==getpgrp: %d\n", tcgetpgrp(0) == getpgrp());
    say("tcgetsid(0)==getsid: %d\n", tcgetsid(0) == getsid(0));
    errno = 0;
    int r = fcntl(sfd, F_GETFD);
    say("original slave descriptor after login_tty: %s errno=%s\n", r == -1 ? "closed" : "open", en(errno));
    int t = open("/dev/tty", O_RDWR);
    say("open /dev/tty after login_tty: %s\n", t >= 0 ? "fd" : "-1");
    say("descriptors 0-2 cloexec: %d %d %d\n", fcntl(0, F_GETFD) & FD_CLOEXEC, fcntl(1, F_GETFD) & FD_CLOEXEC, fcntl(2, F_GETFD) & FD_CLOEXEC);
}

static void lt_devnull(void)
{
    int dn = open("/dev/null", O_RDWR);
    pid_t me = getpid();
    R("login_tty /dev/null", login_tty(dn));
    say("after failed login_tty: session leader=%d fd still open=%d stdin is /dev/null=%d\n", getsid(0) == me, fcntl(dn, F_GETFD) != -1, !isatty(0) && fcntl(0, F_GETFD) != -1);
}

static void lt_badfd(void) { R("login_tty fd -1", login_tty(-1)); }

static void lt_leader(void)
{
    setsid();
    int m, s;
    pair(&m, &s);
    R("login_tty as session leader already", login_tty(s));
    say("ttyname(0) is the slave: %d\n", ttyname(0) && strcmp(ttyname(0), ptsname(m)) == 0);
    say("tcgetsid(0)==getsid: %d\n", tcgetsid(0) == getsid(0));
}

static void lt_master(void)
{
    int m, s;
    pair(&m, &s);
    R("login_tty master", login_tty(m));
    say("isatty 0: %d tcgetsid(0)==getsid: %d\n", isatty(0), tcgetsid(0) == getsid(0));
}

static void lt_fd0(void)
{
    int m, s;
    pair(&m, &s);
    dup2(s, 0);
    close(s);
    R("login_tty fd 0", login_tty(0));
    say("fd 0 still open=%d isatty 1 2: %d %d\n", fcntl(0, F_GETFD) != -1, isatty(1), isatty(2));
}

/* The terminal of another session: login_tty in a second session for a slave that is already the
 * controlling terminal of the first. */
static void lt_stolen(void)
{
    setsid();
    int m, s;
    pair(&m, &s);
    int s2 = open(ptsname(m), O_RDWR); /* acquire it */
    close(s2);
    int sync[2];
    pipe(sync);
    pid_t c = fork();
    if (c == 0) {
        close(sync[0]);
        errno = 0;
        int r = login_tty(s);
        int e = errno;
        say("login_tty of another session's terminal: %d errno=%s controlling now=%d\n", r, en(e), tcgetsid(0) == getsid(0));
        write(sync[1], "x", 1);
        _exit(0);
    }
    close(sync[1]);
    char ch;
    readable(sync[0], 5000);
    read(sync[0], &ch, 1);
    waitpid(c, NULL, 0);
    say("first session keeps the terminal: %d\n", tcgetsid(s) == getsid(0));
}

static void fp_case(int with_args)
{
    int m = -77;
    char name[128] = "unset";
    struct termios want;
    memset(&want, 0, sizeof want);
    want.c_iflag = IXON;
    want.c_cflag = CS8 | CREAD | HUPCL;
    want.c_lflag = ISIG | ICANON;
    want.c_cc[VINTR] = 3;
    want.c_cc[VMIN] = 1;
    cfsetspeed(&want, B57600);
    struct winsize w = {30, 100, 0, 0};
    /* The child reports through its own pipe; the parent prints the report after its own lines. */
    int before = count_fds() + 1; /* the report pipe's read end stays open in the parent */
    int cp[2];
    pipe(cp);
    pid_t pid = with_args ? forkpty(&m, name, &want, &w) : forkpty(&m, NULL, NULL, NULL);
    if (pid == 0) {
        out_fd = cp[1];
        say("child: session leader=%d tcgetpgrp(0)==getpgrp=%d isatty 0 1 2: %d %d %d\n", getsid(0) == getpid(), tcgetpgrp(0) == getpgrp(), isatty(0), isatty(1), isatty(2));
        say("child: amaster in child is %s\n", m == -77 ? "unchanged" : "set");
        int ttys = 0;
        for (int fd = 3; fd < 1024; fd++)
            if (fd != 100 && fd != cp[1] && isatty(fd))
                ttys++;
        say("child: other terminal descriptors: %d\n", ttys);
        if (with_args) {
            say("child: ttyname(0)==name: %d\n", ttyname(0) && strcmp(ttyname(0), name) == 0);
            struct termios u;
            tcgetattr(0, &u);
            say("child: termp applied: %d\n", same_t(&want, &u));
            struct winsize ws;
            ioctl(0, TIOCGWINSZ, &ws);
            say("child: winsize rows=%u cols=%u\n", ws.ws_row, ws.ws_col);
        }
        write(1, "hello\n", 6);
        _exit(7);
    }
    close(cp[1]);
    if (pid < 0) {
        say("forkpty: -1 errno=%s\n", en(errno));
        return;
    }
    say("forkpty%s: parent got master=%d name=[%s] descriptors added=%d\n", with_args ? " with args" : " NULL args", isatty(m), with_args ? pname(name) : name,
        count_fds() - before);
    if (with_args) {
        say("parent: ptsname(master)==name: %d\n", ptsname(m) && strcmp(ptsname(m), name) == 0);
        int slaves = 0;
        for (int fd = 0; fd < 1024; fd++)
            if (fd != m && isatty(fd) && ttyname(fd) && strcmp(ttyname(fd), name) == 0)
                slaves++;
        say("parent: descriptors on the slave: %d\n", slaves);
    }
    char got[256], all[512] = "";
    for (int k = 0; k < 20; k++) {
        if (!readable(m, 1000))
            break;
        int n = take(m, got, sizeof got);
        if (n <= 0)
            break;
        strncat(all, got, sizeof all - strlen(all) - 1);
        if (strstr(all, "\\x0a"))
            break;
    }
    say("parent: read from master [%s]\n", all);
    char rep[2048];
    size_t rn = 0;
    while (rn + 1 < sizeof rep && readable(cp[0], 5000)) {
        ssize_t k = read(cp[0], rep + rn, sizeof rep - 1 - rn);
        if (k <= 0)
            break;
        rn += (size_t)k;
    }
    rep[rn] = 0;
    close(cp[0]);
    say("%s", rep);
    int st;
    waitpid(pid, &st, 0);
    say("parent: child %s\n", wstat(st));
    close(m);
}

static void fp_args(void) { fp_case(1); }
static void fp_null(void) { fp_case(0); }

/* ------------------------------------------------------------------ cancellation points */

static int c_m, c_s, c_pipe[2];
static int (*c_op)(void);
static void *c_thread(void *arg)
{
    (void)arg;
    pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
    char ch;
    read(c_pipe[0], &ch, 1); /* the main thread has cancelled us */
    pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL);
    c_op();
    return (void *)1;
}
static struct termios c_t;
static int c_tcdrain(void) { return tcdrain(c_s); }
static int c_tcsendbreak(void) { return tcsendbreak(c_s, 0); }
static int c_tcsetattr_now(void) { return tcsetattr(c_s, TCSANOW, &c_t); }
static int c_tcsetattr_drain(void) { return tcsetattr(c_s, TCSADRAIN, &c_t); }
static int c_tcflush(void) { return tcflush(c_s, TCIFLUSH); }
static int c_tcflow(void) { return tcflow(c_s, TCOON); }
static int c_tcgetattr(void)
{
    struct termios t;
    return tcgetattr(c_s, &t);
}
static int c_isatty(void) { return isatty(c_s); }
static int c_ttyname_r(void)
{
    char b[64];
    return ttyname_r(c_s, b, sizeof b);
}
static int c_openpty(void)
{
    int a, b;
    int r = openpty(&a, &b, NULL, NULL, NULL);
    close(a);
    close(b);
    return r;
}

static void cancels(void)
{
    pair(&c_m, &c_s);
    tcgetattr(c_s, &c_t);
    struct {
        const char *n;
        int (*op)(void);
    } ops[] = {{"tcdrain", c_tcdrain},   {"tcsendbreak", c_tcsendbreak}, {"tcsetattr TCSANOW", c_tcsetattr_now}, {"tcsetattr TCSADRAIN", c_tcsetattr_drain},
               {"tcflush", c_tcflush},   {"tcflow", c_tcflow},           {"tcgetattr", c_tcgetattr},             {"isatty", c_isatty},
               {"ttyname_r", c_ttyname_r}, {"openpty", c_openpty}};
    for (size_t i = 0; i < sizeof ops / sizeof ops[0]; i++) {
        pipe(c_pipe);
        c_op = ops[i].op;
        pthread_t th;
        pthread_create(&th, NULL, c_thread, NULL);
        pthread_cancel(th);
        write(c_pipe[1], "x", 1);
        void *res;
        pthread_join(th, &res);
        say("cancel pending, %s: %s\n", ops[i].n, res == PTHREAD_CANCELED ? "cancelled" : "returned");
        close(c_pipe[0]);
        close(c_pipe[1]);
    }
    close(c_m);
    close(c_s);
}

/* ------------------------------------------------------------------ §19.5 sgtty, revoke */

static void stubs(void)
{
    int m, s;
    pair(&m, &s);
    int dn = open("/dev/null", O_RDWR);
    long sg[16];
    memset(sg, 0, sizeof sg);
    R("gtty slave", gtty(s, (struct sgttyb *)sg));
    R("gtty /dev/null", gtty(dn, (struct sgttyb *)sg));
    R("gtty fd -1", gtty(-1, (struct sgttyb *)sg));
    R("stty slave", stty(s, (const struct sgttyb *)sg));
    R("stty fd -1", stty(-1, (const struct sgttyb *)sg));
    R("revoke slave", revoke(ptsname(m)));
    R("revoke /dev/null", revoke("/dev/null"));
    R("revoke missing", revoke("/nonexistent/x"));
    R("revoke empty", revoke(""));
    errno = 0;
    ssize_t w = write(s, "x", 1);
    say("slave usable after revoke: %d\n", w == 1);
    close(m);
    close(s);
    close(dn);
}

/* ------------------------------------------------------------------ §19.5 ttyent (namespace) */

static const char FX_TTYS[] = "# /etc/ttys fixture\n"
                              "console\t\"/usr/libexec/getty std.9600\"\tvt100\ton secure\n"
                              "ttyv0\t\"/usr/libexec/getty Pc\"\t\tcons25\ton  secure window=\"/usr/X11/bin/xterm -display :0\"\n"
                              "ttyv1\tnone\tnetwork\toff\n"
                              "# comment line\n"
                              "\n"
                              "   \t\n"
                              "   indented   getty1   type1   on\n"
                              "ttyd0\t\"/usr/libexec/getty std.9600\"\tdialup\toff # trailing comment\n"
                              "ttyd1\tg\tt\ton#nospace\n"
                              "nameonly\n"
                              "nameonly2 # c\n"
                              "quoted\t\"a \\\"b\\\" c\"\t\"t y p e\"\ton insecure\n"
                              "hash\t\"x#y\"\tt#z\ton\n"
                              "flags\tg\tt\ton off secure unknown\n"
                              "winonly\tg\tt\twindow=abc\n"
                              "wquoted\tg\tt\twindow=\"a b\" on\n"
                              "emptyq\t\"\"\t\"\"\ton\n"
                              "upper\tg\tt\tON SECURE\n"
                              "unterminated\t\"abc def\n"
                              "commentonly\t#all comment\n"
                              "ttyd1\tdup\tdup\ton\n"
                              "nonl\tg\tt\ton";

static const char *esc(const char *s)
{
    static char b[6][512];
    static int k;
    if (!s)
        return "NULL";
    k = (k + 1) % 6;
    size_t o = 0;
    for (; *s && o + 5 < sizeof b[k]; s++) {
        unsigned char c = (unsigned char)*s;
        if (c >= 0x20 && c < 0x7f && c != '\\')
            b[k][o++] = (char)c;
        else
            o += (size_t)snprintf(b[k] + o, sizeof b[k] - o, "\\x%02x", c);
    }
    b[k][o] = 0;
    return b[k];
}

static void show_ent(const char *what, struct ttyent *e)
{
    if (!e) {
        say("%s: NULL errno=%s\n", what, en(errno));
        return;
    }
    say("%s: name=[%s] getty=[%s] type=[%s] status=%#x window=[%s] comment=[%s]\n", what, esc(e->ty_name), esc(e->ty_getty), esc(e->ty_type), (unsigned)e->ty_status,
        esc(e->ty_window), esc(e->ty_comment));
}

static void ttyent_cases(void)
{
    say("TTY_ON=%#x TTY_SECURE=%#x _PATH_TTYS=[%s]\n", TTY_ON, TTY_SECURE, _PATH_TTYS);
    R("setttyent", setttyent());
    struct ttyent *first = NULL, *e;
    int k = 0;
    for (;;) {
        errno = 0;
        e = getttyent();
        if (!e)
            break;
        if (!first)
            first = e;
        char nm[32];
        snprintf(nm, sizeof nm, "getttyent %d", ++k);
        show_ent(nm, e);
        if (k > 40)
            break;
    }
    say("getttyent end: NULL errno=%s\n", en(errno));
    errno = 0;
    e = getttyent();
    show_ent("getttyent after end", e);
    R("endttyent", endttyent());
    R("endttyent again", endttyent());
    errno = 0;
    e = getttyent();
    show_ent("getttyent after endttyent (no setttyent)", e);
    say("same static entry: %d\n", e == first);
    e = getttyent();
    show_ent("getttyent next", e);
    R("setttyent rewinds", setttyent());
    show_ent("getttyent after setttyent", getttyent());
    errno = 0;
    show_ent("getttynam console", getttynam("console"));
    errno = 0;
    show_ent("getttynam nonl", getttynam("nonl"));
    errno = 0;
    show_ent("getttynam ttyd1 (two entries)", getttynam("ttyd1"));
    errno = 0;
    show_ent("getttynam missing", getttynam("missing"));
    errno = 0;
    show_ent("getttynam empty", getttynam(""));
    errno = 0;
    show_ent("getttynam indented", getttynam("indented"));
    errno = 0;
    show_ent("getttynam prefix 'ttyv'", getttynam("ttyv"));
    errno = 0;
    show_ent("getttyent after getttynam", getttyent());
    setttyent();
    getttyent();
    getttyent();
    getttynam("console");
    errno = 0;
    show_ent("getttyent after getttynam mid-enumeration", getttyent());
    endttyent();

    /* Without /etc/ttys. */
    rename("/etc/ttys", "/etc/ttys.off");
    R("setttyent without the file", setttyent());
    errno = 0;
    show_ent("getttyent without the file", getttyent());
    errno = 0;
    show_ent("getttynam without the file", getttynam("console"));
    R("endttyent without the file", endttyent());
    rename("/etc/ttys.off", "/etc/ttys");

    /* Long lines: 99 bytes, 100 bytes and 300 bytes (newline included), each after an ordinary
     * entry, then a short last line without a newline. */
    FILE *f = fopen("/etc/ttys", "w");
    const int lens[] = {99, 100, 300};
    for (int i = 0; i < 3; i++) {
        fprintf(f, "before%d\tg\tt\ton\n", lens[i]);
        int used = fprintf(f, "long%d\t", lens[i]);
        for (int j = used; j < lens[i] - 1; j++)
            fputc('a' + j % 26, f);
        fputc('\n', f);
    }
    fputs("after\tg\tt\ton\nnonl\tg\tt\ton", f);
    fclose(f);
    setttyent();
    for (int i = 0; i < 12; i++) {
        errno = 0;
        e = getttyent();
        if (!e) {
            say("long lines %d: NULL errno=%s\n", i, en(errno));
            break;
        }
        say("long lines %d: name=[%s] getty length=%zu status=%#x\n", i, esc(e->ty_name), e->ty_getty ? strlen(e->ty_getty) : 0, (unsigned)e->ty_status);
    }
    endttyent();
}

/* ------------------------------------------------------------------ §19.3 ttyname edge cases (namespace) */

static int pts_index(const char *n)
{
    return n && strncmp(n, "/dev/pts/", 9) == 0 ? atoi(n + 9) : -1;
}

static void ttyname_ns(void)
{
    int m, s;
    if (pair(&m, &s) != 0) {
        say("ns: no pty\n");
        return;
    }
    char sname[128];
    snprintf(sname, sizeof sname, "%s", ptsname(m));
    int idx = pts_index(sname);
    char buf[128];

    /* The /dev/pts entry is gone: an empty tmpfs over /dev/pts. */
    R("mount tmpfs over /dev/pts", mount("none", "/dev/pts", "tmpfs", 0, "size=64k"));
    errno = 0;
    char *c = ttyname(s);
    say("entry gone: ttyname %s errno=%s\n", pname(c), en(errno));
    int r = ttyname_r(s, buf, sizeof buf);
    say("entry gone: ttyname_r %d (%s)\n", r, en(r));
    say("entry gone: isatty %d\n", isatty(s));
    errno = 0;
    c = ptsname(m);
    say("entry gone: ptsname %s errno=%s\n", pname(c), en(errno));
    R("umount tmpfs", umount("/dev/pts"));

    /* A new devpts instance over /dev/pts: the old pty is in another instance. */
    R("mount new devpts instance", mount("devpts", "/dev/pts", "devpts", 0, "newinstance,ptmxmode=0666,mode=0620"));
    errno = 0;
    c = ttyname(s);
    say("other instance: ttyname %s errno=%s\n", pname(c), en(errno));
    r = ttyname_r(s, buf, sizeof buf);
    say("other instance: ttyname_r %d (%s)\n", r, en(r));
    errno = 0;
    c = ptsname(m);
    say("other instance: ptsname of old master %s errno=%s\n", pname(c), en(errno));
    /* The instance's own ptmx, so that new ptys are made there. */
    R("bind /dev/pts/ptmx over /dev/ptmx", mount("/dev/pts/ptmx", "/dev/ptmx", NULL, MS_BIND, NULL));
    int nm, nsl;
    if (pair(&nm, &nsl) == 0) {
        char nn[128];
        snprintf(nn, sizeof nn, "%s", ptsname(nm));
        say("new instance first pty: [%s]\n", nn);
        c = ttyname(nsl);
        say("new instance: ttyname of its slave equals ptsname=%d\n", c && strcmp(c, nn) == 0);
        /* Make ptys in the new instance until one has the old pty's number. */
        int fds[512], nf = 0, found = pts_index(nn) == idx;
        while (!found && nf < 500) {
            int a = posix_openpt(O_RDWR | O_NOCTTY);
            if (a < 0)
                break;
            fds[nf++] = a;
            found = pts_index(ptsname(a)) == idx;
        }
        say("new instance has a pty with the old number: %d\n", found);
        if (found) {
            errno = 0;
            c = ttyname(s);
            say("same number in another instance: ttyname %s errno=%s\n", pname(c), en(errno));
            r = ttyname_r(s, buf, sizeof buf);
            say("same number in another instance: ttyname_r %d (%s)\n", r, en(r));
        }
        for (int i = 0; i < nf; i++)
            close(fds[i]);

        /* /proc unavailable: an empty tmpfs over /proc. */
        R("mount tmpfs over /proc", mount("none", "/proc", "tmpfs", 0, "size=64k"));
        errno = 0;
        c = ttyname(nsl);
        say("no /proc: ttyname %s errno=%s equals ptsname=%d\n", pname(c), en(errno), c && strcmp(c, nn) == 0);
        r = ttyname_r(nsl, buf, sizeof buf);
        say("no /proc: ttyname_r %d (%s)\n", r, en(r));
        errno = 0;
        c = ttyname(s);
        say("no /proc, old instance: ttyname %s errno=%s\n", pname(c), en(errno));
        errno = 0;
        c = ptsname(nm);
        say("no /proc: ptsname %s errno=%s\n", pname(c), en(errno));
        int a, b;
        errno = 0;
        r = openpty(&a, &b, buf, NULL, NULL);
        say("no /proc: openpty %d errno=%s name=[%s]\n", r, en(errno), r == 0 ? pname(buf) : "");
        if (r == 0) {
            close(a);
            close(b);
        }
        R("umount /proc", umount("/proc"));
        close(nm);
        close(nsl);
    }
    close(m);
    close(s);
}

/* ------------------------------------------------------------------ main */

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: %s DIR [ns]\n", argv[0]);
        return 2;
    }
    snprintf(dir, sizeof dir, "%s", argv[1]);
    tw_alarm(240);
    signal(SIGPIPE, SIG_IGN);
    if (argc > 2 && strcmp(argv[2], "ns") == 0) {
        say("== ns: ttyent\n");
        ttyent_cases();
        say("== ns: ttyname\n");
        ttyname_ns();
        return 0;
    }
    say("== speeds\n");
    speeds();
    say("== cfmakeraw\n");
    makeraw();
    say("== attributes\n");
    attrs();
    say("== line control\n");
    control();
    say("== isatty, ctermid\n");
    isatty_ctermid();
    say("== ttyname\n");
    names();
    say("== pty api\n");
    ptyapi();
    say("== openpty\n");
    openpty_cases();
    say("== controlling terminal\n");
    in_child("ctty", ctty);
    say("== login_tty\n");
    in_child("login_tty slave", lt_basic);
    in_child("login_tty /dev/null", lt_devnull);
    in_child("login_tty -1", lt_badfd);
    in_child("login_tty leader", lt_leader);
    in_child("login_tty master", lt_master);
    in_child("login_tty fd 0", lt_fd0);
    in_child("login_tty stolen", lt_stolen);
    say("== forkpty\n");
    in_child("forkpty args", fp_args);
    in_child("forkpty null", fp_null);
    say("== cancellation\n");
    cancels();
    say("== stubs\n");
    stubs();

    char etc[600];
    snprintf(etc, sizeof etc, "%s", path("etc"));
    mkdir(etc, 0755);
    FILE *f = fopen(path("etc/ttys"), "w");
    if (f) {
        fputs(FX_TTYS, f);
        fclose(f);
    }
    return 0;
}
