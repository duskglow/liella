/* WCTYPE-TRIE-FILE first-touch probe (D-5.10). usage: touch <locale> <mode>
 * mode "all": one iswalpha per block of 256 code points (every block's first touch);
 * mode "one": the same number of calls in block 0x4e (one first touch).
 * The difference of the two runs' user instructions / 4351 is the mean cost of resolving a block. */
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <wctype.h>

int main(int argc, char **argv) {
    if (argc < 3 || !setlocale(LC_ALL, argv[1])) {
        printf("usage: touch <locale> all|one (locale must load)\n");
        return 2;
    }
    int all = strcmp(argv[2], "all") == 0;
    unsigned n = 0;
    for (unsigned b = 0; b < 4352; b++)
        n += iswalpha(all ? (b << 8) | 0x41 : 0x4e00 + (b & 0xff)) != 0;
    printf("%s %s members %u\n", argv[1], argv[2], n);
    return 0;
}
