/* net_dns.c: settles getnetbyname/getnetbyaddr through the dns source (NS-NETWORKS-DNS). Runs in
 * its own user, mount and network namespaces (unshare as an unprivileged user: no root, no host
 * state changed): nsswitch.conf and resolv.conf are bind-mounted over /etc, lo is brought up, and
 * a tiny UDP server on 127.0.0.1:53 answers from a table keyed on the question name. Every query
 * is printed (name, type), then the call's result and h_errno. Build: cc -O1 -o net_dns net_dns.c */
#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <unistd.h>
#include <fcntl.h>
#include <netdb.h>
#include <net/if.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <netinet/in.h>
#include <arpa/inet.h>

static void wr(const char *p, const char *s) { int f = open(p, O_WRONLY | O_CREAT | O_TRUNC, 0644); if (f < 0) { perror(p); exit(2); } write(f, s, strlen(s)); close(f); }

enum { PTR, A, NX, NODATA, SERVFAIL, REFUSED, SHORT, BADPTR, MANY, TC, OWNER, CNAMEPTR };
static const struct { const char *q; int kind; const char *data; } table[] = {
  { "p1.test", PTR, "netx.test" },
  { "p2.test", PTR, "0.2.0.192.in-addr.arpa" },
  { "p3.test", PTR, "2.0.192.in-addr.arpa" },
  { "p4.test", PTR, "10.in-addr.arpa" },
  { "p5.test", PTR, "16.172.in-addr.arpa" },
  { "p6.test", PTR, "1.1.168.192.in-addr.arpa" },
  { "p7.test", PTR, "junk.in-addr.arpa" },
  { "p8.test", PTR, "300.in-addr.arpa" },
  { "a1.test", A, 0 },
  { "nx.test", NX, 0 },
  { "nodata.test", NODATA, 0 },
  { "servfail.test", SERVFAIL, 0 },
  { "refused.test", REFUSED, 0 },
  { "short.test", SHORT, "2.0.192.in-addr.arpa" },
  { "badptr.test", BADPTR, 0 },
  { "many.test", MANY, 0 },
  { "tc.test", TC, 0 },
  { "owner.test", OWNER, "2.0.192.in-addr.arpa" },
  { "bare.s.test", PTR, "10.in-addr.arpa" },
  { "0.0.0.11.in-addr.arpa", SERVFAIL, 0 },
  { "0.0.0.12.in-addr.arpa", NODATA, 0 },
  { "0.0.0.13.in-addr.arpa", NX, 0 },
  { "0.0.0.14.in-addr.arpa", SHORT, "net14.test" },
  { "0.0.0.15.in-addr.arpa", MANY, 0 },
  { "0.0.0.16.in-addr.arpa", BADPTR, 0 },
  { "0.0.0.17.in-addr.arpa", TC, 0 },
  { "0.0.0.18.in-addr.arpa", A, 0 },
  { "0.0.0.19.in-addr.arpa", REFUSED, 0 },
  { "0.0.0.20.in-addr.arpa", OWNER, "net20.test" },
  { "0.0.0.21.in-addr.arpa", PTR, "net21.test." },
  { "0.0.0.22.in-addr.arpa", PTR, "x" },
};

static int put_name(unsigned char *o, const char *s) {
  int m = 0;
  while (*s) { const char *d = strchr(s, '.'); int l = d ? (int)(d - s) : (int)strlen(s); o[m++] = l; memcpy(o + m, s, l); m += l; s += l; if (*s) s++; }
  o[m++] = 0; return m;
}

static int put_rr(unsigned char *o, int type, const unsigned char *owner, int ol, const char *target, const unsigned char *raw, int rl) {
  int m = 0;
  memcpy(o, owner, ol); m += ol;
  o[m++] = 0; o[m++] = type; o[m++] = 0; o[m++] = 1; o[m++] = 0; o[m++] = 0; o[m++] = 0; o[m++] = 60;
  int at = m; m += 2;
  int dl = target ? put_name(o + m, target) : rl;
  if (!target) memcpy(o + m, raw, rl);
  o[at] = dl >> 8; o[at + 1] = dl; return m + dl;
}

static void server(int s) {
  unsigned char b[512], o[1500];
  static const unsigned char ptr0[] = { 0xc0, 0x0c };
  for (;;) {
    struct sockaddr_in from; socklen_t fl = sizeof from;
    int n = recvfrom(s, b, sizeof b, 0, (void *)&from, &fl);
    if (n < 17) continue;
    int i = 12; char dotted[256] = ""; int dl = 0;
    while (i < n && b[i]) { int l = b[i++]; if (dl) dotted[dl++] = '.'; memcpy(dotted + dl, b + i, l); dl += l; i += l; }
    dotted[dl] = 0; i++;
    int qtype = b[i] << 8 | b[i + 1]; int qend = i + 4;
    printf("  query [%s] type=%d\n", dotted, qtype);
    int kind = PTR; const char *data = "netx.test";
    /* A table name matches the question or its prefix (the search list's tries get its answer). */
    for (unsigned k = 0; k < sizeof table / sizeof *table; k++) {
      size_t l = strlen(table[k].q);
      if (!strncasecmp(table[k].q, dotted, l) && (dotted[l] == 0 || dotted[l] == '.')) { kind = table[k].kind; data = table[k].data; break; }
    }
    memcpy(o, b, qend); o[2] = 0x84 | (b[2] & 1); o[3] = 0x80; o[6] = o[7] = o[8] = o[9] = o[10] = o[11] = 0;
    int m = qend, an = 0;
    switch (kind) {
    case NX: o[3] |= 3; break;
    case SERVFAIL: o[3] |= 2; break;
    case REFUSED: o[3] |= 5; break;
    case NODATA: break;
    case TC: o[2] |= 2; break;
    case A: { static const unsigned char ip[] = { 192, 0, 2, 0 }; m += put_rr(o + m, 1, ptr0, 2, 0, ip, 4); an = 1; break; }
    case SHORT: m += put_rr(o + m, 12, ptr0, 2, data, 0, 0); an = 3; break;
    case BADPTR: { static const unsigned char bad[] = { 0xc5, 0xff }; m += put_rr(o + m, 12, ptr0, 2, 0, bad, 2); an = 1; break; }
    case MANY: { static const char *t[] = { "m1.test", "m2.test", "m3.test", "m4.test", "m5.test" }; for (int k = 0; k < 5; k++) m += put_rr(o + m, 12, ptr0, 2, t[k], 0, 0); an = 5; break; }
    case OWNER: { unsigned char w[64]; int wl = put_name(w, "other.test"); m += put_rr(o + m, 12, w, wl, data, 0, 0); an = 1; break; }
    default: m += put_rr(o + m, 12, ptr0, 2, data, 0, 0); an = 1; break;
    }
    o[7] = an;
    sendto(s, o, m, 0, (void *)&from, fl);
  }
}

static void show(const char *tag, struct netent *e) {
  if (!e) { printf("%s: NULL h_errno=%d\n", tag, h_errno); return; }
  printf("%s: name=[%s] aliases=", tag, e->n_name);
  for (char **a = e->n_aliases; a && *a; a++) printf("[%s]", *a);
  printf(" type=%d net=%#x h_errno=%d\n", e->n_addrtype, e->n_net, h_errno);
}

static void byname(const char *n) { char t[128]; snprintf(t, sizeof t, "getnetbyname %s", n); h_errno = 0; show(t, getnetbyname(n)); }
static void byaddr(unsigned v, int ty) { char t[128]; snprintf(t, sizeof t, "getnetbyaddr %#x/%d", v, ty); h_errno = 0; show(t, getnetbyaddr(v, ty)); }

int main(void) {
  uid_t u = getuid(); gid_t gi = getgid();
  /* Already namespace root (run under `unshare -r -m -n`, where AppArmor forbids a plain
   * binary's own unshare): skip the namespace set-up. */
  if (u != 0) {
    if (unshare(CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET)) { perror("unshare"); return 2; }
    char m[64];
    snprintf(m, sizeof m, "0 %u 1\n", u); wr("/proc/self/uid_map", m);
    wr("/proc/self/setgroups", "deny");
    snprintf(m, sizeof m, "0 %u 1\n", gi); wr("/proc/self/gid_map", m);
  }
  mount("none", "/", NULL, MS_REC | MS_PRIVATE, NULL);
  int s0 = socket(AF_INET, SOCK_DGRAM, 0); struct ifreq q; memset(&q, 0, sizeof q); strcpy(q.ifr_name, "lo");
  ioctl(s0, SIOCGIFFLAGS, &q); q.ifr_flags |= IFF_UP; if (ioctl(s0, SIOCSIFFLAGS, &q)) { perror("lo up"); return 2; }
  char d[] = "/tmp/netdnsXXXXXX"; if (!mkdtemp(d)) return 2;
  char p[128];
#define FIX(name, text, target) snprintf(p, sizeof p, "%s/" name, d); wr(p, text); if (mount(p, target, NULL, MS_BIND, NULL)) { perror(target); return 2; }
  FIX("nsswitch", "networks: dns\nhosts: files\n", "/etc/nsswitch.conf")
  FIX("resolv", "nameserver 127.0.0.1\nsearch s.test\noptions attempts:1 timeout:1\n", "/etc/resolv.conf")
  int s = socket(AF_INET, SOCK_DGRAM, 0); struct sockaddr_in a = { AF_INET, htons(53), { htonl(INADDR_LOOPBACK) } };
  if (bind(s, (void *)&a, sizeof a)) { perror("bind"); return 2; }
  setvbuf(stdout, NULL, _IONBF, 0);
  pid_t pid = fork(); if (!pid) { server(s); _exit(0); }
  printf("== getnetbyname\n");
  const char *names[] = { "p1.test", "p2.test", "p3.test", "p4.test", "p5.test", "p6.test", "p7.test", "p8.test", "a1.test", "nx.test",
    "nodata.test", "servfail.test", "refused.test", "short.test", "badptr.test", "many.test", "tc.test", "owner.test", "bare", "P3.TEST", "" };
  for (unsigned k = 0; k < sizeof names / sizeof *names; k++) byname(names[k]);
  printf("== getnetbyaddr widths\n");
  unsigned w[] = { 10, 0x7f, 0x80, 0xbf, 0xc0, 0xAC10, 0xAC100000, 0xC00002, 0xC0000200, 0xC0A80101, 0x0A0B, 0x0A0B0C, 0x0A0B0C0D,
    0xE0000001, 0xE0, 0xF0, 0xFF, 0, 0xFFFFFFFF, 0x80000000, 0x8001, 0x800100, 0xC0000000 };
  for (unsigned k = 0; k < sizeof w / sizeof *w; k++) byaddr(w[k], AF_INET);
  byaddr(10, AF_INET6);
  byaddr(10, 0);
  printf("== getnetbyaddr answers\n");
  for (unsigned v = 11; v <= 22; v++) byaddr(v, AF_INET);
  kill(pid, 9); waitpid(pid, NULL, 0);
  return 0;
}
