/* LANGINFO-DIFF: print every nl_langinfo_l item (categories 0..11, items 0..159) for C, POSIX, C.UTF-8 and en_US.UTF-8 */
#define _GNU_SOURCE
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

/* a word item comes back as a number in the pointer: probe readability with write(2) instead of faulting */
static int pfd[2];
static int readable(const char *p)
{
    char c;
    if ((unsigned long)p < 65536UL) return 0;
    if (syscall(SYS_write, pfd[1], p, 1) != 1) return 0;
    return read(pfd[0], &c, 1) == 1;
}

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    if (pipe(pfd)) return 2;
    const char *names[] = { "C", "POSIX", "C.UTF-8", "en_US.UTF-8" };
    for (int k = 0; k < 4; k++) {
        locale_t l = newlocale(LC_ALL_MASK, names[k], (locale_t)0);
        if (!l) { printf("%s: no locale\n", names[k]); continue; }
        for (int cat = 0; cat < 12; cat++) {
            for (int i = 0; i < 160; i++) {
                const char *s = nl_langinfo_l((nl_item)((cat << 16) | i), l);
                printf("%s %d.%d ", names[k], cat, i);
                if (!readable(s)) { printf("N %lu\n", (unsigned long)s); continue; }
                /* tables (ctype/collate) may be binary: show at most 24 bytes, escaped */
                printf("S [");
                for (int q = 0; q < 24 && s[q]; q++) {
                    unsigned char ch = (unsigned char)s[q];
                    if (ch >= 32 && ch < 127 && ch != '\\') putchar(ch); else printf("\\x%02x", ch);
                }
                printf("] len=%zu\n", strnlen(s, 4096));
            }
        }
    }
    return 0;
}
