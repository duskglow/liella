/* nscache attach steps: the kernel cost of each system call the attach makes, measured alone.
   no spec clause (kernel cost probe; raw system calls only). usage: steps <region path> */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/resource.h>
#include <linux/perf_event.h>

static int ku;
static long long rd(void) { long long v = 0; if (read(ku, &v, sizeof v) != 8) return -1; return v; }
static long flt(void) { struct rusage r; getrusage(RUSAGE_SELF, &r); return r.ru_minflt; }
static long long base;
#define STEP(label, expr) do { long f0 = flt(); long long a = rd(); expr; long long b = rd(); long f1 = flt(); \
    printf("%-28s kinstr %7lld faults %ld\n", label, b - a - base, f1 - f0); } while (0)

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "/dev/shm/liella-nscache-1000";
    struct perf_event_attr at;
    memset(&at, 0, sizeof at);
    at.type = PERF_TYPE_HARDWARE; at.size = sizeof at; at.config = PERF_COUNT_HW_INSTRUCTIONS;
    at.exclude_user = 1; at.exclude_hv = 1;
    ku = (int)syscall(SYS_perf_event_open, &at, 0, -1, -1, 0);
    long long a = rd(), b = rd(); a = rd(); b = rd(); base = b - a; /* cost of one read of the counter */
    struct stat st; char text[64]; volatile unsigned long sink = 0;
    int fd = -1, bfd = -1; unsigned long *m = MAP_FAILED; size_t len = 8u << 20;
    for (int rep = 0; rep < 2; rep++) {
        printf("-- round %d\n", rep + 1);
        STEP("getppid (null syscall)", syscall(SYS_getppid));
        STEP("openat region", fd = open(path, O_RDWR | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY | O_CLOEXEC));
        STEP("fstat region", fstat(fd, &st));
        STEP("fcntl F_DUPFD_CLOEXEC", { int d = fcntl(fd, F_DUPFD_CLOEXEC, 3); close(d); });
        STEP("mmap 8MiB shared", m = mmap(0, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0));
        STEP("open+read+close boot_id", { bfd = open("/proc/sys/kernel/random/boot_id", O_RDONLY | O_CLOEXEC); sink += read(bfd, text, sizeof text); close(bfd); });
        STEP("stat /etc/passwd", stat("/etc/passwd", &st));
        STEP("stat /etc/nsswitch.conf", stat("/etc/nsswitch.conf", &st));
        STEP("stat /proc/self/ns/user", stat("/proc/self/ns/user", &st));
        STEP("stat /etc/host.conf", stat("/etc/host.conf", &st));
        STEP("stat /etc/resolv.conf", stat("/etc/resolv.conf", &st));
        STEP("stat /etc/gai.conf", stat("/etc/gai.conf", &st));
        STEP("4 id calls", { sink += getuid() + geteuid() + getgid() + getegid(); });
        if (m != MAP_FAILED) {
            STEP("read header page", sink += m[7]);
            STEP("read slot page (+64K)", sink += m[80 + 9000]);
            STEP("read arena page (+1M)", sink += m[131072]);
            STEP("munmap 8MiB", munmap(m, len));
        }
        STEP("close region", close(fd));
    }
    return (int)(sink & 0);
}
