/* The DNS stub resolver beyond dns_diff.c (spec §16.2, §16.4; phase 10, part 4): the resolv.conf
 * options, several servers, timeouts, rotate, edns0, single-request(-reopen), sortlist,
 * RES_OPTIONS, LOCALDOMAIN, HOSTALIASES, re-reading resolv.conf, the search order, IDNA and
 * getaddrinfo_a. Built against the host C library and against Liella's products, the outputs
 * must match line for line (dns_products.rs). Black-box: every line is a result of a public
 * function, or a query as the test's own DNS servers received it (a log the servers write).
 *
 * Usage: resolv_diff DIR                write DIR/<scenario>/etc for every scenario and run the
 *                                       offline functions;
 *        resolv_diff DIR SCENARIO       with DIR/SCENARIO/etc bind-mounted over /etc in private
 *                                       mount and network namespaces (resolv_run.sh, as root):
 *                                       start the servers and run the scenario.
 *
 * The servers (forked children; nothing leaves the namespace): 127.0.0.1, 127.0.0.3, 127.0.0.4
 * and ::1 answer from the zone below (`who.test` names the server that answered); 127.0.0.5
 * never answers; 127.0.0.6 answers REFUSED; 127.0.0.7 answers after 1.5 s; nothing listens on
 * 127.0.0.2; 127.0.0.1 port 5353 answers like 127.0.0.1. Each logs every query to /etc/qlog.
 * Before the log is read or cleared, every server is sent a marker query over UDP, which it
 * answers without logging: a server handles its datagrams in order, so the answer means every
 * query sent before it has been logged (no waiting for a fixed time). */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <errno.h>
#include <fcntl.h>
#include <locale.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <pthread.h>
#include <resolv.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

static char dir[512];

/* Declared by no installed header. */
extern const struct res_sym __p_type_syms[], __p_class_syms[];
extern unsigned long _getlong(const unsigned char *);
extern unsigned long _getshort(const unsigned char *);

/* ------------------------------------------------------------------ the scenarios */

static const struct {
    const char *name, *resolv, *nsswitch, *aliases;
} SCENARIOS[] = {
    {"conf",
     "# comment\n; comment\nnameserver 127.0.0.1\nnameserver ::1\nnameserver 127.0.0.3\nnameserver 127.0.0.4\n"
     "domain dom.test\nsearch s1.test s2.test s3.test s4.test s5.test s6.test s7.test\n"
     "sortlist 130.155.160.0/255.255.240.0 130.155.0.0 10.0.0.0/8 192.0.2.0/255.255.255.0 1.0.0.0 2.0.0.0 3.0.0.0 4.0.0.0 5.0.0.0 6.0.0.0 7.0.0.0\n"
     "options ndots:20 timeout:99 attempts:9 rotate edns0 single-request single-request-reopen no-tld-query use-vc no-reload trust-ad no-aaaa inet6 debug no-check-names bogus\n",
     "hosts: dns\n", NULL},
    {"conf2", "nameserver 127.0.0.1\nsearch a.test\ndomain b.test\noptions ndots:0 timeout:0 attempts:0\noptions rotate\n", "hosts: dns\n", NULL},
    {"conf3", "domain b.test\nsearch a.test\nnameserver 999.1.1.1\nnameserver 127.0.0.1 extra words\nnameserver\tfe80::1%lo\n", "hosts: dns\n", NULL},
    {"noconf", NULL, "hosts: dns\n", NULL},
    {"servers", "nameserver 127.0.0.2\nnameserver 127.0.0.5\nnameserver 127.0.0.1\noptions timeout:1 attempts:2\n", "hosts: dns\n", NULL},
    {"rotate", "nameserver 127.0.0.1\nnameserver 127.0.0.3\nnameserver 127.0.0.4\noptions rotate timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"norotate", "nameserver 127.0.0.1\nnameserver 127.0.0.3\nnameserver 127.0.0.4\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"silent1", "nameserver 127.0.0.5\noptions timeout:1 attempts:2\n", "hosts: dns\n", NULL},
    {"silent2", "nameserver 127.0.0.5\nnameserver 127.0.0.5\noptions timeout:1 attempts:2\n", "hosts: dns\n", NULL},
    {"refused", "nameserver 127.0.0.6\nnameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"edns", "nameserver 127.0.0.1\noptions edns0 timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"plain", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"single", "nameserver 127.0.0.1\noptions single-request timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"reopen", "nameserver 127.0.0.1\noptions single-request-reopen timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"sortlist", "nameserver 127.0.0.1\nsortlist 130.155.160.0/255.255.240.0 172.16.0.0\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"search", "nameserver 127.0.0.1\nsearch s1.test s2.test\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"notld", "nameserver 127.0.0.1\nsearch s1.test s2.test\noptions no-tld-query timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"env", "nameserver 127.0.0.1\nsearch s1.test\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"envspace", "nameserver 127.0.0.1\nsearch s1.test\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"envempty", "nameserver 127.0.0.1\nsearch s1.test\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"aliases", "nameserver 127.0.0.1\nsearch s1.test\noptions timeout:1 attempts:1\n", "hosts: dns\n", "myalias who.test\nMixed  a.test # c\nother\nfull.alias.x who.test\n"},
    {"reload", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"noreload", "nameserver 127.0.0.1\noptions no-reload timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"modify", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"idn", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns\n", NULL},
    {"gaia", "nameserver 127.0.0.1\noptions timeout:3 attempts:1\n", "hosts: dns\n", NULL},
};
static const int NSCEN = sizeof SCENARIOS / sizeof SCENARIOS[0];

static void put(const char *path, const char *text)
{
    FILE *f = fopen(path, "w");
    fputs(text, f);
    fclose(f);
}

static void fixtures(void)
{
    for (int i = 0; i < NSCEN; i++) {
        char p[1024];
        snprintf(p, sizeof p, "%s/%s", dir, SCENARIOS[i].name);
        mkdir(p, 0755);
        snprintf(p, sizeof p, "%s/%s/etc", dir, SCENARIOS[i].name);
        mkdir(p, 0755);
        if (SCENARIOS[i].resolv) {
            snprintf(p, sizeof p, "%s/%s/etc/resolv.conf", dir, SCENARIOS[i].name);
            put(p, SCENARIOS[i].resolv);
        }
        snprintf(p, sizeof p, "%s/%s/etc/nsswitch.conf", dir, SCENARIOS[i].name);
        put(p, SCENARIOS[i].nsswitch);
        snprintf(p, sizeof p, "%s/%s/etc/hosts", dir, SCENARIOS[i].name);
        put(p, "127.0.0.1 localhost\n");
        snprintf(p, sizeof p, "%s/%s/etc/services", dir, SCENARIOS[i].name);
        put(p, "http 80/tcp\n");
        if (SCENARIOS[i].aliases) {
            snprintf(p, sizeof p, "%s/%s/etc/hostaliases", dir, SCENARIOS[i].name);
            put(p, SCENARIOS[i].aliases);
        }
    }
}

/* ------------------------------------------------------------------ the servers */

enum { NORMAL, SILENT, REFUSE, DELAY };

#define SYNC_NAME "sync.probe.invalid"

static int srv_idx;  /* the last byte of the server's IPv4 address; 9 for ::1 */
static int srv_mode;

static void qlog(const char *fmt, ...)
{
    char line[512];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    int fd = open("/etc/qlog", O_WRONLY | O_APPEND | O_CREAT, 0644);
    if (fd >= 0) {
        (void)!write(fd, line, n);
        close(fd);
    }
}

static int read_name(const unsigned char *m, int len, int at, char *out)
{
    int n = 0, jumped = 0, end = -1, guard = 0;
    while (at < len && m[at] && guard++ < 64) {
        if ((m[at] & 0xc0) == 0xc0) {
            if (!jumped)
                end = at + 2;
            jumped = 1;
            at = ((m[at] & 0x3f) << 8) | m[at + 1];
            continue;
        }
        int l = m[at++];
        if (n)
            out[n++] = '.';
        for (int k = 0; k < l && at < len && n < 250; k++)
            out[n++] = (char)m[at++];
    }
    out[n] = 0;
    return jumped ? end : at + 1;
}

static int put_name(unsigned char *m, int at, const char *name)
{
    const char *p = name;
    while (*p) {
        const char *dot = strchr(p, '.');
        int l = dot ? (int)(dot - p) : (int)strlen(p);
        m[at++] = (unsigned char)l;
        memcpy(m + at, p, l);
        at += l;
        p += l;
        if (*p == '.')
            p++;
    }
    m[at++] = 0;
    return at;
}

/* The names whose repeated host lookups the query logs and server sequences compare get TTL 0:
 * their answers may not be cached (RFC 1035 §3.2.1), so a resolver that caches, Liella's
 * name-service cache (D-16.10), sends the same queries as one that does not. Every other answer
 * keeps TTL 300. The reference's output is the same either way: no line prints these TTLs. */
static const char *const UNCACHED[] = {"who.test", "many.test", "junk.test", "c1.test", "c2.test", "a.test", "sort.test", "xn--bcher-kva.test",
                                       "51.2.0.192.in-addr.arpa", "52.2.0.192.in-addr.arpa", "53.2.0.192.in-addr.arpa", "54.2.0.192.in-addr.arpa",
                                       "55.2.0.192.in-addr.arpa", "56.2.0.192.in-addr.arpa", NULL};

static unsigned ttl_of(const char *name)
{
    for (int i = 0; UNCACHED[i]; i++)
        if (!strcasecmp(name, UNCACHED[i]))
            return 0;
    return 300;
}

static int put_rr(unsigned char *m, int at, const char *name, int type, const char *data)
{
    at = put_name(m, at, name);
    m[at++] = type >> 8, m[at++] = type & 0xff;
    m[at++] = 0, m[at++] = 1;
    unsigned ttl = ttl_of(name);
    m[at++] = (unsigned char)(ttl >> 24), m[at++] = (unsigned char)(ttl >> 16), m[at++] = (unsigned char)(ttl >> 8), m[at++] = (unsigned char)ttl;
    int rdlen_at = at;
    at += 2;
    int start = at;
    if (type == T_A) {
        inet_pton(AF_INET, data, m + at);
        at += 4;
    } else if (type == T_AAAA) {
        inet_pton(AF_INET6, data, m + at);
        at += 16;
    } else if (type == T_CNAME || type == T_PTR) {
        at = put_name(m, at, data);
    } else if (type == T_TXT) {
        int l = (int)strlen(data);
        m[at++] = (unsigned char)l;
        memcpy(m + at, data, l);
        at += l;
    }
    int rd = at - start;
    m[rdlen_at] = rd >> 8, m[rdlen_at + 1] = rd & 0xff;
    return at;
}

struct answer {
    const char *name;
    int type;
    const char *data;
};

static const struct answer ZONE[] = {
    {"a.test", T_A, "192.0.2.10"},
    {"both.test", T_A, "192.0.2.11"},
    {"both.test", T_AAAA, "2001:db8::21"},
    {"par.test", T_A, "192.0.2.12"},
    {"par.test", T_AAAA, "2001:db8::12"},
    {"c1.test", T_CNAME, "c2.test"},
    {"c2.test", T_CNAME, "a.test"},
    {"junk.test", T_A, "192.0.2.13"},
    {"sort.test", T_A, "10.1.1.1"},
    {"sort.test", T_A, "192.0.2.5"},
    {"sort.test", T_A, "130.155.161.1"},
    {"sort.test", T_A, "172.16.0.1"},
    {"sort.test", T_A, "130.155.1.1"},
    {"x.s2.test", T_A, "192.0.2.60"},
    {"y.z.s1.test", T_A, "192.0.2.61"},
    {"y.z", T_A, "192.0.2.62"},
    {"nd.s1.test", T_AAAA, "2001:db8::63"},
    {"one", T_A, "192.0.2.64"},
    {"xn--bcher-kva.test", T_A, "192.0.2.50"},
    {"xn--bcher-kva.test", T_AAAA, "2001:db8::50"},
    {"slow.test", T_A, "192.0.2.70"},
    {"drop.test", T_A, "192.0.2.73"},
    {"spoof.test", T_A, "192.0.2.71"},
    {"51.2.0.192.in-addr.arpa", T_PTR, "xn--bcher-kva.test"},
    {"52.2.0.192.in-addr.arpa", T_PTR, "xn--zz-.test"},
    {"53.2.0.192.in-addr.arpa", T_PTR, "XN--BCHER-KVA.test"},
    {"54.2.0.192.in-addr.arpa", T_PTR, "xn--fa-hia.test"},
    {"55.2.0.192.in-addr.arpa", T_PTR, "plain.test"},
    {"56.2.0.192.in-addr.arpa", T_PTR, "a.xn--bcher-kva.xn--fa-hia.test"},
    {"10.2.0.192.in-addr.arpa", T_PTR, "a.test"},
};

static const char *tname(int t)
{
    static char b[16];
    switch (t) {
    case T_A: return "A";
    case T_AAAA: return "AAAA";
    case T_PTR: return "PTR";
    case T_TXT: return "TXT";
    case T_MX: return "MX";
    case T_CNAME: return "CNAME";
    case 255: return "ANY";
    }
    snprintf(b, sizeof b, "%d", t);
    return b;
}


/* Build the answer to query `q`; 0: no answer. `max`: the most bytes allowed (512 over UDP
 * without EDNS). Logs the query. */
static int respond(const unsigned char *q, int ql, unsigned char *r, int tcp, int max, int port)
{
    if (ql < 12)
        return 0;
    char name[256];
    int qend = read_name(q, ql, 12, name);
    if (qend + 4 > ql)
        return 0;
    int qtype = q[qend] << 8 | q[qend + 1];
    int arcount = q[10] << 8 | q[11];
    char opt[64] = "none";
    if (arcount > 0 && qend + 4 + 11 <= ql && q[qend + 4] == 0 && q[qend + 5] == 0 && q[qend + 6] == 41) {
        int size = q[qend + 7] << 8 | q[qend + 8];
        int flags = q[qend + 11] << 8 | q[qend + 12];
        snprintf(opt, sizeof opt, "size=%d do=%d", size, (flags & 0x8000) != 0);
        if (!tcp && size > max)
            max = size > 4096 ? 4096 : size;
    }
    qlog("  q srv%d %s %s %s rd=%d ad=%d cd=%d an=%d ns=%d ar=%d opt=%s qlen=%d%s\n", srv_idx, tcp ? "tcp" : "udp", name, tname(qtype), q[2] & 1,
         (q[3] & 0x20) != 0, (q[3] & 0x10) != 0, q[6] << 8 | q[7], q[8] << 8 | q[9], arcount, opt, ql, port < 0 ? "" : "");
    if (srv_mode == SILENT)
        return 0;
    for (char *c = name; *c; c++)
        if (*c >= 'A' && *c <= 'Z')
            *c += 32;
    memcpy(r, q, qend + 4);
    r[2] = 0x84 | (q[2] & 0x01);
    r[3] = 0x80;
    r[4] = 0, r[5] = 1;
    r[6] = r[7] = r[8] = r[9] = r[10] = r[11] = 0;
    int at = qend + 4, an = 0;
    if (srv_mode == REFUSE) {
        r[3] = 0x85;
        return at;
    }
    if (strcmp(name, "who.test") == 0 && qtype == T_A) {
        char a[32];
        snprintf(a, sizeof a, "192.0.2.%d", 100 + srv_idx);
        at = put_rr(r, at, "who.test", T_A, a);
        r[7] = 1;
        return at;
    }
    if (strcmp(name, "big.test") == 0) {
        char t[201];
        memset(t, 'x', 200);
        t[200] = 0;
        for (int i = 0; i < 3; i++) {
            t[0] = (char)('a' + i);
            at = put_rr(r, at, "big.test", T_TXT, t);
        }
        r[7] = 3;
        an = 3;
    } else if (strcmp(name, "many.test") == 0 && qtype == T_A) {
        for (int i = 1; i <= 40; i++) {
            char a[32];
            snprintf(a, sizeof a, "192.0.2.%d", i);
            at = put_rr(r, at, "many.test", T_A, a);
        }
        r[7] = 40;
        an = 40;
    } else if (strcmp(name, "junk.test") == 0 && qtype == T_A) {
        at = put_rr(r, at, "evil.test", T_A, "6.6.6.6");
        at = put_rr(r, at, "junk.test", T_A, "192.0.2.13");
        r[7] = 2;
        an = 2;
    } else {
        const char *target = name;
        int found = 0;
        for (int pass = 0; pass < 4; pass++) {
            int cname = 0;
            for (size_t i = 0; i < sizeof ZONE / sizeof ZONE[0]; i++) {
                if (strcasecmp(ZONE[i].name, target) != 0)
                    continue;
                found = 1;
                if (ZONE[i].type == qtype || qtype == T_ANY) {
                    at = put_rr(r, at, ZONE[i].name, ZONE[i].type, ZONE[i].data);
                    an++;
                } else if (ZONE[i].type == T_CNAME) {
                    at = put_rr(r, at, ZONE[i].name, ZONE[i].type, ZONE[i].data);
                    an++;
                    target = ZONE[i].data;
                    cname = 1;
                }
            }
            if (!cname)
                break;
        }
        if (!found && strcmp(name, "who.test") != 0)
            r[3] = 0x83;
        r[6] = an >> 8, r[7] = an & 0xff;
    }
    if (at > max) {
        r[2] |= 0x02;
        r[6] = r[7] = 0;
        return qend + 4;
    }
    return at;
}

static void serve(int ready, const char *ip, int port, int idx, int mode)
{
    srv_idx = idx;
    srv_mode = mode;
    int v6 = strchr(ip, ':') != NULL;
    int fam = v6 ? AF_INET6 : AF_INET;
    int u = socket(fam, SOCK_DGRAM, 0), t = socket(fam, SOCK_STREAM, 0);
    int one = 1;
    setsockopt(t, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_storage a;
    socklen_t al;
    memset(&a, 0, sizeof a);
    if (v6) {
        struct sockaddr_in6 *s = (struct sockaddr_in6 *)&a;
        s->sin6_family = AF_INET6;
        s->sin6_port = htons(port);
        inet_pton(AF_INET6, ip, &s->sin6_addr);
        al = sizeof *s;
    } else {
        struct sockaddr_in *s = (struct sockaddr_in *)&a;
        s->sin_family = AF_INET;
        s->sin_port = htons(port);
        inet_pton(AF_INET, ip, &s->sin_addr);
        al = sizeof *s;
    }
    if (bind(u, (struct sockaddr *)&a, al) || bind(t, (struct sockaddr *)&a, al) || listen(t, 8)) {
        perror("server bind");
        _exit(1);
    }
    /* A second socket on 127.0.0.8 for spoofed answers. */
    int sp = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in spa = {AF_INET, 0, {0}, {0}};
    inet_pton(AF_INET, "127.0.0.8", &spa.sin_addr);
    bind(sp, (struct sockaddr *)&spa, sizeof spa);
    (void)!write(ready, "r", 1);
    close(ready);
    unsigned char q[4096], r[8192];
    for (;;) {
        struct pollfd p[2] = {{u, POLLIN, 0}, {t, POLLIN, 0}};
        if (poll(p, 2, -1) < 0)
            continue;
        if (p[0].revents & POLLIN) {
            struct sockaddr_storage from;
            socklen_t fl = sizeof from;
            int n = (int)recvfrom(u, q, sizeof q, 0, (struct sockaddr *)&from, &fl);
            int fport = from.ss_family == AF_INET ? ntohs(((struct sockaddr_in *)&from)->sin_port) : ntohs(((struct sockaddr_in6 *)&from)->sin6_port);
            char name[256];
            read_name(q, n, 12, name);
            if (strcmp(name, SYNC_NAME) == 0) {
                /* The marker (sync_servers): answered at once, not logged. */
                q[2] |= 0x80;
                sendto(u, q, n, 0, (struct sockaddr *)&from, fl);
                continue;
            }
            if (strcasecmp(name, "par.test") == 0 && mode == NORMAL) {
                /* Hold the first query until a second one comes (300 ms at most). */
                int qt1 = q[n - 4] << 8 | q[n - 3];
                struct pollfd w = {u, POLLIN, 0};
                int second = poll(&w, 1, 300) > 0;
                unsigned char q2[4096];
                struct sockaddr_storage from2;
                socklen_t fl2 = sizeof from2;
                int n2 = 0, port2 = -1;
                if (second) {
                    n2 = (int)recvfrom(u, q2, sizeof q2, 0, (struct sockaddr *)&from2, &fl2);
                    port2 = from2.ss_family == AF_INET ? ntohs(((struct sockaddr_in *)&from2)->sin_port) : ntohs(((struct sockaddr_in6 *)&from2)->sin6_port);
                }
                qlog("  pair srv%d first=%s second-before-answer=%s same-port=%s\n", idx, tname(qt1), second ? "yes" : "no", second ? (port2 == fport ? "yes" : "no") : "-");
                int rl = respond(q, n, r, 0, 512, fport);
                if (rl > 0)
                    sendto(u, r, rl, 0, (struct sockaddr *)&from, fl);
                if (second) {
                    rl = respond(q2, n2, r, 0, 512, port2);
                    if (rl > 0)
                        sendto(u, r, rl, 0, (struct sockaddr *)&from2, fl2);
                }
                continue;
            }
            if (mode == DELAY || strcasecmp(name, "slow.test") == 0)
                usleep(1500000);
            static int drops;
            if (strcasecmp(name, "drop.test") == 0 && drops++ % 2 == 0) {
                /* Every other drop.test query is lost (logged, not answered). */
                unsigned char tmp[4096];
                respond(q, n, tmp, 0, 512, fport);
                continue;
            }
            int rl = respond(q, n, r, 0, 512, fport);
            if (rl > 0) {
                if (strcasecmp(name, "spoof.test") == 0) {
                    unsigned char s[8192];
                    memcpy(s, r, rl);
                    s[rl - 1] = 66; /* 192.0.2.71 -> 192.0.2.66 */
                    sendto(sp, s, rl, 0, (struct sockaddr *)&from, fl);
                    usleep(100000);
                }
                sendto(u, r, rl, 0, (struct sockaddr *)&from, fl);
            }
        }
        if (p[1].revents & POLLIN) {
            int c = accept(t, NULL, NULL);
            for (;;) {
                struct pollfd w = {c, POLLIN, 0};
                if (poll(&w, 1, 2000) <= 0)
                    break;
                unsigned char lb[2];
                if (recv(c, lb, 2, MSG_WAITALL) != 2)
                    break;
                int n = lb[0] << 8 | lb[1];
                if (recv(c, q, n, MSG_WAITALL) != n)
                    break;
                if (mode == DELAY)
                    usleep(1500000);
                int rl = respond(q, n, r, 1, 65535, -1);
                if (rl <= 0)
                    break;
                unsigned char hdr[2] = {(unsigned char)(rl >> 8), (unsigned char)rl};
                send(c, hdr, 2, 0);
                send(c, r, rl, 0);
            }
            close(c);
        }
    }
}

static pid_t servers[16];
static const char *server_ip[16];
static int server_port[16];
static int nservers;

static void start(const char *ip, int port, int idx, int mode)
{
    int p[2];
    if (pipe(p))
        exit(1);
    pid_t pid = fork();
    if (pid == 0) {
        close(p[0]);
        serve(p[1], ip, port, idx, mode);
        _exit(0);
    }
    close(p[1]);
    char c;
    if (read(p[0], &c, 1) != 1)
        exit(1);
    close(p[0]);
    server_ip[nservers] = ip;
    server_port[nservers] = port;
    servers[nservers++] = pid;
}

/* Send the marker to every server and wait for its answer (10 s at most each): afterwards
 * every query a server received before has been logged. */
static void sync_servers(void)
{
    unsigned char q[64] = {0x53, 0x59, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0};
    int ql = 12;
    const char *label = SYNC_NAME;
    while (*label) {
        const char *dot = strchr(label, '.');
        int len = dot ? (int)(dot - label) : (int)strlen(label);
        q[ql++] = (unsigned char)len;
        memcpy(q + ql, label, len);
        ql += len;
        label += len + (dot != NULL);
    }
    q[ql++] = 0;
    q[ql++] = 0;
    q[ql++] = 1;
    q[ql++] = 0;
    q[ql++] = 1;
    for (int i = 0; i < nservers; i++) {
        int v6 = strchr(server_ip[i], ':') != NULL;
        struct sockaddr_storage a;
        socklen_t al;
        memset(&a, 0, sizeof a);
        if (v6) {
            struct sockaddr_in6 *s = (struct sockaddr_in6 *)&a;
            s->sin6_family = AF_INET6;
            s->sin6_port = htons(server_port[i]);
            inet_pton(AF_INET6, server_ip[i], &s->sin6_addr);
            al = sizeof *s;
        } else {
            struct sockaddr_in *s = (struct sockaddr_in *)&a;
            s->sin_family = AF_INET;
            s->sin_port = htons(server_port[i]);
            inet_pton(AF_INET, server_ip[i], &s->sin_addr);
            al = sizeof *s;
        }
        int fd = socket(v6 ? AF_INET6 : AF_INET, SOCK_DGRAM, 0);
        if (fd < 0)
            continue;
        if (connect(fd, (struct sockaddr *)&a, al) == 0 && send(fd, q, ql, 0) == ql) {
            struct pollfd w = {fd, POLLIN, 0};
            unsigned char r[512];
            if (poll(&w, 1, 10000) <= 0 || recv(fd, r, sizeof r, 0) < 12)
                fprintf(stderr, "sync: no answer from %s port %d\n", server_ip[i], server_port[i]);
        }
        close(fd);
    }
}

/* ------------------------------------------------------------------ helpers */

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

/* The elapsed time of a lookup in whole seconds. The resolver's waits are whole multiples of the
 * timeout (1 s here), and the work around them takes milliseconds, but a loaded machine can add
 * a few hundred: rounding to the nearest second made 1.6 s print as 2. Up to 0.7 s past a whole
 * second still counts as that second. */
static void print_elapsed(double e)
{
    printf("  elapsed %.0f s\n", e < 0.7 ? 0.0 : (double)(long)(e + 0.3));
}

static void clear_log(void)
{
    sync_servers();
    int fd = open("/etc/qlog", O_WRONLY | O_TRUNC | O_CREAT, 0644);
    if (fd >= 0)
        close(fd);
}

static int cmp_lines(const void *a, const void *b)
{
    return strcmp(*(char *const *)a, *(char *const *)b);
}

/* Print the queries logged since clear_log (sorted when `sorted`), then clear the log. */
static void show_log(int sorted)
{
    sync_servers();
    FILE *f = fopen("/etc/qlog", "r");
    char *lines[256];
    int n = 0;
    char buf[512];
    while (f && n < 256 && fgets(buf, sizeof buf, f))
        lines[n++] = strdup(buf);
    if (f)
        fclose(f);
    if (sorted)
        qsort(lines, n, sizeof lines[0], cmp_lines);
    for (int i = 0; i < n; i++) {
        fputs(lines[i], stdout);
        free(lines[i]);
    }
    clear_log();
}

static void show_res(const char *tag, res_state s)
{
    printf("%s: retrans=%d retry=%d options=%lx nscount=%d ndots=%d nsort=%d pfcode=%lx\n", tag, s->retrans, s->retry, s->options, s->nscount, s->ndots, s->nsort,
           s->pfcode);
    for (int i = 0; i < MAXNS; i++) {
        char t[64] = "";
        if (s->nsaddr_list[i].sin_family == AF_INET)
            inet_ntop(AF_INET, &s->nsaddr_list[i].sin_addr, t, sizeof t);
        printf("  ns %d: family=%d %s:%d", i, s->nsaddr_list[i].sin_family, t, ntohs(s->nsaddr_list[i].sin_port));
        if (s->nsaddr_list[i].sin_family == 0 && s->_u._ext.nsaddrs[i]) {
            char t6[64];
            inet_ntop(AF_INET6, &s->_u._ext.nsaddrs[i]->sin6_addr, t6, sizeof t6);
            printf(" ext6=[%s]:%d scope=%u", t6, ntohs(s->_u._ext.nsaddrs[i]->sin6_port), s->_u._ext.nsaddrs[i]->sin6_scope_id);
        }
        printf("\n");
    }
    printf("  search:");
    for (int i = 0; i <= MAXDNSRCH && s->dnsrch[i]; i++)
        printf(" [%s]%s", s->dnsrch[i], s->dnsrch[i] >= s->defdname && s->dnsrch[i] < s->defdname + sizeof s->defdname ? "" : "(outside)");
    printf(" defdname=[%s]\n", s->defdname);
    for (int i = 0; i < s->nsort && i < MAXRESOLVSORT; i++) {
        char a[32], m[32];
        struct in_addr mk = {s->sort_list[i].mask};
        printf("  sort %d: %s/%s\n", i, inet_ntop(AF_INET, &s->sort_list[i].addr, a, sizeof a), inet_ntop(AF_INET, &mk, m, sizeof m));
    }
}

static void show_host(const char *what, const struct hostent *h)
{
    if (!h) {
        printf("%s: NULL h_errno=%d\n", what, h_errno);
        return;
    }
    printf("%s: [%s] aliases:", what, h->h_name);
    for (char **a = h->h_aliases; *a; a++)
        printf(" [%s]", *a);
    printf(" type=%d addrs:", h->h_addrtype);
    for (char **a = h->h_addr_list; *a; a++) {
        char t[64];
        printf(" %s", inet_ntop(h->h_addrtype, *a, t, sizeof t));
    }
    printf("\n");
}

static void print_ai(const struct addrinfo *res)
{
    for (const struct addrinfo *a = res; a; a = a->ai_next) {
        char t[64];
        if (a->ai_family == AF_INET)
            inet_ntop(AF_INET, &((struct sockaddr_in *)a->ai_addr)->sin_addr, t, sizeof t);
        else
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)a->ai_addr)->sin6_addr, t, sizeof t);
        printf(" %s", t);
        if (a->ai_canonname)
            printf(" canon=[%s]", a->ai_canonname);
    }
}

static void show_gai(const char *what, const char *node, int family, int flags)
{
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = family;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = flags;
    int r = getaddrinfo(node, NULL, &hints, &res);
    printf("%s [%s] fam=%d flags=%x: %d", what, node, family, flags, r);
    if (r == 0) {
        print_ai(res);
        freeaddrinfo(res);
    }
    printf("\n");
}

static void show_answer(const unsigned char *ans, int n)
{
    ns_msg m;
    if (n <= 0 || ns_initparse(ans, n, &m) != 0)
        return;
    printf(" rcode=%d tc=%d an=%d", ns_msg_getflag(m, ns_f_rcode), ns_msg_getflag(m, ns_f_tc), ns_msg_count(m, ns_s_an));
    for (int i = 0; i < ns_msg_count(m, ns_s_an); i++) {
        ns_rr rr;
        if (ns_parserr(&m, ns_s_an, i, &rr) == 0) {
            if (rr.type == T_A)
                printf(" %s=%d.%d.%d.%d", ns_rr_name(rr), rr.rdata[0], rr.rdata[1], rr.rdata[2], rr.rdata[3]);
            else
                printf(" %s/%d/%d", ns_rr_name(rr), rr.type, rr.rdlength);
        }
    }
}

static void show_query(const char *what, const char *name, int type, int search)
{
    unsigned char ans[8192];
    h_errno = 12345;
    int n = search ? res_search(name, C_IN, type, ans, sizeof ans) : res_query(name, C_IN, type, ans, sizeof ans);
    printf("%s [%s] type=%d: %d", what, name, type, n);
    if (n < 0)
        printf(" h_errno=%d", h_errno);
    show_answer(ans, n);
    printf("\n");
}

/* The server a who.test answer names (0: 127.0.0.1, 1: 127.0.0.3, 2: 127.0.0.4; -1: none). */
static int server_of(int octet)
{
    switch (octet) {
    case 101: return 0;
    case 103: return 1;
    case 104: return 2;
    }
    return -1;
}

static int who(int n, const unsigned char *ans)
{
    ns_msg m;
    ns_rr rr;
    if (n <= 0 || ns_initparse(ans, n, &m) != 0 || ns_parserr(&m, ns_s_an, 0, &rr) != 0 || rr.rdlength != 4)
        return -1;
    return server_of(rr.rdata[3]);
}

/* ------------------------------------------------------------------ getaddrinfo_a */

static volatile sig_atomic_t sig_count, sig_code, sig_val;

static void on_usr1(int s, siginfo_t *si, void *u)
{
    (void)s, (void)u;
    sig_count++;
    sig_code = si->si_code;
    sig_val = si->si_value.sival_int;
}

static volatile int thr_count, thr_val;
static pthread_mutex_t thr_lock = PTHREAD_MUTEX_INITIALIZER;

static void on_thread(union sigval v)
{
    pthread_mutex_lock(&thr_lock);
    thr_count++;
    thr_val = v.sival_int;
    pthread_mutex_unlock(&thr_lock);
}

static void on_alarm(int s)
{
    (void)s;
}

static void show_gaicb(const char *tag, struct gaicb *g)
{
    int e = gai_error(g);
    printf("  %s [%s]: gai_error=%d", tag, g->ar_name ? g->ar_name : "(null)", e);
    if (e == 0) {
        print_ai(g->ar_result);
        freeaddrinfo(g->ar_result);
        g->ar_result = NULL;
    }
    printf("\n");
}

static void wait_all(struct gaicb **list, int n)
{
    for (int k = 0; k < 100; k++) {
        int pending = 0;
        for (int i = 0; i < n; i++)
            if (list[i] && gai_error(list[i]) == EAI_INPROGRESS)
                pending = 1;
        if (!pending)
            return;
        struct timespec ts = {0, 100000000};
        gai_suspend((const struct gaicb *const *)list, n, &ts);
    }
}

static void gaia(void)
{
    struct addrinfo hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    struct gaicb g1 = {"a.test", "80", &hints, NULL}, g2 = {"nx.test", NULL, &hints, NULL}, g3 = {"192.0.2.9", "http", &hints, NULL};
    struct gaicb g4 = {"both.test", NULL, NULL, NULL};
    struct gaicb *list[] = {&g1, NULL, &g2, &g3, &g4};
    int r = getaddrinfo_a(GAI_WAIT, list, 5, NULL);
    printf("getaddrinfo_a WAIT: %d\n", r);
    show_gaicb("g1", &g1);
    show_gaicb("g2", &g2);
    show_gaicb("g3", &g3);
    show_gaicb("g4", &g4);

    errno = 0;
    r = getaddrinfo_a(7, list, 1, NULL);
    printf("getaddrinfo_a bad mode: %d errno=%d\n", r, errno);
    r = getaddrinfo_a(GAI_WAIT, list, 0, NULL);
    printf("getaddrinfo_a no entries: %d\n", r);
    struct gaicb *nulls[] = {NULL, NULL};
    r = getaddrinfo_a(GAI_NOWAIT, nulls, 2, NULL);
    printf("getaddrinfo_a only NULL: %d\n", r);
    struct timespec zero = {0, 0};
    r = gai_suspend((const struct gaicb *const *)nulls, 2, &zero);
    printf("gai_suspend only NULL: %d\n", r);

    /* NOWAIT. */
    struct gaicb d = {"a.test", NULL, &hints, NULL};
    struct gaicb *dl[] = {&d};
    r = getaddrinfo_a(GAI_NOWAIT, dl, 1, NULL);
    printf("getaddrinfo_a NOWAIT: %d gai_error=%d\n", r, gai_error(&d));
    wait_all(dl, 1);
    r = gai_suspend((const struct gaicb *const *)dl, 1, &zero);
    printf("gai_suspend after done: %d\n", r);
    printf("gai_cancel after done: %d\n", gai_cancel(&d));
    show_gaicb("d", &d);
    printf("gai_cancel NULL: %d\n", gai_cancel(NULL));

    /* SIGEV_SIGNAL. */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_usr1;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGUSR1, &sa, NULL);
    struct sigevent ev;
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = SIGUSR1;
    ev.sigev_value.sival_int = 42;
    struct gaicb s1 = {"a.test", NULL, &hints, NULL}, s2 = {"both.test", NULL, &hints, NULL};
    struct gaicb *sl[] = {&s1, &s2};
    r = getaddrinfo_a(GAI_NOWAIT, sl, 2, &ev);
    wait_all(sl, 2);
    for (int k = 0; k < 20 && sig_count == 0; k++)
        usleep(50000);
    usleep(200000);
    printf("SIGEV_SIGNAL: %d signals=%d code=%d value=%d\n", r, sig_count, sig_code, sig_val);
    show_gaicb("s1", &s1);
    show_gaicb("s2", &s2);
    sig_count = 0;
    struct gaicb s3 = {"a.test", NULL, &hints, NULL};
    struct gaicb *sl3[] = {&s3};
    r = getaddrinfo_a(GAI_WAIT, sl3, 1, &ev);
    usleep(300000);
    printf("SIGEV_SIGNAL with WAIT: %d signals=%d\n", r, sig_count);
    show_gaicb("s3", &s3);

    /* SIGEV_THREAD. */
    memset(&ev, 0, sizeof ev);
    ev.sigev_notify = SIGEV_THREAD;
    ev.sigev_notify_function = on_thread;
    ev.sigev_value.sival_int = 7;
    struct gaicb t1 = {"a.test", NULL, &hints, NULL}, t2 = {"nx.test", NULL, &hints, NULL};
    struct gaicb *tl[] = {&t1, &t2};
    r = getaddrinfo_a(GAI_NOWAIT, tl, 2, &ev);
    wait_all(tl, 2);
    for (int k = 0; k < 20; k++) {
        pthread_mutex_lock(&thr_lock);
        int c = thr_count;
        pthread_mutex_unlock(&thr_lock);
        if (c)
            break;
        usleep(50000);
    }
    usleep(200000);
    printf("SIGEV_THREAD: %d calls=%d value=%d\n", r, thr_count, thr_val);
    show_gaicb("t1", &t1);
    show_gaicb("t2", &t2);

    /* A request still in progress: timeout, cancel, interruption. */
    struct addrinfo h2 = hints;
    struct gaicb w = {"slow.test", NULL, &h2, NULL};
    struct gaicb *wl[] = {&w};
    /* The server holds slow.test for 1.5 s. */
    r = getaddrinfo_a(GAI_NOWAIT, wl, 1, NULL);
    struct timespec ts = {0, 100000000};
    int s = gai_suspend((const struct gaicb *const *)wl, 1, &ts);
    printf("in progress: %d suspend=%d gai_error=%d\n", r, s, gai_error(&w));
    struct sigaction al;
    memset(&al, 0, sizeof al);
    al.sa_handler = on_alarm;
    sigaction(SIGALRM, &al, NULL);
    struct itimerval it = {{0, 0}, {0, 100000}};
    setitimer(ITIMER_REAL, &it, NULL);
    s = gai_suspend((const struct gaicb *const *)wl, 1, NULL);
    printf("interrupted: %d\n", s);
    printf("gai_cancel in progress: %d\n", gai_cancel(&w));
    wait_all(wl, 1);
    show_gaicb("w", &w);
}

/* ------------------------------------------------------------------ the scenarios' actions */

/* The environment is set before the first resolver call of the process: the reference keeps
 * parts of a configuration read under one environment after the environment changes (an
 * artifact these scenarios stay clear of). */
static void env_scenario(const char *scen)
{
    if (strcmp(scen, "env") == 0) {
        setenv("LOCALDOMAIN", "l1.test l2.test\tl3.test", 1);
        setenv("RES_OPTIONS", "ndots:4 attempts:3 timeout:7 rotate bogus:1 no-aaaa", 1);
        res_init();
        show_res("env", &_res);
        clear_log();
        show_query("res_search", "full.alias.x", T_A, 1);
        show_query("res_search", "x", T_A, 1);
        show_gai("gai", "both.test", AF_UNSPEC, 0);
        show_log(0);
        return;
    }
    if (strcmp(scen, "envspace") == 0) {
        setenv("LOCALDOMAIN", "  l1.test l2.test ", 1);
        res_init();
        show_res("env with spaces", &_res);
        return;
    }
    if (strcmp(scen, "envempty") == 0) {
        setenv("LOCALDOMAIN", "", 1);
        res_init();
        show_res("empty LOCALDOMAIN", &_res);
        clear_log();
        show_query("res_search", "x", T_A, 1);
        show_log(0);
        return;
    }
    setenv("HOSTALIASES", "/etc/hostaliases", 1);
    res_init();
    printf("hostalias myalias: [%s]\n", hostalias("myalias") ? hostalias("myalias") : "(null)");
    printf("hostalias MYALIAS: [%s]\n", hostalias("MYALIAS") ? hostalias("MYALIAS") : "(null)");
    printf("hostalias mixed: [%s]\n", hostalias("mixed") ? hostalias("mixed") : "(null)");
    printf("hostalias other: [%s]\n", hostalias("other") ? hostalias("other") : "(null)");
    printf("hostalias full.alias.x: [%s]\n", hostalias("full.alias.x") ? hostalias("full.alias.x") : "(null)");
    printf("hostalias nosuch: [%s]\n", hostalias("nosuch") ? hostalias("nosuch") : "(null)");
    char buf[64];
    const char *ha = res_hostalias(&_res, "myalias", buf, sizeof buf);
    printf("res_hostalias: [%s] in-buf=%d\n", ha ? ha : "(null)", ha == buf);
    clear_log();
    show_query("res_search", "myalias", T_A, 1);
    show_query("res_query", "myalias", T_A, 0);
    show_query("res_search", "full.alias.x", T_A, 1);
    show_host("gethostbyname myalias", gethostbyname("myalias"));
    show_gai("gai", "myalias", AF_INET, AI_CANONNAME);
    show_log(0);
    _res.options |= RES_NOALIASES;
    show_query("res_search NOALIASES", "myalias", T_A, 1);
    printf("hostalias NOALIASES: [%s]\n", hostalias("myalias") ? hostalias("myalias") : "(null)");
    show_log(0);
    unsetenv("HOSTALIASES");
}

static void rewrite(const char *text)
{
    /* A different size and inode: write a new file and rename it over the old one. */
    put("/etc/resolv.conf.new", text);
    rename("/etc/resolv.conf.new", "/etc/resolv.conf");
}

static void reload_scenario(int noreload)
{
    res_init();
    show_query("res_query", "who.test", T_A, 0);
    show_host("gethostbyname", gethostbyname("who.test"));
    rewrite(noreload ? "nameserver 127.0.0.3\noptions no-reload timeout:1 attempts:1 ndots:3\n" : "nameserver 127.0.0.3\noptions timeout:1 attempts:1 ndots:3\n");
    show_query("res_query after rewrite", "who.test", T_A, 0);
    show_res("_res after query", &_res);
    show_host("gethostbyname after rewrite", gethostbyname("who.test"));
    show_gai("gai after rewrite", "who.test", AF_INET, 0);
    show_res("_res after gethostbyname", &_res);
    struct __res_state st;
    memset(&st, 0, sizeof st);
    res_ninit(&st);
    show_res("res_ninit after rewrite", &st);
    res_nclose(&st);
    res_init();
    show_res("res_init after rewrite", &_res);
    show_query("res_query after res_init", "who.test", T_A, 0);
    /* Delete the file. */
    unlink("/etc/resolv.conf");
    show_query("res_query no file", "who.test", T_A, 0);
    show_res("_res no file", &_res);
    show_host("gethostbyname no file", gethostbyname("who.test"));
}

static void modify_scenario(void)
{
    res_init();
    inet_pton(AF_INET, "127.0.0.3", &_res.nsaddr_list[0].sin_addr);
    show_query("res_query modified", "who.test", T_A, 0);
    show_host("gethostbyname modified", gethostbyname("who.test"));
    show_gai("gai modified", "who.test", AF_INET, 0);
    _res.nsaddr_list[0].sin_port = htons(5353);
    inet_pton(AF_INET, "127.0.0.1", &_res.nsaddr_list[0].sin_addr);
    show_query("res_query port 5353", "who.test", T_A, 0);
    show_log(0);
    _res.options |= RES_USEVC;
    show_query("res_query RES_USEVC", "a.test", T_A, 0);
    show_host("gethostbyname RES_USEVC", gethostbyname("a.test"));
    show_log(0);
    _res.options &= ~RES_USEVC;
    _res.options &= ~RES_RECURSE;
    show_query("res_query no RD", "a.test", T_A, 0);
    show_log(0);
    _res.options |= RES_RECURSE;
    _res.nscount = 0;
    show_query("res_query nscount 0", "a.test", T_A, 0);
    show_log(0);
    res_init();
    /* A private state on another port. */
    struct __res_state st;
    memset(&st, 0, sizeof st);
    res_ninit(&st);
    st.nsaddr_list[0].sin_port = htons(5353);
    unsigned char q[512], ans[512];
    int ql = res_nmkquery(&st, QUERY, "who.test", C_IN, T_A, NULL, 0, NULL, q, sizeof q);
    int n = res_nsend(&st, q, ql, ans, sizeof ans);
    printf("res_nsend port 5353: %d", n);
    show_answer(ans, n);
    printf("\n");
    n = res_nsend(&st, q, ql, ans, 20);
    printf("res_nsend small answer buffer: %d id-kept=%d tc=%d\n", n, n > 0 && ans[0] == q[0] && ans[1] == q[1], (ans[2] & 2) != 0);
    h_errno = 0;
    n = res_nquery(&st, "a.test", C_IN, T_A, ans, 20);
    printf("res_nquery small answer buffer: %d h_errno=%d res_h_errno=%d\n", n, h_errno, st.res_h_errno);
    n = res_nsend(&st, q, 5, ans, sizeof ans);
    printf("res_nsend short query: %d errno=%d\n", n, errno);
    n = res_nquery(&st, "nx.test", C_IN, T_A, ans, sizeof ans);
    printf("res_nquery nx: %d h_errno=%d res_h_errno=%d\n", n, h_errno, st.res_h_errno);
    n = res_nsend(&st, q, ql, ans, sizeof ans);
    printf("res_nsend again: %d res_h_errno=%d\n", n, st.res_h_errno);
    show_log(0);
    res_nclose(&st);
}

static void idn_scenario(void)
{
    static const char *names[] = {
        "bücher.test", "BÜCHER.TEST", "faß.test", "a‍b.test", "ab--cd.test", "ab--cdé.test", "-bücher.test", "bücher-.test", "a_b.test",
        "a_bé.test", "bücher。test", "xn--bcher-kva.test", "xn--invalid-.test", "XN--BCHER-KVA.test", "aש.test", "שלום.test", "😀.test",
        "Ⅻ.test", "́a.test", "bücher..test", "ﬁ.test", "Ⓐ.test", "a­b.test", "A.TEST", "bücher.test.", "ab.xn--fa-hia.test",
        "ꞵ.test", "ل٠۰.test", "a‌b.test", "क्‍.test", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaé.test", "ü",
    };
    setlocale(LC_ALL, "C.UTF-8");
    clear_log();
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        show_gai("gai idn", names[i], AF_INET, AI_IDN | AI_CANONNAME);
        show_log(1);
    }
    show_gai("gai idn canonidn", "bücher.test", AF_INET, AI_IDN | AI_CANONNAME | AI_CANONIDN);
    show_gai("gai canonidn only", "xn--bcher-kva.test", AF_INET, AI_CANONNAME | AI_CANONIDN);
    show_gai("gai idn no canon", "bücher.test", AF_UNSPEC, AI_IDN);
    show_gai("gai no idn utf8", "bücher.test", AF_INET, 0);
    show_log(1);
    show_host("gethostbyname utf8", gethostbyname("bücher.test"));
    show_log(1);
    static const char *ptrs[] = {"192.0.2.51", "192.0.2.52", "192.0.2.53", "192.0.2.54", "192.0.2.55", "192.0.2.56"};
    for (size_t i = 0; i < sizeof ptrs / sizeof ptrs[0]; i++) {
        struct sockaddr_in s4 = {AF_INET, htons(80), {0}, {0}};
        inet_pton(AF_INET, ptrs[i], &s4.sin_addr);
        char host[NI_MAXHOST];
        int r = getnameinfo((struct sockaddr *)&s4, sizeof s4, host, sizeof host, NULL, 0, NI_NAMEREQD | NI_IDN);
        printf("getnameinfo idn %s: %d [%s]\n", ptrs[i], r, r ? "" : host);
        r = getnameinfo((struct sockaddr *)&s4, sizeof s4, host, sizeof host, NULL, 0, NI_NAMEREQD);
        printf("getnameinfo %s: %d [%s]\n", ptrs[i], r, r ? "" : host);
        char small[8];
        r = getnameinfo((struct sockaddr *)&s4, sizeof s4, small, sizeof small, NULL, 0, NI_NAMEREQD | NI_IDN);
        printf("getnameinfo idn small %s: %d\n", ptrs[i], r);
    }
    show_log(1);
    setlocale(LC_ALL, "C");
    show_gai("gai idn C ascii", "a.test", AF_INET, AI_IDN);
    show_gai("gai idn C latin", "b\xfc" "cher.test", AF_INET, AI_IDN);
    struct sockaddr_in s4 = {AF_INET, htons(80), {0}, {0}};
    inet_pton(AF_INET, "192.0.2.51", &s4.sin_addr);
    char host[NI_MAXHOST];
    int r = getnameinfo((struct sockaddr *)&s4, sizeof s4, host, sizeof host, NULL, 0, NI_NAMEREQD | NI_IDN);
    printf("getnameinfo idn C: %d [%s]\n", r, r ? "" : host);
    show_log(1);
}

static void lookups(const char *scen)
{
    printf("== scenario %s\n", scen);
    start("127.0.0.1", 53, 1, NORMAL);
    start("127.0.0.1", 5353, 11, NORMAL);
    start("127.0.0.3", 53, 3, NORMAL);
    start("127.0.0.4", 53, 4, NORMAL);
    start("127.0.0.5", 53, 5, SILENT);
    start("127.0.0.6", 53, 6, REFUSE);
    start("127.0.0.7", 53, 7, DELAY);
    start("::1", 53, 9, NORMAL);
    clear_log();
    if (strncmp(scen, "conf", 4) == 0 || strcmp(scen, "noconf") == 0) {
        res_init();
        show_res("_res", &_res);
        struct __res_state st;
        memset(&st, 0, sizeof st);
        printf("res_ninit: %d\n", res_ninit(&st));
        show_res("res_ninit", &st);
        res_nclose(&st);
        if (strcmp(scen, "conf") == 0) {
            char buf[64];
            printf("p_option:");
            for (unsigned long o = 1; o; o <<= 1)
                if (o & 0x0fffffff)
                    printf(" [%s]", p_option(o));
            printf("\n");
            (void)buf;
        }
    } else if (strcmp(scen, "servers") == 0 || strncmp(scen, "silent", 6) == 0 || strcmp(scen, "refused") == 0) {
        res_init();
        double t0 = now();
        show_query("res_query", "who.test", T_A, 0);
        print_elapsed(now() - t0);
        show_log(0);
        t0 = now();
        show_host("gethostbyname", gethostbyname("who.test"));
        print_elapsed(now() - t0);
        show_log(0);
        t0 = now();
        show_gai("gai", "both.test", AF_UNSPEC, 0);
        print_elapsed(now() - t0);
        show_log(1);
    } else if (strcmp(scen, "rotate") == 0 || strcmp(scen, "norotate") == 0) {
        /* Which server answered each query (0: 127.0.0.1, 1: .3, 2: .4). With rotation the first
         * server a process asks is not fixed (the reference starts at varying places): the
         * steps between successive queries are printed, without rotation the servers. */
        int seq[32], k = 0;
        unsigned char ans[512];
        res_init();
        for (int i = 0; i < 4; i++)
            seq[k++] = who(res_query("who.test", C_IN, T_A, ans, sizeof ans), ans);
        for (int i = 0; i < 4; i++) {
            struct hostent *h = gethostbyname("who.test");
            seq[k++] = h ? server_of((unsigned char)h->h_addr_list[0][3]) : -1;
        }
        struct __res_state st;
        memset(&st, 0, sizeof st);
        res_ninit(&st);
        for (int i = 0; i < 3; i++)
            seq[k++] = who(res_nquery(&st, "who.test", C_IN, T_A, ans, sizeof ans), ans);
        show_res("own state after", &st);
        res_nclose(&st);
        res_init();
        seq[k++] = who(res_query("who.test", C_IN, T_A, ans, sizeof ans), ans);
        res_init();
        seq[k++] = who(res_query("who.test", C_IN, T_A, ans, sizeof ans), ans);
        memset(&st, 0, sizeof st);
        res_ninit(&st);
        res_nclose(&st);
        seq[k++] = who(res_query("who.test", C_IN, T_A, ans, sizeof ans), ans);
        for (int i = 0; i < 2; i++) {
            struct addrinfo hints, *res = NULL;
            memset(&hints, 0, sizeof hints);
            hints.ai_family = AF_INET;
            hints.ai_socktype = SOCK_STREAM;
            int r = getaddrinfo("who.test", NULL, &hints, &res);
            seq[k++] = r == 0 ? server_of(((unsigned char *)&((struct sockaddr_in *)res->ai_addr)->sin_addr)[3]) : -1;
            if (r == 0)
                freeaddrinfo(res);
        }
        if (strcmp(scen, "rotate") == 0) {
            printf("rotation steps:");
            for (int i = 1; i < k; i++)
                printf(" %d", seq[i] < 0 || seq[i - 1] < 0 ? -1 : (seq[i] - seq[i - 1] + 3) % 3);
        } else {
            printf("servers:");
            for (int i = 0; i < k; i++)
                printf(" %d", seq[i]);
        }
        printf("\n");
        sync_servers();
        FILE *f = fopen("/etc/qlog", "r");
        int lines = 0;
        char line[512];
        while (f && fgets(line, sizeof line, f))
            lines++;
        if (f)
            fclose(f);
        printf("queries sent: %d\n", lines);
        clear_log();
    } else if (strcmp(scen, "edns") == 0 || strcmp(scen, "plain") == 0) {
        res_init();
        show_gai("gai par", "par.test", AF_UNSPEC, 0);
        show_log(1);
        /* The A query is lost twice, the AAAA query answered: the lost one is sent again once. */
        show_gai("gai drop", "drop.test", AF_UNSPEC, 0);
        show_log(1);
        show_query("res_query", "a.test", T_A, 0);
        show_query("res_query", "big.test", T_TXT, 0);
        show_query("res_query", "many.test", T_A, 0);
        show_host("gethostbyname", gethostbyname("many.test"));
        show_gai("gai", "many.test", AF_INET, 0);
        show_log(0);
        show_query("res_query", "junk.test", T_A, 0);
        show_host("gethostbyname junk", gethostbyname("junk.test"));
        show_gai("gai junk", "junk.test", AF_INET, 0);
        show_host("gethostbyname c1", gethostbyname("c1.test"));
        show_gai("gai c1", "c1.test", AF_INET, AI_CANONNAME);
        show_host("gethostbyname spoof", gethostbyname("spoof.test"));
        show_log(0);
        _res.options |= RES_USE_DNSSEC;
        show_query("res_query DNSSEC", "a.test", T_A, 0);
        _res.options &= ~(RES_USE_DNSSEC | RES_USE_EDNS0);
        _res.options |= RES_TRUSTAD;
        show_query("res_query TRUSTAD", "a.test", T_A, 0);
        show_log(0);
        unsigned char q[512];
        int n = res_mkquery(QUERY, "a.test", C_IN, T_A, NULL, 0, NULL, q, sizeof q);
        printf("res_mkquery: %d flags=%02x%02x ar=%d\n", n, q[2], q[3], q[11]);
        _res.options |= RES_USE_EDNS0;
        n = res_mkquery(QUERY, "a.test", C_IN, T_A, NULL, 0, NULL, q, sizeof q);
        printf("res_mkquery EDNS0: %d flags=%02x%02x ar=%d\n", n, q[2], q[3], q[11]);
    } else if (strcmp(scen, "single") == 0 || strcmp(scen, "reopen") == 0) {
        res_init();
        show_gai("gai", "par.test", AF_UNSPEC, 0);
        show_log(1);
        show_host("gethostbyname2 v6", gethostbyname2("par.test", AF_INET6));
        show_log(1);
    } else if (strcmp(scen, "sortlist") == 0) {
        res_init();
        show_res("_res", &_res);
        show_host("gethostbyname", gethostbyname("sort.test"));
        show_gai("gai", "sort.test", AF_INET, 0);
        struct hostent he, *hp;
        char buf[1024];
        int herr;
        gethostbyname_r("sort.test", &he, buf, sizeof buf, &hp, &herr);
        show_host("gethostbyname_r", hp);
        show_host("gethostbyname2", gethostbyname2("sort.test", AF_INET));
        show_log(0);
    } else if (strcmp(scen, "search") == 0 || strcmp(scen, "notld") == 0) {
        res_init();
        static const char *names[] = {"x", "y.z", "y.z.", "nd", "one", "nosuch", "a.b.c.nosuch", "who.test"};
        for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
            show_query("res_search", names[i], T_A, 1);
            show_log(0);
        }
        show_host("gethostbyname x", gethostbyname("x"));
        show_gai("gai x", "x", AF_INET, AI_CANONNAME);
        show_host("gethostbyname nd", gethostbyname("nd"));
        show_log(0);
        _res.options &= ~RES_DNSRCH;
        show_query("res_search no DNSRCH", "x", T_A, 1);
        show_log(0);
        _res.options &= ~RES_DEFNAMES;
        show_query("res_search no DEFNAMES", "x", T_A, 1);
        show_log(0);
        res_init();
        unsigned char ans[512];
        int n = res_querydomain("x", "s2.test", C_IN, T_A, ans, sizeof ans);
        printf("res_querydomain x s2.test: %d\n", n);
        n = res_querydomain("x.", "s2.test", C_IN, T_A, ans, sizeof ans);
        printf("res_querydomain x. s2.test: %d\n", n);
        n = res_querydomain("x.s2.test", NULL, C_IN, T_A, ans, sizeof ans);
        printf("res_querydomain x.s2.test NULL: %d\n", n);
        n = res_querydomain("x.s2.test.", NULL, C_IN, T_A, ans, sizeof ans);
        printf("res_querydomain x.s2.test. NULL: %d\n", n);
        show_log(0);
    } else if (strncmp(scen, "env", 3) == 0 || strcmp(scen, "aliases") == 0) {
        env_scenario(scen);
    } else if (strcmp(scen, "reload") == 0 || strcmp(scen, "noreload") == 0) {
        reload_scenario(strcmp(scen, "noreload") == 0);
        show_log(0);
    } else if (strcmp(scen, "modify") == 0) {
        modify_scenario();
    } else if (strcmp(scen, "idn") == 0) {
        idn_scenario();
    } else if (strcmp(scen, "gaia") == 0) {
        gaia();
        show_log(1);
    }
    fflush(stdout);
    for (int i = 0; i < nservers; i++)
        kill(servers[i], SIGKILL);
    for (int i = 0; i < nservers; i++) {
        int st;
        syscall(SYS_wait4, servers[i], &st, 0, NULL);
    }
}

/* ------------------------------------------------------------------ offline: messages, texts */

static unsigned char msg[4096];
static int mlen;

static void w8(int v) { msg[mlen++] = (unsigned char)v; }
static void w16(int v) { w8(v >> 8), w8(v & 0xff); }
static void w32(unsigned v) { w16((int)(v >> 16)), w16((int)(v & 0xffff)); }
static void wname(const char *n)
{
    if (strcmp(n, ".") == 0) {
        w8(0);
        return;
    }
    mlen = put_name(msg, mlen, n);
}
static int rd_at;
static void rr_begin(const char *name, int type, int class, unsigned ttl)
{
    wname(name);
    w16(type), w16(class), w32(ttl);
    rd_at = mlen;
    w16(0);
}
static void rr_end(void)
{
    int l = mlen - rd_at - 2;
    msg[rd_at] = (unsigned char)(l >> 8), msg[rd_at + 1] = (unsigned char)l;
}
static void wstr(const char *s, int l)
{
    w8(l);
    memcpy(msg + mlen, s, l);
    mlen += l;
}

static void build_message(void)
{
    mlen = 0;
    w16(0x1234), w16(0x8580), w16(1), w16(19), w16(1), w16(2);
    wname("example"), w16(T_ANY), w16(C_IN);
    rr_begin("a.example", T_A, C_IN, 300), w8(192), w8(0), w8(2), w8(1), rr_end();
    rr_begin("a.example", T_AAAA, C_IN, 300);
    unsigned char a6[16];
    inet_pton(AF_INET6, "2001:db8::1", a6);
    memcpy(msg + mlen, a6, 16), mlen += 16, rr_end();
    rr_begin("c.example", T_CNAME, C_IN, 3600), wname("a.example"), rr_end();
    rr_begin("example", T_MX, C_IN, 86400), w16(10), wname("mail.example"), rr_end();
    rr_begin("example", T_TXT, C_IN, 86400), wstr("hello", 5), wstr("wor\"l\\d\001", 8), wstr("", 0), rr_end();
    rr_begin("example", T_SOA, C_IN, 86400), wname("ns1.example"), wname("admin.example"), w32(2024010101), w32(7200), w32(3600), w32(1209600), w32(300),
        rr_end();
    rr_begin("1.2.0.192.in-addr.arpa", T_PTR, C_IN, 60), wname("a.example"), rr_end();
    rr_begin("_sip._tcp.example", T_SRV, C_IN, 60), w16(10), w16(20), w16(5060), wname("sip.example"), rr_end();
    rr_begin("example", T_NS, C_IN, 60), msg[mlen++] = 0xc0, msg[mlen++] = 12, rr_end();
    rr_begin("example", T_HINFO, C_IN, 60), wstr("PC", 2), wstr("Linux x", 7), rr_end();
    rr_begin("example", 999, C_IN, 60), w8(1), w8(2), w8(0xff), rr_end();
    rr_begin("example", T_NAPTR, C_IN, 60), w16(100), w16(10), wstr("u", 1), wstr("E2U+sip", 7), wstr("!^.*$!sip:info@example.com!", 27), wname("."), rr_end();
    rr_begin("example", T_RP, C_IN, 60), wname("admin.example"), wname("txt.example"), rr_end();
    rr_begin("example", T_MINFO, C_IN, 60), wname("r.example"), wname("e.example"), rr_end();
    rr_begin("example", T_AFSDB, C_IN, 60), w16(1), wname("afs.example"), rr_end();
    rr_begin("example", T_RT, C_IN, 60), w16(10), wname("rt.example"), rr_end();
    rr_begin("example", T_X25, C_IN, 60), wstr("311061700956", 12), rr_end();
    rr_begin("example", T_ISDN, C_IN, 60), wstr("150862028003217", 15), wstr("004", 3), rr_end();
    unsigned char loc[16];
    int lr = loc_aton("52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000m 10m", loc);
    rr_begin("example", T_LOC, C_IN, 60);
    memcpy(msg + mlen, loc, lr > 0 ? 16 : 0), mlen += lr > 0 ? 16 : 0, rr_end();
    rr_begin("example", T_NS, C_IN, 60), wname("ns1.example"), rr_end();
    rr_begin("ns1.example", T_A, C_IN, 60), w8(192), w8(0), w8(2), w8(53), rr_end();
    rr_begin(".", 41, 1232, 0), rr_end();
}

static void offline(void)
{
    printf("== offline\n");
    build_message();
    ns_msg m;
    int ip = ns_initparse(msg, mlen, &m);
    printf("ns_initparse: %d id=%d an=%d ns=%d ar=%d\n", ip, ns_msg_id(m), ns_msg_count(m, ns_s_an), ns_msg_count(m, ns_s_ns), ns_msg_count(m, ns_s_ar));
    printf("flags:");
    for (int f = 0; f <= ns_f_max; f++)
        printf(" %d", ns_msg_getflag(m, f));
    printf("\n");
    for (int sect = ns_s_qd; sect < ns_s_max; sect++) {
        for (int i = 0; i < ns_msg_count(m, sect); i++) {
            ns_rr rr;
            int r = ns_parserr(&m, sect, i, &rr);
            char buf[1024];
            int n = sect == ns_s_qd ? -2 : ns_sprintrr(&m, &rr, NULL, NULL, buf, sizeof buf);
            printf("rr %d/%d: %d [%s] type=%d class=%d ttl=%u rdlen=%d sprintrr=%d\n", sect, i, r, ns_rr_name(rr), rr.type, rr.rr_class, rr.ttl, rr.rdlength, n);
            if (n > 0)
                printf("%s\n", buf);
            if (sect != ns_s_qd) {
                n = ns_sprintrr(&m, &rr, "example", "example", buf, sizeof buf);
                if (n > 0)
                    printf("origin: %s\n", buf);
            }
        }
    }
    ns_rr rr;
    errno = 0;
    { int r_ = ns_parserr(&m, ns_s_an, 99, &rr); printf("ns_parserr out of range: %d errno=%d\n", r_, errno); }
    errno = 0;
    { int r_ = ns_parserr(&m, 7, 0, &rr); printf("ns_parserr bad section: %d errno=%d\n", r_, errno); }
    ns_parserr(&m, ns_s_an, 0, &rr);
    char small[8];
    errno = 0;
    { int r_ = ns_sprintrr(&m, &rr, NULL, NULL, small, sizeof small); printf("ns_sprintrr small: %d errno=%d\n", r_, errno); }
    errno = 0;
    { int r_ = ns_initparse(msg, 11, &m); printf("ns_initparse short: %d errno=%d\n", r_, errno); }
    errno = 0;
    { int r_ = ns_initparse(msg, 60, &m); printf("ns_initparse cut: %d errno=%d\n", r_, errno); }
    errno = 0;
    { int r_ = ns_initparse(msg, mlen + 3, &m); printf("ns_initparse extra: %d errno=%d\n", r_, errno); }
    printf("ns_skiprr: %d\n", ns_skiprr(msg + 12, msg + mlen, ns_s_qd, 1));
    printf("ns_skiprr an 3: %d\n", ns_skiprr(msg + 12 + 13, msg + mlen, ns_s_an, 3));
    printf("ns_skiprr too many: %d\n", ns_skiprr(msg + 12 + 13, msg + mlen, ns_s_an, 200));
    char buf[2048];
    int n = ns_sprintrrf(msg, mlen, "www.example", ns_c_in, ns_t_cname, 3600, (const unsigned char *)"\003www\007example\000", 13, "example", "example", buf, sizeof buf);
    printf("ns_sprintrrf: %d [%s]\n", n, n > 0 ? buf : "");
    n = ns_sprintrrf(msg, mlen, "@", ns_c_chaos, ns_t_txt, 0, (const unsigned char *)"\002hi", 3, NULL, NULL, buf, sizeof buf);
    printf("ns_sprintrrf chaos: %d [%s]\n", n, n > 0 ? buf : "");
    n = ns_sprintrrf(msg, mlen, "x.example.", ns_c_in, ns_t_a, 1, (const unsigned char *)"\001\002\003", 3, NULL, NULL, buf, sizeof buf);
    printf("ns_sprintrrf bad A: %d\n", n);
    fflush(stdout);
    printf("-- p_query\n");
    fflush(stdout);
    p_query(msg);
    fflush(stdout);
    printf("-- fp_nquery\n");
    fflush(stdout);
    fp_nquery(msg, mlen, stdout);
    fflush(stdout);
    printf("-- fp_nquery 40\n");
    fflush(stdout);
    fp_nquery(msg, 40, stdout);
    fflush(stdout);
    printf("-- fp_query\n");
    fflush(stdout);
    {
        static unsigned char full[512];
        int l = res_mkquery(QUERY, "a.example", C_IN, T_A, NULL, 0, NULL, full, sizeof full);
        (void)l;
        fp_query(full, stdout);
    }
    fflush(stdout);
    printf("-- fp_resstat\n");
    struct __res_state st;
    memset(&st, 0, sizeof st);
    st.options = RES_INIT | RES_DEBUG | RES_USEVC | RES_RECURSE | RES_ROTATE | RES_TRUSTAD;
    st.nscount = 2;
    st.nsaddr_list[0].sin_family = AF_INET;
    st.nsaddr_list[0].sin_port = htons(53);
    inet_pton(AF_INET, "192.0.2.1", &st.nsaddr_list[0].sin_addr);
    st.nsaddr_list[1].sin_family = AF_INET;
    st.nsaddr_list[1].sin_port = htons(5353);
    inet_pton(AF_INET, "192.0.2.2", &st.nsaddr_list[1].sin_addr);
    fflush(stdout);
    fp_resstat(&st, stdout);
    fflush(stdout);
    printf("-- names\n");
    const unsigned char *e = msg + mlen;
    printf("p_cdname:");
    fflush(stdout);
    const unsigned char *p = p_cdname(msg + 12, msg, stdout);
    printf(" -> %ld\n", p ? (long)(p - msg) : -1L);
    printf("p_fqname:");
    fflush(stdout);
    p = p_fqname(msg + 12, msg, stdout);
    printf(" -> %ld\n", p ? (long)(p - msg) : -1L);
    p = p_fqnname(msg + 12, msg, (int)(e - msg), buf, sizeof buf);
    printf("p_fqnname: [%s] -> %ld\n", p ? buf : "", p ? (long)(p - msg) : -1L);
    p = p_fqnname(msg + 12, msg, (int)(e - msg), buf, 4);
    printf("p_fqnname small: %ld\n", p ? (long)(p - msg) : -1L);
    printf("p_cdnname:");
    fflush(stdout);
    p = p_cdnname(msg + 12, msg, (int)(e - msg), stdout);
    printf(" -> %ld\n", p ? (long)(p - msg) : -1L);
    printf("p_time:");
    static const unsigned pts[] = {0, 1, 59, 60, 3661, 1209600 + 86400 + 3600 + 60 + 1, 4294967295u};
    for (size_t i = 0; i < sizeof pts / sizeof pts[0]; i++)
        printf(" [%s]", p_time(pts[i]));
    printf("\n");
    printf("p_class:");
    for (int c = 0; c < 260; c++)
        if (strcmp(p_class(c), "") && (c < 6 || c > 250))
            printf(" %d=%s", c, p_class(c));
    printf("\n");
    printf("p_type:");
    for (int t = 0; t < 300; t++) {
        char want[16];
        snprintf(want, sizeof want, "%d", t);
        if (strcmp(p_type(t), want))
            printf(" %d=%s", t, p_type(t));
    }
    printf(" 32768=%s", p_type(32768));
    printf(" 32769=%s", p_type(32769));
    printf(" 65535=%s", p_type(65535));
    printf(" -1=%s\n", p_type(-1));
    printf("p_rcode:");
    for (int r = -1; r < 25; r++)
        printf(" %d=%s", r, p_rcode(r));
    printf("\n");
    int ok;
    { ok = 9; const char * r_ = sym_ntos(__p_type_syms, 15, &ok); printf("sym_ntos type 15: [%s] %d\n", r_, ok); }
    { ok = 9; const char * r_ = sym_ntos(__p_type_syms, 7777, &ok); printf("sym_ntos type 7777: [%s] %d\n", r_, ok); }
    { ok = 9; const char * r_ = sym_ntop(__p_type_syms, 15, &ok); printf("sym_ntop type 15: [%s] %d\n", r_, ok); }
    { ok = 9; const char * r_ = sym_ntop(__p_class_syms, 3, &ok); printf("sym_ntop class 3: [%s] %d\n", r_, ok); }
    { ok = 9; int r_ = sym_ston(__p_type_syms, "MX", &ok); printf("sym_ston MX: %d %d\n", r_, ok); }
    { ok = 9; int r_ = sym_ston(__p_type_syms, "mx", &ok); printf("sym_ston mx: %d %d\n", r_, ok); }
    { ok = 9; int r_ = sym_ston(__p_type_syms, "NOSUCH", &ok); printf("sym_ston nosuch: %d %d\n", r_, ok); }
    { ok = 9; int r_ = sym_ston(__p_class_syms, "CHAOS", &ok); printf("sym_ston CHAOS: %d %d\n", r_, ok); }
    printf("type_syms:");
    for (const struct res_sym *s = __p_type_syms; s->name; s++)
        printf(" %d=%s/%s", s->number, s->name, s->humanname ? s->humanname : "(null)");
    printf("\n");
    printf("class_syms:");
    for (const struct res_sym *s = __p_class_syms; s->name; s++)
        printf(" %d=%s/%s", s->number, s->name, s->humanname ? s->humanname : "(null)");
    printf("\n");
    printf("dn_count_labels: %d %d %d %d %d\n", dn_count_labels("a.b.c"), dn_count_labels("a.b.c."), dn_count_labels("*.b.c"), dn_count_labels(""),
           dn_count_labels("."));
    printf("ns_samedomain: %d %d %d %d %d %d\n", ns_samedomain("a.example.com", "example.com"), ns_samedomain("example.com", "example.com."),
           ns_samedomain("aexample.com", "example.com"), ns_samedomain("a.b", ""), ns_samedomain("a\\.example.com", "example.com"),
           ns_samedomain("A.EXAMPLE.com", "example.COM"));
    printf("ns_subdomain: %d %d %d\n", ns_subdomain("a.example.com", "example.com"), ns_subdomain("example.com", "example.com"),
           ns_subdomain("com", "example.com"));
    unsigned char wire[256], wl[256];
    ns_name_pton("WwW.Example.COM", wire, sizeof wire);
    printf("ns_name_ntol: %d", ns_name_ntol(wire, wl, sizeof wl));
    ns_name_ntop(wl, buf, sizeof buf);
    printf(" [%s] small=%d\n", buf, ns_name_ntol(wire, wl, 5));
    const unsigned char *cur = msg + 12;
    printf("ns_name_skip: %d -> %ld\n", ns_name_skip(&cur, msg + mlen), (long)(cur - msg));
    unsigned char out[512];
    const unsigned char *dnptrs[20] = {out, NULL}, **last = dnptrs + 20;
    int c1 = ns_name_compress("a.example.com", out + 12, 100, dnptrs, last);
    int c2 = ns_name_compress("b.example.com", out + 12 + c1, 100, dnptrs, last);
    printf("ns_name_compress: %d %d", c1, c2);
    for (int i = 0; i < 20 && dnptrs[i]; i++)
        printf(" %ld", (long)(dnptrs[i] - out));
    ns_name_rollback(out + 12 + c1, dnptrs, last);
    printf(" rollback:");
    for (int i = 0; i < 20 && dnptrs[i]; i++)
        printf(" %ld", (long)(dnptrs[i] - out));
    printf("\n");
    unsigned char un[256];
    printf("ns_name_unpack: %d", ns_name_unpack(out, out + 12 + c1 + c2, out + 12 + c1, un, sizeof un));
    ns_name_ntop(un, buf, sizeof buf);
    printf(" [%s] small=%d\n", buf, ns_name_unpack(out, out + 12 + c1 + c2, out + 12 + c1, un, 5));
    unsigned char pk[256];
    const unsigned char *dn2[4] = {pk, NULL};
    printf("ns_name_pack: %d nocomp=%d small=%d\n", ns_name_pack(un, pk + 12, 200, dn2, dn2 + 4), ns_name_pack(un, pk, 200, NULL, NULL), ns_name_pack(un, pk, 3, NULL, NULL));
    printf("ns_name_uncompress: %d", ns_name_uncompress(out, out + 12 + c1 + c2, out + 12 + c1, buf, sizeof buf));
    printf(" [%s]\n", buf);
    static const char *ptons[] = {"a.b", "a.b.", ".", "", "a..b", "\\065bc", "a\\256", "a\\", "a\\.b\\\\c", "x\\0", "\\[x]",
                                  "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.b", "\\032sp ace"};
    for (size_t i = 0; i < sizeof ptons / sizeof ptons[0]; i++) {
        memset(wire, 0xee, sizeof wire);
        int r = ns_name_pton(ptons[i], wire, sizeof wire);
        printf("ns_name_pton [%s]: %d", ptons[i], r);
        if (r >= 0) {
            printf(" wire=");
            for (int k = 0; k < 64 && k < (int)sizeof wire; k++) {
                printf("%02x", wire[k]);
                if (wire[k] == 0 && (k == 0 || 1))
                    break;
            }
            int t = ns_name_ntop(wire, buf, sizeof buf);
            printf(" ntop=%d [%s]", t, t >= 0 ? buf : "");
        }
        printf("\n");
    }
    char longname[400];
    memset(longname, 0, sizeof longname);
    for (int i = 0; i < 130; i++)
        strcat(longname, "a.");
    printf("ns_name_pton 260 chars: %d\n", ns_name_pton(longname, wire, sizeof wire));
    unsigned char weird[] = {5, 'a', ' ', '"', ';', 0x7f, 3, '(', ')', '$', 2, '@', '.', 0};
    printf("ns_name_ntop specials: %d", ns_name_ntop(weird, buf, sizeof buf));
    printf(" [%s] small=%d\n", buf, ns_name_ntop(weird, buf, 4));
    unsigned char bitlabel[] = {0x41, 8, 0xff, 0};
    printf("ns_name_ntop bitlabel: %d\n", ns_name_ntop(bitlabel, buf, sizeof buf));
    unsigned char ext[] = {0x80, 0};
    printf("ns_name_ntop extended: %d\n", ns_name_ntop(ext, buf, sizeof buf));

    unsigned char b4[4] = {0xff, 0xfe, 0x80, 0x01};
    printf("ns_get16: %u ns_get32: %lu\n", ns_get16(b4), ns_get32(b4));
    ns_put16(0xabcd, b4);
    printf("ns_put16: %02x%02x\n", b4[0], b4[1]);
    ns_put32(0x1234567890ul, b4);
    printf("ns_put32: %02x%02x%02x%02x\n", b4[0], b4[1], b4[2], b4[3]);

    /* The name checks, per character: in each position of a label. */
    static const char *kinds[] = {"hnok", "ownok", "mailok", "dnok"};
    int (*fns[])(const char *) = {res_hnok, res_ownok, res_mailok, res_dnok};
    for (int k = 0; k < 4; k++) {
        for (int pos = 0; pos < 4; pos++) {
            printf("%s pos%d:", kinds[k], pos);
            for (int ch = 1; ch < 256; ch++) {
                char nm[32];
                if (pos == 0)
                    snprintf(nm, sizeof nm, "%cab.cd", ch);
                else if (pos == 1)
                    snprintf(nm, sizeof nm, "a%cb.cd", ch);
                else if (pos == 2)
                    snprintf(nm, sizeof nm, "ab%c.cd", ch);
                else
                    snprintf(nm, sizeof nm, "ab.cd%c", ch);
                if (ch == '.')
                    continue;
                if (!fns[k](nm))
                    printf(" %02x", ch);
            }
            printf("\n");
        }
    }
    static const char *chk[] = {"a", "1", "1.2.3.4", "a.-b", "a-.b", "-", "*", "*.", "*.a", "a.*", "a.*.b", "**.a", "x.y-", "a_b", "_a", "user.na me.example",
                                "us\\.er.example", "a.b.", "a.b..", ".a", "a b", "a\\ b", "\\065.b", "é.b", "a/b", "foo@bar.example", "a.b\\"};
    for (size_t i = 0; i < sizeof chk / sizeof chk[0]; i++)
        printf("checks [%s]: hnok=%d ownok=%d mailok=%d dnok=%d\n", chk[i], res_hnok(chk[i]), res_ownok(chk[i]), res_mailok(chk[i]), res_dnok(chk[i]));

    /* Base64. */
    static const char *b64s[] = {"", "QQ==", "QUI=", "QUJD", "QQ", "QQ=", "Q===", "QU JD", " QUJD ", "QUJD\n", "QQ==QQ==", "QUJD=", "QUJ=RA==", "Q", "!!!!"};
    for (size_t i = 0; i < sizeof b64s / sizeof b64s[0]; i++) {
        unsigned char o[32];
        memset(o, 0, sizeof o);
        int r = b64_pton(b64s[i], o, sizeof o);
        int r0 = b64_pton(b64s[i], NULL, 0);
        printf("b64_pton [%s]: %d null=%d [%.*s]\n", b64s[i], r, r0, r > 0 ? r : 0, o);
    }
    for (int len = 0; len < 6; len++) {
        char o[32];
        int r = b64_ntop((const unsigned char *)"ABCDE", len, o, sizeof o);
        int exact = b64_ntop((const unsigned char *)"ABCDE", len, o, r > 0 ? r : 1);
        printf("b64_ntop %d: %d [%s] exact=%d\n", len, r, r >= 0 ? o : "", exact);
    }
    unsigned char o2[2];
    printf("b64_pton small: %d\n", b64_pton("QUJD", o2, 2));

    /* TTL texts. */
    static const char *pt[] = {"0", "1s", "1S", "1m1s", "1w", "4294967295", "4294967296", "1h ", " 1h", "1x", "w", "10000000000s", "99999w", "1h1h1h"};
    for (size_t i = 0; i < sizeof pt / sizeof pt[0]; i++) {
        unsigned long v = 777;
        int r = ns_parse_ttl(pt[i], &v);
        printf("ns_parse_ttl [%s]: %d %lu\n", pt[i], r, v);
    }
    static const unsigned long ft[] = {1, 61, 3599, 3601, 86399, 90000, 604801, 4294967295ul, 4294967296ul};
    for (size_t i = 0; i < sizeof ft / sizeof ft[0]; i++) {
        char t[64];
        int r = ns_format_ttl(ft[i], t, sizeof t);
        printf("ns_format_ttl %lu: %d [%s] small=%d\n", ft[i], r, t, ns_format_ttl(ft[i], t, 2));
    }
    static const char *dates[] = {"19700101000000", "20240229235959", "20230229000000", "20241301000000", "2024010100000", "202401010000001", "20240101x00000",
                                  "21060207062816", "19691231235959", "99991231235959", "20240132000000", "20240101250000", "20240101006000"};
    for (size_t i = 0; i < sizeof dates / sizeof dates[0]; i++) {
        int err = 5;
        uint32_t s = ns_datetosecs(dates[i], &err);
        printf("ns_datetosecs [%s]: %u err=%d\n", dates[i], s, err);
    }

    /* LOC. */
    static const char *locs[] = {"52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000m 10m", "42 21 54 N 71 06 18 W -24m 30m", "42 21 43.952 N 71 5 6.344 W -24m 1m 200m",
                                 "0 0 0 N 0 0 0 E 0m", "90 S 180 W 42849672.95m 90000000m 90000000m 90000000m", "91 0 0 N 0 0 0 E 0m", "bogus"};
    for (size_t i = 0; i < sizeof locs / sizeof locs[0]; i++) {
        unsigned char bin[16];
        memset(bin, 0, sizeof bin);
        int r = loc_aton(locs[i], bin);
        char text[128] = "";
        const char *t = loc_ntoa(bin, text);
        printf("loc [%s]: %d bin=", locs[i], r);
        for (int k = 0; k < 16; k++)
            printf("%02x", bin[k]);
        printf(" ntoa=[%s] same-buffer=%d\n", t ? t : "(null)", t == text);
    }
    unsigned char badver[16] = {1};
    char text[128];
    printf("loc_ntoa bad version: [%s]\n", loc_ntoa(badver, text));

    /* Old accessors. */
    unsigned char be[4] = {0x81, 0x02, 0x03, 0x04};
    printf("_getlong: %lx _getshort: %lx\n", _getlong(be), _getshort(be));
    printf("putlong/putshort:");
    putlong(0xdeadbeef, be);
    printf(" %02x%02x%02x%02x", be[0], be[1], be[2], be[3]);
    putshort(0xcafe, be);
    printf(" %02x%02x%02x%02x\n", be[0], be[1], be[2], be[3]);

    /* Queries. */
    unsigned char q[512], q2[512];
    n = res_mkquery(QUERY, "a.example", C_IN, T_A, NULL, 0, NULL, q, sizeof q);
    int n2 = res_mkquery(QUERY, "A.EXAMPLE.", C_IN, T_A, NULL, 0, NULL, q2, sizeof q2);
    memcpy(q2, q, 2);
    printf("res_mkquery: %d %d flags=%02x%02x\n", n, n2, q[2], q[3]);
    printf("res_nameinquery: %d %d %d %d\n", res_nameinquery("a.example", T_A, C_IN, q, q + n), res_nameinquery("A.example.", T_A, C_IN, q, q + n),
           res_nameinquery("a.example", T_AAAA, C_IN, q, q + n), res_nameinquery("b.example", T_A, C_IN, q, q + n));
    printf("res_queriesmatch: %d", res_queriesmatch(q, q + n, q2, q2 + n2));
    q2[0] ^= 1;
    printf(" %d", res_queriesmatch(q, q + n, q2, q2 + n2));
    q2[0] ^= 1;
    q2[2] |= 0x80;
    printf(" %d", res_queriesmatch(q, q + n, q2, q2 + n2));
    printf(" short=%d\n", res_queriesmatch(q, q + 5, q2, q2 + n2));
    static const int ops[] = {0, 1, 2, 4, 5, 6, 15};
    for (size_t i = 0; i < sizeof ops / sizeof ops[0]; i++) {
        n = res_mkquery(ops[i], "a.example", C_IN, T_SOA, NULL, 0, NULL, q, sizeof q);
        printf("res_mkquery op %d: %d", ops[i], n);
        if (n > 0) {
            printf(" hdr=");
            for (int k = 2; k < 12; k++)
                printf("%02x", q[k]);
            printf(" body=");
            for (int k = 12; k < n; k++)
                printf("%02x", q[k]);
        }
        printf("\n");
    }
    n = res_mkquery(QUERY, "a.example", C_CHAOS, T_TXT, (const unsigned char *)"xyz", 3, NULL, q, sizeof q);
    printf("res_mkquery data: %d\n", n);
    n = res_mkquery(NS_NOTIFY_OP, "a.example", C_IN, T_SOA, (const unsigned char *)"\003abc\000", 5, NULL, q, sizeof q);
    printf("res_mkquery notify data: %d", n);
    for (int k = 2; k < n; k++)
        printf("%02x", q[k]);
    printf("\n");
    n = res_mkquery(QUERY, "", C_IN, T_A, NULL, 0, NULL, q, sizeof q);
    printf("res_mkquery empty: %d\n", n);
    n = res_mkquery(QUERY, ".", C_IN, T_NS, NULL, 0, NULL, q, sizeof q);
    printf("res_mkquery root: %d\n", n);
    n = res_mkquery(QUERY, "a.example", C_IN, T_A, NULL, 0, NULL, q, 12);
    printf("res_mkquery 12: %d\n", n);
    n = res_mkquery(QUERY, "a.example", C_IN, T_A, NULL, 0, NULL, q, 27);
    printf("res_mkquery exact: %d %d\n", n, res_mkquery(QUERY, "a.example", C_IN, T_A, NULL, 0, NULL, q, 26));
    struct sockaddr_in sin = {AF_INET, htons(53), {0}, {0}};
    printf("res_isourserver:");
    res_init();
    sin.sin_addr = _res.nsaddr_list[0].sin_addr;
    printf(" %d", res_isourserver(&sin));
    sin.sin_port = htons(54);
    printf(" %d", res_isourserver(&sin));
    sin.sin_port = htons(53);
    sin.sin_addr.s_addr ^= 0x01000000;
    printf(" %d\n", res_isourserver(&sin));
    unsigned id1 = res_randomid(), id2 = res_randomid();
    printf("res_randomid: below-65536=%d\n", id1 < 65536 && id2 < 65536);
    char exp[256];
    unsigned char cn[] = {1, 'a', 0};
    printf("dn_expand small: %d %d\n", dn_expand(cn, cn + 3, cn, exp, 2), dn_expand(cn, cn + 3, cn, exp, 3));
    unsigned char root[] = {0};
    printf("dn_expand root: %d [%s]\n", dn_expand(root, root + 1, root, exp, sizeof exp), exp);
    unsigned char trunc[] = {3, 'a', 'b'};
    printf("dn_expand truncated: %d skip=%d\n", dn_expand(trunc, trunc + 3, trunc, exp, sizeof exp), dn_skipname(trunc, trunc + 3));
    unsigned char comp[64];
    printf("dn_comp small: %d %d\n", dn_comp("abc.de", comp, 4, NULL, NULL), dn_comp("abc.de", comp, 8, NULL, NULL));
    printf("hstrerror 6: [%s]\n", hstrerror(6));
}

int main(int argc, char **argv)
{
    snprintf(dir, sizeof dir, "%s", argc > 1 ? argv[1] : "/tmp");
    if (argc < 3) {
        fixtures();
        offline();
        printf("done\n");
        return 0;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);
    lookups(argv[2]);
    printf("done\n");
    return 0;
}
