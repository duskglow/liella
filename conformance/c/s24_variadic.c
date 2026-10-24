/* Spec §24.1, §24.6: ulimit and syscall through real C varargs. ulimit's new limit arrives when
 * the command takes one; syscall passes its arguments to the kernel in order. Each failing
 * CHECK exits with its line number. */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <ulimit.h>
#include <unistd.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

int main(void)
{
    long blocks = ulimit(UL_GETFSIZE);
    CHECK("s24_1_c", blocks > 0);
    CHECK("s24_1_c", ulimit(UL_SETFSIZE, 1024L) == 1024 && ulimit(UL_GETFSIZE) == 1024);
    errno = 0;
    CHECK("s24_1_c", ulimit(99) == -1 && errno == EINVAL);
    CHECK("d24_5", syscall(SYS_getpid) == getpid());
    char buf[8] = { 0 };
    int p[2];
    CHECK("d24_5", syscall(SYS_pipe2, p, 0) == 0);
    CHECK("d24_5", syscall(SYS_write, p[1], "args", 4) == 4 && syscall(SYS_read, p[0], buf, 8) == 4);
    CHECK("d24_5", buf[0] == 'a' && buf[3] == 's');
    errno = 0;
    CHECK("d24_5", syscall(SYS_close, -1) == -1 && errno == EBADF);
    /* All six arguments, in order (s24_6_c_5): mmap of a memfd's second page, whose first byte
     * was written through pwrite64 (four arguments, the last the offset). */
    int m = (int)syscall(SYS_memfd_create, "c65", 0);
    CHECK("s24_6_c_5", m >= 0);
    CHECK("s24_6_c_5", syscall(SYS_ftruncate, m, 8192L) == 0);
    CHECK("s24_6_c_5", syscall(SYS_pwrite64, m, "6", 1, 4096L) == 1);
    long a = syscall(SYS_mmap, (void *)0, 4096L, PROT_READ, MAP_SHARED, m, 4096L);
    CHECK("s24_6_c_5", a != -1 && *(const char *)a == '6');
    CHECK("s24_6_c_5", syscall(SYS_munmap, a, 4096L) == 0 && syscall(SYS_close, m) == 0);
    return 0;
}
