/* probe.c - DL-DLOPEN-LOAD-2 black-box probe of the dlopen spec gaps (ruling #5266): the dlerror text
 * of a long non-ELF file, of other malformed ELF headers, of an object without PT_DYNAMIC, of EI_CLASS
 * values other than 1 and 2, and whether an RTLD_NOLOAD miss clears a pending dlerror text.
 *   probe DIR      (DIR holds libok.so, a valid object; the variants are written next to it)
 * Each case runs in a forked child (alarm(30)); the child prints "<case>: <dlerror text or ok>", the
 * parent prints "<case>: signal N" when the child dies. Public interfaces only. */
#include <dlfcn.h>
#include <elf.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *dir;
static unsigned char ok[1 << 20];
static size_t oklen;

static void put(const char *name, const void *p, size_t n) {
    char path[4096];
    snprintf(path, sizeof path, "%s/%s", dir, name);
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0 || write(fd, p, n) != (ssize_t)n) { perror(path); exit(2); }
    close(fd);
}

static void open_case(const char *name) {
    char path[4096];
    snprintf(path, sizeof path, "%s/%s", dir, name);
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        alarm(30);
        void *h = dlopen(path, RTLD_NOW);
        const char *e = dlerror();
        const char *t = e ? strstr(e, ": ") : NULL;
        printf("%s: %s\n", name, h ? "ok" : t ? t + 2 : e ? e : "(no text)");
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (WIFSIGNALED(st)) printf("%s: signal %d\n", name, WTERMSIG(st));
}

/* A copy of the valid object with `edit` applied. */
static void variant(const char *name, void (*edit)(unsigned char *)) {
    static unsigned char buf[sizeof ok];
    memcpy(buf, ok, oklen);
    edit(buf);
    put(name, buf, oklen);
    open_case(name);
}

static Elf64_Ehdr *eh(unsigned char *b) { return (Elf64_Ehdr *)b; }
static Elf64_Phdr *ph(unsigned char *b, int i) { return (Elf64_Phdr *)(b + eh(b)->e_phoff) + i; }
static void retype(unsigned char *b, Elf64_Word from, Elf64_Word to, int all) {
    for (int i = 0; i < eh(b)->e_phnum; i++)
        if (ph(b, i)->p_type == from) { ph(b, i)->p_type = to; if (!all) return; }
}
static void class0(unsigned char *b) { b[EI_CLASS] = 0; }
static void class3(unsigned char *b) { b[EI_CLASS] = 3; }
static void class255(unsigned char *b) { b[EI_CLASS] = 255; }
static void identver0(unsigned char *b) { b[EI_VERSION] = 0; }
static void ever0(unsigned char *b) { eh(b)->e_version = 0; }
static void phentsize32(unsigned char *b) { eh(b)->e_phentsize = 32; }
static void phnum0(unsigned char *b) { eh(b)->e_phnum = 0; }
static void ehsize0(unsigned char *b) { eh(b)->e_ehsize = 0; }
static void nodyn(unsigned char *b) { retype(b, PT_DYNAMIC, PT_NULL, 0); }
static void noload(unsigned char *b) { retype(b, PT_LOAD, PT_NULL, 1); }
static void misalign(unsigned char *b) {
    for (int i = 0; i < eh(b)->e_phnum; i++)
        if (ph(b, i)->p_type == PT_LOAD && ph(b, i)->p_offset) { ph(b, i)->p_vaddr += 1; ph(b, i)->p_paddr += 1; return; }
}
static void align3(unsigned char *b) { for (int i = 0; i < eh(b)->e_phnum; i++) if (ph(b, i)->p_type == PT_LOAD) { ph(b, i)->p_align = 3; return; } }

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: probe DIR\n"); return 2; }
    dir = argv[1];
    char path[4096];
    snprintf(path, sizeof path, "%s/libok.so", dir);
    int fd = open(path, O_RDONLY);
    ssize_t n = fd < 0 ? -1 : read(fd, ok, sizeof ok);
    if (n <= 0 || n == (ssize_t)sizeof ok) { fprintf(stderr, "probe: cannot read %s\n", path); return 2; }
    oklen = (size_t)n;
    close(fd);

    printf("== gaps\n");
    open_case("libok.so");
    static char text[65536];
    memset(text, 'x', sizeof text);
    put("text64.txt", text, 64);
    open_case("text64.txt");
    put("text4096.txt", text, 4096);
    open_case("text4096.txt");
    put("text65536.txt", text, sizeof text);
    open_case("text65536.txt");
    static char zero[4096];
    put("zero4096.bin", zero, sizeof zero);
    open_case("zero4096.bin");
    variant("class0.so", class0);
    variant("class3.so", class3);
    variant("class255.so", class255);
    variant("identver0.so", identver0);
    variant("ever0.so", ever0);
    variant("phentsize32.so", phentsize32);
    variant("phnum0.so", phnum0);
    variant("ehsize0.so", ehsize0);
    variant("nodyn.so", nodyn);
    variant("noload.so", noload);
    variant("misalign.so", misalign);
    variant("align3.so", align3);

    /* RTLD_NOLOAD miss after a failure: is the pending text still there? */
    printf("== noload-pending\n");
    fflush(stdout);
    snprintf(path, sizeof path, "%s/libok.so", dir);
    void *h = dlopen("/nonexistent/libnone.so", RTLD_NOW);
    void *h2 = dlopen(path, RTLD_NOW | RTLD_NOLOAD);
    const char *e = dlerror();
    printf("fail then NOLOAD miss: handle %s, dlerror %s\n", h || h2 ? "non-NULL" : "NULL", e ? e : "NULL");
    h = dlopen("/nonexistent/libnone.so", RTLD_NOW);
    h2 = dlopen(path, RTLD_NOW);
    e = dlerror();
    printf("fail then successful open: handle %s, dlerror %s\n", h2 ? "non-NULL" : "NULL", e ? e : "NULL");
    h = dlopen("/nonexistent/libnone.so", RTLD_NOW);
    h2 = dlopen(path, RTLD_NOW | RTLD_NOLOAD);
    e = dlerror();
    printf("fail then NOLOAD hit: handle %s, dlerror %s\n", h2 ? "non-NULL" : "NULL", e ? e : "NULL");
    (void)h;
    return 0;
}
