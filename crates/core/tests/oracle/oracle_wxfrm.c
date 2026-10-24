/* Reads UTF-8 lines, converts to wide (mbstowcs, locale charset), prints the wcsxfrm key words in hex. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
int main(int argc, char **argv) {
    if (!setlocale(LC_ALL, argv[1])) return 1;
    char line[4096]; static wchar_t w[4096], key[65536];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        size_t m = mbstowcs(w, line, 4096);
        if (m == (size_t)-1) { printf("invalid\n"); continue; }
        size_t n = wcsxfrm(key, w, 65536);
        for (size_t i = 0; i < n; i++) printf("%s%x", i ? "," : "", (unsigned)key[i]);
        printf("\n");
    }
    (void)argc; return 0;
}
