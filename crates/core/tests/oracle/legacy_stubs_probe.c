/* LEGACY-STUBS black-box probe (glibc 2.39, x86_64): the removed-syscall and STREAMS compat
 * names, each bound by its compat version. Mode "plain" calls them; mode "seccomp" first installs
 * a filter that makes the old syscall numbers fail with EXDEV, which shows whether a name issues
 * its syscall (EXDEV) or fails in the library (ENOSYS). Build: cc -O1 legacy_probe.c -o legacy_probe */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

__asm__(".symver isastream,isastream@GLIBC_2.2.5");
__asm__(".symver fattach,fattach@GLIBC_2.2.5");
__asm__(".symver fdetach,fdetach@GLIBC_2.2.5");
__asm__(".symver getmsg,getmsg@GLIBC_2.2.5");
__asm__(".symver putmsg,putmsg@GLIBC_2.2.5");
__asm__(".symver getpmsg,getpmsg@GLIBC_2.2.5");
__asm__(".symver putpmsg,putpmsg@GLIBC_2.2.5");
__asm__(".symver bdflush,bdflush@GLIBC_2.2.5");
__asm__(".symver create_module,create_module@GLIBC_2.2.5");
__asm__(".symver get_kernel_syms,get_kernel_syms@GLIBC_2.2.5");
__asm__(".symver nfsservctl,nfsservctl@GLIBC_2.2.5");
__asm__(".symver query_module,query_module@GLIBC_2.2.5");
__asm__(".symver sstk,sstk@GLIBC_2.2.5");
__asm__(".symver uselib,uselib@GLIBC_2.2.5");
__asm__(".symver sysctl,sysctl@GLIBC_2.2.5");
__asm__(".symver __sysctl,__sysctl@GLIBC_2.2.5");

extern int isastream(int);
extern int fattach(int, const char *);
extern int fdetach(const char *);
extern int getmsg(int, void *, void *, int *);
extern int putmsg(int, const void *, const void *, int);
extern int getpmsg(int, void *, void *, int *, int *);
extern int putpmsg(int, const void *, const void *, int, int);
extern int bdflush(int, long);
extern void *create_module(const char *, size_t);
extern int get_kernel_syms(void *);
extern long nfsservctl(int, void *, void *);
extern int query_module(const char *, int, void *, size_t, size_t *);
extern void *sstk(int);
extern int uselib(const char *);
extern int sysctl(int *, int, void *, size_t *, void *, size_t);
extern int __sysctl(int *, int, void *, size_t *, void *, size_t);

static const char *en(int e)
{
    static char b[32];
    switch (e) {
    case 0: return "0";
    case 12345: return "unchanged";
    case ENOSYS: return "ENOSYS";
    case EXDEV: return "EXDEV(syscall)";
    case EBADF: return "EBADF";
    case EFAULT: return "EFAULT";
    case ENOENT: return "ENOENT";
    case EINVAL: return "EINVAL";
    case EPERM: return "EPERM";
    case ENOTTY: return "ENOTTY";
    default: snprintf(b, sizeof b, "errno %d", e); return b;
    }
}

#define I(name, call) do { errno = 12345; long r = (long)(call); int e = errno; \
    printf("%-16s %-34s ret=%ld errno=%s\n", name, #call, r, en(e)); } while (0)
#define P(name, call) do { errno = 12345; void *r = (call); int e = errno; \
    printf("%-16s %-34s ret=%#lx errno=%s\n", name, #call, (unsigned long)r, en(e)); } while (0)

static int filter(void)
{
    static const int nrs[] = {134, 156, 174, 177, 178, 180, 181, 182, 183, 184, 185};
    struct sock_filter f[64];
    int n = 0;
    f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch));
    f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0);
    f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    f[n++] = (struct sock_filter)BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr));
    for (unsigned i = 0; i < sizeof nrs / sizeof nrs[0]; i++)
        f[n++] = (struct sock_filter)BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, (unsigned)nrs[i], 0, 1),
        f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EXDEV);
    f[n++] = (struct sock_filter)BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    struct sock_fprog p = {.len = (unsigned short)n, .filter = f};
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &p)) {
        perror("seccomp");
        return -1;
    }
    return 0;
}

int main(int argc, char **argv)
{
    int sec = argc > 1 && !strcmp(argv[1], "seccomp");
    if (sec && filter())
        return 2;
    printf("mode %s\n", sec ? "seccomp" : "plain");
    int fd = open("/dev/null", O_RDWR);
    int closed = dup(fd);
    close(closed);
    int flags = 0, band = 0;
    char buf[16] = {0};
    size_t len = sizeof buf, ret = 0;
    int name[2] = {1, 1};
    I("isastream", isastream(fd));
    I("isastream", isastream(0));
    I("isastream", isastream(closed));
    I("isastream", isastream(-1));
    I("fattach", fattach(fd, "/nonexistent-liella"));
    I("fattach", fattach(closed, "/nonexistent-liella"));
    I("fattach", fattach(fd, NULL));
    I("fdetach", fdetach("/nonexistent-liella"));
    I("fdetach", fdetach(NULL));
    I("getmsg", getmsg(fd, NULL, NULL, &flags));
    I("getmsg", getmsg(closed, NULL, NULL, NULL));
    I("putmsg", putmsg(fd, NULL, NULL, 0));
    I("putmsg", putmsg(closed, NULL, NULL, 0));
    I("getpmsg", getpmsg(fd, NULL, NULL, &band, &flags));
    I("getpmsg", getpmsg(closed, NULL, NULL, NULL, NULL));
    I("putpmsg", putpmsg(fd, NULL, NULL, 0, 0));
    I("putpmsg", putpmsg(closed, NULL, NULL, 0, 0));
    I("bdflush", bdflush(0, 0));
    I("bdflush", bdflush(1, (long)buf));
    P("create_module", create_module("liella", 0));
    P("create_module", create_module(NULL, 0));
    I("get_kernel_syms", get_kernel_syms(NULL));
    I("get_kernel_syms", get_kernel_syms(buf));
    I("nfsservctl", nfsservctl(0, NULL, NULL));
    I("query_module", query_module(NULL, 0, NULL, 0, NULL));
    I("query_module", query_module("liella", 1, buf, len, &ret));
    P("sstk", sstk(0));
    P("sstk", sstk(4096));
    I("uselib", uselib("/nonexistent-liella"));
    I("uselib", uselib(NULL));
    I("sysctl", sysctl(name, 2, buf, &len, NULL, 0));
    I("sysctl", sysctl(NULL, 0, NULL, NULL, NULL, 0));
    I("__sysctl", __sysctl(name, 2, buf, &len, NULL, 0));
    I("__sysctl", __sysctl(NULL, 0, NULL, NULL, NULL, 0));
    printf("len after sysctl %zu\n", len);
    return 0;
}
