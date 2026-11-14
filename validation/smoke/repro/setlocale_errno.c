/* validation/smoke/repro/setlocale_errno.c: AQOURS-SMOKE finding 3 (spec section 7, setlocale), from bash's
   test run-glob-test: bash reports a failed setlocale with strerror(errno) when errno is not 0.
   Clears errno, asks for a locale that is not installed, prints setlocale's result and errno.

   Build and run as validation/smoke/repro/regcomp_nesting.c says. */
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    const char *name = argc > 1 ? argv[1] : "zh_HK.big5hkscs";
    errno = 0;
    char *r = setlocale(LC_ALL, name);
    int err = errno;
    printf("setlocale(LC_ALL, \"%s\") %s errno %d (%s)\n", name, r ? "ok" : "NULL", err, err ? strerror(err) : "none");
    return 0;
}
