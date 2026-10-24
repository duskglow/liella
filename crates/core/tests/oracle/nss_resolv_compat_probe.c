/* Black-box probe of the compat-only NSS and resolver names (phase 15, LG-7 COMPAT-NSS-RESOLV).
 *
 * Binds each compat version with `.symver` (a program linked anew cannot reach it otherwise) and
 * prints what a call returns and writes. Every call runs in a forked child with an alarm, so a
 * crash or hang is recorded instead of ending the probe. The `__nss_*` names have no public
 * declaration: they are called with six pointer arguments, each a 64-byte buffer of 0xAA (or all
 * NULL), and the probe prints the return value, errno (preset to 12345) and which buffer bytes
 * changed. The host lookups print the entry next to `gethostbyname`'s for the same input.
 *
 * Build: cc -O0 -o nss_resolv_compat_probe nss_resolv_compat_probe.c -lresolv
 * Record: spec/behaviour/nss-resolv-compat.glibc.txt (see spec/behaviour/nss-resolv-compat.md). */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

typedef long (*f6)(void *, void *, void *, void *, void *, void *);
extern long c_nss_database_lookup(void *, void *, void *, void *, void *, void *);
extern long c_nss_group_lookup(void *, void *, void *, void *, void *, void *);
extern long c_nss_hosts_lookup(void *, void *, void *, void *, void *, void *);
extern long c_nss_next(void *, void *, void *, void *, void *, void *);
extern long c_nss_passwd_lookup(void *, void *, void *, void *, void *, void *);
__asm__(".symver c_nss_database_lookup, __nss_database_lookup@GLIBC_2.2.5");
__asm__(".symver c_nss_group_lookup, __nss_group_lookup@GLIBC_2.2.5");
__asm__(".symver c_nss_hosts_lookup, __nss_hosts_lookup@GLIBC_2.2.5");
__asm__(".symver c_nss_next, __nss_next@GLIBC_2.2.5");
__asm__(".symver c_nss_passwd_lookup, __nss_passwd_lookup@GLIBC_2.2.5");

extern struct hostent *c_gethtbyname(const char *);
extern struct hostent *c_gethtbyname2(const char *, int);
extern struct hostent *c_gethtbyaddr(const void *, int, int);
extern struct hostent *c_gethtent(void);
extern void c_sethtent(int);
extern struct hostent *c_res_gethostbyname(const char *);
extern struct hostent *c_res_gethostbyname2(const char *, int);
extern struct hostent *c_res_gethostbyaddr(const void *, int, int);
__asm__(".symver c_gethtbyname, _gethtbyname@GLIBC_2.2.5");
__asm__(".symver c_gethtbyname2, _gethtbyname2@GLIBC_2.2.5");
__asm__(".symver c_gethtbyaddr, _gethtbyaddr@GLIBC_2.2.5");
__asm__(".symver c_gethtent, _gethtent@GLIBC_2.2.5");
__asm__(".symver c_sethtent, _sethtent@GLIBC_2.2.5");
__asm__(".symver c_res_gethostbyname, res_gethostbyname@GLIBC_2.2.5");
__asm__(".symver c_res_gethostbyname2, res_gethostbyname2@GLIBC_2.2.5");
__asm__(".symver c_res_gethostbyaddr, res_gethostbyaddr@GLIBC_2.2.5");

static void child_end(const char *what) {
    int st;
    wait(&st);
    if (WIFSIGNALED(st)) printf("%s: killed by signal %d\n", what, WTERMSIG(st));
    fflush(stdout);
}

static void nss_call(const char *what, f6 f, int mode) {
    fflush(stdout);
    if (fork() == 0) {
        alarm(5);
        static unsigned char b[6][64];
        void *a[6];
        for (int i = 0; i < 6; i++) {
            memset(b[i], 0xAA, 64);
            a[i] = mode == 0 ? NULL : (void *)b[i];
        }
        if (mode == 2) { a[0] = "passwd"; a[1] = "files"; a[2] = "files"; }
        errno = 12345;
        long r = f(a[0], a[1], a[2], a[3], a[4], a[5]);
        int e = errno;
        printf("%s(%s): ret=%ld (int %d) errno=%d", what, mode == 0 ? "NULLs" : mode == 1 ? "0xAA bufs" : "strings,bufs", r, (int)r, e);
        for (int i = 0; i < 6; i++) {
            if (a[i] != (void *)b[i]) continue;
            for (int j = 0; j < 64; j++)
                if (b[i][j] != 0xAA) { printf(" arg%d[%d]=%02x", i, j, b[i][j]); }
        }
        printf("\n");
        fflush(stdout);
        _exit(0);
    }
    child_end(what);
}

static void show(const char *what, struct hostent *h, struct hostent *ref) {
    int he = h_errno;
    printf("%s: ", what);
    if (!h) {
        printf("NULL h_errno=%d", he);
    } else {
        char a[64] = "-";
        if (h->h_addr_list && h->h_addr_list[0]) inet_ntop(h->h_addrtype, h->h_addr_list[0], a, sizeof a);
        int na = 0, nal = 0;
        while (h->h_addr_list && h->h_addr_list[na]) na++;
        while (h->h_aliases && h->h_aliases[nal]) nal++;
        printf("name=%s type=%d len=%d addrs=%d first=%s aliases=%d%s%s", h->h_name, h->h_addrtype, h->h_length, na, a,
               nal, nal ? " alias0=" : "", nal ? h->h_aliases[0] : "");
    }
    if (ref != (struct hostent *)-1) printf(" same-buffer-as-gethostbyname=%s", h && h == ref ? "yes" : "no");
    printf("\n");
}

#define HOSTCALL(label, call, refcall)                                       \
    do {                                                                     \
        fflush(stdout);                                                      \
        if (fork() == 0) {                                                   \
            alarm(10);                                                       \
            struct hostent *r_ = (refcall);                                  \
            if (r_) show("  reference " label, r_, (struct hostent *)-1);    \
            else show("  reference " label, NULL, (struct hostent *)-1);     \
            h_errno = 777; errno = 12345;                                    \
            struct hostent *h_ = (call);                                     \
            int e_ = errno;                                                  \
            show(label, h_, r_);                                             \
            printf("  errno=%d\n", e_);                                      \
            fflush(stdout);                                                  \
            _exit(0);                                                        \
        }                                                                    \
        child_end(label);                                                    \
    } while (0)

int main(void) {
    struct { const char *n; f6 f; } nss[] = {
        {"__nss_database_lookup", c_nss_database_lookup}, {"__nss_group_lookup", c_nss_group_lookup},
        {"__nss_hosts_lookup", c_nss_hosts_lookup},       {"__nss_next", c_nss_next},
        {"__nss_passwd_lookup", c_nss_passwd_lookup},
    };
    for (unsigned i = 0; i < sizeof nss / sizeof nss[0]; i++)
        for (int m = 0; m < 3; m++) nss_call(nss[i].n, nss[i].f, m);

    unsigned char lo4[4] = {127, 0, 0, 1};
    unsigned char lo6[16] = {0};
    lo6[15] = 1;
    HOSTCALL("_gethtbyname(localhost)", c_gethtbyname("localhost"), gethostbyname("localhost"));
    HOSTCALL("_gethtbyname(127.0.0.1)", c_gethtbyname("127.0.0.1"), gethostbyname("127.0.0.1"));
    HOSTCALL("_gethtbyname(no-such-host.invalid)", c_gethtbyname("no-such-host.invalid"), gethostbyname("no-such-host.invalid"));
    HOSTCALL("_gethtbyname2(localhost,AF_INET)", c_gethtbyname2("localhost", AF_INET), gethostbyname2("localhost", AF_INET));
    HOSTCALL("_gethtbyname2(localhost,AF_INET6)", c_gethtbyname2("localhost", AF_INET6), gethostbyname2("localhost", AF_INET6));
    HOSTCALL("_gethtbyname2(ip6-localhost,AF_INET6)", c_gethtbyname2("ip6-localhost", AF_INET6), gethostbyname2("ip6-localhost", AF_INET6));
    HOSTCALL("_gethtbyname2(localhost,99)", c_gethtbyname2("localhost", 99), gethostbyname2("localhost", 99));
    HOSTCALL("_gethtbyaddr(127.0.0.1,4,AF_INET)", c_gethtbyaddr(lo4, 4, AF_INET), gethostbyaddr(lo4, 4, AF_INET));
    HOSTCALL("_gethtbyaddr(::1,16,AF_INET6)", c_gethtbyaddr(lo6, 16, AF_INET6), gethostbyaddr(lo6, 16, AF_INET6));
    HOSTCALL("_gethtbyaddr(127.0.0.1,3,AF_INET)", c_gethtbyaddr(lo4, 3, AF_INET), gethostbyaddr(lo4, 3, AF_INET));
    HOSTCALL("res_gethostbyname(localhost)", c_res_gethostbyname("localhost"), gethostbyname("localhost"));
    HOSTCALL("res_gethostbyname(127.0.0.1)", c_res_gethostbyname("127.0.0.1"), gethostbyname("127.0.0.1"));
    HOSTCALL("res_gethostbyname(no-such-host.invalid)", c_res_gethostbyname("no-such-host.invalid"), gethostbyname("no-such-host.invalid"));
    HOSTCALL("res_gethostbyname2(localhost,AF_INET6)", c_res_gethostbyname2("localhost", AF_INET6), gethostbyname2("localhost", AF_INET6));
    HOSTCALL("res_gethostbyname2(127.0.0.1,AF_INET)", c_res_gethostbyname2("127.0.0.1", AF_INET), gethostbyname2("127.0.0.1", AF_INET));
    HOSTCALL("res_gethostbyaddr(127.0.0.1,4,AF_INET)", c_res_gethostbyaddr(lo4, 4, AF_INET), gethostbyaddr(lo4, 4, AF_INET));
    HOSTCALL("res_gethostbyaddr(127.0.0.1,3,AF_INET)", c_res_gethostbyaddr(lo4, 3, AF_INET), gethostbyaddr(lo4, 3, AF_INET));

    /* _sethtent/_gethtent: walk the whole table; compare with sethostent/gethostent. */
    fflush(stdout);
    if (fork() == 0) {
        alarm(10);
        int n = 0, m = 0;
        errno = 12345;
        c_sethtent(0);
        printf("_sethtent(0): errno=%d\n", errno);
        struct hostent *h;
        while (n < 10000 && (h = c_gethtent())) {
            if (n < 3) show("  _gethtent entry", h, (struct hostent *)-1);
            n++;
        }
        printf("_gethtent: %d entries, end h_errno=%d\n", n, h_errno);
        h = c_gethtent();
        printf("_gethtent after end: %s\n", h ? "entry" : "NULL");
        c_sethtent(1);
        h = c_gethtent();
        printf("_gethtent after _sethtent(1): %s\n", h ? "entry (rewound)" : "NULL");
        sethostent(0);
        while (m < 10000 && gethostent()) m++;
        endhostent();
        printf("gethostent (reference): %d entries\n", m);
        fflush(stdout);
        _exit(0);
    }
    child_end("_gethtent");
    return 0;
}
