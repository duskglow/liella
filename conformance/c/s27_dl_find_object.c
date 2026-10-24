/* Spec §27.1, D-27.5: _dl_find_object in a static program. A pc in the main program's text
 * gives the PT_LOAD segment that holds it and the program's PT_GNU_EH_FRAME (this program is
 * linked with --eh-frame-hdr by LINK-UNWIND); a pc in the vDSO gives the vDSO's range and its
 * PT_GNU_EH_FRAME; the stack, _end and 0 give -1 and leave the result alone. */
/* LINK-UNWIND: yes */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <stdio.h>
#include <string.h>
#include <sys/auxv.h>

extern char _end[];
static unsigned long eh_main, eh_vdso;
static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_dl_find_object: FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)

static int cb(struct dl_phdr_info *i, size_t s, void *d)
{
    (void)s; (void)d;
    for (int k = 0; k < i->dlpi_phnum; k++)
        if (i->dlpi_phdr[k].p_type == PT_GNU_EH_FRAME)
            *(i->dlpi_name[0] ? &eh_vdso : &eh_main) = i->dlpi_addr + i->dlpi_phdr[k].p_vaddr;
    return 0;
}

int main(void)
{
    struct dl_find_object r;
    int local = 0;
    dl_iterate_phdr(cb, 0);
    CHECK(eh_main != 0);
    memset(&r, 0x5a, sizeof r);
    CHECK(_dl_find_object((void *)main, &r) == 0);
    CHECK(r.dlfo_flags == 0);
    CHECK((unsigned long)r.dlfo_map_start <= (unsigned long)main && (unsigned long)main < (unsigned long)r.dlfo_map_end);
    CHECK((unsigned long)r.dlfo_map_end <= (unsigned long)_end);
    CHECK((unsigned long)r.dlfo_eh_frame == eh_main);
    void *const nowhere[] = {&local, _end, 0};
    for (unsigned k = 0; k < 3; k++) {
        memset(&r, 0x5a, sizeof r);
        CHECK(_dl_find_object(nowhere[k], &r) == -1);
        CHECK(r.dlfo_flags == 0x5a5a5a5a5a5a5a5aULL);
    }
    unsigned long v = getauxval(AT_SYSINFO_EHDR);
    if (v) {
        CHECK(_dl_find_object((void *)(v + 0x100), &r) == 0);
        CHECK((unsigned long)r.dlfo_map_start == v && (unsigned long)r.dlfo_map_end > v + 0x100);
        CHECK((unsigned long)r.dlfo_eh_frame == eh_vdso);
    }
    return fails != 0;
}
