/* wl_percall locale set (PERCALL-LOCALE): the locale-sensitive calls of text tools (grep, sed, sort, ls, wc,
 * printf), run in the requested locale (`wl_percall.sh --spec validation/percall/locale.c --locale L`; default
 * en_US.UTF-8, compare with --locale C). Typical-call counts are rough orders of magnitude for a text workload.
 * Inputs: an ASCII byte, a 2-byte UTF-8 character (U+00E9), a short mixed string. */
#include "percall.h"
#include <ctype.h>
#include <iconv.h>
#include <regex.h>
#include <wchar.h>
#include <wctype.h>

CASE(mbrtowc_ascii, 1000000, 1) {
    const char *volatile s = "a"; wchar_t w; mbstate_t st; memset(&st, 0, sizeof st);
    for (long i = 0; i < n; i++) pc_sink += (long)mbrtowc(&w, s, 4, &st);
    PC_CHECKING { long r = (long)mbrtowc(&w, s, 4, &st); pc_check("%ld %ld", r, (long)w); }
}
CASE(mbrtowc_2byte, 1000000, 1) {
    const char *volatile s = "\xc3\xa9x"; wchar_t w = 0; mbstate_t st; memset(&st, 0, sizeof st);
    for (long i = 0; i < n; i++) { pc_sink += (long)mbrtowc(&w, s, 4, &st); memset(&st, 0, sizeof st); }
    PC_CHECKING { w = 0; long r = (long)mbrtowc(&w, s, 4, &st); pc_check("%ld %ld", r, r > 0 ? (long)w : -9L); }
}
CASE(wcrtomb_2byte, 100000, 1) {
    wchar_t volatile wc = 0xe9; char b[16]; mbstate_t st; memset(&st, 0, sizeof st);
    for (long i = 0; i < n; i++) pc_sink += (long)wcrtomb(b, wc, &st);
    PC_CHECKING { long r = (long)wcrtomb(b, wc, &st); pc_check("%ld %d", r, r > 0 ? (unsigned char)b[0] : -9); }
}
CASE(mbstowcs_short, 100000, 1) {
    const char *volatile s = "h\xc3\xa9llo w\xc3\xb6rld"; wchar_t w[32];
    for (long i = 0; i < n; i++) pc_sink += (long)mbstowcs(w, s, 32);
    PC_CHECKING { long r = (long)mbstowcs(w, s, 32); pc_check("%ld %ld", r, r > 1 ? (long)w[1] : -9L); }
}
CASE(mb_cur_max, 100000, 1) {
    for (long i = 0; i < n; i++) pc_sink += (long)MB_CUR_MAX;
    PC_CHECKING pc_check("%ld", (long)MB_CUR_MAX);
}
CASE(wcwidth_2byte, 1000000, 1) {
    wchar_t volatile wc = 0xe9;
    for (long i = 0; i < n; i++) pc_sink += wcwidth(wc);
    PC_CHECKING pc_check("%d", wcwidth(wc));
}
CASE(towupper_2byte, 1000000, 1) {
    wint_t volatile wc = 0xe9;
    for (long i = 0; i < n; i++) pc_sink += (long)towupper(wc);
    PC_CHECKING pc_check("%ld %d", (long)towupper(wc), iswalpha(wc) != 0);
}
CASE(iswalpha_2byte, 1000000, 1) {
    wint_t volatile wc = 0xe9;
    for (long i = 0; i < n; i++) pc_sink += iswalpha(wc);
    PC_CHECKING pc_check("%d", iswalpha(wc) != 0);
}
CASE(toupper_ascii, 1000000, 1) {
    int volatile c = 'q';
    for (long i = 0; i < n; i++) pc_sink += toupper(c) + isalpha(c);
    PC_CHECKING pc_check("%d %d", toupper(c), isalpha(c) != 0);
}
CASE(strxfrm_short, 10000, 1) {
    const char *volatile s = "some_file_name.h"; char b[256];
    for (long i = 0; i < n; i++) pc_sink += (long)strxfrm(b, s, sizeof b);
    PC_CHECKING { /* the transformed bytes are private to each library; only their order must agree */
        char x[256], y[256]; const char *volatile p = "a", *volatile q = "B";
        strxfrm(x, p, sizeof x); strxfrm(y, q, sizeof y); pc_check("%d", PC_SIGN(strcmp(x, y)));
    }
}
CASE(printf_ls, 100000, 1) {
    const wchar_t *volatile ws = L"hello"; char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%ls", ws);
    PC_CHECKING { int r = snprintf(b, sizeof b, "%ls", ws); pc_check("%d %s", r, b); }
}
CASE(printf_lc, 100000, 1) {
    wint_t volatile wc = 0xe9; char b[64];
    for (long i = 0; i < n; i++) pc_sink += snprintf(b, sizeof b, "%lc", wc);
    PC_CHECKING { int r = snprintf(b, sizeof b, "%lc", wc); pc_check("%d", r); }
}
CASE(regexec_short, 100000, 1) {
    regex_t re; const char *volatile s = "some_file_42";
    if (regcomp(&re, "^[a-z_]+[0-9]+$", REG_EXTENDED | REG_NOSUB)) return;
    for (long i = 0; i < n; i++) pc_sink += regexec(&re, s, 0, NULL, 0);
    PC_CHECKING pc_check("%d", regexec(&re, s, 0, NULL, 0));
    regfree(&re);
}
CASE(iconv_short, 10000, 1) {
    iconv_t cd = iconv_open("UTF-8", "ISO-8859-1");
    if (cd == (iconv_t)-1) return;
    char in[] = "h\xe9llo", out[32];
    for (long i = 0; i < n; i++) {
        char *ip = in, *op = out; size_t il = 5, ol = sizeof out;
        pc_sink += (long)iconv(cd, &ip, &il, &op, &ol);
    }
    PC_CHECKING {
        char *ip = in, *op = out; size_t il = 5, ol = sizeof out;
        long r = (long)iconv(cd, &ip, &il, &op, &ol); pc_check("%ld %ld", r, (long)(sizeof out - ol));
    }
    iconv_close(cd);
}
