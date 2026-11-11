/* Locale and text (phase 7): classification, case mapping, multibyte decoding, collation, the
 * radix lookup of strtod, locale objects and iconv throughput, in C and C.UTF-8. The same file
 * builds against the products (`cargo xtask bench`) and against the host C library
 * (`cc -O2 bench/c/cbench_locale.c`), so benchmarks/progress.md compares identical code.
 * Prints `name<TAB>ns/op` lines, the best of several rounds; stdio arrives in phase 8, so the
 * output is formatted by hand. */
#define _GNU_SOURCE
#include <ctype.h>
#include <fnmatch.h>
#include <iconv.h>
#include <langinfo.h>
#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <time.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>
#include "cbench_counters.h"

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        ctr_begin();                                       \
        for (int round = 0; round < 7; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        ctr_end(name, 7.0 * (double)(iters));              \
        put(name, best);                                   \
    } while (0)

/* Through volatile pointers, so that each call is made (no inlining of header macros, no
 * hoisting of calls the headers declare pure). */
static int (*volatile isalpha_fn)(int) = isalpha;
static int (*volatile toupper_fn)(int) = toupper;
static wint_t (*volatile towupper_fn)(wint_t) = towupper;
static int (*volatile iswalpha_fn)(wint_t) = iswalpha;
static int (*volatile wcwidth_fn)(wchar_t) = wcwidth;
static size_t (*volatile mbrtowc_fn)(wchar_t *, const char *, size_t, mbstate_t *) = mbrtowc;
static int (*volatile strcoll_fn)(const char *, const char *) = strcoll;
static int (*volatile snprintf_fn)(char *, size_t, const char *, ...) = snprintf;
static double (*volatile strtod_fn)(const char *, char **) = strtod;

static int (*volatile strcasecmp_fn)(const char *, const char *) = strcasecmp;
static int (*volatile strncasecmp_fn)(const char *, const char *, size_t) = strncasecmp;
static int (*volatile fnmatch_fn)(const char *, const char *, int) = fnmatch;
static char *(*volatile nl_langinfo_fn)(nl_item) = nl_langinfo;
static struct lconv *(*volatile localeconv_fn)(void) = localeconv;
static size_t (*volatile strftime_fn)(char *, size_t, const char *, const struct tm *) = strftime;
static int (*volatile iswalpha_l_fn)(wint_t, locale_t) = iswalpha_l;
static wint_t (*volatile towupper_l_fn)(wint_t, locale_t) = towupper_l;
static int (*volatile iswctype_fn)(wint_t, wctype_t) = iswctype;
static char *(*volatile nl_langinfo_l_fn)(nl_item, locale_t) = nl_langinfo_l;
static wint_t (*volatile towctrans_fn)(wint_t, wctrans_t) = towctrans;
static wint_t (*volatile towlower_fn)(wint_t) = towlower;

/* The towctrans rows' check (BENCH-ROWS-SINCOS-TOWCTRANS), run once before they are timed: for every code
 * point and WEOF, towctrans with the "toupper" and "tolower" descriptors must equal towupper and towlower
 * of the same library (probed on glibc: no difference in C and C.UTF-8). A difference ends the program; the
 * FNV-1a digest of the mapped values goes to stderr, so the Liella and glibc builds can be compared. */
static void check_towctrans(wctrans_t up, wctrans_t lo)
{
    static const char bad[] = "cbench_locale: towctrans identity check failed (towctrans differs from towupper/towlower)\n";
    if (up == (wctrans_t)0 || lo == (wctrans_t)0) {
        (void)!write(2, bad, sizeof bad - 1);
        exit(1);
    }
    unsigned long long h = 0xcbf29ce484222325ULL;
    for (wint_t c = 0;; c++) {
        wint_t x = c > 0x10FFFF ? WEOF : c;
        wint_t u = towctrans_fn(x, up), l = towctrans_fn(x, lo);
        if (u != towupper_fn(x) || l != towlower_fn(x)) {
            (void)!write(2, bad, sizeof bad - 1);
            exit(1);
        }
        h = (h ^ u) * 0x100000001b3ULL;
        h = (h ^ l) * 0x100000001b3ULL;
        if (x == WEOF)
            break;
    }
    fprintf(stderr, "cbench_locale: check towctrans_utf8 digest %#llx\n", h);
}
static const struct tm tm_fixed = { .tm_mday = 5, .tm_mon = 3, .tm_wday = 2, .tm_year = 120 };

/* Case-insensitive pairs (PJ-23): equal apart from case, so every byte is folded.
 * BENCH-CASECMP-ALIGN: the operands sit at fixed page offsets in a page-aligned block, so a change in
 * the data size around them cannot move one across a 4 KiB page boundary (an unaligned 257-byte array
 * drifted to page offset 0xf00/0xf60 and changed the strcasecmp_*_256 rows with no code change).
 * case_a is at offset 0 of page 0, case_b at offset 0x800 of page 0: neither crosses a page.
 * case_x holds case_b's bytes at offset 0xf00 of page 1, so its terminator lies on page 2: the
 * strcasecmp_c_256_xpage row measures a page-crossing operand on purpose.
 * BENCH-CASECMP-ALIGN-2: case_a and case_b both start at a 2 KiB multiple, a best or worst case by
 * accident; strcasecmp_c_256_odd compares copies at offsets 0x13 and 0x421 (neither a multiple of 16,
 * mutual offset 0x40e), and strcasecmp_c_256_pageend a copy of case_a starting 16 bytes before a page
 * end (0xff0 of page 2, running onto page 3) against case_b. main prints every operand's page offset. */
static _Alignas(4096) char case_pages[4][4096];
#define case_a (case_pages[0])
#define case_b (case_pages[0] + 0x800)
#define case_x (case_pages[1] + 0xf00)
#define case_o (case_pages[3] + 0x413)
#define case_p (case_pages[0] + 0x421)
#define case_e (case_pages[2] + 0xff0)

/* Page-aligned for the same reason: the iconv rows always touch exactly 1 and 4 whole pages. */
static _Alignas(4096) char text[4096];
static _Alignas(4096) char out[16384];

/* One whole conversion of `text` (4 KiB) through `cd`. */
static long convert(iconv_t cd)
{
    char *ip = text, *op = out;
    size_t il = sizeof text, ol = sizeof out;
    iconv(cd, NULL, NULL, NULL, NULL);
    return (long)iconv(cd, &ip, &il, &op, &ol) + (long)(sizeof out - ol);
}

static long locale_pair(void)
{
    locale_t l = newlocale(LC_ALL_MASK, "C.UTF-8", (locale_t)0);
    freelocale(l);
    return l != (locale_t)0;
}

int main(void)
{
    /* Mixed text: ASCII words with a two-byte and a three-byte character every 16 bytes. */
    for (size_t i = 0; i < sizeof text; i += 16)
        memcpy(text + i, "abcdefg \xc3\xa9 h \xe2\x82\xac.", 16);

    for (size_t i = 0; i < 256; i++) {
        case_a[i] = (char)("abcdefghijklmnopqrstuvwxyz0123456789 .-_"[i % 40]);
        case_b[i] = (i & 1) && case_a[i] >= 'a' && case_a[i] <= 'z' ? (char)(case_a[i] - 32) : case_a[i];
    }
    memcpy(case_x, case_b, 257);
    memcpy(case_o, case_a, 257);
    memcpy(case_p, case_b, 257);
    memcpy(case_e, case_a, 257);
    fprintf(stderr, "cbench_locale: operand page offsets: case_a %#lx case_b %#lx case_x %#lx case_o %#lx case_p %#lx case_e %#lx\n",
            (unsigned long)((uintptr_t)case_a & 4095), (unsigned long)((uintptr_t)case_b & 4095),
            (unsigned long)((uintptr_t)case_x & 4095), (unsigned long)((uintptr_t)case_o & 4095),
            (unsigned long)((uintptr_t)case_p & 4095), (unsigned long)((uintptr_t)case_e & 4095));

    setlocale(LC_ALL, "C");
    MEASURE("isalpha_c", 10000000L, isalpha_fn((int)(i & 127)));
    MEASURE("toupper_c", 10000000L, toupper_fn((int)(i & 127)));
    MEASURE("strcasecmp_c_16", 10000000L, strcasecmp_fn("Hello World 1234", "hELLO wORLD 1234"));
    MEASURE("strcasecmp_c_256", 1000000L, strcasecmp_fn(case_a, case_b));
    MEASURE("strcasecmp_c_256_xpage", 1000000L, strcasecmp_fn(case_a, case_x));
    MEASURE("strcasecmp_c_256_odd", 1000000L, strcasecmp_fn(case_o, case_p));
    MEASURE("strcasecmp_c_256_pageend", 1000000L, strcasecmp_fn(case_e, case_b));
    MEASURE("strncasecmp_c_64", 10000000L, strncasecmp_fn(case_a, case_b, 64));
    MEASURE("strtod_c", 1000000L, strtod_fn("3.14159", NULL) > 3.0);

    setlocale(LC_ALL, "C.UTF-8");
    MEASURE("towupper_utf8", 10000000L, towupper_fn((wint_t)(0xE0 + (i & 31))));
    {
        wctrans_t up = wctrans("toupper"), lo = wctrans("tolower");
        if (!cb_skip("towctrans_toupper_utf8") || !cb_skip("towctrans_tolower_utf8"))
            check_towctrans(up, lo);
        MEASURE("towctrans_toupper_utf8", 10000000L, towctrans_fn((wint_t)(0xE0 + (i & 31)), up));
        MEASURE("towctrans_tolower_utf8", 10000000L, towctrans_fn((wint_t)(0xC0 + (i & 31)), lo));
    }
    MEASURE("iswalpha_utf8", 10000000L, iswalpha_fn((wint_t)(0x4E00 + (i & 255))));
    MEASURE("wcwidth_utf8", 10000000L, wcwidth_fn((wchar_t)(0x4E00 + (i & 255))));
    mbstate_t st;
    memset(&st, 0, sizeof st);
    wchar_t w;
    MEASURE("mbrtowc_utf8_1", 10000000L, mbrtowc_fn(&w, "a", 1, &st));
    MEASURE("mbrtowc_utf8_2", 10000000L, mbrtowc_fn(&w, "Ã©", 2, &st));
    MEASURE("mbrtowc_utf8_3", 10000000L, mbrtowc_fn(&w, "\xe2\x82\xac", 3, &st));
    MEASURE("strcoll_utf8_16", 10000000L, strcoll_fn("abcdefg \xc3\xa9 h \xe2\x82\xac!", "abcdefg \xc3\xa9 h \xe2\x82\xac."));
    MEASURE("strtod_utf8", 1000000L, strtod_fn("3.14159", NULL) > 3.0);
    MEASURE("newlocale_free", 100000L, locale_pair());
    MEASURE("strcasecmp_utf8_256", 1000000L, strcasecmp_fn(case_a, case_b));

    /* A compiled locale (its tables come from a file); skipped where it is not installed. */
    if (setlocale(LC_ALL, "en_US.UTF-8") != NULL) {
        MEASURE("strcasecmp_en_256", 1000000L, strcasecmp_fn(case_a, case_b));
        MEASURE("fnmatch_casefold_en_32", 1000000L, fnmatch_fn("*hello*WORLD*[a-z]?.TXT", "My Hello big world file x1.txt", FNM_CASEFOLD));
        /* LV-0: the same calls as their C / C.UTF-8 twins, but in a compiled locale, where each call
         * used to pay for working out the locale's encoding and numeric items. */
        memset(&st, 0, sizeof st);
        /* U+20AC as escapes (MB-K2: the literal used to be the euro sign's bytes read as Latin-1 and
         * stored again as UTF-8, so this row decoded the 2-byte U+00E2). */
        MEASURE("mbrtowc_en_us_3", 10000000L, mbrtowc_fn(&w, "\xe2\x82\xac", 3, &st));
        MEASURE("iswalpha_en_us", 10000000L, iswalpha_fn((wint_t)(0x4E00 + (i & 255))));
        MEASURE("strtod_en_us", 1000000L, strtod_fn("3.14159", NULL) > 3.0);
        MEASURE("snprintf_f_en_us", 500000L, snprintf_fn(out, 64, "%.2f", (double)i * 1.37));
        MEASURE("fnmatch_en_us", 1000000L, fnmatch_fn("*.c", "cbench_patterns.c", 0));
        /* LV-3: the item tables (D-7.13): nl_langinfo, localeconv and the LC_TIME names. */
        MEASURE("nl_langinfo_en_us", 10000000L, nl_langinfo_fn((nl_item)(MON_1 + (i & 7)))[0]);
        MEASURE("localeconv_en_us", 10000000L, localeconv_fn()->decimal_point[0]);
        MEASURE("strftime_names_en_us", 1000000L, strftime_fn(out, 64, "%A %B", &tm_fixed));
        /* LV-4: the wide ctype and langinfo `_l` calls (a locale_t -> its view), and iswctype. */
        locale_t en = newlocale(LC_ALL_MASK, "en_US.UTF-8", (locale_t)0);
        wctype_t alpha = wctype("alpha");
        if (en != (locale_t)0) {
            MEASURE("iswalpha_l_en_us", 10000000L, iswalpha_l_fn((wint_t)(0x4E00 + (i & 255)), en));
            MEASURE("towupper_l_en_us", 10000000L, towupper_l_fn((wint_t)(0xE0 + (i & 31)), en));
            MEASURE("nl_langinfo_l_en_us", 10000000L, nl_langinfo_l_fn((nl_item)(MON_1 + (i & 7)), en)[0]);
        }
        MEASURE("iswctype_en_us", 10000000L, iswctype_fn((wint_t)(0x4E00 + (i & 255)), alpha));
    } else {
        static const char skip[] = "cbench_locale: SKIPPED the en_US.UTF-8 rows (mbrtowc_en_us_3 iswalpha_en_us strtod_en_us snprintf_f_en_us fnmatch_en_us nl_langinfo_en_us localeconv_en_us strftime_names_en_us): "
                                   "locale not installed on this host\n";
        (void)!write(2, skip, sizeof skip - 1);
    }
    setlocale(LC_ALL, "C.UTF-8");

    iconv_t to16 = iconv_open("UTF-16LE", "UTF-8");
    iconv_t to8859 = iconv_open("ISO-8859-15", "UTF-8");
    iconv_t translit = iconv_open("ASCII//TRANSLIT", "UTF-8");
    if (to16 != (iconv_t)-1)
        MEASURE("iconv_utf8_utf16_4k", 20000L, convert(to16));
    if (to8859 != (iconv_t)-1)
        MEASURE("iconv_utf8_8859_15_4k", 20000L, convert(to8859));
    if (translit != (iconv_t)-1)
        MEASURE("iconv_utf8_ascii_translit_4k", 20000L, convert(translit));
    return 0;
}
