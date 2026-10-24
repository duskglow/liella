/* Spec C0.4 (clause s0_6_c0_4_0): on x86_64 off_t, ino_t, blkcnt_t, rlim_t and time_t are 64
 * bits, and each *64 symbol is an exact alias of its unsuffixed one: the same answers on a
 * sparse file of 5 GiB (offsets beyond 32 bits), the same records and limits. Each failing CHECK
 * exits with its line number. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)

_Static_assert(sizeof(off_t) == 8 && sizeof(ino_t) == 8 && sizeof(blkcnt_t) == 8, "64-bit file types");
_Static_assert(sizeof(rlim_t) == 8 && sizeof(time_t) == 8, "64-bit limits and times");
_Static_assert(sizeof(off64_t) == sizeof(off_t) && sizeof(struct stat64) == sizeof(struct stat), "the same types");

int main(void)
{
    const off_t big = 5LL << 30, at = (9LL << 29) + 3;
    char dir[] = "/tmp/c04-XXXXXX";
    CHECK(mkdtemp(dir) != NULL);
    char path[64];
    snprintf(path, sizeof path, "%s/sparse", dir);
    int fd = open(path, O_CREAT | O_RDWR, 0600);
    int fd64 = open64(path, O_RDWR);
    CHECK(fd >= 0 && fd64 >= 0);
    CHECK(ftruncate64(fd64, big) == 0);
    struct stat st;
    struct stat64 st64;
    CHECK(stat(path, &st) == 0 && stat64(path, &st64) == 0);
    CHECK(st.st_size == big && st64.st_size == big && st.st_ino == st64.st_ino);
    CHECK(fstat(fd, &st) == 0 && fstat64(fd64, &st64) == 0 && st.st_size == st64.st_size);
    CHECK(lstat(path, &st) == 0 && lstat64(path, &st64) == 0 && st.st_blocks == st64.st_blocks);
    CHECK(lseek(fd, at, SEEK_SET) == at && lseek64(fd64, at, SEEK_SET) == at);
    CHECK(pwrite64(fd64, "far", 3, at) == 3);
    char b[4] = {0};
    CHECK(pread(fd, b, 3, at) == 3 && memcmp(b, "far", 3) == 0);
    CHECK(ftruncate(fd, big + 1) == 0 && fstat64(fd64, &st64) == 0 && st64.st_size == big + 1);
    FILE *f = fopen64(path, "r");
    CHECK(f != NULL && fseeko64(f, at, SEEK_SET) == 0 && ftello(f) == at && ftello64(f) == at);
    CHECK(fgetc(f) == 'f');
    fclose(f);
    struct statvfs v;
    struct statvfs64 v64;
    CHECK(statvfs(dir, &v) == 0 && statvfs64(dir, &v64) == 0 && v.f_bsize == v64.f_bsize && v.f_fsid == v64.f_fsid);
    struct rlimit r;
    struct rlimit64 r64;
    CHECK(getrlimit(RLIMIT_NOFILE, &r) == 0 && getrlimit64(RLIMIT_NOFILE, &r64) == 0);
    CHECK(r.rlim_cur == r64.rlim_cur && r.rlim_max == r64.rlim_max);
    CHECK(setrlimit64(RLIMIT_NOFILE, &r64) == 0);
    DIR *d = opendir(dir), *d64 = opendir(dir);
    CHECK(d && d64);
    for (;;) {
        struct dirent *e = readdir(d);
        struct dirent64 *e64 = readdir64(d64);
        CHECK((e == NULL) == (e64 == NULL));
        if (!e)
            break;
        CHECK(e->d_ino == e64->d_ino && strcmp(e->d_name, e64->d_name) == 0);
    }
    closedir(d);
    closedir(d64);
    /* time_t past 2038. */
    time_t later = (time_t)1 << 33;
    struct tm tm;
    CHECK(gmtime_r(&later, &tm) && tm.tm_year + 1900 == 2242 && timegm(&tm) == later);
    close(fd);
    close(fd64);
    CHECK(unlink(path) == 0 && rmdir(dir) == 0);
    return 0;
}
