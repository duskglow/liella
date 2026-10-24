/* Reads lines (UTF-8 or the locale's charset), prints the strxfrm key of each in hex. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
    if (!setlocale(LC_ALL, argv[1])) return 1;
    char line[4096]; static char key[65536];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        size_t n = strxfrm(key, line, sizeof key);
        for (size_t i = 0; i < n; i++) printf("%02x", (unsigned char)key[i]);
        printf("\n");
    }
    (void)argc; return 0;
}
