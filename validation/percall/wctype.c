/* wl_percall wide ctype set (WCTYPE-LOCALE): iswctype / isw* / tow* / wcwidth in the requested locale
 * (`wl_percall.sh --spec validation/percall/wctype.c`; default en_US.UTF-8). Inputs cycle over ASCII letters
 * (the common case of text tools in a UTF-8 locale), Latin-1 letters (U+00E0..U+00FF) and CJK (U+4E00..). */
#include "percall.h"
#include <wchar.h>
#include <wctype.h>

#define LOOP(expr) for (long i = 0; i < n; i++) pc_sink += (long)(expr)

CASE(iswctype_cjk, 100000, 1) {
    wctype_t volatile d = wctype("alpha"); wctype_t t = d;
    LOOP(iswctype((wint_t)(0x4E00 + (i & 255)), t));
    PC_CHECKING { pc_check("%d %d", iswctype(0x4E00, t) != 0, iswctype(0x3000, t) != 0); }
}
CASE(iswctype_ascii, 1000000, 1) {
    wctype_t volatile d = wctype("upper"); wctype_t t = d;
    LOOP(iswctype((wint_t)(0x40 + (i & 31)), t));
    PC_CHECKING { pc_check("%d %d", iswctype(L'A', t) != 0, iswctype(L'a', t) != 0); }
}
CASE(iswalpha_latin1, 1000000, 1) {
    LOOP(iswalpha((wint_t)(0xE0 + (i & 31))));
    PC_CHECKING { pc_check("%d %d", iswalpha(0xE9) != 0, iswalpha(0xD7) != 0); }
}
CASE(iswalpha_ascii, 1000000, 1) {
    LOOP(iswalpha((wint_t)(0x40 + (i & 31))));
    PC_CHECKING { pc_check("%d %d", iswalpha(L'q') != 0, iswalpha(L'@') != 0); }
}
CASE(towupper_latin1, 1000000, 1) {
    LOOP(towupper((wint_t)(0xE0 + (i & 31))));
    PC_CHECKING { pc_check("%ld %ld", (long)towupper(0xE9), (long)towupper(0xFF)); }
}
CASE(towupper_ascii, 1000000, 1) {
    LOOP(towupper((wint_t)(0x60 + (i & 31))));
    PC_CHECKING { pc_check("%ld", (long)towupper(L'q')); }
}
CASE(towlower_cjk, 100000, 1) {
    LOOP(towlower((wint_t)(0x4E00 + (i & 255))));
    PC_CHECKING { pc_check("%ld", (long)towlower(0x4E01)); }
}
CASE(towupper_l_latin1, 100000, 1) {
    locale_t volatile lv = newlocale(LC_ALL_MASK, "", (locale_t)0); locale_t l = lv;
    LOOP(towupper_l((wint_t)(0xE0 + (i & 31)), l));
    PC_CHECKING { pc_check("%ld", (long)towupper_l(0xE9, l)); }
    freelocale(l);
}
CASE(iswalpha_l_cjk, 100000, 1) {
    locale_t volatile lv = newlocale(LC_ALL_MASK, "", (locale_t)0); locale_t l = lv;
    LOOP(iswalpha_l((wint_t)(0x4E00 + (i & 255)), l));
    PC_CHECKING { pc_check("%d", iswalpha_l(0x4E00, l) != 0); }
    freelocale(l);
}
CASE(wcwidth_latin1, 1000000, 1) {
    LOOP(wcwidth((wchar_t)(0xE0 + (i & 31))));
    PC_CHECKING { pc_check("%d %d", wcwidth(0xE9), wcwidth(0xAD)); }
}
