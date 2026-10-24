/* §8.2, §8.3 (D-8.1 compat; STRTO-INTERNAL-GROUP-4, #2069): the C-level float compat entry
 * points `__strtod_internal`, `__strtold_internal` and `__wcstold_internal` hand their `group`
 * argument through to the parse (the long double ones go through csys/src/adapter/float.c, which
 * the Rust-side test strto_internal_group.rs cannot reach).
 *
 * Behaviour settled by the black-box probe crates/core/tests/oracle/strto_group_table.c against
 * glibc 2.39 (record: crates/boundary/tests/data/strto_internal_group.txt): with thousands_sep
 * `,` and grouping 3;3, "1,234,567" with group=1 gives 1234567 (end 9); with group=0, 1 (end 1).
 *
 * The locale is the test's own: an LC_NUMERIC-only definition with en_US's numeric values
 * (decimal_point `.`, thousands_sep `,`, grouping 3;3), compiled with `localedef -c` into a
 * private directory as en_US.UTF-8 and named by LOCPATH; no installed locale is read. */
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/* The compat exports have no prototype in the 2.39 headers. */
extern double __strtod_internal(const char *, char **, int);
extern long double __strtold_internal(const char *, char **, int);
extern long double __wcstold_internal(const wchar_t *, wchar_t **, int);

static char dir[256];
static int failures;

static void cleanup(void) {
    if (dir[0]) {
        char cmd[400];
        snprintf(cmd, sizeof cmd, "rm -rf '%s'", dir);
        if (system(cmd) == -1) {
            printf("note: could not remove %s\n", dir);
        }
    }
}

static void prepare(void) {
    const char *tmp = getenv("TMPDIR");
    snprintf(dir, sizeof dir, "%s/s08-strto-group-XXXXXX", tmp && *tmp ? tmp : "/tmp");
    if (!mkdtemp(dir)) {
        dir[0] = 0;
        printf("FAIL: mkdtemp\n");
        exit(1);
    }
    atexit(cleanup);
    char def[300];
    snprintf(def, sizeof def, "%s/numeric.def", dir);
    FILE *f = fopen(def, "w");
    if (!f) {
        printf("FAIL: writing %s\n", def);
        exit(1);
    }
    fputs("comment_char %\nescape_char /\nLC_NUMERIC\ndecimal_point \"<U002E>\"\nthousands_sep \"<U002C>\"\ngrouping 3;3\nEND LC_NUMERIC\n", f);
    fclose(f);
    char cmd[800];
    snprintf(cmd, sizeof cmd, "localedef -c -f UTF-8 -i '%s' '%s/en_US.UTF-8' >/dev/null 2>&1", def, dir);
    if (system(cmd) == -1 || setenv("LOCPATH", dir, 1) != 0 || !setlocale(LC_NUMERIC, "en_US.UTF-8")) {
        printf("FAIL: compiling and loading the test's en_US.UTF-8 LC_NUMERIC (localedef)\n");
        exit(1);
    }
}

static void check(const char *what, int ok) {
    if (!ok) {
        printf("FAIL: %s\n", what);
        failures++;
    }
}

int main(void) {
    prepare();
    const char *s = "1,234,567";
    const wchar_t *w = L"1,234,567";
    char *e = NULL;
    wchar_t *we = NULL;

    double d = __strtod_internal(s, &e, 1);
    check("__strtod_internal group=1 value", d == 1234567.0);
    check("__strtod_internal group=1 end", e == s + 9);
    d = __strtod_internal(s, &e, 0);
    check("__strtod_internal group=0 value", d == 1.0);
    check("__strtod_internal group=0 end", e == s + 1);

    long double ld = __strtold_internal(s, &e, 1);
    check("__strtold_internal group=1 value", ld == 1234567.0L);
    check("__strtold_internal group=1 end", e == s + 9);
    ld = __strtold_internal(s, &e, 0);
    check("__strtold_internal group=0 value", ld == 1.0L);
    check("__strtold_internal group=0 end", e == s + 1);

    ld = __wcstold_internal(w, &we, 1);
    check("__wcstold_internal group=1 value", ld == 1234567.0L);
    check("__wcstold_internal group=1 end", we == w + 9);
    ld = __wcstold_internal(w, &we, 0);
    check("__wcstold_internal group=0 value", ld == 1.0L);
    check("__wcstold_internal group=0 end", we == w + 1);

    setlocale(LC_NUMERIC, "C");
    if (failures) {
        return 1;
    }
    printf("PASS\n");
    return 0;
}
