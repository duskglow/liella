/* The mount-table and fstab functions end to end (spec §13.13; D-13.8, D-13.10; phase 12): built
 * against the host C library and against Liella's products, the outputs must match line for line.
 * Black-box: every line is a result, an errno, or a fact read back through a public function.
 *
 * Output conventions: strings are printed in brackets with every byte outside printable ASCII as
 * \xNN and a backslash as \\; a run of more than 60 equal bytes is printed as <c>*<n>; the scratch
 * directory is printed as <dir>. Nothing prints a descriptor number, an address or a time; the
 * offsets printed for getmntent_r are offsets into the caller's buffer.
 *
 * Usage: mntent_diff DIR [root], normally through mntent_run.sh. DIR must be an absolute path
 * without symbolic links (the harness passes `pwd -P`).
 *  - Without a second argument (an ordinary user): parsing of fixture files, getmntent's storage,
 *    getmntent_r's buffer protocol, addmntent, setmntent modes and errors, endmntent, hasmntopt,
 *    and only the structure of the real /proc/self/mounts. It then leaves DIR/etc (a fixture /etc
 *    with fstab, mtab and a marker file) and DIR/m/<names> (mount points) for the second phase.
 *  - `root`: the harness runs it as root in a private mount namespace with tmpfs file systems
 *    mounted on DIR/m/<names> and DIR/etc bind-mounted over /etc. The program refuses to run
 *    unless /etc holds the fixture's marker file, so it never touches the real /etc. It covers the
 *    fstab functions, _PATH_MOUNTED as a regular file and as a link to /proc/self/mounts, and the
 *    kernel's escaping of the probe's own mounts. */
#define _GNU_SOURCE
#undef _FORTIFY_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <fstab.h>
#include <limits.h>
#include <mntent.h>
#include <paths.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include "tscale.h"
#include "../../../../conformance/c/expect_crash.h" /* the expected-crash helper (CORE-STORM) */

static char dir[PATH_MAX];
static size_t dirlen;

/* ---------------------------------------------------------------- formatting helpers */

static const char *en(int e)
{
    static char b[4][24];
    static int k;
    switch (e) {
    case 0: return "0";
    case EPERM: return "EPERM";
    case ENOENT: return "ENOENT";
    case EBADF: return "EBADF";
    case ENOMEM: return "ENOMEM";
    case EACCES: return "EACCES";
    case EEXIST: return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EISDIR: return "EISDIR";
    case EINVAL: return "EINVAL";
    case ERANGE: return "ERANGE";
    case ENOSPC: return "ENOSPC";
    case EINTR: return "EINTR";
    case EFAULT: return "EFAULT";
    case ENAMETOOLONG: return "ENAMETOOLONG";
    case ELOOP: return "ELOOP";
    case EROFS: return "EROFS";
    default:
        k = (k + 1) % 4;
        snprintf(b[k], sizeof b[k], "errno%d", e);
        return b[k];
    }
}

/* A visible rendering of a string: <dir> for the scratch directory, \xNN outside printable ASCII,
 * \\ for a backslash, <c>*<n> for runs over 60. Rotating static buffers. */
static const char *vis_n(const char *s, size_t n)
{
    static char b[6][1024];
    static int k;
    if (!s)
        return "(null)";
    k = (k + 1) % 6;
    char *o = b[k];
    size_t w = 0, cap = sizeof b[k] - 16;
    o[w++] = '[';
    for (size_t i = 0; i < n && w < cap;) {
        if (dirlen && n - i >= dirlen && memcmp(s + i, dir, dirlen) == 0) {
            w += (size_t)snprintf(o + w, sizeof b[k] - w, "<dir>");
            i += dirlen;
            continue;
        }
        size_t run = 1;
        while (i + run < n && s[i + run] == s[i])
            run++;
        unsigned char c = (unsigned char)s[i];
        if (run > 60) {
            if (c >= 0x20 && c < 0x7f && c != '\\')
                w += (size_t)snprintf(o + w, sizeof b[k] - w, "<%c>*%zu", c, run);
            else
                w += (size_t)snprintf(o + w, sizeof b[k] - w, "<\\x%02x>*%zu", c, run);
            i += run;
            continue;
        }
        if (c == '\\')
            w += (size_t)snprintf(o + w, sizeof b[k] - w, "\\\\");
        else if (c >= 0x20 && c < 0x7f)
            o[w++] = (char)c;
        else
            w += (size_t)snprintf(o + w, sizeof b[k] - w, "\\x%02x", c);
        i++;
    }
    if (w >= cap)
        w += (size_t)snprintf(o + w, sizeof b[k] - w, "...");
    o[w++] = ']';
    o[w] = 0;
    return o;
}

static const char *vis(const char *s)
{
    return s ? vis_n(s, strlen(s)) : "(null)";
}

static void show_ent(const char *what, const struct mntent *m)
{
    if (!m) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: %s %s %s %s %d %d\n", what, vis(m->mnt_fsname), vis(m->mnt_dir), vis(m->mnt_type), vis(m->mnt_opts), m->mnt_freq,
           m->mnt_passno);
}

static const char *path(const char *name)
{
    static char p[4][PATH_MAX + 256];
    static int k;
    k = (k + 1) % 4;
    snprintf(p[k], sizeof p[k], "%s/%s", dir, name);
    return p[k];
}

static void write_bytes(const char *name, const char *text, size_t n)
{
    FILE *f = fopen(path(name), "w");
    if (!f) {
        printf("cannot create %s: %s\n", name, en(errno));
        exit(1);
    }
    fwrite(text, 1, n, f);
    fclose(f);
}

static void write_file(const char *name, const char *text)
{
    write_bytes(name, text, strlen(text));
}

/* The whole content of a file, rendered. */
static void show_file(const char *what, const char *name)
{
    static char buf[8192];
    FILE *f = fopen(name, "r");
    if (!f) {
        printf("%s: cannot open %s\n", what, en(errno));
        return;
    }
    size_t n = fread(buf, 1, sizeof buf, f);
    fclose(f);
    printf("%s (%zu bytes): %s\n", what, n, vis_n(buf, n));
}

/* Run fn in a child process; report a signal. Output is flushed before and in the child. */
static void in_child_t(const char *what, void (*fn)(void), unsigned secs)
{
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        tw_alarm(secs);
        expect_crash(); /* fn may kill the process: that is what is reported */
        fn();
        fflush(stdout);
        _exit(0);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    if (WIFSIGNALED(st))
        printf("%s: child killed by signal %s\n", what, WTERMSIG(st) == SIGSEGV ? "SIGSEGV" : WTERMSIG(st) == SIGABRT ? "SIGABRT" : WTERMSIG(st) == SIGALRM ? "SIGALRM" : "other");
    else
        printf("%s: child exited %d\n", what, WEXITSTATUS(st));
}

static void in_child(const char *what, void (*fn)(void))
{
    in_child_t(what, fn, 20);
}

/* ---------------------------------------------------------------- parsing */

/* Read every entry of a fixture with getmntent; at most 40 entries. */
static void parse_case(const char *label, const char *text, size_t n)
{
    printf("-- %s\n", label);
    write_bytes("fx", text, n);
    FILE *f = setmntent(path("fx"), "r");
    if (!f) {
        printf("setmntent: NULL %s\n", en(errno));
        return;
    }
    struct mntent *m;
    int k = 0;
    errno = 0;
    while (k < 40 && (m = getmntent(f)) != NULL) {
        char w[32];
        snprintf(w, sizeof w, "ent %d", k);
        show_ent(w, m);
        printf("  pos %ld\n", ftell(f));
        k++;
        errno = 0;
    }
    printf("end: %d entries, errno=%s feof=%d ferror=%d\n", k, en(errno), feof(f) != 0, ferror(f) != 0);
    errno = EINTR;
    m = getmntent(f);
    printf("again: %s errno=%s\n", m ? "entry" : "NULL", en(errno));
    printf("endmntent: %d\n", endmntent(f));
}

#define PC(label, lit) parse_case(label, lit, sizeof(lit) - 1)

static void parsing(void)
{
    printf("== parsing\n");
    PC("basic", "/dev/sda1 / ext4 rw,relatime 0 1\n");
    PC("tabs", "/dev/sda2\t/boot\tvfat\tdefaults\t0\t2\n");
    PC("runs of blanks", "  /dev/sda3 \t  /home\t\t ext4   rw,noatime \t 1   2  \n");
    PC("trailing blanks only after passno", "a b c d 0 0 \t \n");
    PC("escapes \\040 \\011 \\012 \\134", "/dev/a\\040b /mnt/x\\011y\\012z\\134w ext4 rw 0 0\n");
    PC("escapes in type and opts", "src /m t\\040y\\134pe o\\040p\\011t 0 0\n");
    PC("escape in freq", "src /m t o \\0601 2\n");
    PC("other octal \\101 \\041 \\177 \\200 \\377", "s\\101\\041 /\\177\\200\\377 t o 0 0\n");
    PC("short octal \\0 \\00 \\1 \\12 \\04x", "a\\0b /c\\00d e\\1f g\\12h 0 0\nx\\04y /z t o 0 0\n");
    PC("\\000", "a\\000b /m t o 0 0\n");
    PC("out-of-range octal \\400 \\777", "a\\400b /c\\777d t o 0 0\n");
    PC("non-octal \\8 \\9 \\x41 \\\\ \\n", "a\\8b /c\\9d t\\x41 o\\\\p\\n 0 0\n");
    PC("backslash at end of field and of line", "a\\ /b t o 0 0\nc /d t o\\\n");
    PC("backslash before blank", "a\\ b /c t o 0 0\n");
    PC("comments", "# comment\n  # indented\n\t#tab\na#b /c#d t o 0 0 # trailing\n#\nx /y t o 0 0\n");
    PC("blank lines", "\n\n   \n\t\n a /b t o 0 0\n\n");
    PC("one field", "only\n");
    PC("two fields", "src /dir\n");
    PC("three fields", "src /dir type\n");
    PC("four fields", "src /dir type opts\n");
    PC("five fields", "src /dir type opts 3\n");
    PC("seven fields", "src /dir type opts 1 2 extra\n");
    PC("eight fields", "src /dir type opts 1 2 x y\n");
    PC("missing fields between good lines", "a /a t o 0 0\nb\nc /c t o 0 0\nd /d\ne /e t o 0 0\n");
    PC("freq and passno words", "a /a t o x y\n");
    PC("freq and passno signs", "a /a t o -1 +2\n");
    PC("freq and passno large", "a /a t o 2147483647 2147483648\nb /b t o 99999999999 -2147483649\n");
    PC("freq and passno hex and octal", "a /a t o 0x10 010\n");
    PC("freq and passno trailing letters", "a /a t o 1abc 2x\n");
    PC("passno only letters, freq good", "a /a t o 5 q\n");
    PC("empty opts via escape", "a /a t \\040 0 0\n");
    PC("no newline at end", "a /a t o 0 0\nb /b t o 1 2");
    PC("no newline, five fields", "a /a t o 1");
    PC("CRLF", "a /a t o 0 1\r\nb /b t o\r\nc /c t\r\n");
    PC("CR inside", "a\r /a t o 0 0\n");
    PC("empty file", "");
    PC("only comments", "# x\n#y\n");
    PC("NUL in a line", "a\0b /a t o 0 0\nc /c t o 0 0\n");
    PC("NUL in a line, then two lines", "a /a\0 t o 0 0\nc /c t o 0 0\nd /d t o 0 0\n");
    PC("NUL in the last line, no newline", "a /a t o 0 0\nb\0 /b t o 0 0");
    PC("a line of exactly one NUL", "\0\nc /c t o 0 0\nd /d t o 0 0\n");
    PC("UTF-8 bytes", "/dev/\xc3\xa9 /m\xc3\xa4 t o 0 0\n");
    PC("form feed and vertical tab as separators", "a\f/b\vt o 0 0\n");
    PC("leading comment char after blanks in a field", "a #b t o 0 0\n");
}

/* Lines longer than any internal buffer: a fsname of L bytes on the first line, then a short line. */
static void long_lines(void)
{
    printf("== long lines (getmntent)\n");
    static const size_t lens[] = {100, 1000, 4000, 4090, 4095, 4096, 4097, 4200, 8191, 8192, 8200, 20000, 100000};
    for (size_t i = 0; i < sizeof lens / sizeof lens[0]; i++) {
        size_t L = lens[i];
        char *t = malloc(L + 64);
        memset(t, 'L', L);
        size_t n = L;
        n += (size_t)sprintf(t + n, " /a t o 0 0\nnext /n t o 1 2\n");
        char label[64];
        snprintf(label, sizeof label, "fsname of %zu bytes", L);
        parse_case(label, t, n);
        free(t);
    }
    /* A long options field and a long last field without a newline. */
    {
        size_t L = 5000;
        char *t = malloc(L + 64);
        size_t n = (size_t)sprintf(t, "a /a t ");
        memset(t + n, 'o', L);
        n += L;
        n += (size_t)sprintf(t + n, " 1 2\nb /b t o 0 0");
        parse_case("opts of 5000 bytes", t, n);
        free(t);
    }
    {
        size_t L = 5000;
        char *t = malloc(L + 64);
        size_t n = (size_t)sprintf(t, "a /a t o 1 ");
        memset(t + n, '7', L);
        n += L;
        parse_case("passno of 5000 digits, no newline", t, n);
        free(t);
    }
}

/* ---------------------------------------------------------------- storage */

static void storage(void)
{
    printf("== getmntent storage\n");
    write_file("st", "first /first t1 o1 1 1\nsecond /second t2 o2 2 2\n");
    FILE *f = setmntent(path("st"), "r");
    struct mntent *a = getmntent(f);
    char *afs = a ? a->mnt_fsname : NULL;
    char saved[64];
    snprintf(saved, sizeof saved, "%s", afs ? afs : "");
    struct mntent *b = getmntent(f);
    printf("two calls return the same struct: %s\n", a == b ? "yes" : "no");
    printf("first fsname pointer still names: %s (was [%s])\n", vis(afs), saved);
    show_ent("struct after second call", a);
    endmntent(f);

    /* Two streams share the storage? */
    write_file("st2", "other /other t3 o3 3 3\n");
    FILE *f1 = setmntent(path("st"), "r"), *f2 = setmntent(path("st2"), "r");
    struct mntent *x = getmntent(f1);
    struct mntent *y = getmntent(f2);
    printf("two streams return the same struct: %s\n", x == y ? "yes" : "no");
    show_ent("stream 1's result after stream 2's call", x);
    show_ent("stream 1 next", getmntent(f1));
    endmntent(f1);
    endmntent(f2);

    /* getmntent_r's result is the caller's struct; getmntent is not disturbed. */
    f1 = setmntent(path("st"), "r");
    struct mntent *s = getmntent(f1);
    f2 = setmntent(path("st2"), "r");
    struct mntent me;
    char buf[256];
    struct mntent *r = getmntent_r(f2, &me, buf, sizeof buf);
    printf("getmntent_r returns its argument: %s\n", r == &me ? "yes" : "no");
    printf("getmntent_r fields inside buf: %s\n",
           r && r->mnt_fsname >= buf && r->mnt_fsname < buf + sizeof buf && r->mnt_dir >= buf && r->mnt_dir < buf + sizeof buf && r->mnt_type >= buf &&
                   r->mnt_type < buf + sizeof buf && r->mnt_opts >= buf && r->mnt_opts < buf + sizeof buf
               ? "yes"
               : "no");
    if (r)
        printf("getmntent_r offsets: fsname %td dir %td type %td opts %td\n", r->mnt_fsname - buf, r->mnt_dir - buf, r->mnt_type - buf, r->mnt_opts - buf);
    show_ent("getmntent's static entry after getmntent_r", s);
    endmntent(f1);
    endmntent(f2);
}

/* ---------------------------------------------------------------- getmntent_r buffer sizes */

static int cur_size;

/* One getmntent_r call with a buffer of cur_size bytes, then one with 256 bytes. */
static void r_size_one(void)
{
    int sz = cur_size;
    char *buf = malloc((size_t)sz + 32);
    memset(buf, '@', (size_t)sz + 32);
    FILE *f = setmntent(path("rs"), "r");
    struct mntent me;
    memset(&me, 0, sizeof me);
    errno = 0;
    struct mntent *r = getmntent_r(f, &me, buf, sz);
    int e = errno;
    int written = 0, beyond = 1;
    for (int j = 0; j < sz; j++)
        written += buf[j] != '@';
    for (int j = sz; j < sz + 32; j++)
        beyond &= buf[j] == '@';
    printf("size %d: %s errno=%s pos=%ld written=%d beyond untouched=%s\n", sz, r ? "entry" : "NULL", en(e), ftell(f), written, beyond ? "yes" : "no");
    if (r)
        show_ent("  entry", r);
    char big[256];
    struct mntent n2;
    errno = 0;
    r = getmntent_r(f, &n2, big, sizeof big);
    show_ent("  next (256)", r);
    free(buf);
    endmntent(f);
}

static void r_sizes(void)
{
    printf("== getmntent_r buffer sizes\n");
    static const char line[] = "/dev/sda1 /mnt ext4 rw,relatime 0 2\nnext /n t o 1 1\n";
    printf("first line: %zu bytes with the newline\n", strchr(line, '\n') - line + (size_t)1);
    write_file("rs", line);
    static const int sizes[] = {1, 2, 3, 5, 10, 11, 15, 16, 20, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 64};
    for (size_t i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        cur_size = sizes[i];
        char w[48];
        snprintf(w, sizeof w, "size %d (in a child, 3 s limit)", cur_size);
        in_child_t(w, r_size_one, 3);
    }
    /* A 300-byte line read into a 256-byte buffer, and then the rest. */
    char t[400];
    memset(t, 'F', 280);
    int n = 280 + sprintf(t + 280, " /d t o 1 2\nz /z t o 0 0\n");
    write_bytes("rs2", t, (size_t)n);
    FILE *f = setmntent(path("rs2"), "r");
    char buf[256];
    struct mntent me;
    for (int k = 0; k < 5; k++) {
        errno = 0;
        struct mntent *r = getmntent_r(f, &me, buf, sizeof buf);
        char w[48];
        snprintf(w, sizeof w, "256-byte buffer, call %d (errno=%s)", k, en(errno));
        show_ent(w, r);
        if (!r)
            break;
    }
    endmntent(f);
}

static void r_zero(void)
{
    FILE *f = setmntent(path("rs"), "r");
    struct mntent me;
    char b[1], b2[256];
    errno = 0;
    struct mntent *r = getmntent_r(f, &me, b, 0);
    int e = errno;
    printf("size 0: %s errno=%s pos=%ld\n", r ? "entry" : "NULL", en(e), ftell(f));
    show_ent("  next (256)", getmntent_r(f, &me, b2, sizeof b2));
    endmntent(f);
}

static void r_negative(void)
{
    FILE *f = setmntent(path("rs"), "r");
    struct mntent me;
    char b[256];
    errno = 0;
    struct mntent *r = getmntent_r(f, &me, b, -1);
    int e = errno;
    printf("size -1: %s errno=%s pos=%ld\n", r ? "entry" : "NULL", en(e), ftell(f));
    if (r)
        show_ent("  entry", r);
    char b2[256];
    show_ent("  next (256)", getmntent_r(f, &me, b2, sizeof b2));
    endmntent(f);
}

/* ---------------------------------------------------------------- setmntent, endmntent */

static void open_case(const char *label, const char *file, const char *mode)
{
    errno = 0;
    FILE *f = setmntent(file, mode);
    int e = errno;
    if (!f) {
        printf("setmntent(%s, \"%s\"): NULL %s\n", label, mode, en(e));
        return;
    }
    int fd = fileno(f);
    int fdfl = fcntl(fd, F_GETFD);
    int fl = fcntl(fd, F_GETFL);
    const char *acc = (fl & O_ACCMODE) == O_RDONLY ? "O_RDONLY" : (fl & O_ACCMODE) == O_WRONLY ? "O_WRONLY" : "O_RDWR";
    printf("setmntent(%s, \"%s\"): ok %s%s%s cloexec=%s pos=%ld\n", label, mode, acc, fl & O_APPEND ? "|O_APPEND" : "", fl & O_NONBLOCK ? "|O_NONBLOCK" : "",
           fdfl & FD_CLOEXEC ? "yes" : "no", ftell(f));
    errno = 0;
    struct mntent *m = getmntent(f);
    int ge = errno;
    printf("  getmntent: %s errno=%s ferror=%d\n", m ? "entry" : "NULL", en(ge), ferror(f) != 0);
    if (m)
        show_ent("  entry", m);
    printf("  endmntent: %d\n", endmntent(f));
}

static void end_null(void)
{
    printf("endmntent(NULL): %d\n", endmntent(NULL));
}

static void modes(void)
{
    printf("== setmntent modes and errors\n");
    write_file("mo", "m /m t o 0 0\n");
    const char *modes_[] = {"r", "r+", "w", "w+", "a", "a+", "re", "rm", "rb", "rx", "rc", "z", "", "+r", "ra"};
    for (size_t i = 0; i < sizeof modes_ / sizeof modes_[0]; i++) {
        write_file("mo", "m /m t o 0 0\n");
        open_case("file", path("mo"), modes_[i]);
    }
    open_case("missing", path("missing/none"), "r");
    open_case("missing file, w", path("absent"), "w");
    show_file("  created by w", path("absent"));
    unlink(path("absent"));
    open_case("below a file", path("mo/x"), "r");
    mkdir(path("adir"), 0755);
    open_case("directory", path("adir"), "r");
    open_case("directory", path("adir"), "w");
    open_case("directory", path("adir"), "a");
    write_file("noperm", "q /q t o 0 0\n");
    chmod(path("noperm"), 0);
    open_case("mode 0000 (as a user)", path("noperm"), "r");
    open_case("empty name", "", "r");
    open_case("/dev/null", "/dev/null", "r");
    printf("_PATH_MOUNTED=%s MOUNTED=%s _PATH_MNTTAB=%s MNTTAB=%s _PATH_FSTAB=%s\n", vis(_PATH_MOUNTED), vis(MOUNTED), vis(_PATH_MNTTAB), vis(MNTTAB),
           vis(_PATH_FSTAB));
    printf("MNTTYPE_IGNORE=%s MNTTYPE_NFS=%s MNTTYPE_SWAP=%s\n", vis(MNTTYPE_IGNORE), vis(MNTTYPE_NFS), vis(MNTTYPE_SWAP));
    printf("MNTOPT_DEFAULTS=%s MNTOPT_RO=%s MNTOPT_RW=%s\n", vis(MNTOPT_DEFAULTS), vis(MNTOPT_RO), vis(MNTOPT_RW));
    printf("MNTOPT_SUID=%s MNTOPT_NOSUID=%s MNTOPT_NOAUTO=%s\n", vis(MNTOPT_SUID), vis(MNTOPT_NOSUID), vis(MNTOPT_NOAUTO));
    printf("FSTAB_RW=%s FSTAB_RQ=%s FSTAB_RO=%s FSTAB_SW=%s FSTAB_XX=%s\n", vis(FSTAB_RW), vis(FSTAB_RQ), vis(FSTAB_RO), vis(FSTAB_SW), vis(FSTAB_XX));
    printf("sizeof struct mntent=%zu struct fstab=%zu\n", sizeof(struct mntent), sizeof(struct fstab));

    printf("-- endmntent\n");
    FILE *f = setmntent(path("mo"), "r");
    printf("endmntent(open stream): %d\n", endmntent(f));
    f = setmntent(path("mo"), "w");
    fputs("unflushed", f);
    printf("endmntent(stream with pending output): %d\n", endmntent(f));
    show_file("  file after endmntent", path("mo"));
    f = setmntent("/dev/full", "w");
    if (f) {
        fputs("pending", f);
        errno = 0;
        int r = endmntent(f);
        printf("endmntent(/dev/full with pending output): %d errno=%s\n", r, en(errno));
    }
    in_child("endmntent(NULL)", end_null);
    f = fopen(path("mo"), "r");
    printf("endmntent(stream from fopen): %d\n", endmntent(f));
    errno = 0;
    FILE *g = fopen(path("mo"), "r");
    struct mntent *m = getmntent(g);
    show_ent("getmntent on a stream from fopen", m);
    fclose(g);
}

/* ---------------------------------------------------------------- addmntent */

static void add_one(FILE *f, const char *fs, const char *d, const char *t, const char *o, int fr, int pa)
{
    struct mntent m = {(char *)fs, (char *)d, (char *)t, (char *)o, fr, pa};
    errno = 0;
    int r = addmntent(f, &m);
    printf("addmntent(%s %s %s %s %d %d): %d errno=%s\n", vis(fs), vis(d), vis(t), vis(o), fr, pa, r, en(r ? errno : 0));
}

static void add_null_field(void)
{
    FILE *f = setmntent(path("an"), "w");
    struct mntent m = {"fs", NULL, "t", "o", 0, 0};
    int r = addmntent(f, &m);
    printf("addmntent with a NULL mnt_dir: %d\n", r);
    endmntent(f);
    show_file("  file", path("an"));
}

static void add_null_opts(void)
{
    FILE *f = setmntent(path("an"), "w");
    struct mntent m = {"fs", "/d", "t", NULL, 0, 0};
    int r = addmntent(f, &m);
    printf("addmntent with a NULL mnt_opts: %d\n", r);
    endmntent(f);
    show_file("  file", path("an"));
}

static void roundtrip(const char *name)
{
    FILE *f = setmntent(path(name), "r");
    struct mntent *m;
    int k = 0;
    while (k < 40 && (m = getmntent(f)) != NULL) {
        char w[32];
        snprintf(w, sizeof w, "  read back %d", k++);
        show_ent(w, m);
    }
    endmntent(f);
}

static void adding(void)
{
    printf("== addmntent\n");
    FILE *f = setmntent(path("ad"), "w");
    add_one(f, "/dev/sda1", "/", "ext4", "rw,relatime", 0, 1);
    add_one(f, "a b", "/m p", "t y", "o p", 1, 2);
    add_one(f, "a\tb", "/m\tp", "t\ty", "o\tp", 0, 0);
    add_one(f, "a\nb", "/m\np", "t\ny", "o\np", 0, 0);
    add_one(f, "a\\b", "/m\\p", "t\\y", "o\\p", 0, 0);
    add_one(f, "a#b", "#m", "t", "o", 0, 0);
    add_one(f, "#hash", "/h", "t", "o", 0, 0);
    add_one(f, "a\rb", "/m\vp", "t\fy", "o", 0, 0);
    add_one(f, "\\040", "/x", "t", "o", 0, 0);
    add_one(f, "\xc3\xa9", "/\x01\x7f", "t", "o", 0, 0);
    add_one(f, "", "", "", "", 0, 0);
    add_one(f, "neg", "/n", "t", "o", -1, -2147483647 - 1);
    add_one(f, "big", "/b", "t", "o", 2147483647, 10);
    long pos = ftell(f);
    printf("ftell after the adds: %ld\n", pos);
    struct stat st;
    stat(path("ad"), &st);
    printf("file size before endmntent: %lld\n", (long long)st.st_size);
    printf("endmntent: %d\n", endmntent(f));
    show_file("file", path("ad"));
    roundtrip("ad");

    printf("-- addmntent and the stream position\n");
    write_file("ap", "one /1 t o 0 0\ntwo /2 t o 0 0\nthree /3 t o 0 0\n");
    f = setmntent(path("ap"), "r+");
    show_ent("read", getmntent(f));
    printf("pos before add: %ld\n", ftell(f));
    add_one(f, "added", "/added", "t", "o", 0, 0);
    printf("pos after add: %ld\n", ftell(f));
    show_ent("read after add", getmntent(f));
    printf("endmntent: %d\n", endmntent(f));
    show_file("file", path("ap"));

    write_file("ap", "one /1 t o 0 0\ntwo /2 t o 0 0\n");
    f = setmntent(path("ap"), "w+");
    add_one(f, "x", "/x", "t", "o", 0, 0);
    add_one(f, "y", "/y", "t", "o", 0, 0);
    rewind(f);
    printf("pos after rewind: %ld\n", ftell(f));
    add_one(f, "z", "/z", "t", "o", 0, 0);
    printf("pos after add: %ld\n", ftell(f));
    rewind(f);
    show_ent("w+ read 0", getmntent(f));
    show_ent("w+ read 1", getmntent(f));
    show_ent("w+ read 2", getmntent(f));
    show_ent("w+ read 3", getmntent(f));
    endmntent(f);
    show_file("file", path("ap"));

    write_file("ap", "one /1 t o 0 0\n");
    f = setmntent(path("ap"), "a");
    add_one(f, "app", "/app", "t", "o", 0, 0);
    endmntent(f);
    show_file("mode a", path("ap"));

    printf("-- addmntent errors and flushing\n");
    write_file("ar", "one /1 t o 0 0\n");
    f = setmntent(path("ar"), "r");
    add_one(f, "ro", "/ro", "t", "o", 0, 0);
    printf("ferror after: %d\n", ferror(f) != 0);
    endmntent(f);
    show_file("read-only stream's file", path("ar"));
    f = setmntent("/dev/full", "w");
    if (f) {
        add_one(f, "full", "/full", "t", "o", 0, 0);
        printf("ferror after: %d\n", ferror(f) != 0);
        errno = 0;
        int r = endmntent(f);
        printf("endmntent: %d\n", r);
    }
    f = setmntent(path("af"), "w");
    setvbuf(f, NULL, _IOFBF, 65536);
    add_one(f, "flush", "/f", "t", "o", 0, 0);
    stat(path("af"), &st);
    printf("size right after addmntent with a 64 KiB buffer: %lld\n", (long long)st.st_size);
    endmntent(f);
    in_child("NULL mnt_dir", add_null_field);
    in_child("NULL mnt_opts", add_null_opts);
}

/* ---------------------------------------------------------------- hasmntopt */

static void opt(const char *opts, const char *q)
{
    struct mntent m = {"fs", "/d", "t", (char *)opts, 0, 0};
    char *r = hasmntopt(&m, q);
    if (r)
        printf("hasmntopt(%s, %s): offset %td -> %s\n", vis(opts), vis(q), r - opts, vis(r));
    else
        printf("hasmntopt(%s, %s): NULL\n", vis(opts), vis(q));
}

static void opt_null(void)
{
    struct mntent m = {"fs", "/d", "t", NULL, 0, 0};
    char *r = hasmntopt(&m, "ro");
    printf("hasmntopt(NULL opts, ro): %s\n", r ? "found" : "NULL");
}

static void hasopt(void)
{
    printf("== hasmntopt\n");
    opt("ro", "ro");
    opt("rw", "ro");
    opt("rw,ro", "ro");
    opt("ro,rw", "rw");
    opt("rw,noro", "ro");
    opt("noro,ro", "ro");
    opt("rwx", "rw");
    opt("xrw", "rw");
    opt("xrw,rw", "rw");
    opt("rw=1", "rw");
    opt("rw=1", "rw=1");
    opt("rw=1", "rw=");
    opt("size=10,rw", "size");
    opt("size=10", "size=1");
    opt("size=10", "size=10");
    opt("size=10", "size=100");
    opt("size==1", "size=");
    opt("a,,b", "");
    opt("", "");
    opt("rw", "");
    opt(",rw,", "rw");
    opt(",", "");
    opt("RW", "rw");
    opt("rw", "RW");
    opt("noatime,atime", "atime");
    opt("relatime", "atime");
    opt("uid=0,gid=0", "id");
    opt("uid=0,gid=0", "gid");
    opt("defaults", "default");
    opt("user_xattr", "user");
    opt("user,user_xattr", "user_xattr");
    opt("user_xattr,user", "user");
    opt("a b,c", "c");
    opt("a b,c", "b");
    opt("a\tb,c", "b");
    opt("x=a,b", "b");
    opt("x=a,b", "a");
    opt("nouser,user", "user");
    opt("ro,", "ro");
    opt("mode=0755,ro", "ro");
    opt("ro", "r");
    opt("ro", "ro,");
    opt("rw,ro", "rw,ro");
    opt("rw,ro", "w,r");
    opt("context=\"a,b\",ro", "b\"");
    opt("context=\"a,b\",ro", "ro");
    opt("ro=", "ro");
    opt("ro==", "ro=");
    opt("=ro", "ro");
    opt("ro", "o");
    opt("aro,ro", "ro");
    opt("ro\n", "ro");
    opt("errors=remount-ro", "remount-ro");
    opt("errors=remount-ro", "ro");
    opt("errors=remount-ro,ro", "ro");
    opt("\xc3\xa9,ro", "\xc3\xa9");
    struct mntent m = {"fs", "/d", "t", "rw,ro", 0, 0};
    char *a = hasmntopt(&m, "ro");
    char *b = hasmntopt(&m, "ro");
    printf("hasmntopt returns a pointer into mnt_opts: %s; stable: %s\n", a == m.mnt_opts + 3 ? "yes" : "no", a == b ? "yes" : "no");
    in_child("hasmntopt NULL opts", opt_null);
}

/* ---------------------------------------------------------------- real /proc/self/mounts */

static void real_mounts(void)
{
    printf("== /proc/self/mounts, structure only\n");
    const char *files[] = {"/proc/self/mounts", "/proc/mounts", _PATH_MOUNTED};
    for (int i = 0; i < 3; i++) {
        FILE *f = setmntent(files[i], "r");
        if (!f) {
            printf("%s: NULL %s\n", files[i], en(errno));
            continue;
        }
        int n = 0, ok = 1, freq0 = 1, rwro = 1, root = 0;
        struct mntent *m;
        while ((m = getmntent(f)) != NULL && n < 100000) {
            if (!*m->mnt_fsname || !*m->mnt_dir || !*m->mnt_type || !*m->mnt_opts)
                ok = 0;
            if (m->mnt_freq || m->mnt_passno)
                freq0 = 0;
            if (!hasmntopt(m, "rw") == !hasmntopt(m, "ro"))
                rwro = 0;
            if (strcmp(m->mnt_dir, "/") == 0)
                root = 1;
            n++;
        }
        printf("%s: entries>0 %s; all four strings non-empty %s; freq and passno 0 %s; exactly one of rw/ro %s; / present %s\n", files[i],
               n > 0 ? "yes" : "no", ok ? "yes" : "no", freq0 ? "yes" : "no", rwro ? "yes" : "no", root ? "yes" : "no");
        endmntent(f);
    }
}

/* ---------------------------------------------------------------- fixture for the root phase */

static const char FX_FSTAB[] = "# fixture fstab\n"
                               "/dev/sda1 / ext4 defaults 0 1\n"
                               "\n"
                               "UUID=1234 /boot vfat rw,umask=077 0 2\n"
                               "/dev/sda3 /home ext4 ro,noatime 1 2\n"
                               "/dev/sda4 none swap sw 0 0\n"
                               "/dev/sda5 /ign ext4 xx 0 0\n"
                               "/dev/sda6 /quota ext4 rq 0 0\n"
                               "/dev/sda7 /noauto ext4 noauto,ro 0 0\n"
                               "/dev/sda8 /user ext4 user,rw 0 0\n"
                               "/dev/sda9 /swapro swap ro 0 0\n"
                               "/dev/sda10 /ignored ignore defaults 0 0\n"
                               "/dev/sda11 /rwro ext4 rw,ro 0 0\n"
                               "/dev/sda12 /other ext4 nosuid 0 0\n"
                               "/dev/sda13 /sw2 swap defaults 0 0\n"
                               "/dev/sda14 /xxfirst ext4 xx,rw 0 0\n"
                               "/dev/sp\\040ace /mnt/sp\\040ace ext4 defaults 0 0\n"
                               "  /dev/sda15\t/tabs\text4\tdefaults\t3\t4\n"
                               "/dev/short /short\n"
                               "/dev/sda16 /five ext4 rw 1\n"
                               "/dev/sda1 /dup ext4 rw 0 0\n"
                               "/dev/sda17 / ext4 rw 0 0\n"
                               "tmpfs /tmp tmpfs size=10%,mode=1777 0 0\n"
                               "/dev/sda18 /emptyopts ext4 \\040 0 0\n"
                               "/dev/o1 /o1 ext4 ro,rw 0 0\n"
                               "/dev/o2 /o2 ext4 rq,ro 0 0\n"
                               "/dev/o3 /o3 ext4 ro,rq 0 0\n"
                               "/dev/o4 /o4 ext4 sw,ro 0 0\n"
                               "/dev/o5 /o5 ext4 ro,sw 0 0\n"
                               "/dev/o6 /o6 ext4 xx,sw 0 0\n"
                               "/dev/o7 /o7 ext4 sw,xx 0 0\n"
                               "/dev/o8 /o8 ext4 xx,ro 0 0\n"
                               "/dev/o9 /o9 ext4 rq,sw 0 0\n"
                               "/dev/o10 /o10 ext4 sw,rq 0 0\n"
                               "/dev/o11 /o11 ext4 xx,rq 0 0\n"
                               "/dev/o12 /o12 ext4 ro=1 0 0\n"
                               "/dev/o13 /o13 ext4 noro,rwx 0 0\n"
                               "/dev/o14 /o14 swap defaults,sw 0 0\n"
                               "/dev/last /last ext4 ro 0 0";

static const char FX_MTAB[] = "fixture-src /fixture tmpfs rw,size=1k 0 0\n"
                              "other\\040src /other\\011dir ext4 ro 0 0\n";

static void make_fixture(void)
{
    mkdir(path("etc"), 0755);
    write_file("etc/fstab", FX_FSTAB);
    write_file("etc/mtab", FX_MTAB);
    write_file("etc/liella-mntent-fixture", "marker\n");
    mkdir(path("m"), 0755);
    const char *names[] = {"m/plain", "m/sp ace", "m/ta\tb", "m/nl\nc", "m/back\\sl", "m/ro", "m/bs", "m/hash#x", "m/opts"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++)
        mkdir(path(names[i]), 0755);
}

/* ---------------------------------------------------------------- root phase */

static void show_fs(const char *what, const struct fstab *f)
{
    if (!f) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: spec %s file %s vfstype %s mntops %s type %s freq %d passno %d\n", what, vis(f->fs_spec), vis(f->fs_file), vis(f->fs_vfstype),
           vis(f->fs_mntops), vis(f->fs_type), f->fs_freq, f->fs_passno);
}

static void fsent_cases(void)
{
    printf("== fstab functions over the fixture /etc/fstab\n");
    struct fstab *e;
    int n = 0;
    show_fs("getfsent before setfsent", getfsent());
    printf("setfsent: %d\n", setfsent());
    while ((e = getfsent()) != NULL && n < 60) {
        char w[32];
        snprintf(w, sizeof w, "getfsent %d", n++);
        show_fs(w, e);
    }
    errno = EINTR;
    e = getfsent();
    printf("getfsent after the end: %s errno=%s\n", e ? "entry" : "NULL", en(errno));
    endfsent();
    printf("setfsent: %d\n", setfsent());
    struct fstab *a = getfsent();
    char spec[64];
    snprintf(spec, sizeof spec, "%s", a ? a->fs_spec : "");
    struct fstab *b = getfsent();
    printf("two getfsent calls return the same struct: %s\n", a == b ? "yes" : "no");
    show_fs("first result after the second call", a);
    printf("(first spec was [%s])\n", spec);
    printf("setfsent again (open): %d\n", setfsent());
    show_fs("getfsent after setfsent again", getfsent());
    endfsent();

    printf("-- lookups\n");
    const char *specs[] = {"/dev/sda1", "/dev/sda3", "UUID=1234", "/dev/sda4", "/dev/sp ace", "/dev/sp\\040ace", "/dev/sda15", "/dev/short", "/dev/last",
                           "/dev/sda", "", "/dev/none", "tmpfs"};
    for (size_t i = 0; i < sizeof specs / sizeof specs[0]; i++) {
        char w[80];
        snprintf(w, sizeof w, "getfsspec %s", vis(specs[i]));
        show_fs(w, getfsspec(specs[i]));
    }
    const char *files[] = {"/", "/home", "none", "/mnt/sp ace", "/tabs", "/short", "/dup", "/last", "/ho", "", "/nowhere", "/emptyopts", "/ign"};
    for (size_t i = 0; i < sizeof files / sizeof files[0]; i++) {
        char w[80];
        snprintf(w, sizeof w, "getfsfile %s", vis(files[i]));
        show_fs(w, getfsfile(files[i]));
    }
    printf("-- lookups and the cursor\n");
    printf("setfsent: %d\n", setfsent());
    show_fs("getfsent", getfsent());
    show_fs("getfsent", getfsent());
    show_fs("getfsent", getfsent());
    show_fs("getfsspec /dev/sda1", getfsspec("/dev/sda1"));
    show_fs("getfsent after getfsspec", getfsent());
    show_fs("getfsfile /home", getfsfile("/home"));
    show_fs("getfsent after getfsfile", getfsent());
    show_fs("getfsfile /last", getfsfile("/last"));
    show_fs("getfsent after getfsfile /last", getfsent());
    show_fs("getfsspec missing", getfsspec("/dev/none"));
    show_fs("getfsent after a failed getfsspec", getfsent());
    endfsent();
    show_fs("getfsspec after endfsent", getfsspec("/dev/sda3"));
    show_fs("getfsent after that", getfsent());
    endfsent();
    endfsent();
    printf("endfsent twice: survived\n");
    struct fstab *p1 = getfsspec("/dev/sda3");
    struct fstab *p2 = getfsent();
    printf("getfsspec and getfsent share the struct: %s\n", p1 == p2 ? "yes" : "no");
    endfsent();
}

static void mtab_cases(void)
{
    printf("== _PATH_MOUNTED as a fixture file\n");
    FILE *f = setmntent(_PATH_MOUNTED, "r");
    if (!f) {
        printf("setmntent: NULL %s\n", en(errno));
    } else {
        struct mntent *m;
        int k = 0;
        while (k < 10 && (m = getmntent(f)) != NULL) {
            char w[32];
            snprintf(w, sizeof w, "mtab %d", k++);
            show_ent(w, m);
        }
        endmntent(f);
    }
    printf("== _PATH_MNTTAB through getmntent\n");
    f = setmntent(_PATH_MNTTAB, "r");
    if (f) {
        struct mntent *m;
        int k = 0;
        while (k < 40 && (m = getmntent(f)) != NULL) {
            char w[32];
            snprintf(w, sizeof w, "fstab %d", k++);
            show_ent(w, m);
        }
        endmntent(f);
    }
}

/* The probe's own mounts (below DIR), as /proc/self/mounts and _PATH_MOUNTED show them. */
static void own_mounts(const char *file)
{
    FILE *f = setmntent(file, "r");
    if (!f) {
        printf("%s: NULL %s\n", file, en(errno));
        return;
    }
    struct mntent *m;
    int n = 0, k = 0;
    while ((m = getmntent(f)) != NULL && n < 100000) {
        n++;
        if (strncmp(m->mnt_dir, dir, dirlen) == 0) {
            show_ent("  mount", m);
            printf("    hasmntopt rw %s ro %s size %s nosuid %s\n", hasmntopt(m, "rw") ? "yes" : "no", hasmntopt(m, "ro") ? "yes" : "no",
                   hasmntopt(m, "size") ? vis(hasmntopt(m, "size")) : "no", hasmntopt(m, "nosuid") ? "yes" : "no");
            k++;
        }
    }
    printf("%s: %d mounts below <dir>; entries>0 %s\n", file, k, n > 0 ? "yes" : "no");
    endmntent(f);
}

static void root_phase(void)
{
    struct stat st;
    if (stat("/etc/liella-mntent-fixture", &st) != 0) {
        printf("root phase: /etc is not the fixture; refusing to run\n");
        exit(1);
    }
    printf("== root phase\n");
    printf("running as root: %s\n", geteuid() == 0 ? "yes" : "no");
    fsent_cases();
    mtab_cases();

    printf("== own mounts\n");
    own_mounts("/proc/self/mounts");
    /* _PATH_MOUNTED as the link the system normally has. */
    unlink("/etc/mtab");
    int r = symlink("../proc/self/mounts", "/etc/mtab");
    printf("/etc/mtab -> ../proc/self/mounts: %d\n", r);
    own_mounts(_PATH_MOUNTED);

    printf("== fstab functions without /etc/fstab\n");
    rename("/etc/fstab", "/etc/fstab.away");
    errno = 0;
    {
        int sr = setfsent(), se = errno;
        printf("setfsent: %d errno=%s\n", sr, en(se));
    }
    show_fs("getfsent", getfsent());
    show_fs("getfsspec", getfsspec("/dev/sda1"));
    show_fs("getfsfile", getfsfile("/"));
    endfsent();
    printf("== /etc/fstab empty\n");
    FILE *f = fopen("/etc/fstab", "w");
    fclose(f);
    printf("setfsent: %d\n", setfsent());
    show_fs("getfsent", getfsent());
    endfsent();
    printf("== /etc/fstab a directory\n");
    unlink("/etc/fstab");
    mkdir("/etc/fstab", 0755);
    errno = 0;
    {
        int sr = setfsent(), se = errno;
        printf("setfsent: %d errno=%s\n", sr, en(se));
    }
    show_fs("getfsent", getfsent());
    endfsent();
    rmdir("/etc/fstab");
    printf("== /etc/fstab replaced while open\n");
    f = fopen("/etc/fstab", "w");
    fputs("/dev/a /a ext4 rw 0 0\n/dev/b /b ext4 rw 0 0\n", f);
    fclose(f);
    printf("setfsent: %d\n", setfsent());
    show_fs("getfsent", getfsent());
    f = fopen("/etc/fstab", "w");
    fputs("/dev/c /c ext4 ro 0 0\n", f);
    fclose(f);
    show_fs("getfsent after rewrite in place", getfsent());
    printf("setfsent again: %d\n", setfsent());
    show_fs("getfsent after setfsent again", getfsent());
    endfsent();
    rename("/etc/fstab.away", "/etc/fstab");
}

int main(int argc, char **argv)
{
    struct rlimit nocore = {0, 0};
    setrlimit(RLIMIT_CORE, &nocore);
    tw_alarm(300);
    if (argc < 2) {
        fprintf(stderr, "usage: mntent_diff DIR [root]\n");
        return 2;
    }
    snprintf(dir, sizeof dir, "%s", argv[1]);
    dirlen = strlen(dir);
    if (argc > 2 && strcmp(argv[2], "root") == 0) {
        root_phase();
    } else {
        printf("running as root: %s\n", geteuid() == 0 ? "yes" : "no");
        parsing();
        long_lines();
        storage();
        r_sizes();
        in_child_t("getmntent_r size 0", r_zero, 3);
        in_child_t("getmntent_r size -1", r_negative, 3);
        modes();
        adding();
        hasopt();
        real_mounts();
        const char *junk[] = {"fx", "st", "st2", "rs", "rs2", "mo", "noperm", "ad", "ap", "ar", "af", "an"};
        for (size_t i = 0; i < sizeof junk / sizeof junk[0]; i++)
            unlink(path(junk[i]));
        rmdir(path("adir"));
        make_fixture();
    }
    printf("done\n");
    return 0;
}
