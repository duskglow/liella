/* The integrated name-service cache (D-16.10), end to end in the products: a self-checking program
 * (not a differential one: the reference has no such cache) that `nscache_run.sh` starts as root
 * in private mount and network namespaces, with its own tmpfs on /dev/shm (where the shared
 * regions are), a tmpfs on /mnt (copies of this program, one setuid), a fixture /etc bind-mounted
 * over /etc, and only the loopback interface up. It runs its own DNS server on 127.0.0.1:53.
 * Nothing it does reaches the system: every mount vanishes with the namespace.
 *
 * Each check prints "ok NAME" or "FAIL NAME: why"; the exit status is the number of failures
 * (capped). Many checks start fresh processes (this program again, from /mnt): `sub ACTION ARG`
 * prints one line "ANSWER | mode reason owner hits misses stats generation".
 *
 * Build: cc -static -nostdlib liella-crt1.o -I include nscache_check.c libc.a -lgcc */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <locale.h>
#include <netdb.h>
#include <netinet/in.h>
#include <pwd.h>
#include <gshadow.h>
#include <sched.h>
#include <shadow.h>
#include <resolv.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include <liella/native.h>

#define REGION0 "/dev/shm/liella-nscache-0"
#define PROG "/mnt/nscache_prog"
#define SUID "/mnt/nscache_suid"

static int fails;

static void check(const char *name, int ok, const char *fmt, ...) __attribute__((format(printf, 3, 4)));
static void check(const char *name, int ok, const char *fmt, ...)
{
    if (ok) {
        printf("ok %s\n", name);
    } else {
        va_list ap;
        va_start(ap, fmt);
        printf("FAIL %s: ", name);
        vprintf(fmt, ap);
        printf("\n");
        va_end(ap);
        fails++;
    }
    fflush(stdout);
}

static struct liella_nscache_info st(void)
{
    struct liella_nscache_info s;
    memset(&s, 0, sizeof s);
    liella_nscache_status(&s);
    return s;
}

static void msleep(int ms)
{
    struct timespec t = {ms / 1000, (long)(ms % 1000) * 1000000L};
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {
    }
}

static void put(const char *path, const char *text)
{
    char tmp[256];
    snprintf(tmp, sizeof tmp, "%s.new", path);
    FILE *f = fopen(tmp, "w");
    if (!f) {
        printf("FAIL setup: cannot write %s\n", tmp);
        exit(100);
    }
    fputs(text, f);
    fclose(f);
    chmod(tmp, 0644);
    rename(tmp, path);
}

/* Append a comment to a file (in place: same inode, new size and times). */
static void touch_file(const char *path)
{
    FILE *f = fopen(path, "a");
    if (f) {
        fputs("# changed\n", f);
        fclose(f);
    }
}

static const char *PASSWD = "root:x:0:0:root:/root:/bin/sh\nalice:x:2000:2000:Alice:/home/alice:/bin/sh\nnobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";

static void fixture(void)
{
    put("/etc/nsswitch.conf", "passwd: files\ngroup: files\nshadow: files\ngshadow: files\nhosts: files dns\nservices: files\nprotocols: files\nnetworks: files\n");
    put("/etc/shadow", "root:$6$NSCSECRETROOT$xyz:19000:0:99999:7:::\nalice:$6$NSCSECRETALICE$xyz:19000:0:99999:7:::\n");
    put("/etc/gshadow", "root:$6$NSCSECRETGROUP$xyz::\nstaff:$6$NSCSECRETSTAFF$xyz:root:alice,root\n");
    chmod("/etc/shadow", 0640);
    chmod("/etc/gshadow", 0640);
    put("/etc/host.conf", "multi on\n");
    put("/etc/resolv.conf", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n");
    put("/etc/gai.conf", "# defaults\n");
    put("/etc/hosts", "127.0.0.1 localhost\n192.0.2.1 alpha\n192.0.2.2 both\n2001:db8::2 both\n");
    put("/etc/passwd", PASSWD);
    put("/etc/group", "root:x:0:\nstaff:x:50:alice,root\nalice:x:2000:\n");
    put("/etc/services", "http 80/tcp www\ndomain 53/udp\n");
    put("/etc/protocols", "ip 0 IP\ntcp 6 TCP\n");
    put("/etc/networks", "loopnet 127.0.0.0\ntestnet 192.0.2.0\n");
}

/* ------------------------------------------------------------------ one lookup, as text */

static void join_addrs(char **list, int af, char *out, size_t cap)
{
    out[0] = 0;
    for (int i = 0; list && list[i]; i++) {
        char t[64];
        inet_ntop(af, list[i], t, sizeof t);
        if (i)
            strncat(out, ",", cap - strlen(out) - 1);
        strncat(out, t, cap - strlen(out) - 1);
    }
}

static void look(const char *what, const char *arg, char *out, size_t cap)
{
    out[0] = 0;
    if (!strcmp(what, "pw")) {
        struct passwd *p = getpwnam(arg);
        snprintf(out, cap, "%s", p ? p->pw_shell : "none");
    } else if (!strcmp(what, "pwuid")) {
        struct passwd *p = getpwuid((uid_t)atoi(arg));
        snprintf(out, cap, "%s", p ? p->pw_name : "none");
    } else if (!strcmp(what, "gr")) {
        struct group *g = getgrnam(arg);
        if (!g) {
            snprintf(out, cap, "none");
        } else {
            snprintf(out, cap, "%d:", (int)g->gr_gid);
            for (int i = 0; g->gr_mem[i]; i++) {
                strncat(out, g->gr_mem[i], cap - strlen(out) - 1);
                strncat(out, ";", cap - strlen(out) - 1);
            }
        }
    } else if (!strcmp(what, "grl")) {
        gid_t l[32];
        int n = 32;
        int r = getgrouplist(arg, 99, l, &n);
        snprintf(out, cap, "%d:", r);
        for (int i = 0; i < n && i < 32; i++) {
            char t[16];
            snprintf(t, sizeof t, "%d;", (int)l[i]);
            strncat(out, t, cap - strlen(out) - 1);
        }
    } else if (!strcmp(what, "serv")) {
        struct servent *s = getservbyname(arg, "tcp");
        snprintf(out, cap, "%d", s ? ntohs((unsigned short)s->s_port) : -1);
    } else if (!strcmp(what, "proto")) {
        struct protoent *p = getprotobyname(arg);
        snprintf(out, cap, "%d", p ? p->p_proto : -1);
    } else if (!strcmp(what, "net")) {
        struct netent *n = getnetbyname(arg);
        snprintf(out, cap, "%x", n ? (unsigned)n->n_net : 0xffffffffu);
    } else if (!strcmp(what, "host")) {
        struct hostent *h = gethostbyname(arg);
        if (!h)
            snprintf(out, cap, "herr=%d", h_errno);
        else
            join_addrs(h->h_addr_list, AF_INET, out, cap);
    } else if (!strcmp(what, "gai") || !strcmp(what, "gaicfg") || !strcmp(what, "gaiidn")) {
        struct addrinfo hints, *res = NULL;
        memset(&hints, 0, sizeof hints);
        hints.ai_socktype = SOCK_STREAM;
        hints.ai_flags = !strcmp(what, "gaicfg") ? AI_ADDRCONFIG : !strcmp(what, "gaiidn") ? (AI_IDN | AI_CANONNAME | AI_CANONIDN) : 0;
        int r = getaddrinfo(arg, NULL, &hints, &res);
        if (r) {
            snprintf(out, cap, "eai=%d", r);
        } else {
            for (struct addrinfo *p = res; p; p = p->ai_next) {
                char t[64];
                void *a = p->ai_family == AF_INET ? (void *)&((struct sockaddr_in *)p->ai_addr)->sin_addr : (void *)&((struct sockaddr_in6 *)p->ai_addr)->sin6_addr;
                inet_ntop(p->ai_family, a, t, sizeof t);
                if (p != res)
                    strncat(out, ",", cap - strlen(out) - 1);
                strncat(out, t, cap - strlen(out) - 1);
            }
            if (res->ai_canonname) {
                strncat(out, " canon=", cap - strlen(out) - 1);
                strncat(out, res->ai_canonname, cap - strlen(out) - 1);
            }
            freeaddrinfo(res);
        }
    } else if (!strcmp(what, "none")) {
        snprintf(out, cap, "-");
    } else {
        snprintf(out, cap, "unknown");
    }
}

static void print_status(const char *answer)
{
    struct liella_nscache_info s = st();
    printf("%s | %u %u %llu %llu %llu %llu %llu\n", answer, s.mode, s.reason, (unsigned long long)s.owner, (unsigned long long)s.hits,
           (unsigned long long)s.misses, (unsigned long long)s.stats, (unsigned long long)s.generation);
}

/* ------------------------------------------------------------------ fresh processes */

struct sub {
    char answer[512];
    unsigned mode, reason;
    unsigned long long owner, hits, misses, stats, gen;
    int status;
};

/* Run `PROG sub ARGS...` (as `uid` unless -1, with `env` added to an empty environment), its
 * last line parsed. */
static struct sub run_as(int uid, const char *exe, const char *const env[], const char *const args[])
{
    struct sub r;
    memset(&r, 0, sizeof r);
    int fd[2];
    if (pipe(fd) != 0)
        return r;
    pid_t pid = fork();
    if (pid == 0) {
        dup2(fd[1], 1);
        close(fd[0]);
        close(fd[1]);
        if (uid >= 0 && (setresgid((gid_t)uid, (gid_t)uid, (gid_t)uid) != 0 || setgroups(0, NULL) != 0 || setresuid((uid_t)uid, (uid_t)uid, (uid_t)uid) != 0))
            _exit(120);
        char *argv[16];
        int n = 0;
        argv[n++] = (char *)exe;
        argv[n++] = (char *)"sub";
        for (int i = 0; args[i] && n < 15; i++)
            argv[n++] = (char *)args[i];
        argv[n] = NULL;
        char *envp[16];
        int m = 0;
        for (int i = 0; env && env[i] && m < 15; i++)
            envp[m++] = (char *)env[i];
        envp[m] = NULL;
        execve(exe, argv, envp);
        _exit(121);
    }
    close(fd[1]);
    char buf[4096];
    size_t len = 0;
    ssize_t k;
    while ((k = read(fd[0], buf + len, sizeof buf - 1 - len)) > 0)
        len += (size_t)k;
    buf[len] = 0;
    close(fd[0]);
    waitpid(pid, &r.status, 0);
    char *line = buf, *last = buf;
    while ((line = strchr(line, '\n')) && line[1])
        last = ++line;
    char *bar = strstr(last, " | ");
    if (bar) {
        *bar = 0;
        snprintf(r.answer, sizeof r.answer, "%s", last);
        sscanf(bar + 3, "%u %u %llu %llu %llu %llu %llu", &r.mode, &r.reason, &r.owner, &r.hits, &r.misses, &r.stats, &r.gen);
    } else {
        snprintf(r.answer, sizeof r.answer, "no output (status %d): %s", r.status, buf);
    }
    return r;
}

static struct sub run(const char *const env[], const char *what, const char *arg)
{
    const char *args[] = {what, arg, NULL};
    return run_as(-1, PROG, env, args);
}

static const char *const NOENV[] = {NULL};
static const char *const OFF[] = {"LIELLA_NSCACHE=0", NULL};

/* ------------------------------------------------------------------ the DNS server */

struct counts {
    int q[16];
};
static struct counts *counts;
static const char *NAMES[] = {"ttl2.test", "ttl0.test", "neg.test", "nosoa.test", "www.one.test", "www.two.test", "real.test", "both.test", "xn--bcher-kva.test", "big.test", "negbig.test", NULL};

static int put_name(unsigned char *m, int at, const char *name)
{
    const char *p = name;
    while (*p) {
        const char *dot = strchr(p, '.');
        int l = dot ? (int)(dot - p) : (int)strlen(p);
        m[at++] = (unsigned char)l;
        memcpy(m + at, p, (size_t)l);
        at += l;
        p += l;
        if (*p == '.')
            p++;
    }
    m[at++] = 0;
    return at;
}

static int rr_head(unsigned char *m, int at, const char *name, int type, unsigned ttl)
{
    at = put_name(m, at, name);
    m[at++] = (unsigned char)(type >> 8), m[at++] = (unsigned char)type;
    m[at++] = 0, m[at++] = 1;
    m[at++] = (unsigned char)(ttl >> 24), m[at++] = (unsigned char)(ttl >> 16), m[at++] = (unsigned char)(ttl >> 8), m[at++] = (unsigned char)ttl;
    return at;
}

static void serve(int ready)
{
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(53)};
    inet_pton(AF_INET, "127.0.0.1", &a.sin_addr);
    if (bind(s, (struct sockaddr *)&a, sizeof a) != 0)
        _exit(1);
    (void)!write(ready, "r", 1);
    for (;;) {
        unsigned char q[512], m[1024];
        struct sockaddr_in from;
        socklen_t fl = sizeof from;
        ssize_t n = recvfrom(s, q, sizeof q, 0, (struct sockaddr *)&from, &fl);
        if (n < 17)
            continue;
        char name[256];
        int at = 12, o = 0;
        while (at < n && q[at] && o < 250) {
            int l = q[at++];
            if (o)
                name[o++] = '.';
            for (int i = 0; i < l && at < n && o < 250; i++)
                name[o++] = (char)q[at++];
        }
        name[o] = 0;
        int qend = at + 5;
        int type = (q[at + 1] << 8) | q[at + 2];
        for (int i = 0; NAMES[i]; i++)
            if (!strcasecmp(name, NAMES[i]))
                __atomic_fetch_add(&counts->q[i], 1, __ATOMIC_SEQ_CST);
        memcpy(m, q, (size_t)qend);
        m[2] = 0x81, m[3] = 0x80;
        m[6] = m[7] = m[8] = m[9] = m[10] = m[11] = 0;
        int len = qend;
        const char *addr4 = NULL, *addr6 = NULL;
        unsigned ttl = 300;
        int nx = 0, soa = 1;
        if (!strcasecmp(name, "ttl2.test"))
            addr4 = "192.0.2.20", ttl = 2;
        else if (!strcasecmp(name, "ttl0.test"))
            addr4 = "192.0.2.21", ttl = 0;
        else if (!strcasecmp(name, "neg.test"))
            nx = 1;
        else if (!strcasecmp(name, "nosoa.test"))
            nx = 1, soa = 0;
        else if (!strcasecmp(name, "negbig.test"))
            nx = 1, soa = 2;
        else if (!strcasecmp(name, "www.one.test"))
            addr4 = "192.0.2.11";
        else if (!strcasecmp(name, "www.two.test"))
            addr4 = "192.0.2.12";
        else if (!strcasecmp(name, "real.test"))
            addr4 = "192.0.2.30";
        else if (!strcasecmp(name, "both.test"))
            addr4 = "192.0.2.40", addr6 = "2001:db8::40";
        else if (!strcasecmp(name, "xn--bcher-kva.test"))
            addr4 = "192.0.2.50";
        else if (!strcasecmp(name, "big.test"))
            addr4 = "192.0.2.60", ttl = 7200;
        else
            nx = 1;
        if (nx) {
            m[3] = 0x83;
            if (soa) {
                m[9] = 1;
                len = rr_head(m, len, "test", 6, 600);
                int rdlen_at = len;
                len += 2;
                int start = len;
                len = put_name(m, len, "ns.test");
                len = put_name(m, len, "admin.test");
                /* soa 2: MINIMUM 7200 (with the SOA's TTL 600: ten minutes, above the 5-minute cap) */
                unsigned v[5] = {1, 7200, 3600, 1209600, soa == 2 ? 7200u : 2u};
                for (int i = 0; i < 5; i++)
                    m[len++] = (unsigned char)(v[i] >> 24), m[len++] = (unsigned char)(v[i] >> 16), m[len++] = (unsigned char)(v[i] >> 8), m[len++] = (unsigned char)v[i];
                m[rdlen_at] = (unsigned char)((len - start) >> 8), m[rdlen_at + 1] = (unsigned char)(len - start);
            }
        } else if (type == 1 && addr4) {
            m[7] = 1;
            len = rr_head(m, len, name, 1, ttl);
            m[len++] = 0, m[len++] = 4;
            inet_pton(AF_INET, addr4, m + len);
            len += 4;
        } else if (type == 28 && addr6) {
            m[7] = 1;
            len = rr_head(m, len, name, 28, ttl);
            m[len++] = 0, m[len++] = 16;
            inet_pton(AF_INET6, addr6, m + len);
            len += 16;
        } else {
            /* NODATA with an SOA (TTL 2). */
            m[9] = 1;
            len = rr_head(m, len, "test", 6, 2);
            int rdlen_at = len;
            len += 2;
            int start = len;
            len = put_name(m, len, "ns.test");
            len = put_name(m, len, "admin.test");
            unsigned v[5] = {1, 7200, 3600, 1209600, 2};
            for (int i = 0; i < 5; i++)
                m[len++] = (unsigned char)(v[i] >> 24), m[len++] = (unsigned char)(v[i] >> 16), m[len++] = (unsigned char)(v[i] >> 8), m[len++] = (unsigned char)v[i];
            m[rdlen_at] = (unsigned char)((len - start) >> 8), m[rdlen_at + 1] = (unsigned char)(len - start);
        }
        sendto(s, m, (size_t)len, 0, (struct sockaddr *)&from, fl);
    }
}

static int queries(const char *name)
{
    for (int i = 0; NAMES[i]; i++)
        if (!strcmp(NAMES[i], name))
            return __atomic_load_n(&counts->q[i], __ATOMIC_SEQ_CST);
    return -1;
}

/* ------------------------------------------------------------------ files */

struct fstate {
    int exists;
    struct stat s;
    unsigned long long sum;
};

static struct fstate fstate(const char *path)
{
    struct fstate f;
    memset(&f, 0, sizeof f);
    if (lstat(path, &f.s) != 0)
        return f;
    f.exists = 1;
    if (S_ISREG(f.s.st_mode)) {
        int fd = open(path, O_RDONLY | O_NONBLOCK);
        unsigned char b[65536];
        ssize_t n;
        f.sum = 1469598103934665603ULL;
        while (fd >= 0 && (n = read(fd, b, sizeof b)) > 0)
            for (ssize_t i = 0; i < n; i++)
                f.sum = (f.sum ^ b[i]) * 1099511628211ULL;
        if (fd >= 0)
            close(fd);
    }
    return f;
}

static int same_file(struct fstate a, struct fstate b)
{
    return a.exists == b.exists && a.s.st_ino == b.s.st_ino && a.s.st_size == b.s.st_size && a.s.st_mode == b.s.st_mode && a.s.st_uid == b.s.st_uid &&
           a.s.st_mtim.tv_sec == b.s.st_mtim.tv_sec && a.s.st_mtim.tv_nsec == b.s.st_mtim.tv_nsec && a.sum == b.sum;
}

static int mapped(const char *what)
{
    FILE *f = fopen("/proc/self/maps", "r");
    char line[512];
    int found = 0;
    while (f && fgets(line, sizeof line, f))
        if (strstr(line, what))
            found = 1;
    if (f)
        fclose(f);
    return found;
}

static void write_bytes(const char *path, long off, const void *b, size_t n)
{
    int fd = open(path, O_WRONLY);
    if (fd >= 0) {
        (void)!pwrite(fd, b, n, off);
        close(fd);
    }
}

/* ------------------------------------------------------------------ scenarios */

static void basics(void)
{
    char a[512];
    look("pw", "alice", a, sizeof a);
    struct liella_nscache_info s = st();
    check("shared-region", s.mode == LIELLA_NSCACHE_MODE_SHARED && s.owner == 0 && s.reason == 0 && s.misses == 1, "mode %u reason %u owner %llu misses %llu", s.mode, s.reason,
          (unsigned long long)s.owner, (unsigned long long)s.misses);
    struct stat r;
    check("region-file", stat(REGION0, &r) == 0 && S_ISREG(r.st_mode) && (r.st_mode & 07777) == 0600 && r.st_uid == 0 && r.st_size == 8 << 20, "mode %o size %lld",
          (unsigned)r.st_mode, (long long)r.st_size);
    look("pw", "alice", a, sizeof a);
    s = st();
    check("hit", !strcmp(a, "/bin/sh") && s.hits == 1, "%s hits %llu", a, (unsigned long long)s.hits);
    /* Every category answers the same from the table as from the sources. */
    const char *q[][2] = {{"pw", "alice"}, {"pw", "nosuch"}, {"pwuid", "2000"}, {"gr", "staff"}, {"gr", "nosuch"}, {"grl", "alice"}, {"serv", "http"}, {"serv", "www"},
                          {"proto", "tcp"}, {"net", "testnet"}, {"host", "alpha"}, {"host", "nosuchhost.test"}, {"gai", "both"}, {"gai", "localhost"}, {"host", "real.test"}};
    for (size_t i = 0; i < sizeof q / sizeof q[0]; i++) {
        char c1[512], c2[512];
        look(q[i][0], q[i][1], c1, sizeof c1);
        unsigned long long h0 = st().hits;
        look(q[i][0], q[i][1], c2, sizeof c2);
        struct sub off = run(OFF, q[i][0], q[i][1]);
        char name[64];
        snprintf(name, sizeof name, "cached-equals-uncached %s %s", q[i][0], q[i][1]);
        check(name, !strcmp(c1, c2) && !strcmp(c2, off.answer) && st().hits == h0 + 1 && off.mode == LIELLA_NSCACHE_MODE_OFF, "miss [%s] hit [%s] off [%s] mode %u", c1, c2,
              off.answer, off.mode);
    }
}

static void new_process_sees_edit(void)
{
    struct sub a = run(NOENV, "pw", "alice");
    char text[512];
    snprintf(text, sizeof text, "%s", PASSWD);
    char *sh = strstr(text, "/home/alice:/bin/sh");
    memcpy(sh + strlen("/home/alice:"), "/bin/zz", 7);
    put("/etc/passwd", text);
    struct sub b = run(NOENV, "pw", "alice");
    check("new-process-sees-edit", !strcmp(a.answer, "/bin/sh") && !strcmp(b.answer, "/bin/zz") && b.mode == LIELLA_NSCACHE_MODE_SHARED, "before [%s] after [%s] mode %u", a.answer,
          b.answer, b.mode);
    put("/etc/passwd", PASSWD);
}

static const char *WARM[][2] = {{"host", "alpha"}, {"pw", "alice"}, {"gr", "staff"}, {"serv", "http"}, {"proto", "tcp"}, {"net", "testnet"}};

static void warm(void)
{
    char a[512];
    for (int i = 0; i < 6; i++)
        look(WARM[i][0], WARM[i][1], a, sizeof a);
}

/* After a change and the window: which categories miss (bit per WARM row). */
static int missing(void)
{
    char a[512];
    int mask = 0;
    for (int i = 0; i < 6; i++) {
        unsigned long long m = st().misses;
        look(WARM[i][0], WARM[i][1], a, sizeof a);
        if (st().misses != m)
            mask |= 1 << i;
    }
    return mask;
}

static void invalidation_by_files(void)
{
    const char *global[] = {"/etc/nsswitch.conf", "/etc/host.conf", "/etc/resolv.conf", "/etc/gai.conf"};
    for (int i = 0; i < 4; i++) {
        warm();
        warm();
        touch_file(global[i]);
        msleep(1100);
        int m = missing();
        char name[96];
        snprintf(name, sizeof name, "global-file-invalidates-all %s", global[i]);
        check(name, m == 0x3f, "missed %#x", m);
    }
    const char *own[] = {"/etc/hosts", "/etc/passwd", "/etc/group", "/etc/services", "/etc/protocols", "/etc/networks"};
    for (int i = 0; i < 6; i++) {
        warm();
        warm();
        touch_file(own[i]);
        msleep(1100);
        int m = missing();
        char name[96];
        snprintf(name, sizeof name, "category-file-invalidates-its-own %s", own[i]);
        check(name, m == 1 << i, "missed %#x", m);
    }
    /* Appearing and disappearing are changes too. */
    warm();
    unlink("/etc/gai.conf");
    msleep(1100);
    check("file-disappears", missing() == 0x3f, "-");
    warm();
    put("/etc/gai.conf", "# back\n");
    msleep(1100);
    check("file-appears", missing() == 0x3f, "-");
}

static long long mono_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

static void rate_limit(void)
{
    char a[512];
    warm();
    struct liella_nscache_info s0, s1, s2;
    struct sub other;
    long long took = 0;
    int tries = 0;
    char text[512];
    /* The stat counts hold only while everything below stays inside the 1 s window that the
     * validating look() starts; on a loaded machine it may not. A run that overran says nothing
     * about the cache, so it is redone (up to 3 times). The answer checks stay unconditional. */
    for (;;) {
        tries++;
        if (tries > 1)
            put("/etc/passwd", PASSWD);
        msleep(1100);
        look("pw", "alice", a, sizeof a); /* validates: the window starts */
        long long t0 = mono_ms();
        s0 = st();
        for (int i = 0; i < 1000; i++)
            look("pw", "alice", a, sizeof a);
        s1 = st();
        /* Another process edits and validates (its first lookup); this one sees the generation move
         * within its own window, without a stat. */
        snprintf(text, sizeof text, "%s", PASSWD);
        memcpy(strstr(text, "/home/alice:/bin/sh") + strlen("/home/alice:"), "/bin/yy", 7);
        put("/etc/passwd", text);
        other = run(NOENV, "pw", "alice");
        look("pw", "alice", a, sizeof a);
        s2 = st();
        took = mono_ms() - t0;
        if (took < 900 || tries >= 3)
            break;
    }
    int inside = took < 900;
    check("no-stat-within-the-window", inside && (s1.stats == s0.stats && s1.hits == s0.hits + 1000), "stats %llu -> %llu, hits %llu -> %llu (try %d, %lld ms%s)", (unsigned long long)s0.stats,
          (unsigned long long)s1.stats, (unsigned long long)s0.hits, (unsigned long long)s1.hits, tries, took, inside ? "" : ", every try overran the 1 s window");
    /* (served from the entry the other process stored under the new generation) */
    check("shared-generation-seen-without-stat", !strcmp(other.answer, "/bin/yy") && !strcmp(a, "/bin/yy") && (inside && (s2.stats == s1.stats && s2.hits == s1.hits + 1)),
          "other [%s] here [%s] stats %llu -> %llu (try %d, %lld ms%s)", other.answer, a, (unsigned long long)s1.stats, (unsigned long long)s2.stats, tries, took, inside ? "" : ", every try overran the 1 s window");
    /* An edit nobody validated yet: within the window the old answer stays (the documented
     * lag); liella_nscache_invalidate bypasses the rate limit. */
    int lag = 0;
    /* The lag exists only while the window that the last validating look() started is open. On a loaded
     * machine it may have closed by now (the look then validates and sees the edit), which says nothing
     * about the cache: redo the setup (edited file, a validating look in a fresh window) up to 3 times.
     * Whether the old answer was served stays checked, and `invalidate-bypasses-the-rate-limit` fails if
     * it never was. */
    for (int t = 0; t < 3 && !lag; t++) {
        if (t) {
            put("/etc/passwd", text);
            msleep(1100);
            look("pw", "alice", a, sizeof a); /* validates: the window starts */
        }
        put("/etc/passwd", PASSWD);
        look("pw", "alice", a, sizeof a);
        lag = !strcmp(a, "/bin/yy");
    }
    struct liella_nscache_info s3 = st();
    check("invalidate-api", liella_nscache_invalidate(LIELLA_NSCACHE_PASSWD) == 0 && liella_nscache_invalidate(77) == -EINVAL, "-");
    look("pw", "alice", a, sizeof a);
    struct liella_nscache_info s4 = st();
    check("invalidate-bypasses-the-rate-limit", lag && !strcmp(a, "/bin/sh") && s4.stats > s3.stats && s4.generation != s3.generation, "lag %d now [%s] stats %llu -> %llu",
          lag, a, (unsigned long long)s3.stats, (unsigned long long)s4.stats);
}

static void keys_do_not_leak(void)
{
    /* LOCALDOMAIN, RES_OPTIONS, HOSTALIASES: per process, in the key. */
    const char *one[] = {"LOCALDOMAIN=one.test", NULL};
    const char *two[] = {"LOCALDOMAIN=two.test", NULL};
    struct sub a = run(one, "host", "www");
    struct sub b = run(two, "host", "www");
    check("key-localdomain", !strcmp(a.answer, "192.0.2.11") && !strcmp(b.answer, "192.0.2.12"), "[%s] [%s]", a.answer, b.answer);
    const char *noaaaa[] = {"RES_OPTIONS=no-aaaa", NULL};
    struct sub c = run(NOENV, "gai", "both.test");
    struct sub d = run(noaaaa, "gai", "both.test");
    struct sub e = run(NOENV, "gai", "both.test");
    check("key-res-options", strstr(c.answer, "2001:db8::40") && strstr(c.answer, "192.0.2.40") && !strcmp(d.answer, "192.0.2.40") && !strcmp(e.answer, c.answer) && e.hits == 1, "[%s] [%s] [%s]",
          c.answer, d.answer, e.answer);
    put("/mnt/aliases", "short real.test\n");
    const char *al[] = {"HOSTALIASES=/mnt/aliases", NULL};
    struct sub f = run(al, "host", "short");
    struct sub g = run(NOENV, "host", "short");
    put("/mnt/aliases", "short www.one.test\n");
    struct sub h = run(al, "host", "short");
    check("key-hostaliases", !strcmp(f.answer, "192.0.2.30") && strncmp(g.answer, "herr=", 5) == 0 && !strcmp(h.answer, "192.0.2.11"), "[%s] [%s] [%s]", f.answer, g.answer,
          h.answer);
    /* Locale and IDNA run after the cache. */
    const char *utf8[] = {"LC_ALL=C.UTF-8", NULL};
    struct sub i = run(utf8, "gaiidn", "b\xc3\xbc" "cher.test");
    struct sub j = run(NOENV, "gaiidn", "b\xc3\xbc" "cher.test");
    struct sub k = run(NOENV, "gai", "xn--bcher-kva.test");
    check("idna-after-the-cache", !strcmp(i.answer, "192.0.2.50 canon=b\xc3\xbc" "cher.test") && !strcmp(j.answer, "eai=-105") && !strcmp(k.answer, "192.0.2.50") && k.hits == 1,
          "[%s] [%s] [%s]", i.answer, j.answer, k.answer);
    /* AI_ADDRCONFIG runs after the cache: an IPv4-only interface drops the IPv6 address of a
     * cached answer, in the same process and in another. */
    char x[512], y[512], z[512];
    look("gaicfg", "both", x, sizeof x);
    if (system("ip link add nsc0 type dummy && ip addr add 10.9.9.9/24 dev nsc0 && ip link set nsc0 up") == 0) {
        unsigned long long h0 = st().hits;
        look("gaicfg", "both", y, sizeof y);
        look("gai", "both", z, sizeof z);
        check("addrconfig-after-the-cache", strstr(x, "2001:db8::2") && !strstr(y, "2001:db8::2") && strstr(z, "2001:db8::2") && st().hits == h0 + 2, "[%s] [%s] [%s]", x, y, z);
        (void)!system("ip link del nsc0");
    } else {
        printf("ok addrconfig-after-the-cache (skipped: no dummy interface)\n");
    }
    /* __nss_configure_lookup: that process bypasses the cache; others are unaffected. */
    pid_t p = fork();
    if (p == 0) {
        extern int __nss_configure_lookup(const char *, const char *);
        char t[512];
        look("host", "alpha", t, sizeof t);
        __nss_configure_lookup("hosts", "dns");
        struct liella_nscache_info s0 = st();
        look("host", "alpha", t, sizeof t);
        struct liella_nscache_info s1 = st();
        _exit(!strcmp(t, "herr=1") && s1.hits == s0.hits && s1.misses == s0.misses ? 0 : 1);
    }
    int ws;
    waitpid(p, &ws, 0);
    struct sub l = run(NOENV, "host", "alpha");
    check("overrides-bypass", WIFEXITED(ws) && WEXITSTATUS(ws) == 0 && !strcmp(l.answer, "192.0.2.1") && l.hits == 1, "child %d [%s]", ws, l.answer);
    /* A program-modified _res bypasses the cache. */
    struct sub m = run(NOENV, "host", "real.test");
    p = fork();
    if (p == 0) {
        char t[512];
        res_init();
        _res.nsaddr_list[0].sin_addr.s_addr = htonl(0x7f000009); /* nobody there */
        struct liella_nscache_info s0 = st();
        look("host", "real.test", t, sizeof t);
        struct liella_nscache_info s1 = st();
        _exit(strncmp(t, "herr=", 5) == 0 && s1.hits == s0.hits && s1.misses == s0.misses ? 0 : 1);
    }
    waitpid(p, &ws, 0);
    check("modified-res-bypasses", WIFEXITED(ws) && WEXITSTATUS(ws) == 0 && !strcmp(m.answer, "192.0.2.30"), "child %d [%s]", ws, m.answer);
}

static void ttl_caps(void);

static void ttls(void)
{
    char a[512];
    int q0 = queries("ttl2.test");
    look("host", "ttl2.test", a, sizeof a);
    look("host", "ttl2.test", a, sizeof a);
    int q1 = queries("ttl2.test");
    msleep(2200);
    look("host", "ttl2.test", a, sizeof a);
    int q2 = queries("ttl2.test");
    check("dns-ttl-expiry", q1 == q0 + 1 && q2 == q1 + 1 && !strcmp(a, "192.0.2.20"), "%d %d %d", q0, q1, q2);
    q0 = queries("ttl0.test");
    look("host", "ttl0.test", a, sizeof a);
    look("host", "ttl0.test", a, sizeof a);
    check("dns-ttl-zero-never-cached", queries("ttl0.test") == q0 + 2, "%d", queries("ttl0.test") - q0);
    q0 = queries("neg.test");
    look("host", "neg.test", a, sizeof a);
    look("host", "neg.test", a, sizeof a);
    q1 = queries("neg.test");
    msleep(2200);
    look("host", "neg.test", a, sizeof a);
    check("dns-negative-per-rfc2308", q1 == q0 + 1 && queries("neg.test") == q1 + 1 && !strcmp(a, "herr=1"), "%d %d", q1 - q0, queries("neg.test") - q1);
    q0 = queries("nosoa.test");
    look("host", "nosoa.test", a, sizeof a);
    look("host", "nosoa.test", a, sizeof a);
    check("dns-negative-without-soa-not-cached", queries("nosoa.test") == q0 + 2, "-");
    /* Shared between processes, within the TTL. */
    q0 = queries("big.test");
    struct sub x = run(NOENV, "host", "big.test");
    struct sub y = run(NOENV, "host", "big.test");
    check("dns-shared", queries("big.test") == q0 + 1 && !strcmp(x.answer, "192.0.2.60") && !strcmp(y.answer, x.answer) && y.hits == 1, "%d", queries("big.test") - q0);
    ttl_caps();
}

/* The region file's state and what a fresh process does with it. */
static void squat(const char *name, void (*make)(void), unsigned want_reason)
{
    liella_nscache_enable(0);
    unlink(REGION0);
    rmdir(REGION0);
    make();
    struct fstate before = fstate(REGION0);
    struct sub s = run(NOENV, "pw", "alice");
    struct sub t = run(NOENV, "host", "alpha");
    struct fstate after = fstate(REGION0);
    char n[96];
    snprintf(n, sizeof n, "squat-%s", name);
    check(n, s.mode == LIELLA_NSCACHE_MODE_PRIVATE && s.reason == want_reason && !strcmp(s.answer, "/bin/sh") && !strcmp(t.answer, "192.0.2.1") && same_file(before, after),
          "mode %u reason %u [%s] [%s] unchanged %d", s.mode, s.reason, s.answer, t.answer, same_file(before, after));
    unlink(REGION0);
    rmdir(REGION0);
    liella_nscache_enable(1);
}

static void mk_other_owner(void)
{
    int fd = open(REGION0, O_CREAT | O_EXCL | O_RDWR, 0600);
    (void)!ftruncate(fd, 8 << 20);
    (void)!fchown(fd, 65534, 65534);
    close(fd);
}
static void mk_wrong_mode(void)
{
    int fd = open(REGION0, O_CREAT | O_EXCL | O_RDWR, 0644);
    (void)!fchmod(fd, 0644);
    (void)!ftruncate(fd, 8 << 20);
    close(fd);
}
static void mk_symlink(void)
{
    put("/mnt/target", "x\n");
    (void)!symlink("/mnt/target", REGION0);
}
static void mk_fifo(void)
{
    (void)!mkfifo(REGION0, 0600);
}
static void mk_oversize(void)
{
    int fd = open(REGION0, O_CREAT | O_EXCL | O_RDWR, 0600);
    (void)!ftruncate(fd, 16 << 20);
    close(fd);
}
static void mk_dir(void)
{
    (void)!mkdir(REGION0, 0700);
}

static void squatting(void)
{
    squat("other-owner", mk_other_owner, LIELLA_NSCACHE_REASON_REFUSED);
    squat("wrong-mode", mk_wrong_mode, LIELLA_NSCACHE_REASON_REFUSED);
    squat("symlink", mk_symlink, LIELLA_NSCACHE_REASON_REFUSED);
    squat("fifo", mk_fifo, LIELLA_NSCACHE_REASON_REFUSED);
    squat("oversize", mk_oversize, LIELLA_NSCACHE_REASON_REFUSED);
    squat("directory", mk_dir, LIELLA_NSCACHE_REASON_REFUSED);
    struct fstate t = fstate("/mnt/target");
    check("squat-symlink-target-untouched", t.exists && t.s.st_size == 2, "size %lld", (long long)t.s.st_size);
}

/* Damage a populated region, then check fresh processes answer right and the region recovers. */
static void damaged(const char *name, void (*damage)(void))
{
    liella_nscache_enable(0); /* this process must not have it mapped while it is truncated */
    unlink(REGION0);
    run(NOENV, "pw", "alice");
    run(NOENV, "host", "alpha");
    run(NOENV, "serv", "http");
    damage();
    struct sub a = run(NOENV, "pw", "alice");
    struct sub b = run(NOENV, "host", "alpha");
    struct sub c = run(NOENV, "serv", "http");
    struct sub d = run(NOENV, "pw", "alice");
    char n[96];
    snprintf(n, sizeof n, "corrupt-%s", name);
    int right = !strcmp(a.answer, "/bin/sh") && !strcmp(b.answer, "192.0.2.1") && !strcmp(c.answer, "80") && !strcmp(d.answer, "/bin/sh");
    int alive = WIFEXITED(a.status) && WIFEXITED(b.status) && WIFEXITED(c.status) && WIFEXITED(d.status);
    check(n, right && alive && a.hits == 0, "[%s] [%s] [%s] [%s] hits %llu mode %u reason %u", a.answer, b.answer, c.answer, d.answer, a.hits, a.mode, a.reason);
    liella_nscache_enable(1);
}

static void dmg_random(void)
{
    unsigned char b[65536];
    unsigned long long x = 88172645463325252ULL;
    for (long off = 0; off < (8 << 20); off += sizeof b) {
        for (size_t i = 0; i < sizeof b; i++) {
            x ^= x << 13, x ^= x >> 7, x ^= x << 17;
            b[i] = (unsigned char)x;
        }
        write_bytes(REGION0, off, b, sizeof b);
    }
}
static void dmg_truncate_small(void)
{
    (void)!truncate(REGION0, 1000);
}
static void dmg_truncate_table(void)
{
    (void)!truncate(REGION0, 1 << 20);
}
static void dmg_slots(void)
{
    /* Every slot word: an offset past the arena, or a wrong serial. */
    unsigned long long w[1024];
    for (int i = 0; i < 1024; i++)
        w[i] = i & 1 ? 0xffffffULL | (5ULL << 24) : 0x123456789abcULL;
    for (long off = 80 * 8; off < (80 + 65536) * 8; off += sizeof w)
        write_bytes(REGION0, off, w, sizeof w);
}
static void dmg_payload(void)
{
    /* Flip bytes all over the arena: checksums fail. */
    unsigned char b[1] = {0x5a};
    for (long off = (80 + 65536) * 8; off < (80 + 65536) * 8 + 65536; off += 7)
        write_bytes(REGION0, off, b, 1);
}
static void dmg_writer_killed(void)
{
    /* A writer died in the middle of a change: the sequence word stays odd. */
    unsigned long long seq = 0;
    int fd = open(REGION0, O_RDONLY);
    (void)!pread(fd, &seq, 8, 7 * 8);
    close(fd);
    seq |= 1;
    write_bytes(REGION0, 7 * 8, &seq, 8);
}
static void dmg_header(void)
{
    unsigned long long w[3] = {0x4e41434c4c45494cULL, 99, 12345};
    write_bytes(REGION0, 0, w, sizeof w);
}

static void corruption(void)
{
    damaged("random-bytes", dmg_random);
    damaged("truncated-small", dmg_truncate_small);
    damaged("truncated-to-smaller-table", dmg_truncate_table);
    damaged("bad-offsets", dmg_slots);
    damaged("bad-checksums", dmg_payload);
    damaged("writer-killed-mid-write", dmg_writer_killed);
    damaged("bad-header", dmg_header);
    /* A real writer killed while it holds the lock: SIGKILL in a loop of stores. */
    liella_nscache_enable(0);
    unlink(REGION0);
    for (int round = 0; round < 20; round++) {
        pid_t p = fork();
        if (p == 0) {
            liella_nscache_enable(1);
            char t[64], a[512];
            for (int i = 0;; i++) {
                snprintf(t, sizeof t, "missing%d", i);
                look("pw", t, a, sizeof a);
            }
        }
        msleep(20 + round * 3);
        kill(p, SIGKILL);
        waitpid(p, NULL, 0);
        struct sub a = run(NOENV, "pw", "alice");
        if (strcmp(a.answer, "/bin/sh") || !WIFEXITED(a.status)) {
            check("corrupt-writer-sigkilled", 0, "round %d [%s]", round, a.answer);
            liella_nscache_enable(1);
            return;
        }
    }
    check("corrupt-writer-sigkilled", 1, "-");
    liella_nscache_enable(1);
}

static void credentials(void)
{
    /* AT_SECURE: the setuid copy run by uid 2000 (euid 0): private, LIELLA_NSCACHE=0 ignored. */
    const char *args[] = {"pw", "alice", NULL};
    struct sub s = run_as(2000, SUID, OFF, args);
    check("at-secure-private", s.mode == LIELLA_NSCACHE_MODE_PRIVATE && s.reason == LIELLA_NSCACHE_REASON_SECURE && !strcmp(s.answer, "/bin/sh"), "mode %u reason %u [%s]", s.mode,
          s.reason, s.answer);
    /* ruid != euid. */
    pid_t p = fork();
    if (p == 0) {
        char a[512];
        if (setresuid(2000, 0, 0) != 0)
            _exit(3);
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info i = st();
        _exit(i.mode == LIELLA_NSCACHE_MODE_PRIVATE && i.reason == LIELLA_NSCACHE_REASON_SECURE && !strcmp(a, "/bin/sh") ? 0 : 1);
    }
    int ws;
    waitpid(p, &ws, 0);
    check("ruid-differs-private", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d", ws);
    /* A credential change drops uid 0's mapping and re-keys: uid 2000's region, a miss. */
    p = fork();
    if (p == 0) {
        char a[512];
        look("pw", "alice", a, sizeof a);
        look("pw", "alice", a, sizeof a);
        int had = mapped("liella-nscache-0") && st().owner == 0 && st().hits >= 1;
        if (setgroups(0, NULL) != 0 || setresgid(2000, 2000, 2000) != 0 || setresuid(2000, 2000, 2000) != 0)
            _exit(3);
        prctl(PR_SET_DUMPABLE, 1); /* so that it may read its own /proc/self/maps */
        unsigned long long m = st().misses;
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info i = st();
        int ok = had && i.owner == 2000 && i.mode == LIELLA_NSCACHE_MODE_SHARED && i.misses == m + 1 && !mapped("liella-nscache-0") && mapped("liella-nscache-2000");
        _exit(ok ? 0 : 1);
    }
    waitpid(p, &ws, 0);
    check("credential-change-rekeys", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d", ws);
    /* UID reuse: uid 2000 was alice; alice is deleted and carol gets 2000. carol's first
     * process must not inherit alice's region. */
    struct sub a1 = run_as(2000, PROG, NOENV, (const char *const[]){"host", "alpha", NULL});
    struct sub a2 = run_as(2000, PROG, NOENV, (const char *const[]){"host", "alpha", NULL});
    char text[512];
    snprintf(text, sizeof text, "%s", PASSWD);
    memcpy(strstr(text, "alice:x:2000"), "carol", 5);
    put("/etc/passwd", text);
    struct sub c1 = run_as(2000, PROG, NOENV, (const char *const[]){"host", "alpha", NULL});
    check("uid-reuse-validity-stamp", a2.hits == 1 && c1.hits == 0 && c1.misses == 1 && c1.mode == LIELLA_NSCACHE_MODE_SHARED && c1.owner == 2000 && c1.gen != a2.gen,
          "alice hits %llu carol hits %llu misses %llu gen %llu -> %llu", a2.hits, c1.hits, c1.misses, a2.gen, c1.gen);
    (void)a1;
    put("/etc/passwd", PASSWD);
}

static void size_cap(void)
{
    liella_nscache_enable(0);
    unlink(REGION0);
    pid_t p = fork();
    if (p == 0) {
        liella_nscache_enable(1);
        char t[64], a[512];
        for (int i = 0; i < 120000; i++) {
            snprintf(t, sizeof t, "nouser%06d", i);
            look("pw", t, a, sizeof a);
        }
        unsigned long long m = st().misses;
        look("pw", "nouser000000", a, sizeof a);
        int evicted = st().misses == m + 1;
        m = st().misses;
        look("pw", "nouser119999", a, sizeof a);
        int kept = st().misses == m;
        struct stat r;
        int size = stat(REGION0, &r) == 0 && r.st_size == 8 << 20;
        unsigned long long stores = st().stores;
        /* The exit status names the conditions that failed: 1 evicted, 2 kept, 4 size, 8 stores. */
        _exit((evicted ? 0 : 1) | (kept ? 0 : 2) | (size ? 0 : 4) | (stores >= 120000 ? 0 : 8));
    }
    int ws;
    waitpid(p, &ws, 0);
    check("size-cap-and-eviction", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d (exit bits: 1 evicted, 2 kept, 4 size, 8 stores)", ws);
    liella_nscache_enable(1);
}

static void disabled(void)
{
    liella_nscache_enable(0);
    unlink(REGION0);
    struct sub s = run(OFF, "pw", "alice");
    struct stat r;
    check("disabled-no-file", s.mode == LIELLA_NSCACHE_MODE_OFF && s.stats == 0 && s.hits == 0 && s.misses == 0 && !strcmp(s.answer, "/bin/sh") && stat(REGION0, &r) != 0,
          "mode %u stats %llu hits %llu misses %llu answer [%s] region %s", s.mode, s.stats, s.hits, s.misses, s.answer,
          stat(REGION0, &r) == 0 ? "exists" : "absent");
    {
        pid_t p = fork();
        if (p == 0) {
            char a[512];
            liella_nscache_enable(1);
            look("pw", "alice", a, sizeof a);
            int on = mapped("liella-nscache") && st().mode == LIELLA_NSCACHE_MODE_SHARED;
            int was = (int)liella_nscache_enable(0);
            struct liella_nscache_info i = st();
            look("pw", "alice", a, sizeof a);
            struct liella_nscache_info j = st();
            int off = !mapped("liella-nscache") && i.mode == LIELLA_NSCACHE_MODE_OFF && j.stats == i.stats && j.hits == i.hits && j.misses == i.misses;
            int again = liella_nscache_enable(1) == 0;
            look("pw", "alice", a, sizeof a);
            int back = mapped("liella-nscache") && st().mode == LIELLA_NSCACHE_MODE_SHARED;
            /* One category off: its lookups are not counted, the others are. */
            int prev = (int)liella_nscache_category(LIELLA_NSCACHE_PASSWD, 0);
            unsigned long long h = st().hits, m = st().misses;
            look("pw", "alice", a, sizeof a);
            int cat_off = st().hits == h && st().misses == m && !(st().categories & 2);
            look("serv", "http", a, sizeof a);
            int other_on = st().hits + st().misses > h + m;
            int bad = liella_nscache_category(9, 1) == -EINVAL;
            _exit(on && was == 1 && off && again && back && prev == 1 && cat_off && other_on && bad ? 0 : 1);
        }
        int ws;
        waitpid(p, &ws, 0);
        check("native-enable-and-categories", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d", ws);
    }
    liella_nscache_enable(1);
}

/* The region file as bytes contains `needle`. */
static int region_has(const char *needle)
{
    int fd = open(REGION0, O_RDONLY);
    if (fd < 0)
        return 0;
    static unsigned char b[8 << 20];
    ssize_t n = read(fd, b, sizeof b);
    close(fd);
    size_t l = strlen(needle);
    for (ssize_t i = 0; n > 0 && i + (ssize_t)l <= n; i++)
        if (!memcmp(b + i, needle, l))
            return 1;
    return 0;
}

/* shadow and gshadow (lookups, _r forms, enumerations) never enter the cache. */
static void shadow_never_cached(void)
{
    char a[512];
    look("pw", "alice", a, sizeof a); /* the table is in use */
    struct liella_nscache_info s0 = st();
    char buf[4096];
    struct spwd sp, *spr = NULL;
    struct sgrp sg, *sgr = NULL;
    int found = 0;
    for (int round = 0; round < 2; round++) {
        found += getspnam("alice") != NULL;
        found += getspnam_r("root", &sp, buf, sizeof buf, &spr) == 0 && spr;
        found += getsgnam("staff") != NULL;
        found += getsgnam_r("root", &sg, buf, sizeof buf, &sgr) == 0 && sgr;
        setspent();
        while (getspent())
            found++;
        endspent();
        setsgent();
        while (getsgent())
            found++;
        endsgent();
    }
    struct liella_nscache_info s1 = st();
    int leaked = region_has("NSCSECRET");
    check("shadow-never-cached", found == 16 && s1.hits == s0.hits && s1.misses == s0.misses && s1.stores == s0.stores && s1.stats == s0.stats && !leaked,
          "found %d hits %llu->%llu misses %llu->%llu stores %llu->%llu leaked %d", found, (unsigned long long)s0.hits, (unsigned long long)s1.hits,
          (unsigned long long)s0.misses, (unsigned long long)s1.misses, (unsigned long long)s0.stores, (unsigned long long)s1.stores, leaked);
}

/* The lifetime stored with the entry whose key ends with `name` (the region's words: an entry
 * header of nine words, the key starting at word 9 after its two-byte prefix and parts). */
static long long stored_ttl(const char *name)
{
    int fd = open(REGION0, O_RDONLY);
    if (fd < 0)
        return -1;
    static unsigned long long w[1 << 20];
    ssize_t n = read(fd, w, sizeof w);
    close(fd);
    long words = n / 8;
    size_t l = strlen(name);
    long long found = -1;
    for (long i = 80 + 65536; i + 9 < words; i++) {
        if (w[i] >> 48 != 0x4e43 || ((w[i] >> 40) & 0xff) != 0)
            continue;
        unsigned long long len = w[i] & 0xffffffffULL, klen = w[i + 7] >> 32;
        if (len < 9 || i + (long)len > words || klen < l + 4)
            continue;
        const unsigned char *key = (const unsigned char *)&w[i + 9];
        /* the name is the key's last part, after its 4-byte length */
        unsigned plen = (unsigned)key[klen - l - 4] | (unsigned)key[klen - l - 3] << 8 | (unsigned)key[klen - l - 2] << 16 | (unsigned)key[klen - l - 1] << 24;
        if (klen >= l + 4 && plen == l && !memcmp(key + klen - l, name, l))
            found = (long long)w[i + 6];
    }
    return found;
}

static void ttl_caps(void)
{
    char a[512];
    look("host", "big.test", a, sizeof a);
    look("host", "negbig.test", a, sizeof a);
    long long pos = stored_ttl("big.test"), neg = stored_ttl("negbig.test"), within = stored_ttl("ttl2.test");
    check("dns-ttl-caps", pos == 3600 && neg == 300, "positive %lld (TTL 7200) negative %lld (SOA 7200); ttl2 %lld", pos, neg, within);
}

/* With the standard descriptors closed, the region (and every descriptor the library keeps)
 * lands above them: output to a closed descriptor still fails with EBADF and never reaches the
 * region (validation T12). */
static void closed_standard(void)
{
    static const char MARK[] = "T12-standard-output-marker";
    pid_t p = fork();
    if (p == 0) {
        close(0);
        close(1);
        close(2);
        char a[512];
        look("pw", "alice", a, sizeof a);
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info s = st();
        int bad = 0;
        for (int fd = 0; fd < 3; fd++) {
            errno = 0;
            if (write(fd, MARK, sizeof MARK - 1) != -1 || errno != EBADF)
                bad |= 1 << fd;
            if (fcntl(fd, F_GETFD) != -1)
                bad |= 8 << fd;
        }
        _exit(bad == 0 && s.mode == LIELLA_NSCACHE_MODE_SHARED && mapped("liella-nscache-0") && !strcmp(a, "/bin/sh") ? 0 : 10 + bad);
    }
    int ws;
    waitpid(p, &ws, 0);
    /* The marker is nowhere in the region. */
    int found = 0;
    FILE *f = fopen(REGION0, "r");
    if (f) {
        static char buf[8 << 20];
        size_t n = fread(buf, 1, sizeof buf, f);
        fclose(f);
        for (size_t i = 0; i + sizeof MARK - 1 <= n && !found; i++)
            found = !memcmp(buf + i, MARK, sizeof MARK - 1);
    }
    check("standard-descriptors-closed", WIFEXITED(ws) && WEXITSTATUS(ws) == 0 && f && !found, "status %d region %s marker %d", ws, f ? "read" : "missing", found);
}

/* unshare(CLONE_NEWUSER) and setns into a user namespace drop the mapping and re-key; in a
 * user namespace the table is private. */
static void namespaces(void)
{
    pid_t p = fork();
    if (p == 0) {
        char a[512];
        look("pw", "alice", a, sizeof a);
        look("pw", "alice", a, sizeof a);
        int had = mapped("liella-nscache-0") && st().mode == LIELLA_NSCACHE_MODE_SHARED;
        if (unshare(CLONE_NEWUSER) != 0)
            _exit(3);
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info i = st();
        /* Unmapped in the new namespace: the overflow UID, a private table. */
        _exit(had && geteuid() == 65534 && i.mode == LIELLA_NSCACHE_MODE_PRIVATE && i.reason == LIELLA_NSCACHE_REASON_SECURE && !mapped("liella-nscache-0") && !strcmp(a, "/bin/sh") ? 0 : 1);
    }
    int ws;
    waitpid(p, &ws, 0);
    check("unshare-newuser-rekeys", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d", ws);
    /* A helper in a user namespace mapping 0 to 0; a child joins it with setns. */
    int go[2], done[2];
    if (pipe(go) != 0 || pipe(done) != 0)
        return;
    pid_t helper = fork();
    if (helper == 0) {
        char c;
        if (unshare(CLONE_NEWUSER) != 0)
            _exit(3);
        (void)!write(go[1], "u", 1);
        (void)!read(done[0], &c, 1);
        _exit(0);
    }
    char c;
    (void)!read(go[0], &c, 1);
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/setgroups", (int)helper);
    int f = open(path, O_WRONLY);
    (void)!write(f, "deny", 4), close(f);
    snprintf(path, sizeof path, "/proc/%d/uid_map", (int)helper);
    f = open(path, O_WRONLY);
    (void)!write(f, "0 0 1", 5), close(f);
    snprintf(path, sizeof path, "/proc/%d/gid_map", (int)helper);
    f = open(path, O_WRONLY);
    (void)!write(f, "0 0 1", 5), close(f);
    snprintf(path, sizeof path, "/proc/%d/ns/user", (int)helper);
    p = fork();
    if (p == 0) {
        char a[512];
        int fd = open(path, O_RDONLY);
        look("pw", "alice", a, sizeof a);
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info s0 = st();
        if (fd < 0 || setns(fd, CLONE_NEWUSER) != 0)
            _exit(3);
        /* Within the validation window: only a re-key validates (stats) again. In a user
         * namespace, even one mapping 0 to 0, the IDs are that namespace's while /dev/shm is
         * everyone's: a private table (validation T13). */
        look("pw", "alice", a, sizeof a);
        struct liella_nscache_info s1 = st();
        _exit(s0.mode == LIELLA_NSCACHE_MODE_SHARED && s1.stats > s0.stats && s1.mode == LIELLA_NSCACHE_MODE_PRIVATE && s1.reason == LIELLA_NSCACHE_REASON_SECURE &&
                      !mapped("liella-nscache-0") && !strcmp(a, "/bin/sh")
                  ? 0
                  : 1);
    }
    waitpid(p, &ws, 0);
    (void)!write(done[1], "d", 1);
    waitpid(helper, NULL, 0);
    check("setns-user-rekeys", WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "status %d", ws);
}

int main(int argc, char **argv)
{
    if (argc >= 3 && !strcmp(argv[1], "sub")) {
        char a[512];
        setlocale(LC_ALL, "");
        /* A process's first cacheable lookup goes to the sources (D-16.10, cold start): make
         * one of another category first, so that the lookup checked uses the table. */
        (void)getprotobynumber(6);
        look(argv[2], argc > 3 ? argv[3] : "", a, sizeof a);
        print_status(a);
        return 0;
    }
    if (argc < 2 || strcmp(argv[1], "main") != 0 || geteuid() != 0) {
        fprintf(stderr, "usage: as root in the namespace of nscache_run.sh: %s main\n", argv[0]);
        return 99;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);
    fixture();
    /* The copies the fresh processes run (one setuid root, for AT_SECURE). */
    {
        char self[256];
        ssize_t n = readlink("/proc/self/exe", self, sizeof self - 1);
        self[n > 0 ? n : 0] = 0;
        char cmd[700];
        snprintf(cmd, sizeof cmd, "cp '%s' " PROG " && cp '%s' " SUID " && chmod 755 " PROG " && chown 0:0 " SUID " && chmod 4755 " SUID, self, self);
        if (system(cmd) != 0) {
            printf("FAIL setup: %s\n", cmd);
            return 100;
        }
    }
    counts = mmap(NULL, sizeof *counts, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    int ready[2];
    if (pipe(ready) != 0)
        return 100;
    pid_t server = fork();
    if (server == 0) {
        close(ready[0]);
        serve(ready[1]);
        _exit(0);
    }
    close(ready[1]);
    char c;
    if (read(ready[0], &c, 1) != 1) {
        printf("FAIL setup: no DNS server\n");
        return 100;
    }
    {
        /* The first lookup of a process goes to the sources and leaves the table alone. */
        struct liella_nscache_info s0 = st();
        (void)getprotobynumber(6);
        struct liella_nscache_info s1 = st();
        struct stat r;
        check("first-lookup-uses-the-sources", s0.mode == LIELLA_NSCACHE_MODE_IDLE && s1.mode == LIELLA_NSCACHE_MODE_IDLE && s1.hits + s1.misses + s1.stats == 0 && stat(REGION0, &r) != 0,
              "mode %u %u counters %llu", s0.mode, s1.mode, (unsigned long long)(s1.hits + s1.misses + s1.stats));
    }
    basics();
    shadow_never_cached();
    new_process_sees_edit();
    invalidation_by_files();
    rate_limit();
    keys_do_not_leak();
    ttls();
    squatting();
    corruption();
    credentials();
    closed_standard();
    namespaces();
    size_cap();
    disabled();
    kill(server, SIGKILL);
    waitpid(server, NULL, 0);
    printf("done %d failures\n", fails);
    return fails > 90 ? 90 : fails;
}
