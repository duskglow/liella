/* For every code point: the 12 standard classes (bit mask), towupper, towlower, wcwidth. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <wchar.h>
#include <wctype.h>
int main(int argc, char **argv) {
    if (!setlocale(LC_ALL, argv[1])) return 1;
    const char *names[] = {"upper","lower","alpha","digit","xdigit","space","print","graph","blank","cntrl","punct","alnum","combining","combining_level3"};
    wctype_t t[14]; for (int i = 0; i < 14; i++) t[i] = wctype(names[i]);
    wctrans_t title = wctrans("totitle");
    for (unsigned c = 0; c < 0x110000; c++) {
        unsigned mask = 0; for (int i = 0; i < 14; i++) if (t[i] && iswctype(c, t[i])) mask |= 1u << i;
        printf("%x %x %x %x %d %x\n", c, mask, (unsigned)towupper(c), (unsigned)towlower(c), wcwidth(c), title ? (unsigned)towctrans(c, title) : c);
    }
    (void)argc; return 0;
}
