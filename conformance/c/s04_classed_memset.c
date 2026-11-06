/* The installed memset export on both pages of the classed section (D-4.53, CLASSED-SLOT-INFRA;
 * design docs/design/loadtime-entry.md 2.2 item 2 and section 4 story 1).
 *
 * Clauses: s4_1_c_1 (memset fills n bytes and returns s), at the export on each page; the page
 * swap itself has no spec clause (D-4.53).
 *
 * The program re-executes itself (a static, non-PIE program: the same addresses in every run)
 * in five starts and asks each which page runs at memset, read black-box from /proc/self/maps:
 * after the swap, memset's page is a mapping of its own, one page long, whose file offset is one
 * page further on, and the page after it is a hole. Without the swap memset's page lies inside
 * the text mapping. Each start then sweeps memset against a byte loop.
 *   default                                 the AVX2 page on an AVX2 host (AVX2 and YMM state
 *                                           saved, /proc/self/pagemap readable), else the SSE2 page
 *   GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2   the SSE2 page (the test override, every host)
 *   GLIBC_TUNABLES=liella.compat.no_classed_swap=1   the SSE2 page (COMPAT-20)
 *   PTRACE_TRACEME, one unchanged word of memset's page written back at the exec stop (as a
 *     debugger's breakpoint makes the page anonymous): the SSE2 page (the pagemap guard)
 *   PR_SET_MDWE(PR_MDWE_REFUSE_EXEC_GAIN) set before execve: the AVX2 page on an AVX2 host
 *     (the swap gains no exec permission); skipped, with the reason, without PR_SET_MDWE. */
#include <cpuid.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <unistd.h>

#ifndef PR_SET_MDWE
#define PR_SET_MDWE 65
#endif
#ifndef PR_MDWE_REFUSE_EXEC_GAIN
#define PR_MDWE_REFUSE_EXEC_GAIN 1
#endif

#define PAGE 4096UL
#define MAX 320
#define PAD 64

static void *(*volatile fill)(void *, int, size_t) = memset;

static uintptr_t page0(void) {
    return (uintptr_t)fill & ~(PAGE - 1);
}

/* memset against a byte loop: every size 0 to 320, 32 destination alignments, values whose int
 * has other bytes set; sentinels on both sides. Returns 0 or the failing line. */
static int sweep(void) {
    static unsigned char buf[MAX + 2 * PAD];
    static const int vals[] = {0, 0x7F, 0x80, 0xFF, 0x123, -1};
    for (size_t n = 0; n <= MAX; n++)
        for (size_t da = 0; da < 32; da++)
            for (size_t v = 0; v < sizeof vals / sizeof vals[0]; v++) {
                for (size_t i = 0; i < sizeof buf; i++)
                    buf[i] = 0xA5;
                if (fill(buf + PAD + da, vals[v], n) != buf + PAD + da)
                    return __LINE__;
                for (size_t i = 0; i < sizeof buf; i++) {
                    int in = i >= PAD + da && i < PAD + da + n;
                    if (buf[i] != (in ? (unsigned char)vals[v] : 0xA5)) {
                        fprintf(stderr, "memset n %zu +%zu c %#x: byte %zu is %#x\n", n, da, vals[v], i, buf[i]);
                        return __LINE__;
                    }
                }
            }
    return 0;
}

struct map { uintptr_t start, end, off; unsigned long inode; };

/* The mappings of this process (at most 128), or -1. */
static int read_maps(struct map *m, int cap) {
    static char text[65536];
    int fd = open("/proc/self/maps", O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return -1;
    size_t len = 0;
    ssize_t r;
    while (len < sizeof text - 1 && (r = read(fd, text + len, sizeof text - 1 - len)) > 0)
        len += (size_t)r;
    close(fd);
    text[len] = 0;
    int n = 0;
    for (char *p = text; *p && n < cap; n++) {
        char *e;
        m[n].start = strtoull(p, &e, 16);
        m[n].end = strtoull(e + 1, &e, 16);
        e = strchr(e + 1, ' ');           /* past the permissions */
        m[n].off = strtoull(e + 1, &e, 16);
        e = strchr(e + 1, ' ');           /* past the device */
        m[n].inode = strtoul(e + 1, &e, 10);
        p = strchr(e, '\n');
        if (!p)
            break;
        p++;
    }
    return n;
}

/* 1 when the AVX2 page runs at memset's page (the swap happened), 0 when the file's SSE2 page
 * does, -1 when the maps say neither. */
static int live_page(void) {
    struct map m[128];
    int n = read_maps(m, 128);
    uintptr_t p0 = page0();
    int at = -1, next = 0;
    for (int i = 0; i < n; i++) {
        if (m[i].start <= p0 && p0 < m[i].end)
            at = i;
        if (m[i].start <= p0 + PAGE && p0 + PAGE < m[i].end)
            next = 1;
    }
    if (at < 0 || m[at].inode == 0)
        return -1;
    if (m[at].end >= p0 + 2 * PAGE)
        return 0;                          /* both pages in the text mapping: no swap */
    if (m[at].start != p0 || m[at].end != p0 + PAGE || next)
        return -1;
    /* The file offset page 0 had: from the text mapping just before it, if any. */
    if (at > 0 && m[at - 1].end == p0 && m[at - 1].inode == m[at].inode
        && m[at].off != m[at - 1].off + (p0 - m[at - 1].start) + PAGE)
        return -1;
    return 1;
}

/* AVX2 reported by the CPU with the YMM state saved by the OS (XCR0 bits 1 and 2). */
static int avx2_cpu(void) {
    unsigned a, b, c, d;
    if (!__get_cpuid(1, &a, &b, &c, &d) || (c & (1u << 27)) == 0 || (c & (1u << 28)) == 0)
        return 0;
    unsigned lo, hi;
    __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
    if ((lo & 6) != 6)
        return 0;
    if (__get_cpuid_max(0, 0) < 7)
        return 0;
    __cpuid_count(7, 0, a, b, c, d);
    return (b & (1u << 5)) != 0;
}

/* The child's side: check which page runs, then sweep. */
static int child(const char *expect) {
    int live = live_page();
    int want = strcmp(expect, "avx2") == 0;
    if (live != want) {
        fprintf(stderr, "s04_classed_memset: expected the %s page at memset (%#lx), the maps say %d\n",
                expect, (unsigned long)page0(), live);
        return 1;
    }
    int line = sweep();
    if (line) {
        fprintf(stderr, "s04_classed_memset: %s page: memset sweep failed at line %d\n", expect, line);
        return 1;
    }
    return 0;
}

static int self_exe(char *path, size_t cap) {
    ssize_t r = readlink("/proc/self/exe", path, cap - 1);
    if (r <= 0)
        return -1;
    path[r] = 0;
    return 0;
}

enum how { PLAIN, TRACED, MDWE };

/* One start of this program with `env`, checked for the page `expect`. Returns 0 on a pass,
 * 77 on a skip, 1 on a failure. */
static int start(const char *name, enum how how, char *const env[], const char *expect) {
    char path[4096];
    if (self_exe(path, sizeof path))
        return 1;
    pid_t pid = fork();
    if (pid < 0)
        return 1;
    if (pid == 0) {
        if (how == TRACED && ptrace(PTRACE_TRACEME, 0, 0, 0) != 0)
            _exit(2);
        if (how == MDWE && prctl(PR_SET_MDWE, PR_MDWE_REFUSE_EXEC_GAIN, 0, 0, 0) != 0)
            _exit(errno == EINVAL ? 77 : 3);
        char *argv[] = {path, "--child", (char *)expect, 0};
        execve(path, argv, env);
        _exit(4);
    }
    int st;
    if (how == TRACED) {
        if (waitpid(pid, &st, 0) != pid || !WIFSTOPPED(st)) {
            fprintf(stderr, "s04_classed_memset: %s: no exec stop (status %#x)\n", name, st);
            return 1;
        }
        /* One word of memset's page written back unchanged, as a breakpoint insert and removal
         * leave it: the page is now the child's private copy. */
        long addr = (long)page0() + 16;
        errno = 0;
        long word = ptrace(PTRACE_PEEKDATA, pid, (void *)addr, 0);
        if (errno != 0 || ptrace(PTRACE_POKEDATA, pid, (void *)addr, (void *)word) != 0) {
            fprintf(stderr, "s04_classed_memset: %s: PTRACE_POKEDATA failed (errno %d)\n", name, errno);
            kill(pid, 9);
            waitpid(pid, &st, 0);
            return 1;
        }
        ptrace(PTRACE_DETACH, pid, 0, 0);
    }
    if (waitpid(pid, &st, 0) != pid)
        return 1;
    if (WIFEXITED(st) && WEXITSTATUS(st) == 77) {
        printf("s04_classed_memset: %s: skipped: this kernel has no PR_SET_MDWE (Linux 6.3 and later)\n", name);
        return 77;
    }
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fprintf(stderr, "s04_classed_memset: %s: the child failed (status %#x)\n", name, st);
        return 1;
    }
    printf("s04_classed_memset: %s: the %s page, memset correct\n", name, expect);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--child") == 0)
        return child(argv[2]);
    int line = sweep();
    if (line)
        return line;
    int pagemap = access("/proc/self/pagemap", R_OK) == 0;
    const char *best = avx2_cpu() && pagemap ? "avx2" : "sse2";
    if (strcmp(best, "sse2") == 0)
        printf("s04_classed_memset: no AVX2 class here (or no /proc/self/pagemap): every start keeps the SSE2 page\n");
    char *plain[] = {0};
    char *no_avx2[] = {"GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2", 0};
    char *compat[] = {"GLIBC_TUNABLES=liella.compat.no_classed_swap=1", 0};
    int bad = 0;
    bad |= start("default", PLAIN, plain, best) == 1;
    bad |= start("glibc.cpu.hwcaps=-AVX2", PLAIN, no_avx2, "sse2") == 1;
    bad |= start("COMPAT-20", PLAIN, compat, "sse2") == 1;
    bad |= start("PTRACE_TRACEME guard", TRACED, plain, "sse2") == 1;
    bad |= start("PR_SET_MDWE", MDWE, plain, best) == 1;
    return bad;
}
