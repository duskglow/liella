/* Spec §1.1 (D-27.25, amends D-27.10): s01_static_pie.c with its data in three or more writable
 * PT_LOADs. GNU ld starts a new segment where a section begins at least one whole page past the end
 * of the previous one. Two small 16 KiB-aligned orphan sections and the 16 KiB-aligned .data1 that
 * the default script places after them are laid out back to back, so each of them begins about
 * 16 KiB past the start of the one before it whatever size libc.a's .data has: the second and
 * third each start a new writable segment (the first may share the .data segment; GNU ld 2.42
 * places the orphans in either order, so the test does not depend on which comes first). Each holds
 * pointers the start relocates (RELR). A constructor checks the split really happened (at least
 * three writable PT_LOADs in the running program's headers: exit 92 otherwise, so a linker that
 * stops splitting cannot make this test pass vacuously) and that every pointer is right (exit 91);
 * then the same checks and the same 100 starts as s01_static_pie.c. */
/* LINK-STATIC-PIE: relr */
#define _GNU_SOURCE
#include <elf.h>
#include <sys/auxv.h>
#include <unistd.h>

static int multi_a = 1, multi_b = 2;
int *multi_seg2[] __attribute__((section(".liella_seg2"), aligned(16384))) = { &multi_a, &multi_a };
int *multi_data1[] __attribute__((section(".data1"), aligned(16384))) = { &multi_a, &multi_b };
int *multi_seg3[] __attribute__((section(".liella_seg3"), aligned(16384))) = { &multi_b, &multi_a };

__attribute__((constructor)) static void multi_check(void) {
    const Elf64_Phdr *ph = (const Elf64_Phdr *)getauxval(AT_PHDR);
    unsigned long n = getauxval(AT_PHNUM), writable = 0;
    for (unsigned long i = 0; i < n; i++)
        writable += ph[i].p_type == PT_LOAD && (ph[i].p_flags & PF_W);
    if (writable < 3)
        _exit(92);
    if (*multi_data1[0] != 1 || *multi_data1[1] != 2 || *multi_seg3[0] != 2 || *multi_seg3[1] != 1
        || *multi_seg2[0] != 1 || *multi_seg2[1] != 1)
        _exit(91);
}

#include "s01_static_pie.c"
