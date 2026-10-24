/* Spec §3.3 mcheck/mprobe/mcheck_check_all (D-3.5 as decided in phase 13: Liella's C library
 * provides libc_malloc_debug semantics itself): mcheck before the first allocation turns
 * checking on, once; every block of the malloc family is checked; problems go to abortfn, and
 * the operation goes on when it returns. */
#define _GNU_SOURCE
#include <malloc.h>
#include <mcheck.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static volatile int calls;
static volatile int last = -9;

static void abortfn(enum mcheck_status s)
{
    calls++;
    last = (int)s;
}

int main(void)
{
    CHECK("s3_3_c_2", mcheck(abortfn) == 0);
    CHECK("s3_3_c_2", mcheck(abortfn) == -1);           /* once */
    CHECK("s3_3_c_2", mcheck_pedantic(abortfn) == -1);

    /* A fresh block: OK, its requested size usable. */
    char *volatile p = malloc(10);
    CHECK("s3_3_c_2", p != NULL && mprobe(p) == MCHECK_OK && calls == 0);
    CHECK("s3_3_c_2", malloc_usable_size(p) == 10);

    /* One byte past the end: MCHECK_TAIL, reported to abortfn by mprobe itself. */
    char t = p[10];
    p[10] = (char)(t ^ 0x5a);
    CHECK("s3_3_c_2", mprobe(p) == MCHECK_TAIL && calls == 1 && last == MCHECK_TAIL);
    p[10] = t;
    CHECK("s3_3_c_2", mprobe(p) == MCHECK_OK && calls == 1);

    /* The byte before it: MCHECK_HEAD; restored, OK again. */
    char h = p[-1];
    p[-1] = (char)(h ^ 0x5a);
    CHECK("s3_3_c_2", mprobe(p) == MCHECK_HEAD && calls == 2 && last == MCHECK_HEAD);
    p[-1] = h;
    CHECK("s3_3_c_2", mprobe(p) == MCHECK_OK);

    /* free of a clobbered block reports it and still releases it; a freed block is
     * MCHECK_FREE, and a second free is reported and does nothing. */
    p[10] = (char)(t ^ 1);
    free(p);
    CHECK("s3_3_c_2", calls == 3 && last == MCHECK_TAIL);
    CHECK("s3_3_c_2", mprobe(p) == MCHECK_FREE && calls == 4 && last == MCHECK_FREE);
    free(p);
    CHECK("s3_3_c_2", calls == 5 && last == MCHECK_FREE);

    /* mcheck_check_all reports each damaged live block. */
    char *volatile a = malloc(30), *volatile b = malloc(7);
    char ta = a[30];
    a[30] = (char)(ta ^ 0x33);
    mcheck_check_all();
    CHECK("s3_3_c_2", calls == 6 && last == MCHECK_TAIL);
    a[30] = ta;
    mcheck_check_all();
    CHECK("s3_3_c_2", calls == 6);

    /* realloc keeps the contents and the checking; calloc zeroes; the aligned forms align. */
    memcpy(b, "abcdefg", 7);
    b = realloc(b, 5000);
    CHECK("s3_3_c_2", b != NULL && memcmp(b, "abcdefg", 7) == 0 && mprobe(b) == MCHECK_OK);
    CHECK("s3_3_c_2", malloc_usable_size(b) == 5000);
    unsigned char *volatile z = calloc(13, 17);
    int zero = 1;
    for (int i = 0; i < 13 * 17; i++)
        if (z[i]) zero = 0;
    CHECK("s3_3_c_2", zero && mprobe(z) == MCHECK_OK);
    void *m = NULL;
    CHECK("s3_3_c_2", posix_memalign(&m, 64, 70) == 0 && ((uintptr_t)m & 63) == 0 && mprobe(m) == MCHECK_OK);
    void *volatile al = aligned_alloc(4096, 100);
    CHECK("s3_3_c_2", al != NULL && ((uintptr_t)al & 4095) == 0 && mprobe(al) == MCHECK_OK);
    char *volatile d = strdup("library-made block");
    CHECK("s3_3_c_2", d != NULL && mprobe(d) == MCHECK_OK);

    /* NULL is OK; an address that is not a block is MCHECK_HEAD; free of an interior pointer
     * is reported, not an abort. */
    static char buf[64];
    CHECK("s3_3_c_2", mprobe(NULL) == MCHECK_OK && calls == 6);
    CHECK("s3_3_c_2", mprobe(buf + 32) == MCHECK_HEAD && calls == 7 && last == MCHECK_HEAD);
    free((char *)a + 8);
    CHECK("s3_3_c_2", calls == 8 && last == MCHECK_HEAD);

    free(a);
    free(b);
    free(z);
    free(m);
    free(al);
    free(d);
    CHECK("s3_3_c_2", calls == 8);
    return 0;
}
