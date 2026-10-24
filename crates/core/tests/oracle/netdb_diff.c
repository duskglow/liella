/* The netdb databases through the NSS `files` source, and getaddrinfo/getnameinfo without DNS
 * (spec §16.1-16.3, §16.5-16.6, §15.7; phase 10): built against the host C library and against
 * Liella's products, the outputs must match line for line (netdb_products.rs). Black-box: every
 * line is a result of a public function.
 *
 * Usage: netdb_diff DIR [lookup]. Without `lookup`, the program writes the fixture DIR/etc. With
 * `lookup`, it expects DIR/etc bind-mounted over /etc in private mount and network namespaces with
 * only the loopback interface up (netdb_run.sh, as root), and runs the lookups. */
#define _GNU_SOURCE
#include <aliases.h>
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/ether.h>
#include <netinet/in.h>
#include <rpc/netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

static char dir[512];

static void write_file(const char *name, const char *text)
{
    char p[768];
    snprintf(p, sizeof p, "%s/etc/%s", dir, name);
    FILE *f = fopen(p, "w");
    fputs(text, f);
    fclose(f);
}

static void fixture(void)
{
    char p[768];
    snprintf(p, sizeof p, "%s/etc", dir);
    mkdir(p, 0755);
    write_file("nsswitch.conf", "hosts: files\nnetworks: files\nservices: files\nprotocols: files\nrpc: files\nethers: files\n"
                                "netgroup: files\naliases: files\npasswd: files\ngroup: files\n");
    write_file("hosts", "127.0.0.1\tlocalhost\n"
                        "::1\tlocalhost ip6-localhost ip6-loopback\n"
                        "# comment\n"
                        "\n"
                        "192.0.2.1   alpha alpha.example.org   a1  # trailing comment\n"
                        "192.0.2.2 beta\n"
                        "192.0.2.3 beta\n"
                        "2001:db8::10 gamma gamma6\n"
                        "192.0.2.4 gamma\n"
                        "  192.0.2.5 spaced\n"
                        "300.0.0.1 badaddr\n"
                        "192.0.2.6\n"
                        "192.0.2.7 UPPER\n"
                        "10.1 short\n"
                        "192.0.2.8 dup alpha\n");
    write_file("networks", "loopback 127\nlink-local 169.254.0.0\nexample 192.0.2 exnet\n# c\nbad notanumber\n");
    write_file("services", "echo 7/tcp\necho 7/udp\nftp 21/tcp\nssh 22/tcp # The Secure Shell\nhttp 80/tcp www www-http\n"
                           "http 80/udp\ncustom 65000/tcp alias1 alias2\nbadport 70000/tcp\nnoproto 9999\n#c\n");
    write_file("protocols", "ip 0 IP\nicmp 1 ICMP\ntcp 6 TCP\nudp 17 UDP\nipv6-icmp 58 IPv6-ICMP icmpv6\n# c\nbad x\n");
    write_file("rpc", "portmapper 100000 portmap sunrpc\nnfs 100003 nfsprog\nbogus abc\n");
    write_file("ethers", "00:11:22:33:44:55 alpha\n0:1:2:3:4:5 beta # comment\nbad line\n");
    write_file("netgroup", "trusted (alpha,alice,example.org) (beta,-,) nested\nnested (gamma,bob,)\nempty\nloop1 loop2\nloop2 loop1\n");
    write_file("aliases", "postmaster: root\nroot: alice, bob\n# c\nlist: :include:/etc/listfile\nmulti: a,\n  b,\n  c\n");
    write_file("passwd", "root:x:0:0::/root:/bin/sh\n");
    write_file("group", "root:x:0:\n");
}

static void show_host(const char *what, const struct hostent *h)
{
    if (!h) {
        printf("%s: NULL h_errno=%d\n", what, h_errno);
        return;
    }
    printf("%s: [%s] type=%d len=%d aliases:", what, h->h_name, h->h_addrtype, h->h_length);
    for (char **a = h->h_aliases; *a; a++)
        printf(" [%s]", *a);
    printf(" addrs:");
    for (char **a = h->h_addr_list; *a; a++) {
        char t[64];
        printf(" %s", inet_ntop(h->h_addrtype, *a, t, sizeof t));
    }
    printf("\n");
}

static void hosts(void)
{
    printf("== gethostbyname\n");
    static const char *names[] = {"localhost", "alpha", "alpha.example.org", "a1", "ALPHA", "beta", "gamma", "gamma6", "spaced", "badaddr",
                                  "upper", "short", "dup", "nonesuch", "", "192.0.2.99", "2001:db8::99", "1.2"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        char w[96];
        snprintf(w, sizeof w, "byname [%s]", names[i]);
        show_host(w, gethostbyname(names[i]));
        snprintf(w, sizeof w, "byname2 v6 [%s]", names[i]);
        show_host(w, gethostbyname2(names[i], AF_INET6));
    }
    printf("== gethostbyaddr\n");
    static const char *addrs[] = {"127.0.0.1", "192.0.2.1", "192.0.2.2", "192.0.2.3", "192.0.2.8", "192.0.2.99"};
    for (size_t i = 0; i < sizeof addrs / sizeof addrs[0]; i++) {
        struct in_addr a;
        inet_pton(AF_INET, addrs[i], &a);
        char w[64];
        snprintf(w, sizeof w, "byaddr %s", addrs[i]);
        show_host(w, gethostbyaddr(&a, 4, AF_INET));
    }
    struct in6_addr a6;
    inet_pton(AF_INET6, "2001:db8::10", &a6);
    show_host("byaddr 2001:db8::10", gethostbyaddr(&a6, 16, AF_INET6));
    inet_pton(AF_INET6, "::1", &a6);
    show_host("byaddr ::1", gethostbyaddr(&a6, 16, AF_INET6));
    printf("== gethostbyname_r ERANGE\n");
    for (size_t sz = 8; sz <= 1024; sz *= 2) {
        struct hostent he, *res = (struct hostent *)1;
        int herr = 0;
        char *buf = malloc(sz);
        int r = gethostbyname_r("alpha", &he, buf, sz, &res, &herr);
        printf("size %zu: r=%d res=%s herr=%d\n", sz, r, res ? "set" : "NULL", herr);
        free(buf);
    }
    printf("== gethostent\n");
    sethostent(0);
    struct hostent *h;
    int n = 0;
    while ((h = gethostent()) != NULL && n < 30) {
        show_host("ent", h);
        n++;
    }
    endhostent();
}

static void others(void)
{
    printf("== networks\n");
    static const char *nets[] = {"loopback", "example", "exnet", "link-local", "bad", "none"};
    for (size_t i = 0; i < sizeof nets / sizeof nets[0]; i++) {
        struct netent *e = getnetbyname(nets[i]);
        if (e)
            printf("getnetbyname [%s]: [%s] type=%d net=%08x aliases=%s\n", nets[i], e->n_name, e->n_addrtype, e->n_net, e->n_aliases[0] ? e->n_aliases[0] : "-");
        else
            printf("getnetbyname [%s]: NULL\n", nets[i]);
    }
    struct netent *e = getnetbyaddr(127, AF_INET);
    printf("getnetbyaddr 127: %s\n", e ? e->n_name : "NULL");
    e = getnetbyaddr(0xc00002, AF_INET);
    printf("getnetbyaddr c00002: %s\n", e ? e->n_name : "NULL");
    setnetent(0);
    int n = 0;
    while ((e = getnetent()) != NULL && n < 20) {
        printf("netent: [%s] %08x\n", e->n_name, e->n_net);
        n++;
    }
    endnetent();

    printf("== services\n");
    static const char *sv[][2] = {{"echo", "tcp"}, {"echo", "udp"}, {"echo", NULL}, {"www", "tcp"}, {"http", NULL}, {"ssh", "udp"},
                                  {"alias2", "tcp"}, {"badport", "tcp"}, {"noproto", NULL}, {"nonesuch", NULL}, {"ECHO", "tcp"}};
    for (size_t i = 0; i < sizeof sv / sizeof sv[0]; i++) {
        struct servent *s = getservbyname(sv[i][0], sv[i][1]);
        if (s)
            printf("getservbyname [%s/%s]: [%s] %d [%s] aliases:%s\n", sv[i][0], sv[i][1] ? sv[i][1] : "NULL", s->s_name, ntohs(s->s_port), s->s_proto,
                   s->s_aliases[0] ? s->s_aliases[0] : "-");
        else
            printf("getservbyname [%s/%s]: NULL\n", sv[i][0], sv[i][1] ? sv[i][1] : "NULL");
    }
    static const int ports[] = {7, 22, 80, 65000, 12345};
    for (size_t i = 0; i < sizeof ports / sizeof ports[0]; i++) {
        struct servent *s = getservbyport(htons(ports[i]), "tcp");
        printf("getservbyport %d/tcp: %s\n", ports[i], s ? s->s_name : "NULL");
    }
    struct servent *s = getservbyport(htons(80), NULL);
    printf("getservbyport 80/NULL: %s/%s\n", s ? s->s_name : "NULL", s ? s->s_proto : "");
    setservent(0);
    n = 0;
    while ((s = getservent()) != NULL && n < 20) {
        printf("servent: [%s] %d/%s\n", s->s_name, ntohs(s->s_port), s->s_proto);
        n++;
    }
    errno = 77;
    printf("getservent end: %s errno=%d\n", getservent() ? "rec" : "NULL", errno); /* #409 */
    endservent();

    printf("== protocols\n");
    static const char *pr[] = {"tcp", "TCP", "icmpv6", "ipv6-icmp", "bad", "none"};
    for (size_t i = 0; i < sizeof pr / sizeof pr[0]; i++) {
        struct protoent *p = getprotobyname(pr[i]);
        printf("getprotobyname [%s]: %s %d\n", pr[i], p ? p->p_name : "NULL", p ? p->p_proto : -1);
    }
    struct protoent *p = getprotobynumber(17);
    printf("getprotobynumber 17: %s\n", p ? p->p_name : "NULL");
    p = getprotobynumber(99);
    printf("getprotobynumber 99: %s\n", p ? p->p_name : "NULL");

    printf("== rpc\n");
    struct rpcent *r = getrpcbyname("portmap");
    printf("getrpcbyname portmap: %s %d\n", r ? r->r_name : "NULL", r ? r->r_number : -1);
    r = getrpcbynumber(100003);
    printf("getrpcbynumber 100003: %s\n", r ? r->r_name : "NULL");
    r = getrpcbyname("bogus");
    printf("getrpcbyname bogus: %s\n", r ? r->r_name : "NULL");

    printf("== ethers\n");
    struct ether_addr ea;
    int er = ether_hostton("alpha", &ea);
    printf("ether_hostton alpha: %d %s\n", er, er == 0 ? ether_ntoa(&ea) : "-");
    er = ether_hostton("nonesuch", &ea);
    printf("ether_hostton nonesuch: %d\n", er);
    char host[256];
    struct ether_addr *b = ether_aton("0:1:2:3:4:5");
    er = ether_ntohost(host, b);
    printf("ether_ntohost 0:1:2:3:4:5: %d %s\n", er, er == 0 ? host : "-");

    printf("== netgroup\n");
    static const char *ng[][4] = {{"trusted", "alpha", "alice", "example.org"}, {"trusted", "beta", NULL, NULL}, {"trusted", "gamma", "bob", NULL},
                                  {"trusted", "delta", NULL, NULL}, {"nested", "gamma", NULL, NULL}, {"empty", NULL, NULL, NULL},
                                  {"loop1", "x", NULL, NULL}, {"nonesuch", NULL, NULL, NULL}};
    for (size_t i = 0; i < sizeof ng / sizeof ng[0]; i++)
        printf("innetgr [%s] (%s,%s,%s): %d\n", ng[i][0], ng[i][1] ? ng[i][1] : "NULL", ng[i][2] ? ng[i][2] : "NULL", ng[i][3] ? ng[i][3] : "NULL",
               innetgr(ng[i][0], ng[i][1], ng[i][2], ng[i][3]));
    int sr = setnetgrent("trusted");
    printf("setnetgrent trusted: %d\n", sr);
    char *hh, *uu, *dd;
    n = 0;
    while (getnetgrent(&hh, &uu, &dd) == 1 && n < 20) {
        printf("netgrent: (%s,%s,%s)\n", hh ? hh : "NULL", uu ? uu : "NULL", dd ? dd : "NULL");
        n++;
    }
    endnetgrent();

    printf("== aliases\n");
    struct aliasent *al = getaliasbyname("root");
    if (al) {
        printf("getaliasbyname root: [%s] %zu:", al->alias_name, al->alias_members_len);
        for (size_t i = 0; i < al->alias_members_len; i++)
            printf(" [%s]", al->alias_members[i]);
        printf("\n");
    } else
        printf("getaliasbyname root: NULL\n");
    setaliasent();
    n = 0;
    while ((al = getaliasent()) != NULL && n < 20) {
        printf("aliasent: [%s] %zu:", al->alias_name, al->alias_members_len);
        for (size_t i = 0; i < al->alias_members_len; i++)
            printf(" [%s]", al->alias_members[i]);
        printf("\n");
        n++;
    }
    errno = 77;
    printf("getaliasent end: %s errno=%d\n", getaliasent() ? "rec" : "NULL", errno); /* #409 */
    endaliasent();
}

static void show_ai(const char *what, int r, struct addrinfo *res)
{
    printf("%s: %d", what, r);
    if (r != 0) {
        printf(" (%s)\n", gai_strerror(r));
        return;
    }
    for (struct addrinfo *a = res; a; a = a->ai_next) {
        char t[64] = "?";
        int port = 0;
        if (a->ai_family == AF_INET) {
            inet_ntop(AF_INET, &((struct sockaddr_in *)a->ai_addr)->sin_addr, t, sizeof t);
            port = ntohs(((struct sockaddr_in *)a->ai_addr)->sin_port);
        } else if (a->ai_family == AF_INET6) {
            inet_ntop(AF_INET6, &((struct sockaddr_in6 *)a->ai_addr)->sin6_addr, t, sizeof t);
            port = ntohs(((struct sockaddr_in6 *)a->ai_addr)->sin6_port);
        }
        printf(" {fam=%d type=%d proto=%d %s:%d canon=%s}", a->ai_family, a->ai_socktype, a->ai_protocol, t, port, a->ai_canonname ? a->ai_canonname : "-");
    }
    printf("\n");
    freeaddrinfo(res);
}

static void gai(void)
{
    printf("== getaddrinfo\n");
    struct {
        const char *node, *service;
        int family, socktype, flags;
    } c[] = {
        {"localhost", "80", AF_UNSPEC, SOCK_STREAM, 0},
        {"localhost", NULL, AF_UNSPEC, 0, 0},
        {"alpha", "http", AF_INET, SOCK_STREAM, AI_CANONNAME},
        {"a1", "ssh", AF_UNSPEC, SOCK_STREAM, AI_CANONNAME},
        {"gamma", NULL, AF_UNSPEC, SOCK_DGRAM, 0},
        {"gamma", NULL, AF_INET6, SOCK_DGRAM, AI_V4MAPPED},
        {"gamma", NULL, AF_INET6, SOCK_DGRAM, AI_V4MAPPED | AI_ALL},
        {"beta", "7", AF_UNSPEC, 0, 0},
        {"192.0.2.50", "80", AF_UNSPEC, SOCK_STREAM, AI_NUMERICHOST},
        {"alpha", "80", AF_UNSPEC, SOCK_STREAM, AI_NUMERICHOST},
        {"2001:db8::1", "80", AF_INET, SOCK_STREAM, 0},
        {NULL, "80", AF_UNSPEC, SOCK_STREAM, AI_PASSIVE},
        {NULL, "80", AF_UNSPEC, SOCK_STREAM, 0},
        {NULL, NULL, AF_UNSPEC, 0, 0},
        {"localhost", "nonesuch", AF_UNSPEC, SOCK_STREAM, 0},
        {"localhost", "http", AF_UNSPEC, SOCK_STREAM, AI_NUMERICSERV},
        {"localhost", "80", AF_UNSPEC, SOCK_RAW, 0},
        {"localhost", "80", 99, SOCK_STREAM, 0},
        {"localhost", "80", AF_UNSPEC, SOCK_STREAM, 0x10000},
        {"nonesuch", "80", AF_UNSPEC, SOCK_STREAM, 0},
        {"localhost", "80", AF_UNSPEC, SOCK_STREAM, AI_ADDRCONFIG},
        {"alpha", "80", AF_UNSPEC, SOCK_STREAM, AI_ADDRCONFIG},
        {"", "80", AF_UNSPEC, SOCK_STREAM, 0},
        {"localhost", "65536", AF_UNSPEC, SOCK_STREAM, 0},
        {"localhost", "-1", AF_UNSPEC, SOCK_STREAM, 0},
        {"1.2", "80", AF_INET, SOCK_STREAM, 0},
        {"0x7f.1", "80", AF_INET, SOCK_STREAM, 0},
        {"fe80::1%lo", "80", AF_INET6, SOCK_STREAM, AI_NUMERICHOST},
    };
    for (size_t i = 0; i < sizeof c / sizeof c[0]; i++) {
        struct addrinfo hints, *res = NULL;
        memset(&hints, 0, sizeof hints);
        hints.ai_family = c[i].family;
        hints.ai_socktype = c[i].socktype;
        hints.ai_flags = c[i].flags;
        int r = getaddrinfo(c[i].node, c[i].service, &hints, &res);
        char w[160];
        snprintf(w, sizeof w, "gai [%s] [%s] fam=%d type=%d flags=%x", c[i].node ? c[i].node : "NULL", c[i].service ? c[i].service : "NULL", c[i].family,
                 c[i].socktype, c[i].flags);
        show_ai(w, r, res);
    }
    struct addrinfo *res = NULL;
    int r = getaddrinfo("alpha", "80", NULL, &res);
    show_ai("gai NULL hints", r, res);
    printf("== getnameinfo\n");
    struct {
        const char *addr;
        int port, flags;
    } g[] = {{"192.0.2.1", 80, 0},       {"192.0.2.1", 80, NI_NUMERICHOST}, {"192.0.2.99", 22, 0},          {"192.0.2.99", 22, NI_NAMEREQD},
             {"192.0.2.1", 7, NI_DGRAM}, {"192.0.2.1", 12345, 0},           {"192.0.2.1", 80, NI_NOFQDN},  {"2001:db8::10", 80, 0},
             {"::1", 22, NI_NUMERICSERV}, {"192.0.2.1", 80, 0x4000}};
    for (size_t i = 0; i < sizeof g / sizeof g[0]; i++) {
        struct sockaddr_storage ss;
        memset(&ss, 0, sizeof ss);
        socklen_t len;
        if (strchr(g[i].addr, ':')) {
            struct sockaddr_in6 *s6 = (struct sockaddr_in6 *)&ss;
            s6->sin6_family = AF_INET6;
            s6->sin6_port = htons(g[i].port);
            inet_pton(AF_INET6, g[i].addr, &s6->sin6_addr);
            len = sizeof *s6;
        } else {
            struct sockaddr_in *s4 = (struct sockaddr_in *)&ss;
            s4->sin_family = AF_INET;
            s4->sin_port = htons(g[i].port);
            inet_pton(AF_INET, g[i].addr, &s4->sin_addr);
            len = sizeof *s4;
        }
        char host[NI_MAXHOST] = "-", serv[NI_MAXSERV] = "-";
        int rr = getnameinfo((struct sockaddr *)&ss, len, host, sizeof host, serv, sizeof serv, g[i].flags);
        printf("gni %s:%d flags=%x -> %d host=[%s] serv=[%s]\n", g[i].addr, g[i].port, g[i].flags, rr, rr == 0 ? host : "", rr == 0 ? serv : "");
    }
    struct sockaddr_in s4;
    memset(&s4, 0, sizeof s4);
    s4.sin_family = AF_INET;
    inet_pton(AF_INET, "192.0.2.1", &s4.sin_addr);
    char tiny[4];
    r = getnameinfo((struct sockaddr *)&s4, sizeof s4, tiny, sizeof tiny, NULL, 0, 0);
    printf("gni small host buffer: %d\n", r);
    r = getnameinfo((struct sockaddr *)&s4, 3, tiny, sizeof tiny, NULL, 0, 0);
    printf("gni bad length: %d\n", r);
    r = getnameinfo((struct sockaddr *)&s4, sizeof s4, NULL, 0, NULL, 0, 0);
    printf("gni nothing requested: %d\n", r);
    printf("== gai_strerror\n");
    for (int code = -12; code <= 1; code++)
        printf("gai_strerror %d: %s\n", code, gai_strerror(code));
    printf("gai_strerror -100: %s\n", gai_strerror(-100));
    printf("gai_strerror -101..-105:");
    for (int code = -105; code <= -101; code++)
        printf(" [%s]", gai_strerror(code));
    printf("\n");
}

int main(int argc, char **argv)
{
    snprintf(dir, sizeof dir, "%s", argc > 1 ? argv[1] : "/tmp");
    if (argc > 2 && strcmp(argv[2], "lookup") == 0) {
        hosts();
        others();
        gai();
    } else {
        fixture();
    }
    printf("done\n");
    return 0;
}
