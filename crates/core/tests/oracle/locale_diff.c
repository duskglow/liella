/* Differential locale test (phase 7): built once against the host C library and once against
 * Liella's products, run with the same LOCPATH; the two outputs must be identical.
 * stdio is not used (Liella's streams are phase 8): output goes through write(2).
 * argv[1..]: locale names to examine. rpmatch (phase 12, D-7.9; milestone 5) in each. */
#define _GNU_SOURCE
#include <langinfo.h>
#include <locale.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>
#include <ctype.h>

static char out[1 << 16];
static size_t used;

static void flush(void) { if (used) { (void)!write(1, out, used); used = 0; } }
static void ch(char c) { if (used == sizeof out) flush(); out[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void hexb(unsigned char b) { const char *d = "0123456789abcdef"; ch(d[b >> 4]); ch(d[b & 15]); }
static void hexs(const char *s) { for (; *s; s++) hexb((unsigned char)*s); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void hexn(unsigned long v) {
    char b[20]; int n = 0; const char *d = "0123456789abcdef";
    do { b[n++] = d[v & 15]; v >>= 4; } while (v);
    while (n) ch(b[--n]);
}
static void nl(void) { ch('\n'); }

static const char *names[] = {
    "C", "POSIX", "C.UTF-8", "C.utf8", "", "en_US.UTF-8", "en_US.utf8", "en_US", "de_DE.UTF-8", "de_DE.UTF-8@euro",
    "xx_YY.UTF-8", "en_US.UTF-8/../x", "french", "LC_CTYPE=C;LC_NUMERIC=de_DE.UTF-8", NULL,
};

static void setlocale_names(void) {
    for (int i = 0; names[i]; i++) {
        setlocale(LC_ALL, "C");
        const char *r = setlocale(LC_ALL, names[i]);
        str("setlocale ["); str(names[i]); str("] -> "); str(r ? r : "(null)"); nl();
    }
    /* Mixed categories and the composite name round-trip. */
    setlocale(LC_ALL, "C");
    const char *r = setlocale(LC_NUMERIC, "de_DE.UTF-8");
    str("numeric de -> "); str(r ? r : "(null)"); nl();
    r = setlocale(LC_COLLATE, "en_US.UTF-8");
    str("collate en -> "); str(r ? r : "(null)"); nl();
    char saved[1024]; strncpy(saved, setlocale(LC_ALL, NULL), sizeof saved - 1); saved[sizeof saved - 1] = 0;
    str("composite "); str(saved); nl();
    setlocale(LC_ALL, "C");
    r = setlocale(LC_ALL, saved);
    str("restored "); str(r ? r : "(null)"); nl();
    str("query numeric "); str(setlocale(LC_NUMERIC, NULL)); nl();
    setlocale(LC_ALL, "C");
}

static const int items[] = {
    CODESET, RADIXCHAR, THOUSEP, D_T_FMT, D_FMT, T_FMT, T_FMT_AMPM, AM_STR, PM_STR, DAY_1, DAY_7, ABDAY_1,
    MON_1, MON_12, ABMON_1, ALTMON_1, ERA, ALT_DIGITS, YESEXPR, NOEXPR, CRNCYSTR, _DATE_FMT,
    _NL_ADDRESS_POSTAL_FMT, _NL_TELEPHONE_TEL_INT_FMT, _NL_NAME_NAME_FMT, _NL_IDENTIFICATION_LANGUAGE, 0x7fff0000,
};
static const int word_items[] = { _NL_PAPER_HEIGHT, _NL_PAPER_WIDTH, _NL_CTYPE_MB_CUR_MAX, _NL_TIME_WEEK_1STDAY, _NL_COLLATE_NRULES, _NL_NUMERIC_DECIMAL_POINT_WC };

static const int byte_items[] = { _NL_MEASUREMENT_MEASUREMENT, _NL_TIME_WEEK_NDAYS, _NL_TIME_FIRST_WEEKDAY, _NL_TIME_FIRST_WORKDAY, _NL_TIME_CAL_DIRECTION };

static void langinfo(void) {
    for (unsigned i = 0; i < sizeof items / sizeof items[0]; i++) {
        str("  langinfo "); hexn((unsigned)items[i]); str(" = "); hexs(nl_langinfo(items[i])); nl();
    }
    /* Byte items are one byte, not strings. */
    for (unsigned i = 0; i < sizeof byte_items / sizeof byte_items[0]; i++) {
        str("  byte "); hexn((unsigned)byte_items[i]); str(" = "); hexb((unsigned char)*nl_langinfo(byte_items[i])); nl();
    }
    static const int cats[] = { LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES, LC_PAPER, LC_IDENTIFICATION };
    for (unsigned i = 0; i < sizeof cats / sizeof cats[0]; i++) {
        str("  name "); num(cats[i]); str(" = "); str(nl_langinfo(_NL_LOCALE_NAME(cats[i]))); nl();
    }
    for (unsigned i = 0; i < sizeof word_items / sizeof word_items[0]; i++) {
        str("  word "); hexn((unsigned)word_items[i]); str(" = ");
        hexn((unsigned long)(unsigned)(unsigned long)nl_langinfo(word_items[i])); nl();
    }
}

static void lconv_fields(void) {
    struct lconv *l = localeconv();
    const char *s[] = { l->decimal_point, l->thousands_sep, l->grouping, l->int_curr_symbol, l->currency_symbol,
                        l->mon_decimal_point, l->mon_thousands_sep, l->mon_grouping, l->positive_sign, l->negative_sign };
    str("  lconv");
    for (int i = 0; i < 10; i++) { ch(' '); hexs(s[i]); ch('|'); }
    const char c[] = { l->int_frac_digits, l->frac_digits, l->p_cs_precedes, l->p_sep_by_space, l->n_cs_precedes,
                       l->n_sep_by_space, l->p_sign_posn, l->n_sign_posn, l->int_p_cs_precedes, l->int_p_sep_by_space,
                       l->int_n_cs_precedes, l->int_n_sep_by_space, l->int_p_sign_posn, l->int_n_sign_posn };
    for (int i = 0; i < 14; i++) { ch(' '); num(c[i]); }
    nl();
}

static void bytes(void) {
    static const int masks[] = { _ISupper, _ISlower, _ISalpha, _ISdigit, _ISxdigit, _ISspace, _ISprint, _ISgraph, _ISblank, _IScntrl, _ISpunct, _ISalnum };
    str("  classes ");
    for (int c = -128; c < 256; c++) {
        unsigned m = 0;
        for (int k = 0; k < 12; k++) if ((*__ctype_b_loc())[c] & masks[k]) m |= 1u << k;
        hexn(m); ch(',');
    }
    nl();
    str("  isalpha");
    for (int c = -1; c < 256; c++) { ch(isalpha(c) ? '1' : '0'); }
    nl();
    str("  toupper");
    for (int c = -128; c < 256; c++) { ch(' '); num(toupper(c)); }
    nl();
    str("  mb_cur_max "); num((long)MB_CUR_MAX); nl();
}

static const unsigned wide[] = { 0x41, 0x61, 0xdf, 0xe9, 0x130, 0x131, 0x3a3, 0x3c2, 0x1c5, 0x301, 0x4e00, 0x3000, 0xff21,
                                 0x1f600, 0x200b, 0xad, 0x10ffff, 0xd800, 0x110000, 0xffffffffu };

static void wides(void) {
    static const char *cls[] = { "alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space",
                                 "upper", "xdigit", "combining", "combining_level3", "jspace", NULL };
    for (unsigned i = 0; i < sizeof wide / sizeof wide[0]; i++) {
        wint_t c = wide[i];
        str("  wc "); hexn(c); str(":");
        for (int k = 0; cls[k]; k++) { wctype_t t = wctype(cls[k]); ch(t ? (iswctype(c, t) ? '1' : '0') : '-'); }
        str(" up "); hexn(towupper(c)); str(" lo "); hexn(towlower(c));
        wctrans_t ti = wctrans("totitle"); str(" ti "); if (ti) hexn(towctrans(c, ti)); else ch('-');
        str(" w "); num(wcwidth(c)); nl();
    }
}

static void multibyte(void) {
    /* UTF-8, then EUC-JP (JIS X 0208, half-width katakana, JIS X 0212), GB18030 four-byte, and
     * single high bytes: each locale decodes the ones its charset has. */
    static const char *in[] = { "a", "\xc3\xa9", "\xe2\x82\xac", "\xf0\x9f\x98\x80", "\xc3", "\xff", "\xed\xa0\x80", "\xc0\xaf",
                                "\xa4\xa2", "\x8e\xb1", "\x8f\xb0\xa1", "\xa4", "\x81\x30\x81\x30", "\xe9", "\x80", "\xa0", NULL };
    for (int i = 0; in[i]; i++) {
        mbstate_t st; memset(&st, 0, sizeof st); wchar_t w = 0;
        size_t r = mbrtowc(&w, in[i], strlen(in[i]), &st);
        str("  mbrtowc "); hexs(in[i]); str(" -> "); num((long)r); ch(' '); hexn((unsigned)w);
        /* The same bytes one at a time: the state keeps the incomplete prefix. */
        memset(&st, 0, sizeof st); w = 0;
        str(" bytewise");
        for (size_t k = 0; in[i][k]; k++) {
            size_t q = mbrtowc(&w, in[i] + k, 1, &st);
            ch(' '); num((long)q);
            if (q != (size_t)-2) break;
        }
        ch(' '); hexn((unsigned)w);
        str(" mblen "); num(mblen(in[i], strlen(in[i]))); mblen(NULL, 0);
        nl();
    }
    static const unsigned out[] = { 0x41, 0xe9, 0x20ac, 0x3042, 0xff71, 0x4e02, 0x430, 0x1f600, 0 };
    for (int i = 0; out[i]; i++) {
        char buf[16]; mbstate_t st; memset(&st, 0, sizeof st);
        size_t r = wcrtomb(buf, out[i], &st);
        str("  wcrtomb "); hexn(out[i]); str(" -> "); num((long)r);
        if (r != (size_t)-1) { ch(' '); for (size_t k = 0; k < r; k++) hexb((unsigned char)buf[k]); }
        nl();
    }
    char s[64]; wchar_t ws[16];
    size_t n = mbstowcs(ws, "a\xa4\xa2\xe9z", 16);
    str("  mbstowcs -> "); num((long)n); nl();
    const wchar_t wsrc[] = { 0x61, 0x3042, 0xe9, 0 };
    n = wcstombs(s, wsrc, sizeof s);
    str("  wcstombs -> "); num((long)n); if (n != (size_t)-1) { ch(' '); for (size_t k = 0; k < n; k++) hexb((unsigned char)s[k]); } nl();
    str("  btowc"); for (int c = 0x7e; c < 0x100; c += 0x11) { ch(' '); hexn((unsigned)btowc(c)); } nl();
    str("  wctob"); for (int i = 0; out[i]; i++) { ch(' '); num(wctob(out[i])); } nl();
}

static int cmp(const void *a, const void *b) { return strcoll(*(const char *const *)a, *(const char *const *)b); }

static void collation(void) {
    const char *words[] = { "b", "a", "A", "B", "ä", "Ä", "z", "é", "e", "E", "ab", "a b", "a-b", "Ab", "aB", "1", "10", "2", "-", " ", "ß", "ss", "日本", "co-op", "coop", "Coop", "zebra", "Zebra", "" };
    const int n = sizeof words / sizeof words[0];
    const char *sorted[64]; memcpy(sorted, words, sizeof words);
    qsort(sorted, n, sizeof sorted[0], cmp);
    str("  sorted");
    for (int i = 0; i < n; i++) { ch(' '); hexs(sorted[i]); ch(';'); }
    nl();
    for (int i = 0; i < 6; i++) {
        char key[256]; size_t k = strxfrm(key, words[i * 4], sizeof key);
        str("  xfrm "); hexs(words[i * 4]); str(" "); num((long)k); ch(' ');
        for (size_t j = 0; j < k && j < sizeof key; j++) hexb((unsigned char)key[j]);
        nl();
    }
    wchar_t wa[] = { 0xe4, 0 }, wb[] = { 0x62, 0 };
    str("  wcscoll "); num(wcscoll(wa, wb) < 0 ? -1 : wcscoll(wa, wb) > 0); nl();
    str("  strcasecmp "); num(strcasecmp("\xc9t\xe9", "\xe9T\xc9") == 0); nl();
}

/* The radix of strtod and wcstod (D-8.3): the value's bits and the characters used. */
static void numbers(void) {
    static const char *in[] = { "1.5", "1,5", "1\xd9\xab" "5", "-0x1,8p1", "-0x1.8p1", "2,5e3", "7", NULL };
    for (int i = 0; in[i]; i++) {
        char *end; double v = strtod(in[i], &end);
        union { double d; unsigned long u; } b = { v };
        str("  strtod "); hexs(in[i]); str(" -> "); hexn(b.u); ch(' '); num(end - in[i]);
        wchar_t w[16]; size_t n = mbstowcs(w, in[i], 16);
        if (n != (size_t)-1) {
            wchar_t *wend; double wv = wcstod(w, &wend);
            union { double d; unsigned long u; } wb = { wv };
            str(" wcstod "); hexn(wb.u); ch(' '); num(wend - w);
        }
        nl();
    }
}

/* rpmatch (spec §7.6): each response against the locale's YESEXPR/NOEXPR. The responses are
 * bytes: UTF-8 words and single bytes of the 8-bit and EUC charsets alike. */
static void responses(void) {
    static const char *in[] = { "y", "Y", "yes", "n", "N", "no", "j", "ja", "nein", "o", "oui", "non", "e", "evet",
                                "h", "hay\xc4\xb1r", "\xd0\xb4", "\xd0\xb4\xd0\xb0", "\xd0\xbd", "\xc4", "\xce",
                                "\xe6\x98\xaf", "\xe5\x90\xa6", "1", "0", "+", "-", " y", "", "x", "yn", NULL };
    str("  rpmatch");
    for (int i = 0; in[i]; i++) {
        int r = rpmatch(in[i]);
        ch(' '); ch(r == 1 ? 'Y' : r == 0 ? 'N' : '-');
    }
    nl();
}

static void objects(void) {
    locale_t l = newlocale(LC_ALL_MASK, "de_DE.UTF-8", (locale_t)0);
    str("newlocale de "); str(l ? "ok" : "fail"); nl();
    if (l) {
        str("  radix_l "); str(nl_langinfo_l(RADIXCHAR, l)); nl();
        locale_t prev = uselocale(l);
        str("  uselocale prev global "); num(prev == LC_GLOBAL_LOCALE); nl();
        str("  radix in thread "); str(nl_langinfo(RADIXCHAR)); nl();
        str("  isalpha_l e9 "); num(isalpha_l(0xe9, l) != 0); nl();
        locale_t d = duplocale(l);
        uselocale(LC_GLOBAL_LOCALE);
        str("  radix back "); str(nl_langinfo(RADIXCHAR)); nl();
        str("  dup radix "); str(nl_langinfo_l(RADIXCHAR, d)); nl();
        static const char comma[] = "1,5";
        char *end; double v = strtod_l(comma, &end, d);
        str("  strtod_l 1,5 "); num((long)(v * 10)); ch(' '); num(end - comma); nl();
        locale_t m = newlocale(LC_NUMERIC_MASK, "C", d);
        str("  mixed radix "); str(nl_langinfo_l(RADIXCHAR, m)); str(" codeset "); str(nl_langinfo_l(CODESET, m)); nl();
        freelocale(m); freelocale(l);
    }
    locale_t bad = newlocale(LC_ALL_MASK, "xx_YY.UTF-8", (locale_t)0);
    str("newlocale unknown "); str(bad ? "ok" : "fail"); nl();
    locale_t badmask = newlocale(1 << 30, "C", (locale_t)0);
    str("newlocale bad mask "); str(badmask ? "ok" : "fail"); nl();
}

int main(int argc, char **argv) {
    setlocale_names();
    objects();
    for (int i = 1; i < argc; i++) {
        setlocale(LC_ALL, "C");
        const char *r = setlocale(LC_ALL, argv[i]);
        str("== "); str(argv[i]); str(" -> "); str(r ? r : "(null)"); nl();
        if (!r) continue;
        langinfo();
        lconv_fields();
        bytes();
        wides();
        multibyte();
        collation();
        numbers();
        responses();
    }
    flush();
    return 0;
}
