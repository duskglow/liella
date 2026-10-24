/* D-X.14 (owner ruling 2026-10-05): a program links statically against libc.a while defining its
 * own copies of functions and variables the library also defines.
 *
 * - It links: the library's definitions are weak, the program's win (no "multiple definition").
 * - The library's own calls never reach the program's functions: the program's strlen, getenv,
 *   qsort, error, error_at_line, strlcpy and strerror count their calls, and library functions
 *   that use those names internally leave the counts at zero. So do the program's memcpy,
 *   memmove and memset: the library's own copies and fills go to its internal entries (D-X.15).
 * - Malloc replacement is supported, as glibc's manual documents ("Replacing malloc"): the
 *   program's malloc family is the allocator of everything the library hands to the caller
 *   (every such pointer must be one the program's allocator made), and the library frees and
 *   resizes caller memory through the program's free and realloc.
 * - A documented hook variable the program defines is the library's: obstack's
 *   obstack_alloc_failed_handler.
 * Each failed check prints FAIL and the line; the exit status is the number of failures. */
#define _GNU_SOURCE
#include <dirent.h>
#include <execinfo.h>
#include <glob.h>
#include <locale.h>
#include <obstack.h>
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>
#include <signal.h>
#include <wordexp.h>

static int failures;
#define CHECK(cond)                                                                                \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            failures++;                                                                            \
            dprintf(2, "FAIL line %d: %s\n", __LINE__, #cond);                                     \
        }                                                                                          \
    } while (0)

/* ---------------------------------------------------------------- the program's allocator */

#define ARENA (64u << 20)
#define MAXBLOCKS 65536
static _Alignas(64) unsigned char arena[ARENA];
static size_t used;
static struct block {
    unsigned char *p;
    size_t n;
    int live;
} blocks[MAXBLOCKS];
static int nblocks;
/* Volatile: the counters change inside calls the compiler may assume do not touch them (the
 * allocator functions carry library attributes in the headers). */
static volatile unsigned long n_malloc, n_free, n_realloc;

static struct block *find(void *p)
{
    for (int i = nblocks - 1; i >= 0; i--)
        if (blocks[i].p == p)
            return &blocks[i];
    return NULL;
}

static void *take(size_t n, size_t align)
{
    if (align < 16)
        align = 16;
    size_t at = (used + align - 1) & ~(align - 1);
    if (n > ARENA || at > ARENA - (n ? n : 1) || nblocks == MAXBLOCKS)
        return NULL;
    used = at + (n ? n : 1);
    blocks[nblocks++] = (struct block){arena + at, n, 1};
    n_malloc++;
    return arena + at;
}

/* Whether p is a live block of the program's allocator. */
static int ours(const void *p)
{
    struct block *b = find((void *)p);
    return b && b->live;
}

/* Whether p lies inside the live block that starts at base (wordexp keeps the words and the
 * vector in one block, so a word is an interior pointer). */
static int inside(const void *base, const void *p)
{
    struct block *b = find((void *)base);
    return b && b->live && (const unsigned char *)p >= b->p && (const unsigned char *)p < b->p + b->n;
}

/* The allocator's own copies, so that only the library's and main's calls reach the program's
 * memcpy and memset. */
static void fill_bytes(void *d, int c, size_t n)
{
    volatile unsigned char *dd = d;
    for (size_t i = 0; i < n; i++)
        dd[i] = (unsigned char)c;
}
static void copy_bytes(void *d, const void *s, size_t n)
{
    volatile unsigned char *dd = d;
    const unsigned char *ss = s;
    for (size_t i = 0; i < n; i++)
        dd[i] = ss[i];
}
void *malloc(size_t n) { return take(n, 16); }
void *calloc(size_t k, size_t n)
{
    if (n && k > SIZE_MAX / n)
        return NULL;
    void *p = take(k * n, 16);
    if (p)
        fill_bytes(p, 0, k * n);
    return p;
}
void free(void *p)
{
    if (!p)
        return;
    struct block *b = find(p);
    if (!b || !b->live) {
        dprintf(2, "FAIL: free of %p, not a live block of the program's allocator\n", p);
        failures++;
        return;
    }
    b->live = 0;
    n_free++;
}
void *realloc(void *p, size_t n)
{
    n_realloc++;
    if (!p)
        return take(n, 16);
    struct block *b = find(p);
    if (!b || !b->live) {
        dprintf(2, "FAIL: realloc of %p, not a live block of the program's allocator\n", p);
        failures++;
        return NULL;
    }
    void *q = take(n, 16);
    if (q) {
        copy_bytes(q, p, b->n < n ? b->n : n);
        b->live = 0;
    }
    return q;
}
int posix_memalign(void **out, size_t align, size_t n)
{
    void *p = take(n, align);
    if (!p)
        return 12;
    *out = p;
    return 0;
}
void *aligned_alloc(size_t align, size_t n) { return take(n, align); }
void *memalign(size_t align, size_t n) { return take(n, align); }
size_t malloc_usable_size(void *p)
{
    struct block *b = find(p);
    return b ? b->n : 0;
}

/* ---------------------------------------------------------------- functions the library also uses */

static volatile unsigned long n_strlen, n_getenv, n_qsort, n_error, n_strlcpy, n_strerror;
static volatile unsigned long n_memcpy, n_memmove, n_memset;

void *memmove(void *d, const void *s, size_t n)
{
    n_memmove++;
    volatile unsigned char *dd = d;
    const volatile unsigned char *ss = s;
    if (dd < ss)
        for (size_t i = 0; i < n; i++)
            dd[i] = ss[i];
    else
        for (size_t i = n; i-- > 0;)
            dd[i] = ss[i];
    return d;
}
void *memcpy(void *d, const void *s, size_t n)
{
    n_memcpy++;
    volatile unsigned char *dd = d;
    const volatile unsigned char *ss = s;
    for (size_t i = 0; i < n; i++)
        dd[i] = ss[i];
    return d;
}
void *memset(void *d, int c, size_t n)
{
    n_memset++;
    volatile unsigned char *dd = d;
    for (size_t i = 0; i < n; i++)
        dd[i] = (unsigned char)c;
    return d;
}

size_t strlen(const char *s)
{
    n_strlen++;
    size_t n = 0;
    while (s[n])
        n++;
    return n;
}
char *getenv(const char *name)
{
    (void)name;
    n_getenv++;
    return NULL;
}
void qsort(void *b, size_t n, size_t sz, int (*cmp)(const void *, const void *))
{
    (void)b, (void)n, (void)sz, (void)cmp;
    n_qsort++;
}
void error(int status, int errnum, const char *fmt, ...)
{
    (void)status, (void)errnum, (void)fmt;
    n_error++;
}
void error_at_line(int status, int errnum, const char *file, unsigned line, const char *fmt, ...)
{
    (void)status, (void)errnum, (void)file, (void)line, (void)fmt;
    n_error++;
}
size_t strlcpy(char *d, const char *s, size_t n)
{
    (void)d, (void)s, (void)n;
    n_strlcpy++;
    return 0;
}
char *strerror(int e)
{
    (void)e;
    n_strerror++;
    return "program's strerror";
}

/* ---------------------------------------------------------------- a hook variable */

static jmp_buf hook_jump;
static int hook_called;
static void program_handler(void)
{
    hook_called = 1;
    longjmp(hook_jump, 1);
}
void (*obstack_alloc_failed_handler)(void) = program_handler;
static void *no_memory(size_t n)
{
    (void)n;
    return NULL;
}
#define obstack_chunk_alloc no_memory
#define obstack_chunk_free free

static int asprintf_v(char **out, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    int r = vasprintf(out, fmt, ap);
    va_end(ap);
    return r;
}

static int by_name(const struct dirent **a, const struct dirent **b) { return strcmp((*a)->d_name, (*b)->d_name); }

int main(void)
{
    /* The library's own use of the overridden names: none of these reach the program's. */
    char *d = strdup("abcdef");
    CHECK(d && memcmp(d, "abcdef", 7) == 0);
    char line[64];
    snprintf(line, sizeof line, "%s|%.2s|%ls", "xyz", "pqrs", L"w");
    CHECK(memcmp(line, "xyz|pq|w", 9) == 0);
    setenv("TZ", "UTC0", 1);
    time_t t = 86400;
    struct tm tm;
    CHECK(localtime_r(&t, &tm) && tm.tm_mday == 2 && tm.tm_gmtoff == 0);
    CHECK(setlocale(LC_ALL, "") != NULL);
    char cwd[4096];
    CHECK(getcwd(cwd, sizeof cwd) != NULL);
    perror("probe");          /* the library's strerror, not the program's */
    psignal(15, "probe");
    CHECK(n_strlen == 0 && n_getenv == 0 && n_qsort == 0 && n_error == 0 && n_strlcpy == 0 && n_strerror == 0);
    if (n_strlen || n_getenv || n_qsort || n_error || n_strlcpy || n_strerror)
        dprintf(2, "counts: strlen %lu getenv %lu qsort %lu error %lu strlcpy %lu strerror %lu\n", n_strlen, n_getenv, n_qsort, n_error, n_strlcpy,
                n_strerror);
    /* D-X.15: long copies and fills inside the library (formatting, duplication, a stream's
     * buffer, wide conversion) never reach the program's memcpy, memmove or memset. */
    static char big[600], out[700];
    for (int i = 0; i < 599; i++)
        big[i] = (char)('a' + i % 26);
    n_memcpy = n_memmove = n_memset = 0;
    CHECK(snprintf(out, sizeof out, "%s|%s", big, "tail") == 604);
    char *bigdup = strdup(big);
    CHECK(bigdup && ours(bigdup));
    FILE *bf = fmemopen(out, sizeof out, "w+");
    CHECK(bf && fputs(big, bf) >= 0 && fflush(bf) == 0);
    fclose(bf);
    wchar_t wide[700];
    CHECK(mbstowcs(wide, big, 700) == 599);
    unsigned long cp = n_memcpy, mv = n_memmove, st = n_memset;
    CHECK(cp == 0 && mv == 0 && st == 0);
    if (cp || mv || st)
        dprintf(2, "counts: memcpy %lu memmove %lu memset %lu\n", cp, mv, st);
    free(bigdup);
    /* The program's own calls reach its definitions. */
    /* (strlen is declared pure in <string.h>, so only its value is checked.) */
    CHECK(strlen("ab") == 2);
    CHECK(memcpy(out, "xy", 2) == out && n_memcpy == 1 && out[1] == 'y');
    CHECK(memmove(out + 1, out, 2) == out + 1 && n_memmove == 1 && out[2] == 'y');
    CHECK(memset(out, 'z', 3) == out && n_memset == 1 && out[2] == 'z');
    CHECK(getenv("TZ") == NULL && n_getenv == 1);
    char tiny[4];
    CHECK(strlcpy(tiny, "x", sizeof tiny) == 0 && n_strlcpy == 1);

    /* Caller-owned memory comes from the program's allocator. */
    unsigned long frees = n_free;
    CHECK(ours(d));
    free(d);
    char *nd = strndup("abcdef", 3);
    CHECK(ours(nd) && memcmp(nd, "abc", 4) == 0);
    free(nd);
    wchar_t *wd = wcsdup(L"wide");
    CHECK(ours(wd));
    free(wd);
    char *a = NULL;
    CHECK(asprintf(&a, "%d-%s", 42, "x") == 4 && ours(a));
    free(a);
    CHECK(asprintf_v(&a, "%s", "va") == 2 && ours(a));
    free(a);
    FILE *f = fmemopen((void *)"one\ntwo three\n", 14, "r");
    char *buf = NULL;
    size_t cap = 0;
    CHECK(getline(&buf, &cap, f) == 4 && ours(buf));
    CHECK(getdelim(&buf, &cap, ' ', f) == 4 && ours(buf));
    free(buf);
    fclose(f);
    char *mem = NULL;
    size_t memlen = 0;
    FILE *ms = open_memstream(&mem, &memlen);
    for (int i = 0; i < 1000; i++)
        fputs("0123456789", ms);
    fclose(ms);
    CHECK(memlen == 10000 && ours(mem));
    free(mem);
    wchar_t *wmem = NULL;
    size_t wlen = 0;
    FILE *wms = open_wmemstream(&wmem, &wlen);
    fputws(L"wide text", wms);
    fclose(wms);
    CHECK(wlen == 9 && ours(wmem));
    free(wmem);
    void *frames[4];
    int nf = backtrace(frames, 4);
    char **syms = backtrace_symbols(frames, nf);
    CHECK(nf > 0 && ours(syms));
    free(syms);
    char *c2 = getcwd(NULL, 4096);
    CHECK(ours(c2));
    free(c2);
    char *c1 = getcwd(NULL, 0);
    CHECK(ours(c1));
    char *r1 = realpath(".", NULL);
    CHECK(ours(r1) && strcmp(r1, c1) == 0);
    char *r2 = canonicalize_file_name(".");
    CHECK(ours(r2));
    free(c1);
    free(r1);
    free(r2);
    char *gc = get_current_dir_name();
    CHECK(ours(gc));
    free(gc);
    struct dirent **list = NULL;
    int n = scandir("/", &list, NULL, by_name);
    CHECK(n > 2 && ours(list));
    for (int i = 0; i < n; i++) {
        CHECK(ours(list[i]));
        free(list[i]);
    }
    free(list);
    unsigned long before = n_free;
    wordexp_t we;
    CHECK(wordexp("a b c", &we, 0) == 0 && we.we_wordc == 3 && ours(we.we_wordv) && inside(we.we_wordv, we.we_wordv[0]));
    wordfree(&we);
    CHECK(n_free > before);
    glob_t g;
    CHECK(glob("/*", 0, NULL, &g) == 0 && g.gl_pathc > 2 && ours(g.gl_pathv) && ours(g.gl_pathv[0]));
    before = n_free;
    globfree(&g);
    CHECK(n_free > before);
    if (n_free <= before)
        dprintf(2, "globfree: frees %lu -> %lu, gl_offs %zu, gl_pathv %p\n", before, n_free, g.gl_offs, (void *)g.gl_pathv);
    char *tn = tempnam(NULL, "x");
    CHECK(ours(tn));
    free(tn);
    int *arr = reallocarray(NULL, 4, sizeof(int));
    CHECK(ours(arr));
    arr = reallocarray(arr, 8, sizeof(int));
    CHECK(ours(arr));
    free(arr);
    CHECK(n_free > frees);

    /* A hook variable the program defines is the library's. */
    struct obstack ob;
    if (setjmp(hook_jump) == 0)
        obstack_init(&ob);
    CHECK(hook_called);

    if (failures)
        dprintf(2, "%d failures\n", failures);
    return failures;
}
