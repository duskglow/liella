/* Spec C0.1 (clauses s0_6_c0_1_0, s0_6_c0_1_1, s0_6_c0_1_2): a failing call returns its
 * sentinel (-1, NULL, EOF, (size_t)-1, MAP_FAILED, (iconv_t)-1) and stores a positive error code
 * in the calling thread's errno; errno is per thread; no successful call leaves errno 0 when it
 * was not 0 (a sweep of successful calls across the sections, glob with GLOB_ALTDIRFUNC
 * included); the entries that say otherwise do otherwise (the error-number convention of
 * C0.1's clause 4, fts_read's 0 at the end). Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <fts.h>
#include <glob.h>
#include <iconv.h>
#include <limits.h>
#include <locale.h>
#include <pthread.h>
#include <pwd.h>
#include <regex.h>
#include <search.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>

#define CHECK(cond) do { if (!(cond)) return __LINE__; } while (0)
/* A successful call must not leave errno 0 (it may change it). */
#define KEEPS(call) do { errno = 1234; (void)(call); if (errno == 0) return __LINE__; } while (0)

static void *alt_opendir(const char *p) { return opendir(p); }
static struct dirent *alt_readdir(void *d) { return readdir(d); }
static void alt_closedir(void *d) { closedir(d); }
static int alt_stat(const char *p, struct stat *st) { return stat(p, st); }
static int cmp(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

static int other_errno = -1;
static void *fail_elsewhere(void *arg)
{
    (void)arg;
    errno = 0;
    (void)close(-1);
    other_errno = errno;
    return NULL;
}

int main(void)
{
    /* The sentinels, each with a positive code. */
    errno = 0;
    CHECK(close(-1) == -1 && errno == EBADF);
    errno = 0;
    CHECK(fopen("/nonexistent/c01", "r") == NULL && errno == ENOENT);
    FILE *ro = fopen("/dev/null", "r");
    CHECK(ro != NULL);
    errno = 0;
    CHECK(fputc('x', ro) == EOF && errno == EBADF);
    fclose(ro);
    errno = 0;
    CHECK(iconv_open("no-such-charset", "UTF-8") == (iconv_t)-1 && errno == EINVAL);
    errno = 0;
    CHECK(mmap(NULL, 0, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) == MAP_FAILED && errno == EINVAL);
    CHECK(setlocale(LC_ALL, "C.UTF-8") != NULL);
    mbstate_t ps;
    memset(&ps, 0, sizeof ps);
    wchar_t wc;
    errno = 0;
    CHECK(mbrtowc(&wc, "\xff", 1, &ps) == (size_t)-1 && errno == EILSEQ);
    errno = 0;
    CHECK(strtol("99999999999999999999", NULL, 10) == LONG_MAX && errno == ERANGE);

    /* Per thread: another thread's failure leaves this thread's errno alone. */
    errno = 4321;
    pthread_t t;
    CHECK(pthread_create(&t, NULL, fail_elsewhere, NULL) == 0 && pthread_join(t, NULL) == 0);
    CHECK(other_errno == EBADF && errno == 4321);

    /* Entries that say otherwise: the error number returned, errno untouched (C0.1 clause 4). */
    errno = 4321;
    void *mem;
    CHECK(posix_memalign(&mem, 3, 8) == EINVAL && errno == 4321);
    CHECK(pthread_join(pthread_self(), NULL) == EDEADLK && errno == 4321);

    /* The sweep: successful calls never leave errno 0. */
    char dir[] = "/tmp/c01-XXXXXX";
    CHECK(mkdtemp(dir) != NULL);
    char path[64];
    snprintf(path, sizeof path, "%s/f", dir);
    KEEPS(getpid());
    KEEPS(strlen("abc"));
    KEEPS(free(malloc(10)));
    KEEPS(free(strdup("x")));
    char buf[256];
    KEEPS(snprintf(buf, sizeof buf, "%d %s %g", 1, "a", 2.5));
    KEEPS(strtol("12", NULL, 10));
    KEEPS(strtod("1.5", NULL));
    KEEPS(getenv("PATH"));
    KEEPS(setenv("C01", "1", 1));
    KEEPS(time(NULL));
    struct timespec ts;
    KEEPS(clock_gettime(CLOCK_MONOTONIC, &ts));
    time_t now = 0;
    struct tm tm;
    KEEPS(localtime_r(&now, &tm));
    KEEPS(strftime(buf, sizeof buf, "%c", &tm));
    int fd;
    KEEPS(fd = open(path, O_CREAT | O_RDWR, 0600));
    KEEPS(write(fd, "data", 4));
    KEEPS(lseek(fd, 0, SEEK_SET));
    KEEPS(read(fd, buf, 4));
    struct stat st;
    KEEPS(fstat(fd, &st));
    KEEPS(close(fd));
    FILE *f;
    KEEPS(f = fopen(path, "r"));
    KEEPS(fgets(buf, sizeof buf, f));
    KEEPS(fgetc(f));
    KEEPS(fclose(f));
    DIR *d;
    KEEPS(d = opendir(dir));
    while (d) {
        errno = 1234;
        struct dirent *e = readdir(d);
        CHECK(errno != 0);
        if (!e)
            break;
    }
    KEEPS(closedir(d));
    glob_t g;
    char pat[80];
    snprintf(pat, sizeof pat, "%s/*", dir);
    KEEPS(glob(pat, 0, NULL, &g));
    globfree(&g);
    memset(&g, 0, sizeof g);
    g.gl_opendir = alt_opendir;
    g.gl_readdir = alt_readdir;
    g.gl_closedir = alt_closedir;
    g.gl_stat = alt_stat;
    g.gl_lstat = alt_stat;
    KEEPS(glob(pat, GLOB_ALTDIRFUNC | GLOB_MARK, NULL, &g));
    CHECK(g.gl_pathc == 1);
    globfree(&g);
    regex_t re;
    KEEPS(regcomp(&re, "a+b", REG_EXTENDED));
    KEEPS(regexec(&re, "xaab", 0, NULL, 0));
    regfree(&re);
    int v[] = {3, 1, 2};
    KEEPS(qsort(v, 3, sizeof v[0], cmp));
    KEEPS(bsearch(&v[1], v, 3, sizeof v[0], cmp));
    KEEPS(setlocale(LC_ALL, NULL));
    KEEPS(getpwuid(getuid()));
    KEEPS(sysconf(_SC_PAGESIZE));
    KEEPS(getcwd(buf, sizeof buf));
    char rb[PATH_MAX];
    KEEPS(realpath(dir, rb));
    KEEPS(unlink(path));

    /* fts_read says otherwise (fts(3)): errno 0 at the end of the walk. */
    char *roots[] = {dir, NULL};
    FTS *fts = fts_open(roots, FTS_PHYSICAL, NULL);
    CHECK(fts != NULL);
    FTSENT *e;
    do {
        errno = 1234;
        e = fts_read(fts);
    } while (e);
    CHECK(errno == 0);
    fts_close(fts);
    KEEPS(rmdir(dir));
    return 0;
}
