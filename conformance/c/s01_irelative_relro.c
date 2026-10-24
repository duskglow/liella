/* Spec §1.1 (RELRO-IRELATIVE-ORDER, D-1.13): in a static program the start-up applies the
 * program's own R_X86_64_IRELATIVE entries BEFORE it makes the PT_GNU_RELRO segment read-only.
 * The .got slot of an indirect function lies inside RELRO: by the time the constructors run it
 * holds the resolver's choice, its page is read-only, and a later write to it faults (in a
 * child). Started the other way round, the IRELATIVE store would fault at start.
 * The slot is named through an x86_64 GOTPCREL reference (lea, which the linker does not relax).
 * LINK-NOW: GNU ld (static, non-PIE) puts the slot in .got.plt, which is outside RELRO unless the
 * program is linked -z now (then the whole GOT is in RELRO). The same program passes against
 * glibc 2.39 linked -static -z now (and fails its premise check without -z now).
 * Each failing CHECK exits with its line number. */
/* LINK-NOW: yes */
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

typedef int fn_t(int);

static int resolved;
static int h_impl(int x) { return x * 3 + 1; }
static fn_t *resolve_h(void) { resolved++; return h_impl; }
int h(int) __attribute__((ifunc("resolve_h")));

/* The address of h's .got slot (the word its IRELATIVE entry writes). */
static unsigned long *got_slot(void) {
    unsigned long *slot;
    __asm__("leaq h@GOTPCREL(%%rip), %0" : "=r"(slot));
    return slot;
}

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

/* 0 when the word at `at` lies in a mapping without write permission, else a line. */
static int word_read_only(unsigned long at) {
    FILE *f = fopen("/proc/self/maps", "r");
    CHECK("1.1", f != 0);
    char line[512];
    int found = 0;
    while (!found && fgets(line, sizeof line, f)) {
        unsigned long s, e;
        char perms[8];
        if (sscanf(line, "%lx-%lx %7s", &s, &e, perms) != 3) continue;
        if (s <= at && at + sizeof(unsigned long) <= e) {
            if (perms[0] != 'r' || perms[1] != '-') { fclose(f); return __LINE__; }
            found = 1;
        }
    }
    fclose(f);
    CHECK("1.1", found);
    return 0;
}

static int ctor_slot_ok = -1, ctor_ro = -1, ctor_result = -1;

__attribute__((constructor)) static void early(void) {
    /* Resolved (the slot holds the resolver's choice) and already read-only. */
    ctor_slot_ok = *(volatile unsigned long *)got_slot() == (unsigned long)h_impl;
    ctor_ro = word_read_only((unsigned long)got_slot());
    ctor_result = h(2);
}

int main(void) {
    unsigned long lo, hi, slot = (unsigned long)got_slot();
    CHECK("1.1-c-5", relro_range(&lo, &hi));
    /* The premise: the slot lies on a protected RELRO page. */
    CHECK("1.1-c-5", slot >= lo && slot + sizeof(unsigned long) <= hi);
    /* Resolved once, before the constructors, and read-only when they ran. */
    CHECK("1.1-c-5", resolved == 1);
    CHECK("1.1-c-5", ctor_slot_ok == 1);
    CHECK("1.1-c-5", ctor_ro == 0);
    CHECK("1.1-c-5", ctor_result == 7);
    CHECK("1.1-c-5", *(volatile unsigned long *)slot == (unsigned long)h_impl);
    CHECK("1.1-c-5", h(10) == 31);
    CHECK("1.1-c-5", word_read_only(slot) == 0);
    /* A write to the resolved slot faults. */
    pid_t pid = fork();
    CHECK("1.1-c-5", pid >= 0);
    if (pid == 0) {
        expect_crash();
        *(volatile unsigned long *)slot = 0;
        _exit(0);
    }
    int status;
    CHECK("1.1-c-5", waitpid(pid, &status, 0) == pid);
    CHECK("1.1-c-5", WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV);
    CHECK("1.1-c-5", resolved == 1);
    return 0;
}
