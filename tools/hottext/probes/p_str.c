#include <stdio.h>
#include <stdlib.h>
#include <string.h>
/* RC1-FIRST: the string and memory calls most programs make early; through volatile pointers so the
   compiler cannot fold them, and the library's own entry points (and their CPU-variant kernels) run. */
static size_t (*volatile len_fn)(const char *) = strlen;
static int (*volatile cmp_fn)(const char *, const char *) = strcmp;
static int (*volatile ncmp_fn)(const char *, const char *, size_t) = strncmp;
static int (*volatile mcmp_fn)(const void *, const void *, size_t) = memcmp;
static void *(*volatile cpy_fn)(void *, const void *, size_t) = memcpy;
static void *(*volatile mov_fn)(void *, const void *, size_t) = memmove;
static void *(*volatile set_fn)(void *, int, size_t) = memset;
static char *(*volatile chr_fn)(const char *, int) = strchr;
static char *(*volatile rchr_fn)(const char *, int) = strrchr;
static void *(*volatile mchr_fn)(const void *, int, size_t) = memchr;
static char *(*volatile scpy_fn)(char *, const char *) = strcpy;
static char *(*volatile dup_fn)(const char *) = strdup;
int main(int argc, char **argv)
{
    char a[256], b[256];
    const char *s = argc > 5 ? argv[1] : "hello, world: a line of text to compare";
    long n = (long)len_fn(s);
    set_fn(a, 'x', sizeof a);
    cpy_fn(b, s, (size_t)n + 1);
    mov_fn(a + 1, a, 64);
    scpy_fn(a, s);
    n += cmp_fn(a, b) + ncmp_fn(a, "hello", 5) + mcmp_fn(a, b, (size_t)n);
    /* HOT-ORDER-3: memcmp's head probe covers 8..64 bytes only; under 8 it tail-jumps to
       liella_hot_compare_rest, which a long compare never reaches (page_faults.c "strings": 1 fault). */
    n += mcmp_fn(a, "hel", 3);
    n += chr_fn(a, ':') != NULL;
    n += rchr_fn(a, 't') != NULL;
    n += mchr_fn(a, 'w', 20) != NULL;
    char *d = dup_fn(a);
    n += d != NULL;
    free(d);
    printf("%ld\n", n);
    return 0;
}
