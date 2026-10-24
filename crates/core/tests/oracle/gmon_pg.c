/* A -pg program for tests/gmon_products.rs (GCRT1-START): built with -pg against the host C
 * library and against Liella's liella-gcrt1.o + libc.a. It makes no profiling call itself: the
 * start object must start profiling and have gmon.out written. Modes (argv[1]): none (return
 * from main), "exit", "_exit" (no gmon.out), "fork" (the child exits too: two writes). */
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

__attribute__((noipa)) int leaf(int x) { return x * 3 + 1; }
__attribute__((noipa)) int mid(int x) { return leaf(x) + leaf(x + 1); }
__attribute__((noipa)) int top(int x) { return mid(x) + leaf(x); }
__attribute__((noipa)) int in_ctor(int x) { return x + 7; }
static volatile int seen;
/* An arc out of a constructor: profiling runs before the program's constructors. */
__attribute__((constructor)) static void ctor(void) { seen = in_ctor(1); }

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    int s = 0;
    for (int i = 0; i < 5; i++) s += mid(i);
    for (int i = 0; i < 3; i++) s += top(i);
    if (!strcmp(mode, "exit")) exit(s == 0);
    if (!strcmp(mode, "_exit")) _exit(s == 0);
    if (!strcmp(mode, "fork")) {
        pid_t p = fork();
        if (p == 0) exit(leaf(9) == 0);
        int st;
        if (waitpid(p, &st, 0) != p || st != 0) return 3;
    }
    return s == 0 || seen != 8;
}
