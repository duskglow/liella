/* No spec clause (resource bound of the implementation, decision D-14.11, RX-CAP): regcomp and
 * regexec with submatches work on a thread whose stack is PTHREAD_STACK_MIN (16 KiB); the
 * matcher keeps at most about 4 KiB of scratch on the stack. Exits 0 when all 9 calls match. */
#define _GNU_SOURCE
#include <limits.h>
#include <locale.h>
#include <pthread.h>
#include <regex.h>
#include <stdio.h>
#include <stdlib.h>

static void *body(void *arg) {
    (void)arg;
    static const char *pats[] = {"^\\([a-z0-9_]*\\) = \\(.*\\)$", "([a-z]+)@([a-z]+)\\.(com|org)", "(sshd|cron|kernel)\\[[0-9]+\\]"};
    static const char *subj[] = {"max_connections = 1024", "user@example.com and more", "host sshd[1234]: x"};
    static const int fl[] = {0, REG_EXTENDED, REG_EXTENDED};
    long ok = 0;
    for (int i = 0; i < 3; i++) {
        regex_t re;
        regmatch_t m[4];
        if (regcomp(&re, pats[i], fl[i])) return NULL;
        for (int k = 0; k < 3; k++) ok += regexec(&re, subj[i], 4, m, 0) == 0;
        regfree(&re);
    }
    return (void *)ok;
}

int main(int argc, char **argv) {
    setlocale(LC_ALL, "C.UTF-8");
    size_t sz = argc > 1 ? strtoul(argv[1], NULL, 0) : PTHREAD_STACK_MIN;
    pthread_attr_t a;
    pthread_t t;
    void *r = NULL;
    pthread_attr_init(&a);
    if (pthread_attr_setstacksize(&a, sz)) return 3;
    if (pthread_create(&t, &a, body, NULL)) return 4;
    pthread_join(t, &r);
    printf("stack %zu: %ld of 9 matched\n", sz, (long)r);
    return (long)r == 9 ? 0 : 1;
}
