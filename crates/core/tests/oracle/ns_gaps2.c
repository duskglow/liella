/* ns_gaps2.c: settles the networks database through dns and the group-list merge when the groups
 * disagree on name or GID. Runs in its own user, mount and network namespaces (unshare as an
 * unprivileged user: no root, no host state changed): the fixture files are bind-mounted over the
 * /etc files, lo is brought up, and a tiny UDP server on 127.0.0.1:53 answers PTR queries. */
#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <netdb.h>
#include <grp.h>
#include <net/if.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <netinet/in.h>
#include <arpa/inet.h>

static void wr(const char *p, const char *s) { int f = open(p, O_WRONLY | O_CREAT | O_TRUNC, 0644); if (f < 0) { perror(p); exit(2); } write(f, s, strlen(s)); close(f); }

static void server(int s) {
  unsigned char b[512], o[600];
  for (;;) {
    struct sockaddr_in from; socklen_t fl = sizeof from;
    int n = recvfrom(s, b, sizeof b, 0, (void *)&from, &fl);
    if (n < 17) continue;
    int i = 12; char dotted[256] = ""; int dl = 0;
    while (i < n && b[i]) { int l = b[i++]; if (dl) dotted[dl++] = '.'; memcpy(dotted + dl, b + i, l); dl += l; i += l; }
    dotted[dl] = 0; i++;
    int qtype = b[i] << 8 | b[i + 1]; int qend = i + 4;
    char line[400]; int k = snprintf(line, sizeof line, "  query [%s] type=%d\n", dotted, qtype); write(1, line, k);
    memcpy(o, b, qend); o[2] = 0x84; o[3] = 0x80; o[6] = 0; o[7] = qtype == 12 && strncmp(dotted, "nx.", 3) ? 1 : 0; o[8] = o[9] = o[10] = o[11] = 0;
    int m = qend;
    if (o[7]) { static const unsigned char rr[] = { 0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 60, 0, 11, 4, 'n', 'e', 't', 'x', 4, 't', 'e', 's', 't', 0 };
      /* rdlength 11: 4 netx 4 test 0 = 1+4+1+4+1 */
      memcpy(o + m, rr, sizeof rr); m += sizeof rr; }
    sendto(s, o, m, 0, (void *)&from, fl);
  }
}

static void show_net(const char *tag, struct netent *e) {
  if (!e) { printf("%s: NULL h_errno=%d\n", tag, h_errno); return; }
  printf("%s: name=[%s] aliases=", tag, e->n_name);
  for (char **a = e->n_aliases; a && *a; a++) printf("[%s]", *a);
  printf(" type=%d net=%#x\n", e->n_addrtype, e->n_net);
}

static void groups(const char *user, gid_t base) {
  gid_t g[32]; int n = 32; int r = getgrouplist(user, base, g, &n);
  printf("getgrouplist %s base=%u: r=%d n=%d [", user, base, r, n);
  for (int i = 0; i < n && i < 32; i++) printf("%s%u", i ? " " : "", g[i]);
  printf("]\n");
}

int main(void) {
  uid_t u = getuid(); gid_t gi = getgid();
  if (unshare(CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET)) { perror("unshare"); return 2; }
  char m[64];
  snprintf(m, sizeof m, "0 %u 1\n", u); wr("/proc/self/uid_map", m);
  wr("/proc/self/setgroups", "deny");
  snprintf(m, sizeof m, "0 %u 1\n", gi); wr("/proc/self/gid_map", m);
  mount("none", "/", NULL, MS_REC | MS_PRIVATE, NULL);
  int s0 = socket(AF_INET, SOCK_DGRAM, 0); struct ifreq q; memset(&q, 0, sizeof q); strcpy(q.ifr_name, "lo");
  ioctl(s0, SIOCGIFFLAGS, &q); q.ifr_flags |= IFF_UP; if (ioctl(s0, SIOCSIFFLAGS, &q)) { perror("lo up"); return 2; }
  char d[] = "/tmp/nsgapsXXXXXX"; if (!mkdtemp(d)) return 2;
  char p[128];
#define FIX(name, text, target) snprintf(p, sizeof p, "%s/" name, d); wr(p, text); if (mount(p, target, NULL, MS_BIND, NULL)) { perror(target); return 2; }
  FIX("nsswitch", "networks: dns\ngroup: files\ninitgroups: files\nhosts: files\n", "/etc/nsswitch.conf")
  FIX("resolv", "nameserver 127.0.0.1\noptions attempts:1 timeout:1\n", "/etc/resolv.conf")
  FIX("group", "a:x:50:u\nb:x:50:u\nc:x:60:u\nc:x:61:u\nd:x:60:v\ne:x:70:u,u\nf:x:99:u\n", "/etc/group")
  int s = socket(AF_INET, SOCK_DGRAM, 0); struct sockaddr_in a = { AF_INET, htons(53), { htonl(INADDR_LOOPBACK) } };
  if (bind(s, (void *)&a, sizeof a)) { perror("bind"); return 2; }
  pid_t pid = fork(); if (!pid) { server(s); _exit(0); }
  setvbuf(stdout, NULL, _IONBF, 0);
  printf("== networks through dns\n");
  show_net("getnetbyname foo.test", getnetbyname("foo.test"));
  show_net("getnetbyname bare", getnetbyname("bare"));
  show_net("getnetbyname nx.test", getnetbyname("nx.test"));
  show_net("getnetbyaddr 192.0.2.0", getnetbyaddr(0xC0000200, AF_INET));
  show_net("getnetbyaddr 10", getnetbyaddr(10, AF_INET));
  show_net("getnetbyaddr 172.16", getnetbyaddr(0xAC100000, AF_INET));
  printf("== group list merge\n");
  groups("u", 50); groups("u", 99); groups("u", 1); groups("v", 60); groups("v", 5); groups("nobody-here", 7);
  kill(pid, 9); waitpid(pid, NULL, 0);
  return 0;
}
