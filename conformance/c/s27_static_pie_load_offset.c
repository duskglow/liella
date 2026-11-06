/* C++17 source (LINK-CXX). Spec §27, §27.1, §12.6, §1.1 (D-27.12, D-27.4, D-27.5, D-12.4): a
 * static PIE (no PT_PHDR) sees its real load bias wherever the library looks the executable up.
 * The base is found independently of the library: AT_ENTRY minus the e_entry of the file
 * /proc/self/exe. Checked in each start: dl_iterate_phdr's first object has dlpi_addr equal to
 * that base and dlpi_phdr equal to AT_PHDR, and its PT_LOAD segments moved by dlpi_addr hold a
 * known function (in an executable segment) and a known object (in a writable one);
 * _dl_find_object finds the program for both (range around the address, dlfo_eh_frame = base +
 * PT_GNU_EH_FRAME's p_vaddr) and nothing for an anonymous mapping; __printf_chk with flag 1
 * accepts "%n" in a format in the read-only segment and aborts on one in the writable segment;
 * a C++ exception thrown three frames down reaches its catch and runs the destructors between
 * (the unwinder's _dl_find_object / dl_iterate_phdr lookups). Run without arguments it starts
 * itself 100 times through /proc/self/exe with address randomisation; every start must pass and
 * the bases must not all be equal (unless /proc/sys/kernel/randomize_va_space is 0: then said on
 * stdout and not required). A failing CHECK exits with its line number. */
/* LINK-CXX: yes */
/* LINK-LIBM: yes */
/* LINK-UNWIND: yes */
/* LINK-STATIC-PIE: relr */
#include <dlfcn.h>
#include <elf.h>
#include <fcntl.h>
#include <link.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>
#include <stdexcept>

extern "C" int __printf_chk(int flag, const char *__restrict fmt, ...);

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

int known_object = 7;
__attribute__((noinline)) int known_function(int x) { return x + known_object; }
static const char ro_format[] = "ab%n\n";
static char rw_format[sizeof ro_format];

struct Seen {
    uintptr_t addr;
    const ElfW(Phdr) *phdr;
    int phnum;
    int calls;
};

static int first_object(struct dl_phdr_info *info, size_t, void *data) {
    Seen *s = static_cast<Seen *>(data);
    if (s->calls++ == 0) {
        s->addr = info->dlpi_addr;
        s->phdr = info->dlpi_phdr;
        s->phnum = info->dlpi_phnum;
    }
    return 0;
}

/* The PT_LOAD of the first object that holds `addr`, moved by `bias`; its flags in *flags. */
static bool in_load(const Seen &s, uintptr_t bias, uintptr_t addr, unsigned *flags) {
    for (int i = 0; i < s.phnum; i++) {
        const ElfW(Phdr) &p = s.phdr[i];
        if (p.p_type == PT_LOAD && addr >= bias + p.p_vaddr && addr - (bias + p.p_vaddr) < p.p_memsz) {
            *flags = p.p_flags;
            return true;
        }
    }
    return false;
}

/* The status of a child that runs __printf_chk(1, fmt, &n) and exits 0 when n == 2. */
static int printf_chk_status(const char *fmt) {
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        int null = open("/dev/null", O_WRONLY);
        if (null >= 0) { dup2(null, 1); dup2(null, 2); }
        int n = -1;
        __printf_chk(1, fmt, &n);
        fflush(stdout);
        _exit(n == 2 ? 0 : 3);
    }
    int st = -1;
    if (pid < 0 || waitpid(pid, &st, 0) != pid) return -1;
    return st;
}

static int destroyed;
struct Guard { ~Guard() { destroyed++; } };
__attribute__((noinline)) static void thrower(int depth) {
    Guard g;
    if (depth == 0) throw std::runtime_error("deep");
    thrower(depth - 1);
    destroyed += 100; /* never reached */
}

static int checks(uintptr_t *base_out) {
    /* The base from the file: AT_ENTRY minus the link-time entry point. */
    ElfW(Ehdr) eh;
    int fd = open("/proc/self/exe", O_RDONLY);
    CHECK("§27", fd >= 0);
    CHECK("§27", read(fd, &eh, sizeof eh) == (ssize_t)sizeof eh);
    close(fd);
    CHECK("§1.1", eh.e_type == ET_DYN);
    uintptr_t base = getauxval(AT_ENTRY) - eh.e_entry;
    CHECK("§1.1", base > 0x10000 && base % 4096 == 0);
    *base_out = base;

    /* dl_iterate_phdr: the program first, at its real base. */
    Seen s = {0, nullptr, 0, 0};
    CHECK("§27", dl_iterate_phdr(first_object, &s) == 0);
    CHECK("§27", s.calls >= 1);
    CHECK("§27", s.addr == base);
    CHECK("§27", (uintptr_t)s.phdr == getauxval(AT_PHDR) && s.phnum == (int)getauxval(AT_PHNUM));
    unsigned flags = 0;
    uintptr_t fn = (uintptr_t)&known_function, obj = (uintptr_t)&known_object;
    CHECK("§27", in_load(s, s.addr, fn, &flags) && (flags & PF_X) != 0);
    CHECK("§27", in_load(s, s.addr, obj, &flags) && (flags & PF_W) != 0);
    uintptr_t eh_frame = 0;
    for (int i = 0; i < s.phnum; i++)
        if (s.phdr[i].p_type == PT_GNU_EH_FRAME) eh_frame = base + s.phdr[i].p_vaddr;
    CHECK("§27.1", eh_frame != 0);

    /* _dl_find_object: the program's segment for an address in it, nothing for a fresh mapping. */
    struct dl_find_object fo;
    CHECK("§27.1", _dl_find_object((void *)fn, &fo) == 0);
    CHECK("§27.1", fo.dlfo_map_start <= (void *)fn && (void *)fn < fo.dlfo_map_end);
    CHECK("§27.1", (uintptr_t)fo.dlfo_eh_frame == eh_frame);
    CHECK("§27.1", _dl_find_object((void *)obj, &fo) == 0);
    CHECK("§27.1", fo.dlfo_map_start <= (void *)obj && (void *)obj < fo.dlfo_map_end);
    void *anon = mmap(nullptr, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    CHECK("§27.1", anon != MAP_FAILED);
    CHECK("§27.1", _dl_find_object(anon, &fo) == -1);
    munmap(anon, 4096);

    /* The fortified %n: accepted from the read-only segment, refused from the writable one. */
    CHECK("§12.6", in_load(s, base, (uintptr_t)ro_format, &flags) && (flags & PF_W) == 0);
    memcpy(rw_format, ro_format, sizeof ro_format);
    CHECK("§12.6", in_load(s, base, (uintptr_t)rw_format, &flags) && (flags & PF_W) != 0);
    int st = printf_chk_status(ro_format);
    CHECK("§12.6", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    st = printf_chk_status(rw_format);
    CHECK("§12.6", WIFSIGNALED(st) && WTERMSIG(st) == SIGABRT);

    /* A C++ exception through three frames, their destructors run. */
    destroyed = 0;
    bool caught = false;
    try {
        thrower(2);
    } catch (const std::runtime_error &e) {
        caught = strcmp(e.what(), "deep") == 0;
    }
    CHECK("§27.1", caught && destroyed == 3);
    CHECK("§27", known_function(1) == 8);
    return 0;
}

static int start_many(const char *self) {
    uintptr_t base[100];
    for (int i = 0; i < 100; i++) {
        int fds[2];
        CHECK("§1.1", pipe(fds) == 0);
        fflush(stdout);
        pid_t pid = fork();
        CHECK("§1.1", pid >= 0);
        if (pid == 0) {
            dup2(fds[1], 1);
            close(fds[0]);
            execl("/proc/self/exe", self, "child", (char *)nullptr);
            _exit(98);
        }
        close(fds[1]);
        char buf[64] = {0};
        ssize_t n = read(fds[0], buf, sizeof buf - 1);
        close(fds[0]);
        int st;
        CHECK("§1.1", waitpid(pid, &st, 0) == pid);
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            printf("start %d: status %#x\n", i, st);
            return __LINE__;
        }
        CHECK("§1.1", n > 0);
        base[i] = (uintptr_t)strtoull(buf, nullptr, 16);
        CHECK("§1.1", base[i] > 0x10000);
    }
    bool same = true;
    for (int i = 1; i < 100; i++) same &= base[i] == base[0];
    FILE *f = fopen("/proc/sys/kernel/randomize_va_space", "r");
    int level = f ? fgetc(f) : '?';
    if (f) fclose(f);
    if (level == '0') {
        printf("address randomisation is off on this system: the starts were not checked for distinct bases\n");
    } else {
        CHECK("§1.1", !same);
    }
    return 0;
}

int main(int argc, char **argv) {
    uintptr_t base = 0;
    int r = checks(&base);
    if (r != 0) return r;
    if (argc == 2 && strcmp(argv[1], "child") == 0) {
        printf("%lx\n", (unsigned long)base);
        return 0;
    }
    return start_many(argv[0]);
}
