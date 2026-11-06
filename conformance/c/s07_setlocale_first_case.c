/* §7.1, §7.2, §5.2, §5.3, Appendix D.1 (s7_1_c_0, s7_2_c_0, s5_3_c_0), COLDSTART-SETLOCALE-FIRST:
 * the first setlocale(LC_ALL, "en_US.UTF-8") of a process (no ctype or case function used before
 * it) loads the locale from the system archive, and the ctype and case functions, first used
 * after it, answer from the archive's LC_CTYPE (not from the "C" tables): ASCII letters, Latin-1
 * letters through the wide functions, a Greek and a Turkish letter, MB_CUR_MAX and the codeset.
 * A second setlocale updates the answers and the reported names (whole locale back to "C",
 * then en_US.UTF-8 again, then a composite with LC_CTYPE alone set to "C").
 *
 * The expected values are the Unicode simple case mappings (UnicodeData.txt) that the en_US.UTF-8
 * locale source uses; the composite name form is the one §7.1 specifies. The program prints
 * nothing on success and gives the same results under glibc 2.39 (black-box run of this
 * program). A host without en_US.UTF-8 installed (`locale -a`) is reported SKIPPED. */
/* RUN-ENV: PATH=/usr/bin:/bin */
#include <ctype.h>
#include <langinfo.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wctype.h>

static int failures;

static void check(int ok, const char *what) {
    if (!ok) {
        printf("FAIL: %s\n", what);
        failures++;
    }
}

static int installed(void) {
    return system("locale -a 2>/dev/null | grep -qix 'en_US.utf-\\{0,1\\}8'") == 0;
}

int main(void) {
    /* Nothing locale-dependent runs before this call. */
    const char *r = setlocale(LC_ALL, "en_US.UTF-8");
    if (!r) {
        if (!installed()) {
            printf("SKIPPED: en_US.UTF-8 is not installed on this host\n");
            return 0;
        }
        printf("FAIL: setlocale(LC_ALL, \"en_US.UTF-8\") returned NULL\n");
        return 1;
    }
    check(strcmp(r, "en_US.UTF-8") == 0, "first setlocale returns the name");
    check(toupper('a') == 'A' && tolower('Q') == 'q' && isalpha('z') && !isalpha('1'), "ASCII case and class");
    check(towupper(0xe9) == 0xc9 && towlower(0xc9) == 0xe9, "towupper/towlower of e-acute");
    check(iswalpha(0xe9) && iswlower(0xe9) && iswupper(0xc9), "iswalpha/iswlower/iswupper of Latin-1 letters");
    check(towupper(0x3c3) == 0x3a3 && towlower(0x3a3) == 0x3c3, "Greek sigma case pair");
    check(towlower(0x130) == 0x69 && towupper('i') == 'I' && towlower('I') == 'i', "dotted capital I and ASCII i");
    check(!isalpha(0xe9) && !isupper(0xc9), "bytes above 0x7f are not letters in UTF-8");
    check(MB_CUR_MAX == 6, "MB_CUR_MAX is 6");
    check(strcmp(nl_langinfo(CODESET), "UTF-8") == 0, "CODESET is UTF-8");

    r = setlocale(LC_ALL, "C");
    check(r && strcmp(r, "C") == 0, "second setlocale to C returns C");
    check(strcmp(setlocale(LC_ALL, NULL), "C") == 0 && strcmp(setlocale(LC_CTYPE, NULL), "C") == 0, "query after C");
    check(towupper(0xe9) == 0xe9 && !iswalpha(0xe9) && toupper('a') == 'A', "C answers after the second setlocale");
    check(MB_CUR_MAX == 1, "MB_CUR_MAX is 1 in C");

    r = setlocale(LC_ALL, "en_US.UTF-8");
    check(r && strcmp(r, "en_US.UTF-8") == 0, "third setlocale returns en_US.UTF-8");
    check(towupper(0xe9) == 0xc9, "archive answers again");

    r = setlocale(LC_CTYPE, "C");
    check(r && strcmp(r, "C") == 0, "LC_CTYPE alone to C");
    const char *all = setlocale(LC_ALL, NULL);
    check(all && strstr(all, "LC_CTYPE=C;") && strstr(all, "LC_NUMERIC=en_US.UTF-8"), "composite name after LC_CTYPE=C");
    check(strcmp(setlocale(LC_TIME, NULL), "en_US.UTF-8") == 0, "LC_TIME keeps en_US.UTF-8");
    check(towupper(0xe9) == 0xe9, "C ctype answers in the composite");

    if (failures)
        printf("%d failure(s)\n", failures);
    return failures != 0;
}
