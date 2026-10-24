/* Spec §11.1, §11.5, §11.6, §11.10: the variadic entry points (open, openat, fcntl, ioctl,
 * mremap and their aliases) through real C varargs: the optional argument arrives when given
 * and is ignored when not. Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

/* A failing check removes the file before it exits, so a failure leaves nothing behind. */
#define CHECK(clause, cond) do { if (!(cond)) { unlink(path); return __LINE__; } } while (0)

extern int __open(const char *, int, ...);
extern int __open64(const char *, int, ...);
extern int __fcntl(int, int, ...);

static const char path[] = "/tmp/liella-s11-variadic";

int main(void)
{
    struct stat st;
    unlink(path);
    umask(022);
    int fd = open(path, O_CREAT | O_EXCL | O_RDWR, 0640);
    CHECK("s11_1_c_0", fd >= 0);
    CHECK("s11_1_g_3", fstat(fd, &st) == 0 && (st.st_mode & 0777) == 0640);
    CHECK("s11_1_g_2", open(path, O_CREAT | O_EXCL | O_RDWR, 0600) == -1 && errno == EEXIST);
    /* The lowest free number: closed and reopened, the same one (descriptors inherited from
     * the test harness may occupy any other). */
    int probe = open(path, O_RDONLY);
    close(probe);
    int ro = open(path, O_RDONLY);
    CHECK("s11_1_g_1", ro == probe);
    close(ro);
    CHECK("s11_1_c_0", (ro = open64(path, O_RDONLY)) >= 0 && close(ro) == 0);
    CHECK("s11_1_c_0", (ro = __open(path, O_RDONLY)) >= 0 && close(ro) == 0);
    CHECK("s11_1_c_0", (ro = __open64(path, O_RDONLY)) >= 0 && close(ro) == 0);
    int dir = open("/tmp", O_RDONLY | O_DIRECTORY);
    CHECK("s11_1_c_0", dir >= 0);
    CHECK("s11_1_c_0", (ro = openat(dir, "liella-s11-variadic", O_RDONLY)) >= 0 && close(ro) == 0);
    CHECK("s11_1_c_0", (ro = openat64(AT_FDCWD, path, O_RDONLY)) >= 0 && close(ro) == 0);

    /* fcntl: an int argument, a pointer argument, and none. */
    CHECK("s11_5_c_4_2", fcntl(fd, F_SETFD, FD_CLOEXEC) == 0 && fcntl(fd, F_GETFD) == FD_CLOEXEC);
    CHECK("s11_5_c_4_1", fcntl(fd, F_DUPFD, 100) == 100 && close(100) == 0);
    struct flock fl = { .l_type = F_WRLCK, .l_whence = SEEK_SET, .l_start = 0, .l_len = 10 };
    CHECK("s11_5_c_4_4", fcntl64(fd, F_SETLK, &fl) == 0);
    fl.l_type = F_WRLCK;
    CHECK("s11_5_c_4_4", __fcntl(fd, F_GETLK, &fl) == 0 && fl.l_type == F_UNLCK);
    CHECK("s11_5_c_4_3", (fcntl(fd, F_GETFL) & O_ACCMODE) == O_RDWR);
    errno = 0;
    CHECK("s11_5_c_4", fcntl(fd, 0x7fff) == -1 && errno == EINVAL);

    /* ioctl: an untyped request (FIONREAD on a pipe). */
    int p[2];
    int avail = -1;
    CHECK("s11_6_c_0", pipe(p) == 0 && write(p[1], "abc", 3) == 3);
    CHECK("s11_6_c_0", ioctl(p[0], FIONREAD, &avail) == 0 && avail == 3);

    /* mremap: grow in place or move, with and without the optional address. */
    char *m = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    CHECK("s11_10_c_1", m != MAP_FAILED);
    m[0] = 'x';
    char *g = mremap(m, 4096, 8192, MREMAP_MAYMOVE);
    CHECK("s11_10_c_7", g != MAP_FAILED && g[0] == 'x');
    char *target = mmap(NULL, 8192, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    char *f = mremap(g, 8192, 8192, MREMAP_MAYMOVE | MREMAP_FIXED, target);
    CHECK("s11_10_c_7", f == target && f[0] == 'x');
    close(fd);
    unlink(path);
    return 0;
}
