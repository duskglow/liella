/* Address conversion, interfaces, IPv6 option helpers and Ethernet addresses end to end (spec
 * §15.3-15.8; phase 10): built against the host C library and against Liella's products, the
 * outputs must match line for line (netaddr_products.rs). Black-box: every line is a result of a
 * public function; interface output is limited to the loopback interface so that it does not
 * depend on the machine. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <ifaddrs.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <netinet/ether.h>
#include <netinet/in.h>
#include <netinet/ip6.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static void hex(const void *p, size_t n)
{
    const unsigned char *b = p;
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

static void pton_cases(void)
{
    printf("== inet_pton AF_INET\n");
    static const char *v4[] = {"1.2.3.4", "0.0.0.0", "255.255.255.255", "256.1.1.1", "1.2.3", "1.2.3.4.5", "01.2.3.4", "1.2.3.04", "0x1.2.3.4",
                               " 1.2.3.4", "1.2.3.4 ", "1..3.4", "", "1.2.3.4\n", "127.1", "1.2.3.-4", "+1.2.3.4", "999.0.0.0"};
    for (size_t i = 0; i < sizeof v4 / sizeof v4[0]; i++) {
        unsigned char b[4] = {0xee, 0xee, 0xee, 0xee};
        errno = 0;
        int r = inet_pton(AF_INET, v4[i], b);
        printf("[%s] -> %d errno=%d ", v4[i], r, errno);
        hex(b, 4);
        printf("\n");
    }
    printf("== inet_pton AF_INET6\n");
    static const char *v6[] = {"::", "::1", "1::", "1:2:3:4:5:6:7:8", "1:2:3:4:5:6:7:8:9", "1:2:3:4:5:6:7", "1::2::3", "::ffff:1.2.3.4",
                               "::1.2.3.4", "1:2:3:4:5:6:1.2.3.4", "1:2:3:4:5:6:7:1.2.3.4", "fe80::1%eth0", "FE80::ABCD", "12345::",
                               "1:2:3:4:5:6:7::", "::1:2:3:4:5:6:7", ":::", ":1::", "1:::2", "0000:0000::", "00000::", "1:2:3:4:5:6:7:8:", ":1:2:3:4:5:6:7:8",
                               "::ffff:256.1.1.1", "::ffff:1.2.3", "1.2.3.4", "", " ::1", "g::"};
    for (size_t i = 0; i < sizeof v6 / sizeof v6[0]; i++) {
        unsigned char b[16];
        memset(b, 0xee, 16);
        errno = 0;
        int r = inet_pton(AF_INET6, v6[i], b);
        printf("[%s] -> %d errno=%d ", v6[i], r, errno);
        hex(b, 16);
        printf("\n");
    }
    unsigned char b[16];
    errno = 0;
    int ru = inet_pton(AF_UNIX, "1.2.3.4", b);
    printf("inet_pton AF_UNIX -> %d errno=%d\n", ru, errno);
}

static void ntop_cases(void)
{
    printf("== inet_ntop\n");
    static const char *v6[] = {"::", "::1", "1::", "1:0:0:2:0:0:0:3", "1:0:0:2:0:0:3:4", "1:2:3:4:5:6:7:8", "0:0:1:0:0:0:0:0", "1:0:2:0:3:0:4:0",
                               "::ffff:1.2.3.4", "::1.2.3.4", "::ffff:0:1.2.3.4", "64:ff9b::1.2.3.4", "fe80::abcd:ef01", "2001:db8::1:0:0:1",
                               "2001:db8:0:0:1:0:0:1", "0:0:0:0:0:0:0:1:", "abcd:ef01:2345:6789:abcd:ef01:2345:6789", "::0.0.0.1", "::ffff:0.0.0.0"};
    for (size_t i = 0; i < sizeof v6 / sizeof v6[0]; i++) {
        unsigned char b[16];
        if (inet_pton(AF_INET6, v6[i], b) != 1) {
            printf("[%s] not parsed\n", v6[i]);
            continue;
        }
        char out[64];
        errno = 0;
        const char *r = inet_ntop(AF_INET6, b, out, sizeof out);
        printf("[%s] -> %s errno=%d\n", v6[i], r ? r : "NULL", errno);
    }
    unsigned char a[4] = {192, 168, 0, 1};
    char out[64];
    for (socklen_t sz = 10; sz <= 13; sz++) {
        memset(out, 'x', sizeof out);
        errno = 0;
        const char *r = inet_ntop(AF_INET, a, out, sz);
        printf("inet_ntop v4 size %u -> %s errno=%d\n", sz, r ? r : "NULL", errno);
    }
    unsigned char l[16];
    inet_pton(AF_INET6, "2001:db8::1", l);
    for (socklen_t sz = 10; sz <= 13; sz++) {
        errno = 0;
        const char *r = inet_ntop(AF_INET6, l, out, sz);
        printf("inet_ntop v6 size %u -> %s errno=%d\n", sz, r ? r : "NULL", errno);
    }
    errno = 0;
    const char *ru = inet_ntop(AF_UNIX, a, out, sizeof out);
    printf("inet_ntop AF_UNIX -> %s errno=%d\n", ru ? "ok" : "NULL", errno);
}

static void aton_cases(void)
{
    printf("== inet_aton / inet_addr / inet_network\n");
    static const char *in[] = {"1.2.3.4", "10.1", "10.1.2", "10", "0x7f.1", "0177.0.0.1", "0x7f000001", "4294967295", "4294967296", "1.2.3.4.5",
                               "1.2.3.256", "1.2.65535", "1.16777215", "1.16777216", "08.1.1.1", "0x.1.1.1", "1.2.3.4 trailing", "1.2.3.4\ttab",
                               " 1.2.3.4", "", "255.255.255.255", "1.2.3.", ".1.2.3", "0xffffffff", "0X10.0.0.1", "1.2.3.4x"};
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        struct in_addr a;
        a.s_addr = 0xeeeeeeee;
        int r = inet_aton(in[i], &a);
        printf("[%s] aton=%d %08x addr=%08x network=%08x\n", in[i], r, ntohl(a.s_addr), ntohl(inet_addr(in[i])), inet_network(in[i]));
    }
    printf("inet_aton NULL out: %d\n", inet_aton("1.2.3.4", NULL));
}

static void classful(void)
{
    printf("== classful\n");
    static const unsigned addrs[] = {0x0a010203, 0x80010203, 0xc0a80001, 0xe0000001, 0xf0000001, 0x7f000001, 0};
    for (size_t i = 0; i < sizeof addrs / sizeof addrs[0]; i++) {
        struct in_addr a = {htonl(addrs[i])};
        printf("%08x lnaof=%08x netof=%08x ntoa=%s\n", addrs[i], inet_lnaof(a), inet_netof(a), inet_ntoa(a));
    }
    static const unsigned nets[][2] = {{10, 0x010203}, {0x80, 0x0102}, {0x8001, 0x0203}, {0xc0a800, 1}, {0x0a, 0xffffffff}, {0, 5}, {0xffffff01, 2}};
    for (size_t i = 0; i < sizeof nets / sizeof nets[0]; i++)
        printf("makeaddr(%x, %x) = %08x\n", nets[i][0], nets[i][1], ntohl(inet_makeaddr(nets[i][0], nets[i][1]).s_addr));
    struct in_addr x = {htonl(0x01020304)}, y = {htonl(0x05060708)};
    char *p1 = inet_ntoa(x);
    char *p2 = inet_ntoa(y);
    printf("inet_ntoa shared buffer: %d [%s]\n", p1 == p2, p1);
}

static void net_cases(void)
{
    printf("== inet_net_pton / inet_net_ntop\n");
    static const char *in[] = {"10/8", "10.1/16", "10.1.2.3/24", "10.1.2.3", "192.168.1/24", "0x0a/8", "10/33", "10/0", "1.2.3.4/32",
                               "default", "0/0", "10.1.2.3/7", "224/4", "128/2", "abc", "10/", "/8", "1.2.3.4.5/32", "12345/8"};
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        unsigned char b[4] = {0xee, 0xee, 0xee, 0xee};
        errno = 0;
        int bits = inet_net_pton(AF_INET, in[i], b, sizeof b);
        printf("[%s] -> %d errno=%d ", in[i], bits, errno);
        hex(b, 4);
        if (bits >= 0) {
            char out[64];
            const char *r = inet_net_ntop(AF_INET, b, bits, out, sizeof out);
            printf(" ntop=%s", r ? r : "NULL");
        }
        printf("\n");
    }
    unsigned char b[4] = {10, 1, 0, 0};
    char out[64];
    for (int bits = 0; bits <= 33; bits += 3) {
        errno = 0;
        const char *r = inet_net_ntop(AF_INET, b, bits, out, sizeof out);
        printf("net_ntop bits %d -> %s errno=%d\n", bits, r ? r : "NULL", errno);
    }
    errno = 0;
    const char *rs = inet_net_ntop(AF_INET, b, 16, out, 4);
    printf("net_ntop small -> %s errno=%d\n", rs ? "ok" : "NULL", errno);
    unsigned char b6[16];
    errno = 0;
    int r6 = inet_net_pton(AF_INET6, "2001:db8::/32", b6, sizeof b6);
    printf("net_pton v6 -> %d errno=%d\n", r6, errno);
    printf("== inet_nsap / inet_neta\n");
    static const char *ns[] = {"0x47.0005.80ffef00", "47000580ffef00", "0x", "0x1", "0x12.34", "0xgg", "0x0123456789abcdef0123456789abcdef0123456789"};
    for (size_t i = 0; i < sizeof ns / sizeof ns[0]; i++) {
        unsigned char buf[32];
        unsigned n = inet_nsap_addr(ns[i], buf, sizeof buf);
        printf("nsap_addr [%s] -> %u ", ns[i], n);
        hex(buf, n);
        if (n) {
            char t[128];
            printf(" ntoa=%s", inet_nsap_ntoa((int)n, buf, t));
        }
        printf("\n");
    }
    for (unsigned long v = 0; v < 0x1000000; v = v * 0x101 + 0x0a) {
        char t[32];
        char *r = inet_neta(v, t, sizeof t);
        printf("neta %lx -> %s\n", v, r ? r : "NULL");
        if (v > 0x10000)
            break;
    }
    char t2[4];
    errno = 0;
    char *rn = inet_neta(0x0a010203, t2, sizeof t2);
    printf("neta small -> %s errno=%d\n", rn ? "ok" : "NULL", errno);
}

static void ether_cases(void)
{
    printf("== ether\n");
    static const char *in[] = {"00:11:22:33:44:55", "0:1:2:3:4:5", "AA:BB:CC:DD:EE:FF", "00:11:22:33:44", "00:11:22:33:44:55:66", "00-11-22-33-44-55",
                               "001122334455", "00:11:22:33:44:5g", "100:11:22:33:44:55", " 00:11:22:33:44:55", "00:11:22:33:44:55 x", ""};
    for (size_t i = 0; i < sizeof in / sizeof in[0]; i++) {
        struct ether_addr e;
        memset(&e, 0xee, sizeof e);
        struct ether_addr *r = ether_aton_r(in[i], &e);
        printf("[%s] -> %s ", in[i], r ? "ok" : "NULL");
        hex(&e, 6);
        if (r) {
            char buf[32];
            printf(" ntoa=%s", ether_ntoa_r(&e, buf));
        }
        printf("\n");
    }
    struct ether_addr *s1 = ether_aton("01:02:03:04:05:06");
    struct ether_addr *s2 = ether_aton("0a:0b:0c:0d:0e:0f");
    printf("ether_aton shared: %d %s\n", s1 == s2, ether_ntoa(s1));
    static const char *lines[] = {"00:11:22:33:44:55 hostname", "00:11:22:33:44:55\thost # comment", "# comment", "00:11:22:33:44:55",
                                  "  00:11:22:33:44:55   spaced  ", "zz:11:22:33:44:55 bad", "00:11:22:33:44:55 a b", ""};
    for (size_t i = 0; i < sizeof lines / sizeof lines[0]; i++) {
        struct ether_addr e;
        char host[256] = "untouched";
        memset(&e, 0xee, sizeof e);
        int r = ether_line(lines[i], &e, host);
        printf("ether_line [%s] -> %d host=[%s] ", lines[i], r, host);
        hex(&e, 6);
        printf("\n");
    }
}

static void rfc3542(void)
{
    printf("== inet6_opt\n");
    unsigned char buf[64];
    memset(buf, 0xee, sizeof buf);
    int len = inet6_opt_init(NULL, 0);
    printf("init size: %d\n", len);
    int off = inet6_opt_init(buf, sizeof buf);
    printf("init: %d\n", off);
    void *db;
    int o2 = inet6_opt_append(buf, sizeof buf, off, 0xc2, 4, 4, &db);
    printf("append 4/4: %d\n", o2);
    if (o2 > 0) {
        unsigned v = htonl(0x11223344);
        printf("set_val: %d\n", inet6_opt_set_val(db, 0, &v, 4));
    }
    int o3 = inet6_opt_append(buf, sizeof buf, o2, 0x05, 2, 2, &db);
    printf("append 2/2: %d\n", o3);
    if (o3 > 0) {
        unsigned short v = htons(0xabcd);
        inet6_opt_set_val(db, 0, &v, 2);
    }
    int o4 = inet6_opt_append(buf, sizeof buf, o3, 0x10, 3, 8, &db);
    printf("append 3/8 (bad align): %d\n", o4);
    int o5 = inet6_opt_append(buf, sizeof buf, o3, 0x00, 4, 4, &db);
    printf("append type 0 (pad): %d\n", o5);
    int fin = inet6_opt_finish(buf, sizeof buf, o3);
    printf("finish: %d sizeonly=%d\n", fin, inet6_opt_finish(NULL, 0, o3));
    if (fin > 0) {
        printf("bytes: ");
        hex(buf, (size_t)fin);
        printf("\n");
        buf[1] = (unsigned char)(fin / 8 - 1);
        int pos = 0;
        uint8_t type;
        socklen_t olen;
        void *val;
        while ((pos = inet6_opt_next(buf, fin, pos, &type, &olen, &val)) != -1) {
            printf("next: pos=%d type=%02x len=%u\n", pos, type, olen);
            if (olen == 4) {
                unsigned v;
                inet6_opt_get_val(val, 0, &v, 4);
                printf("  val=%08x\n", ntohl(v));
            }
        }
        int f = inet6_opt_find(buf, fin, 0, 0x05, &olen, &val);
        printf("find 05: %d len=%u\n", f, olen);
        printf("find 99: %d\n", inet6_opt_find(buf, fin, 0, 0x99, &olen, &val));
    }
    printf("== inet6_rth\n");
    for (int seg = 0; seg <= 3; seg++)
        printf("rth_space type0 %d: %u\n", seg, (unsigned)inet6_rth_space(IPV6_RTHDR_TYPE_0, seg));
    printf("rth_space type 9: %u\n", (unsigned)inet6_rth_space(9, 1));
    unsigned char rb[128];
    memset(rb, 0xee, sizeof rb);
    void *rh = inet6_rth_init(rb, sizeof rb, IPV6_RTHDR_TYPE_0, 3);
    printf("rth_init: %s\n", rh ? "ok" : "NULL");
    printf("rth_init small: %s\n", inet6_rth_init(rb, 8, IPV6_RTHDR_TYPE_0, 3) ? "ok" : "NULL");
    rh = inet6_rth_init(rb, sizeof rb, IPV6_RTHDR_TYPE_0, 3);
    static const char *hops[] = {"2001:db8::1", "2001:db8::2", "2001:db8::3", "2001:db8::4"};
    for (int i = 0; i < 4; i++) {
        struct in6_addr a;
        inet_pton(AF_INET6, hops[i], &a);
        printf("rth_add %d: %d\n", i, inet6_rth_add(rh, &a));
    }
    printf("segments: %d\n", inet6_rth_segments(rh));
    unsigned char rev[128];
    memset(rev, 0xee, sizeof rev);
    printf("reverse: %d\n", inet6_rth_reverse(rh, rev));
    for (int i = 0; i <= 3; i++) {
        struct in6_addr *g = inet6_rth_getaddr(rev, i);
        char t[64];
        printf("getaddr %d: %s\n", i, g ? inet_ntop(AF_INET6, g, t, sizeof t) : "NULL");
    }
    printf("header: ");
    hex(rb, 8);
    printf("\n");
}

static void interfaces(void)
{
    printf("== interfaces\n");
    unsigned lo = if_nametoindex("lo");
    printf("if_nametoindex lo: %d\n", lo != 0);
    errno = 0;
    unsigned ix = if_nametoindex("nosuch0");
    printf("if_nametoindex nosuch: %u errno=%d\n", ix, errno);
    errno = 0;
    ix = if_nametoindex("abcdefghijklmnopqrstuvwxyz");
    printf("if_nametoindex long: %u errno=%d\n", ix, errno);
    char name[IF_NAMESIZE];
    char *n = if_indextoname(lo, name);
    printf("if_indextoname lo: %s\n", n ? n : "NULL");
    errno = 0;
    n = if_indextoname(999999, name);
    printf("if_indextoname 999999: %s errno=%d\n", n ? n : "NULL", errno);
    struct if_nameindex *ni = if_nameindex();
    int found = 0, count = 0, terminated = 0;
    if (ni) {
        for (struct if_nameindex *p = ni; ; p++) {
            if (p->if_index == 0 && p->if_name == NULL) {
                terminated = 1;
                break;
            }
            count++;
            if (strcmp(p->if_name, "lo") == 0 && p->if_index == lo)
                found = 1;
        }
        if_freenameindex(ni);
    }
    printf("if_nameindex: lo=%d count>0=%d terminated=%d\n", found, count > 0, terminated);
    struct ifaddrs *ifa;
    errno = 0;
    int r = getifaddrs(&ifa);
    printf("getifaddrs: %d errno=%d\n", r, errno);
    if (r == 0) {
        for (struct ifaddrs *p = ifa; p; p = p->ifa_next) {
            if (strcmp(p->ifa_name, "lo") != 0)
                continue;
            int fam = p->ifa_addr ? p->ifa_addr->sa_family : -1;
            char a[64] = "-", m[64] = "-";
            if (fam == AF_INET) {
                inet_ntop(AF_INET, &((struct sockaddr_in *)p->ifa_addr)->sin_addr, a, sizeof a);
                if (p->ifa_netmask)
                    inet_ntop(AF_INET, &((struct sockaddr_in *)p->ifa_netmask)->sin_addr, m, sizeof m);
            } else if (fam == AF_INET6) {
                inet_ntop(AF_INET6, &((struct sockaddr_in6 *)p->ifa_addr)->sin6_addr, a, sizeof a);
                if (p->ifa_netmask)
                    inet_ntop(AF_INET6, &((struct sockaddr_in6 *)p->ifa_netmask)->sin6_addr, m, sizeof m);
            }
            printf("lo fam=%d flags_up_loop=%d addr=%s mask=%s data=%d\n", fam, (p->ifa_flags & (IFF_UP | IFF_LOOPBACK)) == (IFF_UP | IFF_LOOPBACK), a, m,
                   p->ifa_data != NULL);
        }
        freeifaddrs(ifa);
    }
}

static void misc(void)
{
    printf("== misc\n");
    char t[64];
    printf("in6addr_any=%s in6addr_loopback=%s\n", inet_ntop(AF_INET6, &in6addr_any, t, sizeof t), inet_ntop(AF_INET6, &in6addr_loopback, t + 32, 32));
    printf("htonl=%08x htons=%04x ntohl=%08x ntohs=%04x\n", htonl(0x11223344), htons(0x1122), ntohl(0x11223344), ntohs(0x1122));
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct in_addr grp, ifc, src;
    inet_aton("239.1.1.1", &grp);
    inet_aton("127.0.0.1", &ifc);
    uint32_t mode = 99, n = 4;
    errno = 0;
    int rf = getipv4sourcefilter(s, ifc, grp, &mode, &n, &src);
    printf("getipv4sourcefilter unjoined: %d errno=%d\n", rf, errno);
    errno = 0;
    rf = getipv4sourcefilter(-1, ifc, grp, &mode, &n, &src);
    printf("getipv4sourcefilter on -1: %d errno=%d\n", rf, errno);
    close(s);
    int rk = ruserok("localhost", 0, "nobody", "nobody");
    printf("ruserok no files: %d\n", rk);
}

int main(void)
{
    pton_cases();
    ntop_cases();
    aton_cases();
    classful();
    net_cases();
    ether_cases();
    rfc3542();
    interfaces();
    misc();
    printf("done\n");
    return 0;
}
