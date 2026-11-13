/* newlocale category-mask bits: which masks are accepted, and what the result holds
 * (NEWLOCALE-LC-ALL-BIT; spec/behaviour/newlocale-mask.md). Built against the host C library
 * and statically against Liella's products; locale_products.rs compares the outputs line by
 * line. Without arguments: every single bit 0..13, LC_ALL_MASK, bit 6 with other bits, with
 * "C", "POSIX", "", "C.UTF-8", "en_US.UTF-8", an unknown and a composite name, base NULL or an
 * en_US.UTF-8 object, and a NULL name. With an argument: only the "" rows (the caller sets the
 * environment). Each result prints its codeset, radix and the name of each category
 * (_NL_LOCALE_NAME), then the codeset seen through uselocale. */
#define _GNU_SOURCE
#include <errno.h>
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* _NL_LOCALE_NAME(cat): the object's own name for one category. */
#define NAME_ITEM(cat) ((nl_item)(((cat) << 16) | 0xFFFF))

static const char *const NAMES[] = {"C", "POSIX", "", "C.UTF-8", "en_US.UTF-8"};

static void show(const char *label, int mask, const char *name, int with_base) {
    locale_t base = (locale_t)0;
    if (with_base) {
        base = newlocale(LC_ALL_MASK, "en_US.UTF-8", (locale_t)0);
        if (base == (locale_t)0) {
            printf("base failed errno=%d\n", errno);
            return;
        }
    }
    errno = 0;
    locale_t l = newlocale(mask, name, base);
    int e = errno;
    printf("%s mask=%#x name=\"%s\" base=%s: ", label, (unsigned)mask, name, with_base ? "en_US" : "NULL");
    if (l == (locale_t)0) {
        printf("NULL errno=%d base_kept=%s\n", e, with_base ? "yes" : "-");
        if (with_base) freelocale(base);
        return;
    }
    printf("ok codeset=%s dp=%s names=", nl_langinfo_l(CODESET, l), nl_langinfo_l(RADIXCHAR, l));
    for (int c = 0; c <= 12; c++) {
        if (c == LC_ALL) continue;
        printf("%s%s", c ? "," : "", nl_langinfo_l(NAME_ITEM(c), l));
    }
    printf("\n");
    /* The object is usable: uselocale and back. */
    locale_t prev = uselocale(l);
    printf("  uselocale: codeset=%s\n", nl_langinfo(CODESET));
    uselocale(prev);
    freelocale(l);
}

int main(int argc, char **argv) {
    if (argc > 1) {
        /* Environment mode: only the "" rows (the caller sets the variables). */
        printf("env LC_ALL=%s LC_CTYPE=%s LANG=%s\n", getenv("LC_ALL") ? getenv("LC_ALL") : "(unset)",
               getenv("LC_CTYPE") ? getenv("LC_CTYPE") : "(unset)", getenv("LANG") ? getenv("LANG") : "(unset)");
        for (int b = 0; b < 2; b++) {
            show("bit6", 1 << LC_ALL, "", b);
            show("allmask", LC_ALL_MASK, "", b);
            show("numeric|time", LC_NUMERIC_MASK | LC_TIME_MASK, "", b);
        }
        return 0;
    }
    for (int m = 0; m < 2; m++) {
        errno = 0;
        locale_t n = newlocale(m ? LC_ALL_MASK : LC_CTYPE_MASK, NULL, (locale_t)0);
        printf("mask=%#x name=NULL: %s errno=%d\n", m ? (unsigned)LC_ALL_MASK : (unsigned)LC_CTYPE_MASK, n ? "ok" : "NULL", errno);
        if (n) freelocale(n);
    }
    printf("LC_ALL=%d LC_ALL_MASK=%#x LC_CTYPE_MASK=%#x\n", LC_ALL, (unsigned)LC_ALL_MASK, (unsigned)LC_CTYPE_MASK);
    for (int n = 0; n <= 13; n++) {
        char label[16];
        snprintf(label, sizeof label, "bit%d", n);
        for (int b = 0; b < 2; b++) show(label, 1 << n, b ? "C" : "C.UTF-8", b);
    }
    for (int b = 0; b < 2; b++) {
        show("allmask", LC_ALL_MASK, "C", b);
        show("allmask|bit6", LC_ALL_MASK | (1 << LC_ALL), "C", b);
        show("zero", 0, "C", b);
        show("bit31", (int)(1u << 31), "C", b);
        show("bit6|bit13", (1 << LC_ALL) | (1 << 13), "C", b);
    }
    for (size_t i = 0; i < sizeof NAMES / sizeof *NAMES; i++) {
        for (int b = 0; b < 2; b++) {
            show("bit6", 1 << LC_ALL, NAMES[i], b);
            show("bit6|ctype", (1 << LC_ALL) | LC_CTYPE_MASK, NAMES[i], b);
            show("bit6|numeric|time", (1 << LC_ALL) | LC_NUMERIC_MASK | LC_TIME_MASK, NAMES[i], b);
        }
    }
    show("bit6", 1 << LC_ALL, "xx_YY.NOPE", 0);
    show("bit6", 1 << LC_ALL, "xx_YY.NOPE", 1);
    show("allmask", LC_ALL_MASK, "xx_YY.NOPE", 1);
    show("ctype", LC_CTYPE_MASK, "xx_YY.NOPE", 1);
    /* A composite name (setlocale's LC_ALL form) through bit 6 and through LC_ALL_MASK. */
    setlocale(LC_ALL, "C");
    setlocale(LC_CTYPE, "C.UTF-8");
    setlocale(LC_TIME, "en_US.UTF-8");
    char composite[512];
    snprintf(composite, sizeof composite, "%s", setlocale(LC_ALL, NULL));
    setlocale(LC_ALL, "C");
    for (int b = 0; b < 2; b++) {
        show("bit6 composite", 1 << LC_ALL, composite, b);
        show("allmask composite", LC_ALL_MASK, composite, b);
    }
    errno = 0;
    locale_t n = newlocale(1 << LC_ALL, NULL, (locale_t)0);
    printf("bit6 name=NULL: %s errno=%d\n", n ? "ok" : "NULL", errno);
    if (n) freelocale(n);
    return 0;
}
