/* Spec §1.1 (STATIC-RELRO, D-1.13): a static program's PT_GNU_RELRO segment is read-only from
 * before its constructors run: every page from the segment's first page to its last whole page
 * is mapped without write permission, and a write to a .data.rel.ro object faults (in a child).
 * Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <elf.h>
#include <link.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/wait.h>
#include <unistd.h>
#include "expect_crash.h"

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

/* An object the linker places in the RELRO segment, and one that must stay writable. */
static int relro_obj __attribute__((section(".data.rel.ro"), used)) = 41;
static volatile int data_obj = 1;

/* The pages [*lo, *hi) the start-up must have made read-only; 0 when there are none. */
static int relro_range(unsigned long *lo, unsigned long *hi) {
    const ElfW(Phdr) *ph = (const ElfW(Phdr) *)getauxval(AT_PHDR);
    unsigned long n = getauxval(AT_PHNUM), page = getauxval(AT_PAGESZ), bias = 0;
    const ElfW(Phdr) *relro = 0;
    for (unsigned long i = 0; i < n; i++) {
        if (ph[i].p_type == PT_PHDR) bias = (unsigned long)ph - ph[i].p_vaddr;
        if (ph[i].p_type == PT_GNU_RELRO) relro = &ph[i];
    }
    if (!relro || page == 0) return 0;
    *lo = (relro->p_vaddr + bias) & ~(page - 1);
    *hi = (relro->p_vaddr + bias + relro->p_memsz) & ~(page - 1);
    return *hi > *lo;
}

/* 0 when every byte of [lo, hi) lies in mappings without write permission, else a line. */
static int read_only(unsigned long lo, unsigned long hi) {
    FILE *f = fopen("/proc/self/maps", "r");
    CHECK("1.1", f != 0);
    char line[512];
    unsigned long at = lo;
    while (at < hi && fgets(line, sizeof line, f)) {
        unsigned long s, e;
        char perms[8];
        if (sscanf(line, "%lx-%lx %7s", &s, &e, perms) != 3) continue;
        if (s <= at && at < e) {
            if (perms[0] != 'r' || perms[1] != '-') { fclose(f); return __LINE__; }
            at = e;
        }
    }
    fclose(f);
    CHECK("1.1", at >= hi);
    return 0;
}

static int ctor_result = -1;

__attribute__((constructor)) static void early(void) {
    unsigned long lo, hi;
    ctor_result = relro_range(&lo, &hi) ? read_only(lo, hi) : 1;
}

int main(void) {
    unsigned long lo, hi;
    CHECK("1.1", relro_range(&lo, &hi));
    /* Already read-only when the constructors ran. */
    CHECK("1.1", ctor_result == 0);
    CHECK("1.1", read_only(lo, hi) == 0);
    /* The object is inside the protected pages and still readable. */
    CHECK("1.1", (unsigned long)&relro_obj >= lo && (unsigned long)&relro_obj + sizeof relro_obj <= hi);
    CHECK("1.1", *(volatile int *)&relro_obj == 41);
    /* Ordinary data stays writable. */
    data_obj = 2;
    CHECK("1.1", data_obj == 2);
    /* A write to the RELRO object faults. */
    pid_t pid = fork();
    CHECK("1.1", pid >= 0);
    if (pid == 0) {
        expect_crash();
        *(volatile int *)&relro_obj = 42;
        _exit(0);
    }
    int status;
    CHECK("1.1", waitpid(pid, &status, 0) == pid);
    CHECK("1.1", WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV);
    return 0;
}
