/* wl_percall NO-ROW set: high-call entry points of the 30 real workloads that had no per-call row (WL-ENTRIES
 * targets.md: real call counts). Typical-call counts are the largest single-workload counts seen there.
 * const-attributed getters (__errno_location, __ctype_b_loc) go through a volatile function pointer so gcc cannot
 * hoist them out of the loop; the indirect call costs the same on both sides. */
#include "percall.h"
#include <ctype.h>
#include <errno.h>
#include <inttypes.h>
#include <setjmp.h>
#include <sys/stat.h>

extern int *__errno_location(void);
extern const unsigned short **__ctype_b_loc(void);
extern size_t __ctype_get_mb_cur_max(void);

CASE(errno_location, 37885964, 1) {
    int *(*volatile f)(void) = __errno_location;
    for (long i = 0; i < n; i++) pc_sink += *f();
    PC_CHECKING { errno = 7; pc_check("%d", *f()); }
}
CASE(ctype_b_loc, 3300386, 1) {
    const unsigned short **(*volatile f)(void) = __ctype_b_loc;
    for (long i = 0; i < n; i++) pc_sink += (*f())['a'];
    PC_CHECKING pc_check("%d", ((*f())['a'] & _ISalpha) != 0);
}
CASE(mb_cur_max_get, 1110960, 1) {
    size_t (*volatile f)(void) = __ctype_get_mb_cur_max;
    for (long i = 0; i < n; i++) pc_sink += (long)f();
    PC_CHECKING pc_check("%ld", (long)f());
}
CASE(clearerr_unlocked, 2000003, 1) {
    FILE *volatile fp = stdin;
    for (long i = 0; i < n; i++) { clearerr_unlocked(fp); pc_sink++; }
    PC_CHECKING { clearerr_unlocked(fp); pc_check("%d %d", ferror(fp), feof(fp)); }
}
CASE(ferror, 1000000, 1) {
    FILE *volatile fp = stdout;
    for (long i = 0; i < n; i++) pc_sink += ferror(fp);
    PC_CHECKING pc_check("%d", ferror(fp));
}
CASE(sigsetjmp_nomask, 1800008, 1) {
    static sigjmp_buf env;
    for (long i = 0; i < n; i++) pc_sink += sigsetjmp(env, 0);
    PC_CHECKING pc_check("%d", sigsetjmp(env, 0));
}
CASE(sigsetjmp_mask, 1800008, 1) {
    static sigjmp_buf env;
    for (long i = 0; i < n; i++) pc_sink += sigsetjmp(env, 1);
    PC_CHECKING pc_check("%d", sigsetjmp(env, 1));
}
CASE(setjmp_bsd, 1000001, 1) {
    static jmp_buf env;
    for (long i = 0; i < n; i++) pc_sink += _setjmp(env);
    PC_CHECKING pc_check("%d", _setjmp(env));
}
CASE(strncpy_short, 600001, 1) {
    const char *volatile s = "hello_world"; char d[32];
    for (long i = 0; i < n; i++) { strncpy(d, s, sizeof d); pc_sink += d[3]; }
    PC_CHECKING { strncpy(d, s, sizeof d); pc_check("%s %d", d, d[31]); }
}
CASE(strtoimax_short, 300003, 1) {
    const char *volatile s = "12345"; char *e;
    for (long i = 0; i < n; i++) pc_sink += (long)strtoimax(s, &e, 10);
    PC_CHECKING { long r = (long)strtoimax(s, &e, 10); pc_check("%ld %d", r, (int)(e - s)); }
}
CASE(imaxdiv, 300000, 1) {
    volatile intmax_t a = 123456789, b = 10;
    for (long i = 0; i < n; i++) { imaxdiv_t q = imaxdiv(a, b); pc_sink += (long)(q.quot + q.rem); }
    PC_CHECKING { imaxdiv_t q = imaxdiv(a, b); pc_check("%ld %ld", (long)q.quot, (long)q.rem); }
}
CASE(setlocale_query_all, 135700, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)(setlocale(LC_ALL, NULL) != 0);
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, NULL));
}
CASE(strdup_short, 281234, 1) {
    const char *volatile s = "src/file.c";
    for (long i = 0; i < n; i++) { char *p = strdup(s); pc_sink += p[0]; free(p); }
    PC_CHECKING { char *p = strdup(s); pc_check("%s", p); free(p); }
}
CASE(strndup_short, 224042, 1) {
    const char *volatile s = "src/file.c:rest";
    for (long i = 0; i < n; i++) { char *p = strndup(s, 10); pc_sink += p[0]; free(p); }
    PC_CHECKING { char *p = strndup(s, 10); pc_check("%s", p); free(p); }
}
CASE(statx_file, 139190, 1) {
    char p[512]; struct statx sx; snprintf(p, sizeof p, "%s/a", dir);
    for (long i = 0; i < n; i++) pc_sink += statx(AT_FDCWD, p, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS, &sx);
    PC_CHECKING { int r = statx(AT_FDCWD, p, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS, &sx); pc_check("%d %lld", r, (long long)sx.stx_size); }
}
CASE(uselocale_query, 135680, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)(uselocale((locale_t)0) != 0);
    PC_CHECKING pc_check("%d", uselocale((locale_t)0) == LC_GLOBAL_LOCALE);
}
