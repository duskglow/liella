#include <stdio.h>
#include <string.h>
/* HEAP-FIRST-USE-4: the first string calls of a program that has not taken the lazy NULL-folding
   decision yet (dispatch::resolve_fold: printf and the first malloc take none). They run the checked
   entries (memchr's liella_hot_memchr_rest takes the decision here), which p_str misses: there a
   40-byte memcpy takes the decision first and every later call runs folded. Through volatile
   pointers, as in p_str. The order is the strings oracle's (crates/core/tests/oracle/page_faults.c). */
static size_t (*volatile len_fn)(const char *) = strlen;
static int (*volatile ncmp_fn)(const char *, const char *, size_t) = strncmp;
static void *(*volatile mchr_fn)(const void *, int, size_t) = memchr;
static int (*volatile mcmp_fn)(const void *, const void *, size_t) = memcmp;
static char *(*volatile scpy_fn)(char *, const char *) = strcpy;
int main(void)
{
    printf("%s %d\n", "a", 5);
    fflush(stdout);
    char b[64];
    const char *volatile s = "a line of text: compared";
    long n = (long)len_fn(s) + ncmp_fn(s, "a line", 6) + (mchr_fn(s, ':', 20) != NULL) + mcmp_fn(s, "a l", 3);
    n += scpy_fn(b, s) == b;
    return n == 26 ? 0 : 1;
}
