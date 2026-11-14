/* Black-box probe: does regcomp (ERE, C locale) accept a nested pattern of depth n?
 * usage: depth_probe <shape> <n>...   prints: shape n rc secs maxrss_kb (rc: regcomp code, or SIGn / TIMEOUT)
 * shapes: rep "(" a ")*", plus ")+", opt ")?", grp ")", alt "(a|" b ")", bnd "){1,2}", rep2 "(" "a*" ")*" */
#include <regex.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <unistd.h>

static char *build(const char *shape, long n) {
    const char *open = "(", *mid = "a", *close = ")*";
    if (!strcmp(shape, "plus")) close = ")+";
    else if (!strcmp(shape, "opt")) close = ")?";
    else if (!strcmp(shape, "grp")) close = ")";
    else if (!strcmp(shape, "alt")) { open = "(a|"; mid = "b"; close = ")"; }
    else if (!strcmp(shape, "bnd")) close = "){1,2}";
    else if (!strcmp(shape, "rep2")) mid = "a*";
    size_t lo = strlen(open), lm = strlen(mid), lc = strlen(close);
    char *p = malloc(n * (lo + lc) + lm + 1), *q = p;
    for (long i = 0; i < n; i++) { memcpy(q, open, lo); q += lo; }
    memcpy(q, mid, lm); q += lm;
    for (long i = 0; i < n; i++) { memcpy(q, close, lc); q += lc; }
    *q = 0;
    return p;
}

int main(int argc, char **argv) {
    if (argc < 3) return 2;
    for (int k = 2; k < argc; k++) {
        long n = atol(argv[k]);
        struct timeval t0, t1;
        gettimeofday(&t0, 0);
        pid_t pid = fork();
        if (pid == 0) {
            struct rlimit lim = { 4UL << 30, 4UL << 30 };
            setrlimit(RLIMIT_AS, &lim);
            alarm(40);
            char *p = build(argv[1], n);
            regex_t re;
            int rc = regcomp(&re, p, REG_EXTENDED);
            _exit(rc & 0x7f);
        }
        int st = 0;
        struct rusage ru;
        wait4(pid, &st, 0, &ru);
        gettimeofday(&t1, 0);
        double s = (t1.tv_sec - t0.tv_sec) + (t1.tv_usec - t0.tv_usec) / 1e6;
        char rc[32];
        if (WIFEXITED(st)) snprintf(rc, sizeof rc, "%d", WEXITSTATUS(st));
        else if (WTERMSIG(st) == SIGALRM) snprintf(rc, sizeof rc, "TIMEOUT");
        else snprintf(rc, sizeof rc, "SIG%d", WTERMSIG(st));
        printf("%s %ld %s %.3f %ld\n", argv[1], n, rc, s, ru.ru_maxrss);
        fflush(stdout);
    }
    return 0;
}
