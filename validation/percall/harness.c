/* wl_percall harness: percall <case> <n> <dir> | percall <case> check <dir> | percall --list | percall --locale */
#include "percall.h"
extern struct pc_case *const __start_pc_cases[], *const __stop_pc_cases[];
volatile long pc_sink;
volatile int pc_check_mode;
void pc_check(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    putchar('\n');
}
/* The case's locale ("" = the environment's), or exit 4: a missing locale must not silently measure "C". */
static const char *pc_locale(const char *want) {
    const char *got = setlocale(LC_ALL, want);
    if (!got) {
        const char *e = getenv("LC_ALL");
        fprintf(stderr, "percall: setlocale(LC_ALL, \"%s\") failed (LC_ALL=%s); locale not installed?\n", want, e ? e : "(unset)");
        exit(4);
    }
    return got;
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--list")) {
        for (struct pc_case *const *p = __start_pc_cases; p < __stop_pc_cases; p++) printf("%s %ld %ld\n", (*p)->name, (*p)->calls, (*p)->per);
        return 0;
    }
    if (argc == 2 && !strcmp(argv[1], "--locale")) {
        const char *l = pc_locale("");
        printf("%s %s\n", l, nl_langinfo(CODESET));
        return 0;
    }
    if (argc < 4) return 2;
    long n = atol(argv[2]);
    if (!strcmp(argv[2], "check")) { pc_check_mode = 1; n = 0; }
    for (struct pc_case *const *p = __start_pc_cases; p < __stop_pc_cases; p++) {
        struct pc_case *c = *p;
        if (!strcmp(c->name, argv[1])) { pc_locale(c->locale); c->fn(n, argv[3]); return 0; }
    }
    return 3;
}
