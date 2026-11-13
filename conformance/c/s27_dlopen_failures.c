/* Spec §27.1 (s27_1_f_0 dlopen failures and their texts; s27_1_c_5 dlerror once, cleared by a
 * success; s27_1_c_1_2 modes 0, NOLOAD): the failure corpus of tools/probe/dl/run.sh ("errors")
 * as a self-checking program, with the probed texts of spec/behaviour/dynlink.md. The malformed
 * files are made here, in a fresh directory, from the plain helper dlf_c and from this program;
 * the missing dependency, undefined symbol and version cases are helpers (DLOPEN-HELPERS).
 * Each failed check prints its line; the exit status is the number of failed checks. */
/* DLOPEN-HELPERS: dlf_c dlf_gone dlf_missingdep dlf_undef dlf_ver dlf_ver2 dlf_w */
/* HELPER-FLAGS: dlf_gone -Wl,-soname,libdlf_absent.so */
/* HELPER-FLAGS: dlf_missingdep -ldlf_gone */
/* HELPER-FLAGS: dlf_ver -Wl,-soname,libdlf_ver.so -Wl,--version-script=dlf_v1.map */
/* HELPER-FLAGS: dlf_ver2 -Wl,-soname,libdlf_ver.so -Wl,--version-script=dlf_v2.map */
/* HELPER-FLAGS: dlf_w -ldlf_ver2 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int fails;
static char dir[4096], hdir[4096];
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_dlopen_failures: line %d: %s\n", __LINE__, #c); fails++; } } while (0)

static const char *at(const char *d, const char *f)
{
    static char b[4][4096];
    static int i;
    i = (i + 1) % 4;
    snprintf(b[i], sizeof b[i], "%s/%s", d, f);
    return b[i];
}

/* dlopen(path, mode) fails with exactly WANT (PREFIX ": " TAIL when PREFIX is given), once. */
static int refused(const char *path, int mode, const char *prefix, const char *tail)
{
    char want[8192];
    if (prefix)
        snprintf(want, sizeof want, "%s: %s", prefix, tail);
    else
        snprintf(want, sizeof want, "%s", tail);
    void *h = dlopen(path, mode);
    const char *e = dlerror();
    int ok = h == NULL && e != NULL && strcmp(e, want) == 0;
    if (!ok)
        fprintf(stderr, "s27_dlopen_failures: %s: want [%s], got %s [%s]\n", path, want, h ? "a handle" : "NULL", e ? e : "no text");
    return ok && dlerror() == NULL;
}

static int put(const char *path, const void *p, size_t n)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0)
        return 0;
    int ok = write(fd, p, n) == (ssize_t)n;
    return close(fd) == 0 && ok;
}

static unsigned char *slurp(const char *path, size_t *n)
{
    static unsigned char b[1 << 22];
    int fd = open(path, O_RDONLY);
    if (fd < 0)
        return NULL;
    ssize_t r = read(fd, b, sizeof b);
    close(fd);
    if (r <= 0 || (size_t)r == sizeof b)
        return NULL;
    *n = (size_t)r;
    return b;
}

/* A copy of SRC as dir/NAME with byte OFF set to V (OFF < 0: unchanged), truncated to KEEP bytes when KEEP > 0. */
static int copy(const char *src, const char *name, long off, unsigned char v, size_t keep)
{
    size_t n;
    unsigned char *b = slurp(src, &n);
    if (b == NULL || (off >= 0 && (size_t)off >= n))
        return 0;
    unsigned char old = off >= 0 ? b[off] : 0;
    if (off >= 0)
        b[off] = v;
    int ok = put(at(dir, name), b, keep && keep < n ? keep : n);
    if (off >= 0)
        b[off] = old;
    return ok;
}

int main(void)
{
    const char *h = getenv("CONF_HELPERS");
    const char *tmp = getenv("TMPDIR");
    if (h == NULL)
        return 100;
    snprintf(hdir, sizeof hdir, "%s", h);
    snprintf(dir, sizeof dir, "%s/s27_dlopen_failures.XXXXXX", tmp && *tmp ? tmp : "/tmp");
    if (mkdtemp(dir) == NULL)
        return 101;
    char c[4096];
    snprintf(c, sizeof c, "%s/libdlf_c.so", hdir);
    size_t self_n;
    unsigned char *self = slurp("/proc/self/exe", &self_n);
    int pie = self != NULL && self_n > 17 && self[16] == 3; /* this program's e_type: ET_DYN */
    int made = put(at(dir, "text.so"), "not an object\n", 14) && put(at(dir, "empty.so"), "", 0) &&
               put(at(dir, "short.so"), "\177ELF\002\001\001", 7) && mkdir(at(dir, "subdir"), 0755) == 0 &&
               copy(c, "class32.so", 4, 1, 0) && copy(c, "msb.so", 5, 2, 0) && copy(c, "osabi.so", 7, 97, 0) &&
               copy(c, "machine.so", 18, 40, 0) && copy(c, "trunc.so", -1, 0, 200) && copy(c, "etype_rel.so", 16, 1, 0) &&
               copy("/proc/self/exe", "exe_copy", -1, 0, 0);
    CHECK(made);

    /* The file itself: not there, not an object, not one this machine runs. */
    CHECK(refused(at(dir, "nope.so"), RTLD_NOW, at(dir, "nope.so"), "cannot open shared object file: No such file or directory"));
    CHECK(refused("libnope_xyz.so", RTLD_NOW, "libnope_xyz.so", "cannot open shared object file: No such file or directory"));
    CHECK(refused(at(dir, "text.so"), RTLD_NOW, at(dir, "text.so"), "file too short"));
    CHECK(refused(at(dir, "empty.so"), RTLD_NOW, at(dir, "empty.so"), "file too short"));
    CHECK(refused(at(dir, "short.so"), RTLD_NOW, at(dir, "short.so"), "file too short"));
    CHECK(refused(at(dir, "subdir"), RTLD_NOW, at(dir, "subdir"), "cannot read file data: Is a directory"));
    CHECK(refused(at(dir, "class32.so"), RTLD_NOW, at(dir, "class32.so"), "wrong ELF class: ELFCLASS32"));
    CHECK(refused(at(dir, "msb.so"), RTLD_NOW, at(dir, "msb.so"), "ELF file data encoding not little-endian"));
    CHECK(refused(at(dir, "osabi.so"), RTLD_NOW, at(dir, "osabi.so"), "ELF file OS ABI invalid"));
    /* Another machine's object is skipped like a missing file (one rule, plan §1). */
    CHECK(refused(at(dir, "machine.so"), RTLD_NOW, at(dir, "machine.so"), "cannot open shared object file: No such file or directory"));
    CHECK(refused(at(dir, "trunc.so"), RTLD_NOW, at(dir, "trunc.so"), "cannot read file data"));
    CHECK(refused(at(dir, "etype_rel.so"), RTLD_NOW, at(dir, "etype_rel.so"), "only ET_DYN and ET_EXEC can be loaded"));
    CHECK(refused(at(dir, "exe_copy"), RTLD_NOW, at(dir, "exe_copy"),
                  pie ? "cannot dynamically load position-independent executable" : "cannot dynamically load executable"));

    /* What the object needs: a dependency, a symbol, a version. */
    CHECK(refused(at(hdir, "libdlf_missingdep.so"), RTLD_NOW, "libdlf_absent.so", "cannot open shared object file: No such file or directory"));
    CHECK(refused(at(hdir, "libdlf_undef.so"), RTLD_NOW, at(hdir, "libdlf_undef.so"), "undefined symbol: dlf_undefined_xyz"));
    char ver[8192];
    snprintf(ver, sizeof ver, "version `VER_2' not found (required by %s)", at(hdir, "libdlf_w.so"));
    CHECK(refused(at(hdir, "libdlf_w.so"), RTLD_NOW, at(hdir, "libdlf_ver.so"), ver));

    /* Modes: none, an undefined bit, NOLOAD of an object not loaded (NULL and no text). */
    CHECK(refused(c, 0, c, "invalid mode for dlopen(): Invalid argument"));
    CHECK(refused(c, RTLD_NOW | 0x10, NULL, "invalid mode parameter"));
    CHECK(dlopen(c, RTLD_NOW | RTLD_NOLOAD) == NULL && dlerror() == NULL);

    /* Success: no text; a success clears a pending failure's text (P-6). */
    CHECK(dlopen(at(dir, "nope.so"), RTLD_NOW) == NULL);
    void *hc = dlopen(c, RTLD_NOW);
    CHECK(hc != NULL && dlerror() == NULL);
    CHECK(hc && dlopen(c, RTLD_NOW | RTLD_LAZY) == hc && dlopen(c, RTLD_NOW | RTLD_NOLOAD) == hc);
    CHECK(dlopen(NULL, RTLD_NOW) != NULL && dlopen("", RTLD_NOW) != NULL && dlerror() == NULL);

    static const char *made_files[] = { "text.so", "empty.so", "short.so", "class32.so", "msb.so", "osabi.so",
                                        "machine.so", "trunc.so", "etype_rel.so", "exe_copy" };
    for (size_t i = 0; i < sizeof made_files / sizeof *made_files; i++)
        unlink(at(dir, made_files[i]));
    rmdir(at(dir, "subdir"));
    rmdir(dir);
    return fails;
}
