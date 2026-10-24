/* Name services and databases (phase 10): address conversion, getaddrinfo/getnameinfo without
 * the network, the files databases, and the resolver's message helpers. The same file builds
 * against the products (`cargo xtask bench`) and against the host C library
 * (`cc -O2 bench/c/cbench_netdb.c`), so benchmarks/progress.md compares identical code. Prints
 * `name<TAB>ns/op` lines, the best of several rounds; the lines are formatted by hand so that the
 * measured functions do not format their own results.
 *
 * Nothing here may reach a DNS server: the numeric rows set AI_NUMERICHOST, "localhost" is in
 * /etc/hosts (the `files` source answers first), and the resolver rows only build and parse
 * messages in memory. Every call is checked once before it is timed; a call that fails ends the
 * program, so a row never times an error path by accident. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <grp.h>
#include <netdb.h>
#include <netinet/in.h>
#include <pwd.h>
#include <resolv.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

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
    (void)!write(2, "cbench_netdb: unexpected result: ", 33);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

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

static int (*volatile inet_pton_fn)(int, const char *, void *) = inet_pton;
static const char *(*volatile inet_ntop_fn)(int, const void *, char *, socklen_t) = inet_ntop;
static int (*volatile inet_aton_fn)(const char *, struct in_addr *) = inet_aton;
static int (*volatile getaddrinfo_fn)(const char *, const char *, const struct addrinfo *, struct addrinfo **) = getaddrinfo;
static int (*volatile getnameinfo_fn)(const struct sockaddr *, socklen_t, char *, socklen_t, char *, socklen_t, int) = getnameinfo;
static int (*volatile getpwnam_r_fn)(const char *, struct passwd *, char *, size_t, struct passwd **) = getpwnam_r;
static int (*volatile getgrgid_r_fn)(gid_t, struct group *, char *, size_t, struct group **) = getgrgid_r;
static int (*volatile getservbyname_r_fn)(const char *, const char *, struct servent *, char *, size_t, struct servent **) = getservbyname_r;

static unsigned char addr4[4], addr6[16];
static char text[64];

/* getaddrinfo + freeaddrinfo; returns the status, or 100 + the number of results. */
static long gai(const char *host, int flags, int family)
{
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = family;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = flags;
    int rc = getaddrinfo_fn(host, NULL, &hints, &res);
    if (rc != 0)
        return rc;
    long n = 100;
    for (struct addrinfo *p = res; p; p = p->ai_next)
        n++;
    freeaddrinfo(res);
    return n;
}

static struct sockaddr_in sin4;

static long gni(void)
{
    char host[NI_MAXHOST], serv[NI_MAXSERV];
    return getnameinfo_fn((const struct sockaddr *)&sin4, sizeof sin4, host, sizeof host, serv, sizeof serv,
                          NI_NUMERICHOST | NI_NUMERICSERV) == 0 ? host[0] + serv[0] : -1;
}

static char dbbuf[4096];

static long pw(void)
{
    struct passwd p, *r = NULL;
    return getpwnam_r_fn("root", &p, dbbuf, sizeof dbbuf, &r) == 0 && r ? (long)r->pw_uid + 1 : -1;
}

/* STRESS-ENV-AUDIT-2: CBENCH_STRESS_USER / CBENCH_STRESS_GROUP name the last of the 50,000 entries the
 * `names` profile appends to /etc/passwd and /etc/group; without them (the fixed environment) root. */
static const char *named_user, *named_group;

static long pw_named(void)
{
    struct passwd p, *r = NULL;
    return getpwnam_r_fn(named_user, &p, dbbuf, sizeof dbbuf, &r) == 0 && r ? 1 : -1;
}

static long gr_named(void)
{
    struct group g, *r = NULL;
    return getgrnam_r(named_group, &g, dbbuf, sizeof dbbuf, &r) == 0 && r ? 1 : -1;
}

/* The groups of the named user (every group entry is read). */
static long grouplist_named(void)
{
    gid_t groups[64];
    int n = 64;
    return getgrouplist(named_user, 0, groups, &n) >= 1 ? n : -1;
}

/* GETGROUPLIST-NAMES-PROBE: the row's result is checked, not only its count: base (0) first, then
 * each GID whose entry lists the named user, once (compared as a set with one getgrent pass, done
 * once outside the measurement). Under `names` that is 0, 249998 and 249999. */
static int grouplist_checked(void)
{
    gid_t groups[64], want[64];
    int n = 64, w = 0;
    if (getgrouplist(named_user, 0, groups, &n) < 1 || n < 1 || n > 64 || groups[0] != 0)
        return 0;
    struct group *g;
    setgrent();
    while ((g = getgrent()) != NULL) {
        int listed = 0, seen = g->gr_gid == 0;
        for (char **m = g->gr_mem; m && *m && !listed; m++)
            listed = strcmp(*m, named_user) == 0;
        for (int k = 0; k < w && !seen; k++)
            seen = want[k] == g->gr_gid;
        if (listed && !seen) {
            if (w == 64)
                break;
            want[w++] = g->gr_gid;
        }
    }
    endgrent();
    if (n != w + 1)
        return 0;
    for (int k = 0; k < w; k++) {
        int found = 0;
        for (int i = 1; i < n && !found; i++)
            found = groups[i] == want[k];
        if (!found)
            return 0;
    }
    return 1;
}

static long gr(void)
{
    struct group g, *r = NULL;
    return getgrgid_r_fn(0, &g, dbbuf, sizeof dbbuf, &r) == 0 && r ? (long)r->gr_name[0] : -1;
}

static long serv(void)
{
    struct servent s, *r = NULL;
    return getservbyname_r_fn("http", "tcp", &s, dbbuf, sizeof dbbuf, &r) == 0 && r ? (long)ntohs((unsigned short)r->s_port) : -1;
}

static unsigned char query[512];

static long mkq(void)
{
    return res_mkquery(ns_o_query, "www.example.com", ns_c_in, ns_t_a, NULL, 0, NULL, query, sizeof query);
}

/* A message whose header is followed by "example.com" (offset 12); `www.example.com` is then
 * compressed after it into a label and a pointer to offset 12. */
static unsigned char msg[512];
static const unsigned char *dnptrs[8];
static int first_len;

static long comp(void)
{
    dnptrs[0] = msg;
    dnptrs[1] = msg + 12;
    dnptrs[2] = NULL;
    return dn_comp("www.example.com", msg + 12 + first_len, (int)(sizeof msg) - 12 - first_len,
                   (unsigned char **)dnptrs, (unsigned char **)dnptrs + 8);
}

static char expanded[NS_MAXDNAME];

static long expand(void)
{
    return dn_expand(msg, msg + sizeof msg, msg + 12 + first_len, expanded, sizeof expanded);
}

int main(void)
{
    if (inet_pton(AF_INET, "192.168.100.200", addr4) != 1 ||
        inet_pton(AF_INET6, "2001:db8:85a3::8a2e:370:7334", addr6) != 1)
        fail("inet_pton");
    MEASURE("inet_pton_v4", 2000000, inet_pton_fn(AF_INET, "192.168.100.200", addr4));
    MEASURE("inet_pton_v6", 2000000, inet_pton_fn(AF_INET6, "2001:db8:85a3::8a2e:370:7334", addr6));
    MEASURE("inet_ntop_v4", 2000000, inet_ntop_fn(AF_INET, addr4, text, sizeof text)[0]);
    MEASURE("inet_ntop_v6", 2000000, inet_ntop_fn(AF_INET6, addr6, text, sizeof text)[0]);
    struct in_addr ia;
    if (inet_aton("10.1.2.3", &ia) != 1)
        fail("inet_aton");
    MEASURE("inet_aton", 2000000, inet_aton_fn("10.1.2.3", &ia));

    if (gai("192.0.2.1", AI_NUMERICHOST, AF_UNSPEC) != 101 || gai("2001:db8::1", AI_NUMERICHOST, AF_UNSPEC) != 101)
        fail("getaddrinfo numeric");
    MEASURE("getaddrinfo_numeric_v4", 500000, gai("192.0.2.1", AI_NUMERICHOST, AF_UNSPEC));
    MEASURE("getaddrinfo_numeric_v6", 500000, gai("2001:db8::1", AI_NUMERICHOST, AF_UNSPEC));
    if (gai("localhost", 0, AF_INET) != 101)
        fail("getaddrinfo localhost");
    MEASURE("getaddrinfo_localhost", 100000, gai("localhost", 0, AF_INET));

    sin4.sin_family = AF_INET;
    sin4.sin_port = htons(80);
    memcpy(&sin4.sin_addr, addr4, 4);
    if (gni() < 0)
        fail("getnameinfo");
    MEASURE("getnameinfo_numeric", 1000000, gni());

    if (pw() != 1 || gr() != 'r' || serv() != 80)
        fail("files databases");
    MEASURE("getpwnam_r_root", 100000, pw());
    MEASURE("getgrgid_r_root", 100000, gr());
    MEASURE("getservbyname_r_http", 50000, serv());
    named_user = getenv("CBENCH_STRESS_USER") ? getenv("CBENCH_STRESS_USER") : "root";
    named_group = getenv("CBENCH_STRESS_GROUP") ? getenv("CBENCH_STRESS_GROUP") : "root";
    if (pw_named() != 1 || gr_named() != 1 || grouplist_named() < 1 || !grouplist_checked())
        fail("named user and group");
    MEASURE("getpwnam_r_named", 300, pw_named());
    MEASURE("getgrnam_r_named", 300, gr_named());
    MEASURE("getgrouplist_named", 100, grouplist_named());

    if (mkq() <= 0)
        fail("res_mkquery");
    MEASURE("res_mkquery", 1000000, mkq());
    first_len = dn_comp("example.com", msg + 12, (int)(sizeof msg) - 12, NULL, NULL);
    if (first_len != 13 || comp() != 6 || expand() != 6 || strcmp(expanded, "www.example.com") != 0)
        fail("dn_comp/dn_expand");
    MEASURE("dn_comp", 2000000, comp());
    MEASURE("dn_expand", 2000000, expand());

    /* No IDNA row: without DNS the only way to reach the conversion is a non-ASCII name with
     * AI_IDN | AI_NUMERICHOST, and the two libraries do different work there (the reference
     * converts the name first, about 5.7 us; Liella reads it as a numeric address first and
     * fails before converting), so the row would not compare the same thing. A name in
     * /etc/hosts would, but the benchmark does not change the system's files. */
    return 0;
}
