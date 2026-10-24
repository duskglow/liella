/* §4.3 (s4_3_c_1), D-4.27, D-5.6, CASECMP: strcasecmp, strncasecmp and their _l forms order two
 * strings exactly as a byte loop comparing tolower_l of each byte in the locale's LC_CTYPE does, in
 * every locale - including those whose case map of 0..127 is NOT ASCII's (tr_TR: the kernels'
 * ASCII fold must not run there) and single-byte locales whose letters above 0x7f fold.
 *
 * Every byte pair (x, y), 1..255 each, is planted at several positions behind a mixed-case ASCII
 * prefix (so the vector kernels run whole blocks before the difference), followed by a short tail;
 * the bounded forms are checked with the bound just before, at and after the planted byte. The
 * export runs through the _l form and, under uselocale, through the plain one. The output (one
 * summary line per locale: bytes below 0x80 whose fold is not ASCII's, and a hash of every result
 * sign) is the same under glibc 2.39 (black-box probe, CASECMP).
 *
 * A locale the host does not provide is compiled with the host's `localedef` into a private
 * directory named by LOCPATH (removed at exit); one that still does not load fails the program. */
#define _GNU_SOURCE
#include <ctype.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

static int failures;
static char dir[256];

static int (*volatile f_cmp_l)(const char *, const char *, locale_t) = strcasecmp_l;
static int (*volatile f_ncmp_l)(const char *, const char *, size_t, locale_t) = strncasecmp_l;
static int (*volatile f_cmp)(const char *, const char *) = strcasecmp;
static int (*volatile f_ncmp)(const char *, const char *, size_t) = strncasecmp;

static void cleanup(void) {
    if (dir[0]) {
        char cmd[300];
        snprintf(cmd, sizeof cmd, "rm -rf '%s'", dir);
        if (system(cmd) != 0)
            printf("could not remove %s\n", dir);
    }
}

static void prepare(void) {
    const char *tmp = getenv("TMPDIR");
    snprintf(dir, sizeof dir, "%s/s04-casecmp-XXXXXX", tmp && *tmp ? tmp : "/tmp");
    if (!mkdtemp(dir)) {
        dir[0] = 0;
        printf("FAIL: mkdtemp\n");
        exit(1);
    }
    atexit(cleanup);
    /* The output name must contain a '/': a bare name makes localedef write the system archive
     * (needs root) instead of the private directory. Each locale also gets its normalized-codeset
     * alias, as in s05_ctype_words. A locale that fails to compile fails in run(), not here. */
    char cmd[1600];
    snprintf(cmd, sizeof cmd,
             "cd '%s' && localedef -i tr_TR -f UTF-8 ./tr_TR.UTF-8 >/dev/null 2>&1;"
             " localedef -i tr_TR -f ISO-8859-9 ./tr_TR.ISO-8859-9 >/dev/null 2>&1;"
             " localedef -i en_US -f UTF-8 ./en_US.UTF-8 >/dev/null 2>&1;"
             " localedef -i de_DE -f ISO-8859-1 ./de_DE.ISO-8859-1 >/dev/null 2>&1;"
             " ln -s tr_TR.UTF-8 tr_TR.utf8; ln -s tr_TR.ISO-8859-9 tr_TR.iso88599;"
             " ln -s en_US.UTF-8 en_US.utf8; ln -s de_DE.ISO-8859-1 de_DE.iso88591; true",
             dir);
    if (system(cmd) == -1 || setenv("LOCPATH", dir, 1) != 0) {
        printf("FAIL: compiling the locales (localedef from the locales package)\n");
        exit(1);
    }
}

static int sign(int v) {
    return (v > 0) - (v < 0);
}

/* The reference: the first n bytes (or to the NUL) compared after tolower_l, byte by byte. */
static int ref(const unsigned char *a, const unsigned char *b, size_t n, locale_t loc) {
    for (size_t i = 0; i < n; i++) {
        int x = tolower_l(a[i], loc), y = tolower_l(b[i], loc);
        if (x != y)
            return x < y ? -1 : 1;
        if (a[i] == 0)
            return 0;
    }
    return 0;
}

static const char TEXT[] = "AbCdEfGhIjKlMnOpQrStUvWxYz0123 .-_iIoO";

static void run(const char *name) {
    /* The host's own locales first; the private copies when the host lacks one. */
    locale_t loc = newlocale(LC_ALL_MASK, name, (locale_t)0);
    if (!loc && !dir[0]) {
        prepare();
        loc = newlocale(LC_ALL_MASK, name, (locale_t)0);
    }
    if (!loc) {
        printf("FAIL: newlocale(%s)\n", name);
        failures++;
        return;
    }
    int nonascii = 0;
    for (int c = 0; c < 128; c++)
        if (tolower_l(c, loc) != tolower(c) || toupper_l(c, loc) != toupper(c))
            nonascii++;
    static const size_t POS[] = {0, 7, 16, 33, 70};
    unsigned char a[128], b[128];
    unsigned long hash = 1469598103u;
    int bad = 0;
    for (size_t pi = 0; pi < sizeof POS / sizeof POS[0]; pi++) {
        size_t p = POS[pi];
        for (size_t i = 0; i < p; i++) {
            unsigned char t = (unsigned char)TEXT[(i * 7) % (sizeof TEXT - 1)];
            /* Prefix bytes are ASCII letters/digits/punctuation; b gets the other case of each
             * letter only where the locale folds both to the same byte (tr_TR's I/i do not). */
            a[i] = t;
            unsigned char u = (unsigned char)(isupper(t) ? t + 32 : islower(t) ? t - 32 : t);
            b[i] = tolower_l(u, loc) == tolower_l(t, loc) ? u : t;
        }
        for (int x = 1; x < 256; x++) {
            for (int y = 1; y < 256; y++) {
                a[p] = (unsigned char)x;
                b[p] = (unsigned char)y;
                memcpy(a + p + 1, "tail", 5);
                memcpy(b + p + 1, "TAIL", 5);
                int want = ref(a, b, (size_t)-1, loc);
                int got = sign(f_cmp_l((const char *)a, (const char *)b, loc));
                int wn0 = ref(a, b, p, loc), gn0 = sign(f_ncmp_l((const char *)a, (const char *)b, p, loc));
                int wn1 = ref(a, b, p + 1, loc), gn1 = sign(f_ncmp_l((const char *)a, (const char *)b, p + 1, loc));
                if (got != want || gn0 != wn0 || gn1 != wn1) {
                    if (bad++ < 5)
                        printf("FAIL: %s pos %zu bytes %#x/%#x: strcasecmp_l %d (want %d), n=%zu %d (want %d), n=%zu %d (want %d)\n",
                               name, p, x, y, got, want, p, gn0, wn0, p + 1, gn1, wn1);
                }
                hash = (hash ^ (unsigned long)(got + 1 + 3 * (gn1 + 1))) * 1099511628211u;
            }
        }
    }
    /* The plain exports under uselocale, at one position. */
    locale_t old = uselocale(loc);
    size_t p = 16;
    for (int x = 1; x < 256; x++) {
        for (int y = 1; y < 256; y += 3) {
            a[p] = (unsigned char)x;
            b[p] = (unsigned char)y;
            /* The bounded form has its own reference: past the bound the tails may differ
             * (tr_TR folds "tail" and "TAIL" apart at i/I). */
            int want = ref(a, b, (size_t)-1, loc), wn = ref(a, b, p + 1, loc);
            int got = sign(f_cmp((const char *)a, (const char *)b));
            int gn = sign(f_ncmp((const char *)a, (const char *)b, p + 1));
            if (got != want || gn != wn) {
                if (bad++ < 5)
                    printf("FAIL: %s (uselocale) bytes %#x/%#x: strcasecmp %d (want %d) strncasecmp %d (want %d)\n", name, x, y, got, want, gn, wn);
            }
        }
    }
    uselocale(old);
    freelocale(loc);
    if (bad)
        failures++;
    printf("%s: non-ASCII folds below 0x80: %d, results %016lx%s\n", name, nonascii, hash, bad ? " FAILED" : "");
}

int main(void) {
    const char *names[] = {"C", "C.UTF-8", "en_US.UTF-8", "de_DE.ISO-8859-1", "tr_TR.UTF-8", "tr_TR.ISO-8859-9"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++)
        run(names[i]);
    printf("%s\n", failures ? "FAILED" : "ok");
    return failures != 0;
}
