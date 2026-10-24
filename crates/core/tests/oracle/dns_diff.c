/* The DNS stub resolver, getaddrinfo/getnameinfo through `dns`, and nsswitch.conf actions end to
 * end (spec §16.1-16.4; phase 10): built against the host C library and against Liella's
 * products, the outputs must match line for line (dns_products.rs). Black-box: every line is a
 * result of a public function. The DNS server is part of this program (a forked child answering
 * from a script on 127.0.0.1:53), so nothing depends on real DNS.
 *
 * Usage: dns_diff DIR                write DIR/<scenario>/etc for every scenario, and run the
 *                                    offline message functions;
 *        dns_diff DIR SCENARIO       with DIR/SCENARIO/etc bind-mounted over /etc in private
 *                                    mount and network namespaces (dns_run.sh, as root): start
 *                                    the server and run the scenario's lookups. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <resolv.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

static char dir[512];

/* ------------------------------------------------------------------ the scenarios */

static const struct {
    const char *name, *resolv, *nsswitch;
} SCENARIOS[] = {
    {"basic", "nameserver 127.0.0.1\noptions timeout:1 attempts:2\n", "hosts: files dns\n"},
    {"search", "nameserver 127.0.0.1\nsearch example.test other.test\noptions ndots:2 timeout:1 attempts:1\n", "hosts: dns\n"},
    {"trustad", "nameserver 127.0.0.1\noptions trust-ad timeout:1 attempts:1\n", "hosts: dns\n"},
    {"noaaaa", "nameserver 127.0.0.1\noptions no-aaaa timeout:1 attempts:1\n", "hosts: dns\n"},
    {"usevc", "nameserver 127.0.0.1\noptions use-vc timeout:1 attempts:1\n", "hosts: dns\n"},
    {"notfound_return", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns [NOTFOUND=return] files\n"},
    {"notfound_continue", "nameserver 127.0.0.1\noptions timeout:1 attempts:1\n", "hosts: dns files\n"},
    {"unavail", "nameserver 127.0.0.2\noptions timeout:1 attempts:1\n", "hosts: dns [!UNAVAIL=return] files\n"},
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
        snprintf(p, sizeof p, "%s/%s/etc/resolv.conf", dir, SCENARIOS[i].name);
        put(p, SCENARIOS[i].resolv);
        snprintf(p, sizeof p, "%s/%s/etc/nsswitch.conf", dir, SCENARIOS[i].name);
        put(p, SCENARIOS[i].nsswitch);
        snprintf(p, sizeof p, "%s/%s/etc/hosts", dir, SCENARIOS[i].name);
        put(p, "127.0.0.1 localhost\n192.0.2.200 local.test nx.test\n");
        snprintf(p, sizeof p, "%s/%s/etc/services", dir, SCENARIOS[i].name);
        put(p, "http 80/tcp\n");
    }
}

/* ------------------------------------------------------------------ the server */

struct answer {
    const char *name;
    int type;
    const char *data;
};

/* The zone: (name, type, data); CNAME data is a name, PTR data a name. */
static const struct answer ZONE[] = {
    {"a.test", T_A, "192.0.2.10"},
    {"aaaa.test", T_AAAA, "2001:db8::20"},
    {"both.test", T_A, "192.0.2.11"},
    {"both.test", T_AAAA, "2001:db8::21"},
    {"multi.test", T_A, "192.0.2.12"},
    {"multi.test", T_A, "192.0.2.13"},
    {"cname.test", T_CNAME, "a.test"},
    {"trunc.test", T_A, "192.0.2.30"},
    {"slow.test", T_A, "192.0.2.31"},
    {"badid.test", T_A, "192.0.2.32"},
    {"ad.test", T_A, "192.0.2.33"},
    {"host.example.test", T_A, "192.0.2.40"},
    {"x.y.example.test", T_A, "192.0.2.41"},
    {"x.y", T_A, "192.0.2.42"},
    {"xn--bcher-kva.test", T_A, "192.0.2.50"},
    {"mx.test", T_MX, "10 mail.test"},
    {"txt.test", T_TXT, "hello world"},
    {"10.2.0.192.in-addr.arpa", T_PTR, "a.test"},
    {"0.2.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa", T_PTR, "aaaa.test"},
};

static int read_name(const unsigned char *m, int len, int at, char *out)
{
    int n = 0;
    while (at < len && m[at]) {
        int l = m[at++];
        if (n)
            out[n++] = '.';
        for (int k = 0; k < l && at < len; k++)
            out[n++] = (char)m[at++];
    }
    out[n] = 0;
    return at + 1;
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

static int put_rr(unsigned char *m, int at, const char *name, int type, const char *data)
{
    at = put_name(m, at, name);
    m[at++] = type >> 8, m[at++] = type & 0xff;
    m[at++] = 0, m[at++] = 1;
    m[at++] = 0, m[at++] = 0, m[at++] = 0x01, m[at++] = 0x2c; /* TTL 300 */
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
    } else if (type == T_MX) {
        int pref = atoi(data);
        m[at++] = pref >> 8, m[at++] = pref & 0xff;
        at = put_name(m, at, strchr(data, ' ') + 1);
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

static int slow_count;

/* Build the response to query `q` (`ql` bytes) into `r`; 0: send nothing. `tcp`: over TCP. */
static int respond(const unsigned char *q, int ql, unsigned char *r, int tcp, int *extra_wrong_id)
{
    if (ql < 12)
        return 0;
    char name[256];
    int qend = read_name(q, ql, 12, name);
    if (qend + 4 > ql)
        return 0;
    int qtype = q[qend] << 8 | q[qend + 1];
    for (char *c = name; *c; c++)
        if (*c >= 'A' && *c <= 'Z')
            *c += 32;
    memcpy(r, q, qend + 4);
    r[2] = 0x84 | (q[2] & 0x01); /* QR, AA, RD copied */
    r[3] = 0x80;                 /* RA, rcode 0 */
    r[4] = 0, r[5] = 1;
    r[6] = r[7] = r[8] = r[9] = r[10] = r[11] = 0;
    int at = qend + 4, an = 0;
    *extra_wrong_id = 0;
    if (strcmp(name, "nx.test") == 0 || strcmp(name, "host.other.test") == 0 || strncmp(name, "nx", 2) == 0) {
        r[3] = 0x83;
        return at;
    }
    if (strcmp(name, "servfail.test") == 0) {
        r[3] = 0x82;
        return at;
    }
    if (strcmp(name, "refused.test") == 0) {
        r[3] = 0x85;
        return at;
    }
    if (strcmp(name, "silent.test") == 0)
        return 0;
    if (strcmp(name, "wrongq.test") == 0) {
        int a2 = put_name(r, 12, "other.test");
        r[a2] = 0, r[a2 + 1] = T_A, r[a2 + 2] = 0, r[a2 + 3] = 1;
        return a2 + 4;
    }
    if (strcmp(name, "slow.test") == 0 && !tcp && slow_count++ % 2 == 0)
        return 0;
    if (strcmp(name, "trunc.test") == 0 && !tcp) {
        r[2] |= 0x02;
        return at;
    }
    if (strcmp(name, "badid.test") == 0 && !tcp)
        *extra_wrong_id = 1;
    if (strcmp(name, "ad.test") == 0)
        r[3] |= 0x20;
    const char *target = name;
    for (int pass = 0; pass < 2; pass++) {
        int cname = 0;
        for (size_t i = 0; i < sizeof ZONE / sizeof ZONE[0]; i++) {
            if (strcmp(ZONE[i].name, target) != 0)
                continue;
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
    r[6] = an >> 8, r[7] = an & 0xff;
    return at;
}

static void serve(int ready)
{
    int u = socket(AF_INET, SOCK_DGRAM, 0), t = socket(AF_INET, SOCK_STREAM, 0);
    int one = 1;
    setsockopt(t, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons(53);
    inet_pton(AF_INET, "127.0.0.1", &a.sin_addr);
    if (bind(u, (struct sockaddr *)&a, sizeof a) || bind(t, (struct sockaddr *)&a, sizeof a) || listen(t, 8)) {
        perror("server bind");
        _exit(1);
    }
    (void)!write(ready, "r", 1);
    close(ready);
    unsigned char q[4096], r[4096];
    for (;;) {
        struct pollfd p[2] = {{u, POLLIN, 0}, {t, POLLIN, 0}};
        if (poll(p, 2, -1) < 0)
            continue;
        if (p[0].revents & POLLIN) {
            struct sockaddr_in from;
            socklen_t fl = sizeof from;
            int n = (int)recvfrom(u, q, sizeof q, 0, (struct sockaddr *)&from, &fl);
            int wrong;
            int rl = respond(q, n, r, 0, &wrong);
            if (rl > 0) {
                if (wrong) {
                    r[0] ^= 0xff;
                    sendto(u, r, rl, 0, (struct sockaddr *)&from, fl);
                    r[0] ^= 0xff;
                }
                sendto(u, r, rl, 0, (struct sockaddr *)&from, fl);
            }
        }
        if (p[1].revents & POLLIN) {
            int c = accept(t, NULL, NULL);
            for (;;) {
                unsigned char lb[2];
                if (recv(c, lb, 2, MSG_WAITALL) != 2)
                    break;
                int n = lb[0] << 8 | lb[1];
                if (recv(c, q, n, MSG_WAITALL) != n)
                    break;
                int wrong;
                int rl = respond(q, n, r, 1, &wrong);
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

/* ------------------------------------------------------------------ lookups */

static void show_host(const char *what, const struct hostent *h)
{
    if (!h) {
        printf("%s: NULL h_errno=%d\n", what, h_errno);
        return;
    }
    printf("%s: [%s] aliases:", what, h->h_name);
    for (char **a = h->h_aliases; *a; a++)
        printf(" [%s]", *a);
    printf(" addrs:");
    for (char **a = h->h_addr_list; *a; a++) {
        char t[64];
        printf(" %s", inet_ntop(h->h_addrtype, *a, t, sizeof t));
    }
    printf("\n");
}

static void show_gai(const char *what, const char *node, int family, int flags)
{
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = family;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = flags;
    int r = getaddrinfo(node, "80", &hints, &res);
    printf("%s [%s] fam=%d flags=%x: %d", what, node, family, flags, r);
    if (r == 0) {
        for (struct addrinfo *a = res; a; a = a->ai_next) {
            char t[64];
            if (a->ai_family == AF_INET)
                inet_ntop(AF_INET, &((struct sockaddr_in *)a->ai_addr)->sin_addr, t, sizeof t);
            else
                inet_ntop(AF_INET6, &((struct sockaddr_in6 *)a->ai_addr)->sin6_addr, t, sizeof t);
            printf(" %s", t);
            if (a->ai_canonname)
                printf(" canon=%s", a->ai_canonname);
        }
        freeaddrinfo(res);
    } else {
        printf(" (%s)", gai_strerror(r));
    }
    printf("\n");
}

static void show_query(const char *what, const char *name, int type, int search)
{
    unsigned char ans[4096];
    h_errno = 0;
    int n = search ? res_search(name, C_IN, type, ans, sizeof ans) : res_query(name, C_IN, type, ans, sizeof ans);
    printf("%s [%s] type=%d: %d h_errno=%d", what, name, type, n, h_errno);
    if (n > 0) {
        ns_msg m;
        if (ns_initparse(ans, n, &m) == 0) {
            printf(" rcode=%d aa=%d tc=%d ad=%d an=%d", ns_msg_getflag(m, ns_f_rcode), ns_msg_getflag(m, ns_f_aa), ns_msg_getflag(m, ns_f_tc),
                   ns_msg_getflag(m, ns_f_ad), ns_msg_count(m, ns_s_an));
            for (int i = 0; i < ns_msg_count(m, ns_s_an); i++) {
                ns_rr rr;
                if (ns_parserr(&m, ns_s_an, i, &rr) == 0) {
                    char buf[512];
                    if (ns_sprintrr(&m, &rr, NULL, NULL, buf, sizeof buf) > 0)
                        printf("\n    %s", buf);
                }
            }
        }
    }
    printf("\n");
}

static void lookups(const char *scen)
{
    printf("== scenario %s\n", scen);
    res_init();
    printf("_res: nscount=%d ndots=%d retrans=%d retry=%d options_vc=%d trustad=%d noaaaa=%d\n", _res.nscount, _res.ndots, _res.retrans,
           _res.retry, (_res.options & RES_USEVC) != 0, (_res.options & RES_TRUSTAD) != 0, (_res.options & RES_NOAAAA) != 0);
    for (int i = 0; i < _res.nscount && i < MAXNS; i++) {
        char t[64];
        printf("  ns %d: %s:%d\n", i, inet_ntop(AF_INET, &_res.nsaddr_list[i].sin_addr, t, sizeof t), ntohs(_res.nsaddr_list[i].sin_port));
    }
    for (int i = 0; i < MAXDNSRCH && _res.dnsrch[i]; i++)
        printf("  search %d: %s\n", i, _res.dnsrch[i]);
    if (strcmp(scen, "basic") == 0 || strcmp(scen, "usevc") == 0) {
        static const char *names[] = {"a.test", "aaaa.test", "both.test", "multi.test", "cname.test", "nx.test", "servfail.test",
                                      "refused.test", "trunc.test", "slow.test", "badid.test", "wrongq.test", "local.test", "A.TEST"};
        for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
            show_host("gethostbyname", gethostbyname(names[i]));
            show_gai("gai", names[i], AF_UNSPEC, AI_CANONNAME);
        }
        show_gai("gai v6", "a.test", AF_INET6, 0);
        show_gai("gai v6 mapped", "a.test", AF_INET6, AI_V4MAPPED);
        show_gai("gai idn", "bücher.test", AF_INET, AI_IDN);
        show_gai("gai no idn", "bücher.test", AF_INET, 0);
        show_query("res_query", "a.test", T_A, 0);
        show_query("res_query", "mx.test", T_MX, 0);
        show_query("res_query", "txt.test", T_TXT, 0);
        show_query("res_query", "cname.test", T_A, 0);
        show_query("res_query", "nx.test", T_A, 0);
        show_query("res_query", "servfail.test", T_A, 0);
        show_query("res_query", "ad.test", T_A, 0);
        show_query("res_query", "aaaa.test", T_A, 0);
        struct in_addr a4;
        inet_pton(AF_INET, "192.0.2.10", &a4);
        show_host("gethostbyaddr", gethostbyaddr(&a4, 4, AF_INET));
        struct sockaddr_in s4 = {AF_INET, htons(80), a4, {0}};
        char host[NI_MAXHOST], serv[NI_MAXSERV];
        int r = getnameinfo((struct sockaddr *)&s4, sizeof s4, host, sizeof host, serv, sizeof serv, 0);
        printf("getnameinfo 192.0.2.10: %d %s %s\n", r, r ? "" : host, r ? "" : serv);
        struct sockaddr_in6 s6;
        memset(&s6, 0, sizeof s6);
        s6.sin6_family = AF_INET6;
        inet_pton(AF_INET6, "2001:db8::20", &s6.sin6_addr);
        r = getnameinfo((struct sockaddr *)&s6, sizeof s6, host, sizeof host, NULL, 0, NI_NAMEREQD);
        printf("getnameinfo 2001:db8::20: %d %s\n", r, r ? "" : host);
        inet_pton(AF_INET, "192.0.2.99", &a4);
        s4.sin_addr = a4;
        r = getnameinfo((struct sockaddr *)&s4, sizeof s4, host, sizeof host, NULL, 0, NI_NAMEREQD);
        printf("getnameinfo 192.0.2.99 NAMEREQD: %d\n", r);
    } else if (strcmp(scen, "search") == 0) {
        show_query("res_search", "host", T_A, 1);
        show_query("res_search", "x.y", T_A, 1);
        show_query("res_search", "x.y.", T_A, 1);
        show_query("res_search", "nx", T_A, 1);
        show_host("gethostbyname host", gethostbyname("host"));
        show_gai("gai", "host", AF_INET, AI_CANONNAME);
        show_gai("gai", "x.y", AF_INET, AI_CANONNAME);
        unsigned char ans[512];
        int n = res_querydomain("host", "example.test", C_IN, T_A, ans, sizeof ans);
        printf("res_querydomain host example.test: %d\n", n);
    } else if (strcmp(scen, "trustad") == 0) {
        show_query("res_query", "ad.test", T_A, 0);
        show_query("res_query", "a.test", T_A, 0);
    } else if (strcmp(scen, "noaaaa") == 0) {
        show_query("res_query", "aaaa.test", T_AAAA, 0);
        show_gai("gai", "both.test", AF_UNSPEC, 0);
        show_gai("gai", "aaaa.test", AF_UNSPEC, 0);
        show_host("gethostbyname2 v6", gethostbyname2("both.test", AF_INET6));
    } else {
        show_host("gethostbyname nx.test", gethostbyname("nx.test"));
        show_host("gethostbyname local.test", gethostbyname("local.test"));
        show_host("gethostbyname a.test", gethostbyname("a.test"));
        show_gai("gai", "nx.test", AF_INET, 0);
        show_gai("gai", "local.test", AF_INET, 0);
    }
}

/* ------------------------------------------------------------------ offline message functions */

static void offline(void)
{
    printf("== offline\n");
    unsigned char buf[512];
    int n = res_mkquery(QUERY, "www.example.com", C_IN, T_A, NULL, 0, NULL, buf, sizeof buf);
    printf("res_mkquery: %d flags=%02x%02x qd=%d body=", n, buf[2], buf[3], buf[5]);
    for (int i = 12; i < n; i++)
        printf("%02x", buf[i]);
    printf("\n");
    n = res_mkquery(QUERY, "a..b", C_IN, T_A, NULL, 0, NULL, buf, sizeof buf);
    printf("res_mkquery bad name: %d\n", n);
    n = res_mkquery(QUERY, "www.example.com", C_IN, T_A, NULL, 0, NULL, buf, 20);
    printf("res_mkquery small buffer: %d\n", n);

    unsigned char msg[512];
    const unsigned char *dnptrs[16];
    unsigned char **dp = (unsigned char **)dnptrs;
    dnptrs[0] = msg;
    dnptrs[1] = NULL;
    const unsigned char **lastdnptr = dnptrs + 16;
    int c1 = dn_comp("www.example.com", msg + 12, 200, dp, (unsigned char **)lastdnptr);
    int c2 = dn_comp("mail.example.com", msg + 12 + c1, 200, dp, (unsigned char **)lastdnptr);
    int c3 = dn_comp("example.com", msg + 12 + c1 + c2, 200, dp, (unsigned char **)lastdnptr);
    printf("dn_comp: %d %d %d bytes=", c1, c2, c3);
    for (int i = 12; i < 12 + c1 + c2 + c3; i++)
        printf("%02x", msg[i]);
    printf("\n");
    char exp[256];
    int e = dn_expand(msg, msg + 12 + c1 + c2 + c3, msg + 12 + c1, exp, sizeof exp);
    printf("dn_expand: %d [%s]\n", e, exp);
    e = dn_expand(msg, msg + 12 + c1 + c2 + c3, msg + 12 + c1 + c2, exp, sizeof exp);
    printf("dn_expand 2: %d [%s]\n", e, exp);
    printf("dn_skipname: %d\n", dn_skipname(msg + 12 + c1, msg + 12 + c1 + c2 + c3));
    unsigned char loop[] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 12};
    e = dn_expand(loop, loop + sizeof loop, loop + 12, exp, sizeof exp);
    printf("dn_expand pointer loop: %d\n", e);
    unsigned char fwd[] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 14, 1, 'a', 0};
    e = dn_expand(fwd, fwd + sizeof fwd, fwd + 12, exp, sizeof exp);
    printf("dn_expand forward pointer: %d [%s]\n", e, e >= 0 ? exp : "");
    unsigned char esc[] = {3, 'a', '.', 'b', 1, '\\', 2, 0x01, ' ', 0};
    e = dn_expand(esc, esc + sizeof esc, esc, exp, sizeof exp);
    printf("dn_expand escapes: %d [%s]\n", e, e >= 0 ? exp : "");

    static const char *hn[] = {"example.com", "ex-ample.com", "-example.com", "example-.com", "ex_ample.com", "1example.com", "example.com.", "",
                               ".", "a..b", "*.example.com", "ex ample.com", "xn--bcher-kva.com", "a.b-c.d"};
    for (size_t i = 0; i < sizeof hn / sizeof hn[0]; i++)
        printf("checks [%s]: hnok=%d ownok=%d mailok=%d dnok=%d\n", hn[i], res_hnok(hn[i]), res_ownok(hn[i]), res_mailok(hn[i]), res_dnok(hn[i]));

    printf("ns_samename: %d %d %d\n", ns_samename("Example.COM", "example.com."), ns_samename("a.b", "a.c"), ns_samename("a\\.b", "a.b"));
    char canon[256];
    printf("ns_makecanon: %d [%s]\n", ns_makecanon("Example.Com", canon, sizeof canon), canon);
    printf("ns_makecanon trailing dots: %d [%s]\n", ns_makecanon("a.b..", canon, sizeof canon), canon);
    unsigned char wire[256];
    int w = ns_name_pton("www.Example.com.", wire, sizeof wire);
    printf("ns_name_pton: %d\n", w);
    char text[256];
    int t = ns_name_ntop(wire, text, sizeof text);
    printf("ns_name_ntop: %d [%s]\n", t, text);
    w = ns_name_pton("a\\046b.c\\.d", wire, sizeof wire);
    t = ns_name_ntop(wire, text, sizeof text);
    printf("ns_name escapes: %d %d [%s]\n", w, t, text);
    char ttl[64];
    static const unsigned long ttls[] = {0, 59, 60, 3600, 86400, 604800, 90061, 1209600 + 3661};
    for (size_t i = 0; i < sizeof ttls / sizeof ttls[0]; i++) {
        ns_format_ttl(ttls[i], ttl, sizeof ttl);
        printf("ns_format_ttl %lu: %s\n", ttls[i], ttl);
    }
    static const char *pt[] = {"1W2D3H4M5S", "3600", "1h", "2w", "1D1D", "x", "", "1H30"};
    for (size_t i = 0; i < sizeof pt / sizeof pt[0]; i++) {
        unsigned long v = 0;
        int r = ns_parse_ttl(pt[i], &v);
        printf("ns_parse_ttl [%s]: %d %lu\n", pt[i], r, v);
    }
    int err = 0;
    printf("ns_datetosecs: %u err=%d\n", ns_datetosecs("20240101000000", &err), err);
    printf("ns_datetosecs bad: %u err=%d\n", ns_datetosecs("2024", &err), err);
    unsigned char b64in[] = "Hello, world!";
    char b64[64];
    int bl = b64_ntop(b64in, 13, b64, sizeof b64);
    printf("b64_ntop: %d [%s]\n", bl, b64);
    unsigned char back[64];
    int pl = b64_pton(b64, back, sizeof back);
    printf("b64_pton: %d [%.*s]\n", pl, pl > 0 ? pl : 0, back);
    printf("b64_pton bad: %d\n", b64_pton("SGVsbG8@", back, sizeof back));
    printf("b64_ntop small: %d\n", b64_ntop(b64in, 13, b64, 8));
    printf("hstrerror:");
    for (int i = -1; i <= 5; i++)
        printf(" [%s]", hstrerror(i));
    printf("\n");
    printf("p_type 1 28 15 16 999: %s %s %s %s %s\n", p_type(1), p_type(28), p_type(15), p_type(16), p_type(999));
    printf("p_class 1 3 255 42: %s %s %s %s\n", p_class(1), p_class(3), p_class(255), p_class(42));
    printf("p_rcode 0 3 99: %s %s %s\n", p_rcode(0), p_rcode(3), p_rcode(99));
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
    int p[2];
    if (pipe(p))
        return 1;
    pid_t server = fork();
    if (server == 0) {
        close(p[0]);
        serve(p[1]);
        _exit(0);
    }
    close(p[1]);
    char c;
    if (read(p[0], &c, 1) != 1)
        return 1;
    close(p[0]);
    fflush(stdout);
    lookups(argv[2]);
    fflush(stdout);
    kill(server, SIGKILL);
    int st;
    syscall(SYS_wait4, server, &st, 0, NULL);
    printf("done\n");
    return 0;
}
