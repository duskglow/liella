/* LV-4: wide ctype _l calls, iswctype, nl_langinfo edge items, strftime names and fields in en_US.UTF-8 and C */
#define _GNU_SOURCE
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <wchar.h>
#include <wctype.h>
#include <errno.h>
#include <sys/syscall.h>
#include <fcntl.h>
#include <unistd.h>

/* a word item comes back as a number in the pointer: probe readability with write(2) instead of faulting */
static int pfd[2];
static int readable(const char *p) { char c; if ((unsigned long)p < 65536UL) return 0; if (syscall(SYS_write, pfd[1], p, 1) != 1) return 0; return read(pfd[0], &c, 1) == 1; }

static unsigned long long h;
static void mix(unsigned long long v) { h = (h ^ v) * 1099511628211ULL; }

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    if (pipe(pfd)) return 2;
    const char *names[] = { "C", "en_US.UTF-8", "C.UTF-8" };
    for (int k = 0; k < 3; k++) {
        locale_t l = newlocale(LC_ALL_MASK, names[k], (locale_t)0);
        if (!l) { printf("%s: no locale\n", names[k]); continue; }
        h = 1469598103934665603ULL;
        wint_t probes[] = { 0, 'a', 'Z', '5', ' ', 0x7f, 0x80, 0xe0, 0xc9, 0x3b1, 0x3a3, 0x4e00, 0x20ac, 0x10ffff, 0x110000, 0xffffffffu, 0x7fffffffu };
        const char *classes[] = { "alpha", "digit", "upper", "lower", "space", "punct", "alnum", "print", "graph", "cntrl", "xdigit", "blank", "nonesuch" };
        for (unsigned i = 0; i < sizeof probes / sizeof probes[0]; i++) {
            wint_t c = probes[i];
            mix((unsigned)!!iswalpha_l(c, l)); mix((unsigned)!!iswalnum_l(c, l)); mix((unsigned)!!iswblank_l(c, l));
            mix((unsigned)!!iswcntrl_l(c, l)); mix((unsigned)!!iswdigit_l(c, l)); mix((unsigned)!!iswgraph_l(c, l));
            mix((unsigned)!!iswlower_l(c, l)); mix((unsigned)!!iswprint_l(c, l)); mix((unsigned)!!iswpunct_l(c, l));
            mix((unsigned)!!iswspace_l(c, l)); mix((unsigned)!!iswupper_l(c, l)); mix((unsigned)!!iswxdigit_l(c, l));
            mix(towlower_l(c, l)); mix(towupper_l(c, l));
            for (unsigned j = 0; j < sizeof classes / sizeof classes[0]; j++) {
                wctype_t t = wctype_l(classes[j], l);
                mix(t != 0);
                mix((unsigned)!!iswctype_l(c, t, l));
            }
            wctype_t d = wctype_l("alpha", l);
            mix((unsigned)!!iswctype_l(c, d, l));
        }
        printf("%s: wide _l hash %016llx\n", names[k], h);
        /* nl_langinfo_l: every item of every category, plus edge items */
        h = 1469598103934665603ULL;
        int cats[] = { LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES, LC_PAPER, LC_NAME, LC_ADDRESS, LC_TELEPHONE, LC_MEASUREMENT, LC_IDENTIFICATION };
        for (unsigned c = 0; c < sizeof cats / sizeof cats[0]; c++) {
            if (c) printf("%s: cat %d hash %016llx\n", names[k], cats[c - 1], h);
            h = 1469598103934665603ULL;
            for (int i = 0; i < 80; i++) {
                const char *s = nl_langinfo_l((nl_item)((cats[c] << 16) | i), l);
                /* word items come back as small numbers in the pointer; hash those as numbers */
                if (!readable(s)) { mix((unsigned long)s); continue; }
                if (cats[c] != LC_COLLATE && cats[c] != LC_CTYPE) {
                    for (const char *p = s; *p && p - s < 64; p++) mix((unsigned char)*p);
                    mix((unsigned char)s[0]);
                } else if (i < 16) {
                    mix((unsigned char)s[0]);
                }
            }
        }
        printf("%s: cat %d hash %016llx\n", names[k], cats[11], h);
        const char *e1 = nl_langinfo_l((nl_item)-1, l), *e2 = nl_langinfo_l((nl_item)0xffff, l), *e3 = nl_langinfo_l((nl_item)(LC_ALL << 16), l),
                   *e4 = nl_langinfo_l((nl_item)(13 << 16), l), *e5 = nl_langinfo_l((nl_item)0x7fffffff, l), *e6 = nl_langinfo_l((nl_item)(LC_TIME << 16 | 0xffff), l);
        const char *es[] = { e1, e2, e3, e4, e5, e6 };
        printf("%s: langinfo edge", names[k]);
        for (int q = 0; q < 6; q++) {
            if (readable(es[q])) printf(" [%s]", es[q]); else printf(" <%lu>", (unsigned long)es[q]);
        }
        printf("\n");
        /* strftime names and fields */
        char out[256];
        struct tm tm;
        memset(&tm, 0, sizeof tm);
        h = 1469598103934665603ULL;
        for (int wd = -1; wd < 8; wd++)
            for (int mo = -1; mo < 13; mo++) {
                tm.tm_wday = wd; tm.tm_mon = mo; tm.tm_mday = 5; tm.tm_year = 120; tm.tm_hour = 13; tm.tm_min = 7; tm.tm_sec = 9; tm.tm_yday = 95;
                size_t n = strftime_l(out, sizeof out, "%A|%a|%B|%b|%h|%p|%c|%x|%X|%r|%D|%e", &tm, l);
                for (size_t i = 0; i < n; i++) mix((unsigned char)out[i]);
                mix(n);
                size_t m = strftime_l(out, 12, "%A %B", &tm, l);
                mix(m);
                for (size_t i = 0; i < m; i++) mix((unsigned char)out[i]);
            }
        printf("%s: strftime hash %016llx\n", names[k], h);
        freelocale(l);
    }
    return 0;
}
