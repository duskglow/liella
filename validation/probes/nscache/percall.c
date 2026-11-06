/* nscache per-call probe: per getpwnam_r call, kernel and user instructions, minor faults, ns.
   no spec clause (measurement probe, Liella only). usage: percall N name */
#define _GNU_SOURCE
#include <pwd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/ioctl.h>
#include <linux/perf_event.h>

static int ctr(int excl_user, int excl_kernel) {
    struct perf_event_attr a;
    memset(&a, 0, sizeof a);
    a.type = PERF_TYPE_HARDWARE;
    a.size = sizeof a;
    a.config = PERF_COUNT_HW_INSTRUCTIONS;
    a.exclude_user = excl_user;
    a.exclude_kernel = excl_kernel;
    a.exclude_hv = 1;
    return (int)syscall(SYS_perf_event_open, &a, 0, -1, -1, 0);
}

static long long rd(int fd) {
    long long v = 0;
    if (read(fd, &v, sizeof v) != (ssize_t)sizeof v) return -1;
    return v;
}

static long flt(void) {
    struct rusage r;
    getrusage(RUSAGE_SELF, &r);
    return r.ru_minflt;
}

static long long ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 3;
    const char *name = argc > 2 ? argv[2] : "root";
    int ku = ctr(1, 0), uu = ctr(0, 1);
    static char buf[4096];
    for (int i = 0; i < n; i++) {
        if (argc > 3 && i == 2) setuid(getuid());
        struct passwd pw, *res = NULL;
        long f0 = flt();
        long long k0 = rd(ku), u0 = rd(uu), t0 = ns();
        syscall(SYS_getppid); /* marker */
        int rc = getpwnam_r(name, &pw, buf, sizeof buf, &res);
        syscall(SYS_getppid); /* marker */
        long long t1 = ns(), k1 = rd(ku), u1 = rd(uu);
        long f1 = flt();
        printf("call %d rc %d found %d kinstr %lld uinstr %lld minflt %ld ns %lld\n", i + 1, rc, res != NULL, k1 - k0, u1 - u0, f1 - f0, t1 - t0);
    }
    return 0;
}
