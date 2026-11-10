/* The vDSO lookup's system calls (HEAP-FIRST-USE-3, #4170): the read system calls of the first
 * clock_gettime, which resolves the vDSO's functions, taken from this process's own syscr count
 * in /proc/self/io. The lookup compares symbol names; it must not take the copy/compare entries'
 * NULL-folding decision (a read of vm.mmap_min_addr: one more read). Prints "reads <n>" on stderr
 * (n = reads between the two samples, the first sample's own pread excluded), or "skip <why>". */
#include <fcntl.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>

/* The value after "syscr:" in the text of /proc/self/io, parsed by hand (no library compare or
 * copy, which could take the decision this program looks for). */
static long syscr(int fd) {
    char b[512];
    ssize_t n = pread(fd, b, sizeof b - 1, 0);
    if (n <= 0)
        return -1;
    b[n] = 0;
    for (char *p = b; *p; p++) {
        if (p[0] == 's' && p[1] == 'y' && p[2] == 's' && p[3] == 'c' && p[4] == 'r' && p[5] == ':') {
            long v = 0;
            p += 6;
            while (*p == ' ')
                p++;
            if (*p < '0' || *p > '9')
                return -1;
            while (*p >= '0' && *p <= '9')
                v = v * 10 + (*p++ - '0');
            return v;
        }
    }
    return -1;
}

int main(void) {
    int fd = open("/proc/self/io", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        fprintf(stderr, "skip /proc/self/io cannot be opened (no task I/O accounting)\n");
        return 0;
    }
    long a = syscr(fd);
    struct timespec t;
    int rc = clock_gettime(CLOCK_MONOTONIC, &t);
    long b = syscr(fd);
    if (a < 0 || b < 0) {
        fprintf(stderr, "skip /proc/self/io has no syscr line\n");
        return 0;
    }
    if (rc != 0)
        return 1;
    fprintf(stderr, "reads %ld\n", b - a - 1);
    return 0;
}
