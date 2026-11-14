/* No spec clause (an internal per-thread cache; the answers are localtime_r's); TB-SLOTS,
 * D-X.58: in a process Liella's runtime started, the thread's time-zone period lives in its own
 * block's cache words (%fs:0x140-0x157). The %fs-relative loads read what the block holds, each
 * thread keeps its own period, a cache hit stores nothing, and a forked child starts with its
 * parent's words (zone numbers are never reused) and changes only its own. */
/* RUN-ENV: TZ=EST5EDT,M3.2.0,M11.1.0 */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define SUMMER ((time_t)1720000000) /* 05:46 EDT */
#define WINTER ((time_t)1700000000) /* 17:13 EST */
#define DST (1UL << 32)

static int failures;

static void check(int ok, const char *what) {
    if (!ok) {
        printf("FAIL: %s\n", what);
        fflush(stdout);
        __atomic_add_fetch(&failures, 1, __ATOMIC_SEQ_CST);
    }
}

struct words { unsigned long w[3]; };

static struct words words(void) {
    struct words r;
    __asm__ volatile("movq %%fs:0x140, %0" : "=r"(r.w[0]));
    __asm__ volatile("movq %%fs:0x148, %0" : "=r"(r.w[1]));
    __asm__ volatile("movq %%fs:0x150, %0" : "=r"(r.w[2]));
    return r;
}

static int same(struct words a, struct words b) { return memcmp(&a, &b, sizeof a) == 0; }

/* The hour and DST flag localtime_r gives for t, as hour * 2 + isdst. */
static int kind_at(time_t t) {
    struct tm o;
    memset(&o, 0, sizeof o);
    localtime_r(&t, &o);
    return o.tm_hour * 2 + (o.tm_isdst > 0);
}

static struct words theirs;

static void *other(void *arg) {
    (void)arg;
    check(kind_at(WINTER) == 17 * 2, "thread: winter is 17:13 EST");
    struct words w = words();
    check(kind_at(WINTER + 60) == 17 * 2, "thread: a minute later");
    check(same(words(), w), "thread: a hit stores nothing");
    theirs = w;
    return 0;
}

int main(void) {
    setvbuf(stdout, 0, _IONBF, 0);
    check(kind_at(SUMMER) == 5 * 2 + 1, "summer is 05:46 EDT");
    struct words mine = words();
    check((mine.w[0] & 0xffffffffUL) != 0 && (mine.w[0] & DST) != 0, "a DST period is kept in the block");

    pthread_t t;
    check(pthread_create(&t, 0, other, 0) == 0 && pthread_join(t, 0) == 0, "pthread_create and join");
    check(theirs.w[0] == (mine.w[0] & ~DST), "the other thread: the same zone, standard time");
    check(same(words(), mine), "another thread's period is its own");
    check(kind_at(SUMMER + 60) == 5 * 2 + 1 && same(words(), mine), "a hit after the thread");

    pid_t pid = fork();
    if (pid == 0) {
        int code = 0;
        if (!same(words(), mine))
            code = 1;
        else if (kind_at(SUMMER + 120) != 5 * 2 + 1 || (words().w[0] & DST) == 0)
            code = 2;
        else if (kind_at(WINTER) != 17 * 2 || (words().w[0] & DST) != 0)
            code = 3;
        _exit(code);
    }
    int status = -1;
    check(pid > 0 && waitpid(pid, &status, 0) == pid, "fork and wait");
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
        printf("child status %#x (1: words not inherited, 2: summer, 3: winter)\n", status);
    check(WIFEXITED(status) && WEXITSTATUS(status) == 0, "the child starts with its parent's period");
    check(same(words(), mine), "the child's period stays in the child");

    if (failures)
        printf("%d failures\n", failures);
    return failures != 0;
}
