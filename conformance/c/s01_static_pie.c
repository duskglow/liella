/* Spec §1.1 (program start-up; D-27.10, phase 14 DL-4): a static PIE relocates itself before any
 * of its code reads an address from its data. The program holds every kind of word the
 * self-relocator writes: pointer tables in .data.rel.ro and .data (RELATIVE, packed as RELR with
 * this file's LINK-STATIC-PIE: relr; s01_static_pie_rela.c builds the same source with RELA
 * entries), a pointer in the TLS image (relocated before the image is copied), a constructor that
 * reads them, and an indirect function both called and taken by address (IRELATIVE, applied after
 * the relocation pass). Run without arguments it starts itself 100 times through /proc/self/exe:
 * 50 starts with address randomisation, 50 with ADDR_NO_RANDOMIZE (personality(2)); every start
 * must pass, the fixed starts must share one load address and the random ones must not all share
 * one (unless /proc/sys/kernel/randomize_va_space is 0: then said on stdout and not required).
 * Each failing CHECK exits with its line number. */
/* LINK-STATIC-PIE: relr */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/personality.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static int twice(int x) { return 2 * x; }
static int square(int x) { return x * x; }
static int (*const ops_ro[])(int) = { twice, square };
static int (*ops_rw[])(int) = { square, twice };
static const char *const names[] = { "alpha", "beta", "gamma" };
static const char *greeting = "hello";
static __thread const char *tls_name = "tls-image";
static __thread int tls_value = 41;

static int triple(int x) { return 3 * x; }
static int resolved;
static int (*pick(void))(int) { resolved++; return triple; }
int via_ifunc(int) __attribute__((ifunc("pick")));
static int (*const ifunc_ptr)(int) = via_ifunc;

static int ctor_saw;
__attribute__((constructor)) static void ctor(void) {
    ctor_saw = strcmp(names[1], "beta") == 0 && ops_ro[1](3) == 9 && strcmp(tls_name, "tls-image") == 0;
}

static int checks(void) {
    CHECK("§1.1", ctor_saw == 1);
    CHECK("§1.1", resolved == 1);
    CHECK("§1.1", strcmp(names[0], "alpha") == 0 && strcmp(names[2], "gamma") == 0);
    CHECK("§1.1", ops_ro[0](5) == 10 && ops_rw[0](5) == 25 && ops_rw[1](5) == 10);
    CHECK("§1.1", strcmp(greeting, "hello") == 0);
    CHECK("§1.1", strcmp(tls_name, "tls-image") == 0 && tls_value == 41);
    CHECK("§1.1", via_ifunc(4) == 12 && ifunc_ptr(5) == 15 && ifunc_ptr == via_ifunc);
    /* Loaded away from its link address: the executable's headers are where the kernel put them. */
    CHECK("§1.1", getauxval(AT_PHDR) > 0x10000);
    CHECK("§1.1", (uintptr_t)&checks > 0x10000);
    return 0;
}

static int start_many(const char *self) {
    uintptr_t base[100];
    for (int i = 0; i < 100; i++) {
        int fds[2];
        CHECK("§1.1", pipe(fds) == 0);
        pid_t pid = fork();
        CHECK("§1.1", pid >= 0);
        if (pid == 0) {
            if (i >= 50 && personality(ADDR_NO_RANDOMIZE) == -1) _exit(97);
            dup2(fds[1], 1);
            close(fds[0]);
            execl("/proc/self/exe", self, "child", (char *)0);
            _exit(98);
        }
        close(fds[1]);
        char buf[64] = { 0 };
        ssize_t n = read(fds[0], buf, sizeof buf - 1);
        close(fds[0]);
        int st;
        CHECK("§1.1", waitpid(pid, &st, 0) == pid);
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            printf("start %d: status %#x\n", i, st);
            return __LINE__;
        }
        CHECK("§1.1", n > 0);
        base[i] = (uintptr_t)strtoull(buf, 0, 16);
        CHECK("§1.1", base[i] > 0x10000);
    }
    int same_random = 1;
    for (int i = 1; i < 50; i++) same_random &= base[i] == base[0];
    for (int i = 51; i < 100; i++) CHECK("§1.1", base[i] == base[50]);
    FILE *f = fopen("/proc/sys/kernel/randomize_va_space", "r");
    int level = f ? fgetc(f) : '?';
    if (f) fclose(f);
    if (level == '0') {
        printf("address randomisation is off on this system: the random starts were not checked for distinct bases\n");
    } else {
        CHECK("§1.1", !same_random);
    }
    return 0;
}

int main(int argc, char **argv) {
    int r = checks();
    if (r != 0) return r;
    if (argc == 2 && strcmp(argv[1], "child") == 0) {
        printf("%lx\n", (unsigned long)(uintptr_t)&checks);
        return 0;
    }
    return start_many(argv[0]);
}
