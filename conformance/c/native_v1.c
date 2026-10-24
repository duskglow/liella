/* The native C ABI, node LIELLA_NATIVE_1 (implementation-plan.md D-X.10), as compiled C code
 * sees it through the generated header <liella/native.h>: the header compiles (its layout
 * assertions hold for this compiler), every function links from libc.a, and each one works in
 * a process Liella started, sharing its state with the POSIX functions (one heap, one
 * environment, one set of exit handlers). Each failing CHECK exits with its line number; the
 * end is liella_process_exit, whose status, exit handler and stream flush the runner checks. */
/* RUN-ARGS: first second-argument */
/* RUN-ENV: NATIVE_A=1 NATIVE_B=two TZ=CET-1CEST,M3.5.0,M10.5.0/3 */
/* EXPECT-EXIT: 42 */
/* EXPECT-STDERR: native_v1: exit handler ran */
/* EXPECT-STDERR: native_v1: buffered output flushed at exit */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <liella/native.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)
#define S(text) (const uint8_t *)(text), strlen(text)

static void handler(void)
{
    static const char msg[] = "native_v1: exit handler ran\n";
    ssize_t n = write(2, msg, sizeof msg - 1);
    (void)n;
}

/* The buffer the native functions copy text into (never NUL-terminated by them). */
static char text[4096];

static int copy_matches(int64_t n, const char *want)
{
    return n >= 0 && (size_t)n == strlen(want) && memcmp(text, want, (size_t)n) == 0;
}

static int heap(void)
{
    uint8_t *p = liella_heap_alloc(100, 64);
    CHECK("heap", p != NULL && (uintptr_t)p % 64 == 0 && liella_heap_usable_size(p) >= 100);
    memset(p, 0x5a, 100);
    p = liella_heap_realloc(p, 5000);
    CHECK("heap", p != NULL && p[0] == 0x5a && p[99] == 0x5a);
    liella_heap_free(p);
    uint8_t *z = liella_heap_alloc_zeroed(4096, 4096);
    CHECK("heap", z != NULL && (uintptr_t)z % 4096 == 0);
    for (int i = 0; i < 4096; i++)
        CHECK("heap", z[i] == 0);
    free(z); /* one heap: interchangeable with free */
    void *m = malloc(10);
    CHECK("heap", liella_heap_usable_size(m) >= 10);
    liella_heap_free(m);
    CHECK("heap", liella_heap_alloc(8, 3) == NULL);
    CHECK("heap", liella_heap_usable_size(NULL) == 0);
    liella_heap_free(NULL);
    return 0;
}

/* Secure memory (D-X.11): zeroing that is never optimised away, and secure heap blocks, which
 * read as zero, stay secure when reallocated and are interchangeable with free. That the heap
 * zeroes them on release is observed inside the library (crates/boundary/tests/native_v1.rs). */
static int secure(void)
{
    uint8_t key[32];
    memset(key, 0xa5, sizeof key);
    CHECK("secure", liella_secure_zero(key, sizeof key) == 0);
    for (size_t i = 0; i < sizeof key; i++)
        CHECK("secure", key[i] == 0);
    CHECK("secure", liella_secure_zero(NULL, 0) == 0 && liella_secure_zero(NULL, 1) == -EFAULT);
    memset(key, 0xa5, sizeof key);
    explicit_bzero(key, sizeof key); /* the same function */
    CHECK("secure", key[0] == 0 && key[31] == 0);

    uint8_t *s = liella_heap_alloc_secure(100, 64);
    CHECK("secure", s != NULL && (uintptr_t)s % 64 == 0 && liella_heap_usable_size(s) >= 100);
    for (int i = 0; i < 100; i++)
        CHECK("secure", s[i] == 0);
    memset(s, 0x5a, 100);
    s = liella_heap_realloc(s, 10000); /* moved: the old block zeroed, the new one secure */
    CHECK("secure", s != NULL && s[0] == 0x5a && s[99] == 0x5a && s[100] == 0 && s[9999] == 0);
    s = realloc(s, 20000);
    CHECK("secure", s != NULL && s[99] == 0x5a);
    free(s);
    CHECK("secure", liella_heap_alloc_secure(8, 3) == NULL);
    return 0;
}

static int env(void)
{
    CHECK("env", copy_matches(liella_env_get(S("NATIVE_B"), (uint8_t *)text, sizeof text), "two"));
    CHECK("env", liella_env_get(S("NATIVE_B"), (uint8_t *)text, 2) == 3);
    CHECK("env", liella_env_get(S("NATIVE_C"), (uint8_t *)text, sizeof text) == -ENOENT);
    CHECK("env", liella_env_count() == 3);
    CHECK("env", copy_matches(liella_env_entry(0, (uint8_t *)text, sizeof text), "NATIVE_A=1"));
    CHECK("env", liella_env_entry(3, (uint8_t *)text, sizeof text) == -ENOENT);
    CHECK("env", liella_env_set(S("NATIVE_C"), S("three"), 0) == 0);
    CHECK("env", strcmp(getenv("NATIVE_C"), "three") == 0); /* the same environment */
    CHECK("env", setenv("NATIVE_D", "four", 1) == 0);
    CHECK("env", copy_matches(liella_env_get(S("NATIVE_D"), (uint8_t *)text, sizeof text), "four"));
    CHECK("env", liella_env_count() == 5);
    CHECK("env", liella_env_unset(S("NATIVE_C")) == 0 && getenv("NATIVE_C") == NULL);
    CHECK("env", liella_env_set(S("A=B"), S("x"), 1) == -EINVAL);
    CHECK("env", liella_env_set((const uint8_t *)"A\0B", 3, S("x"), 1) == -EINVAL);
    CHECK("env", liella_env_get(NULL, 1, NULL, 0) == -EFAULT);
    errno = 0;
    CHECK("env", liella_env_unset(S("")) == -EINVAL && errno == 0); /* errno untouched */
    return 0;
}

static int files_in(const char *dir, const char *file, const char *moved);

/* The file checks in a scratch directory, which is removed afterwards whatever the outcome. */
static int files(void)
{
    char dir[64], file[80], moved[80];
    snprintf(dir, sizeof dir, "/tmp/liella-native-c-%lld", (long long)liella_process_id());
    snprintf(file, sizeof file, "%s/f", dir);
    snprintf(moved, sizeof moved, "%s/g", dir);
    int line = files_in(dir, file, moved);
    unlink(file);
    unlink(moved);
    rmdir(dir);
    return line;
}

static int files_in(const char *dir, const char *file, const char *moved)
{
    CHECK("fs", liella_fs_mkdir(S(dir), 0700) == 0);
    CHECK("fs", liella_fs_mkdir(S(dir), 0700) == -EEXIST);
    int64_t fd = liella_fs_open(S(file), LIELLA_O_RDWR | LIELLA_O_CREAT | LIELLA_O_EXCL | LIELLA_O_CLOEXEC, 0600);
    CHECK("fs", fd >= 0);
    CHECK("fd", liella_fd_write((int32_t)fd, S("native bytes")) == 12);
    CHECK("fd", liella_fd_pwrite((int32_t)fd, S("B"), 7) == 1);
    CHECK("fd", liella_fd_pread((int32_t)fd, (uint8_t *)text, 12, 0) == 12 && memcmp(text, "native Bytes", 12) == 0);
    CHECK("fd", liella_fd_seek((int32_t)fd, 0, LIELLA_SEEK_CUR) == 12);
    CHECK("fd", liella_fd_seek((int32_t)fd, 7, LIELLA_SEEK_SET) == 7);
    CHECK("fd", liella_fd_read((int32_t)fd, (uint8_t *)text, sizeof text) == 5 && memcmp(text, "Bytes", 5) == 0);
    CHECK("fd", liella_fd_read((int32_t)fd, (uint8_t *)text, sizeof text) == 0);
    struct liella_stat st, by_path;
    CHECK("fd", liella_fd_stat((int32_t)fd, &st) == 0 && st.size == 12);
    CHECK("fd", (st.mode & LIELLA_S_IFMT) == LIELLA_S_IFREG && st.reserved == 0);
    CHECK("fd", liella_fd_stat((int32_t)fd, NULL) == -EFAULT);
    CHECK("fd", liella_fd_close((int32_t)fd) == 0 && liella_fd_close((int32_t)fd) == -EBADF);
    CHECK("fs", liella_fs_stat(S(file), 0, &by_path) == 0 && by_path.ino == st.ino);
    CHECK("fs", liella_fs_stat(S(dir), LIELLA_FS_NOFOLLOW, &by_path) == 0 && (by_path.mode & LIELLA_S_IFMT) == LIELLA_S_IFDIR);
    CHECK("fs", liella_fs_rename(S(file), S(moved)) == 0);
    CHECK("fs", liella_fs_stat(S(file), 0, &by_path) == -ENOENT);
    CHECK("fs", liella_fs_unlink(S(moved)) == 0);
    CHECK("fs", liella_fs_rmdir(S(dir)) == 0);
    /* A path is its first len bytes, without a terminator. */
    CHECK("fs", liella_fs_stat((const uint8_t *)"/tmpXYZ", 4, 0, &by_path) == 0);
    CHECK("fs", liella_fs_stat((const uint8_t *)"/t\0mp", 5, 0, &by_path) == -EINVAL);
    return 0;
}

static int clocks_and_zone(void)
{
    struct liella_timespec a, b;
    CHECK("clock", liella_clock_realtime(&a) == 0 && a.sec > 1700000000 && a.nsec >= 0 && a.nsec < 1000000000);
    CHECK("clock", liella_clock_monotonic(&a) == 0 && liella_clock_monotonic(&b) == 0);
    CHECK("clock", b.sec > a.sec || (b.sec == a.sec && b.nsec >= a.nsec));
    CHECK("clock", liella_clock_realtime(NULL) == -EFAULT);

    struct liella_tm tm;
    CHECK("tz", liella_tz_to_local(0, &tm) == 0);
    CHECK("tz", tm.year == 1970 && tm.month == 1 && tm.day == 1 && tm.hour == 1 && tm.weekday == 4);
    CHECK("tz", tm.isdst == 0 && tm.utc_offset == 3600 && tm.abbr_len == 3 && memcmp(tm.abbr, "CET", 3) == 0);
    CHECK("tz", liella_tz_to_local(1782907200, &tm) == 0 && tm.hour == 14 && tm.isdst == 1);
    CHECK("tz", tm.abbr_len == 4 && memcmp(tm.abbr, "CEST", 4) == 0);
    int64_t t = 0;
    struct liella_tm in = { .year = 2025, .month = 13, .day = 1, .isdst = -1 };
    CHECK("tz", liella_tz_from_local(&in, &t) == 0 && t == 1767222000 && in.year == 2026 && in.month == 1);
    CHECK("tz", liella_tz_from_local(&in, NULL) == -EFAULT);
    /* The same zone as localtime. */
    time_t now = (time_t)a.sec;
    struct tm *c = localtime(&now);
    CHECK("tz", liella_tz_to_local(a.sec, &tm) == 0 && tm.year == c->tm_year + 1900 && tm.hour == c->tm_hour);
    return 0;
}

static int locale_and_process(int argc, char **argv)
{
    CHECK("locale", copy_matches(liella_locale_name(LIELLA_LC_ALL, (uint8_t *)text, sizeof text), "C"));
    CHECK("locale", setlocale(LC_ALL, "C.UTF-8") != NULL);
    CHECK("locale", copy_matches(liella_locale_name(LIELLA_LC_CTYPE, (uint8_t *)text, sizeof text), "C.UTF-8"));
    CHECK("locale", copy_matches(liella_locale_name(LIELLA_LC_ALL, (uint8_t *)text, sizeof text), "C.UTF-8"));
    CHECK("locale", setlocale(LC_TIME, "C") != NULL);
    int64_t n = liella_locale_name(LIELLA_LC_ALL, (uint8_t *)text, sizeof text);
    CHECK("locale", n > 0 && (size_t)n < sizeof text && memmem(text, (size_t)n, "LC_TIME=C;", 10) != NULL);
    CHECK("locale", liella_locale_name(99, (uint8_t *)text, sizeof text) == -EINVAL);
    CHECK("locale", copy_matches(liella_locale_decimal_point((uint8_t *)text, sizeof text), "."));

    CHECK("process", liella_process_id() == getpid());
    CHECK("process", liella_process_arg_count() == argc && argc == 3);
    for (int i = 0; i < argc; i++)
        CHECK("process", copy_matches(liella_process_arg((size_t)i, (uint8_t *)text, sizeof text), argv[i]));
    CHECK("process", liella_process_arg((size_t)argc, (uint8_t *)text, sizeof text) == -ENOENT);
    return 0;
}

int main(int argc, char **argv)
{
    int line;
    if ((line = heap()) || (line = secure()) || (line = env()) || (line = files()) || (line = clocks_and_zone()) || (line = locale_and_process(argc, argv)))
        return line;
    /* The end: buffered output and an exit handler, both run by liella_process_exit. */
    static char buf[256];
    CHECK("exit", setvbuf(stderr, buf, _IOFBF, sizeof buf) == 0);
    fputs("native_v1: buffered output flushed at exit\n", stderr);
    CHECK("exit", atexit(handler) == 0);
    liella_process_exit(42);
}
