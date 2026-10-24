/* The user, group and shadow databases, login shells and login accounting end to end (spec
 * §17.3-17.6; phase 10): built against the host C library and against Liella's products, the
 * outputs must match line for line (users_products.rs). Black-box: every line is a result of a
 * public function.
 *
 * Usage: users_diff DIR [nss]. DIR is a scratch directory. Without `nss`, the stream functions
 * (fget*, sget*, put*) and utmp run on files in DIR. With `nss`, the program expects to run with
 * DIR/etc bind-mounted over /etc in a private mount namespace (the harness does this as root),
 * and runs the database lookups that read /etc. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <gshadow.h>
#include <pwd.h>
#include <shadow.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <utmp.h>
#include <utmpx.h>

/* Groups fixture_etc appends to the fixture group file (GETGROUPLIST-NAMES-PROBE). */
#define BIG_GROUPS 50000u

static char dir[512];

static const char *path(const char *name)
{
    static char p[4][768];
    static int k;
    k = (k + 1) % 4;
    snprintf(p[k], sizeof p[k], "%s/%s", dir, name);
    return p[k];
}

static void write_file(const char *name, const char *text)
{
    FILE *f = fopen(path(name), "w");
    fputs(text, f);
    fclose(f);
}

static void show_pw(const char *what, const struct passwd *p)
{
    if (!p) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: [%s][%s] %u %u [%s][%s][%s]\n", what, p->pw_name, p->pw_passwd, p->pw_uid, p->pw_gid, p->pw_gecos, p->pw_dir, p->pw_shell);
}

static void show_gr(const char *what, const struct group *g)
{
    if (!g) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: [%s][%s] %u members:", what, g->gr_name, g->gr_passwd, g->gr_gid);
    for (char **m = g->gr_mem; *m; m++)
        printf(" [%s]", *m);
    printf("\n");
}

static void show_sp(const char *what, const struct spwd *s)
{
    if (!s) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: [%s][%s] %ld %ld %ld %ld %ld %ld %lu\n", what, s->sp_namp, s->sp_pwdp, s->sp_lstchg, s->sp_min, s->sp_max, s->sp_warn, s->sp_inact,
           s->sp_expire, s->sp_flag);
}

static void show_sg(const char *what, const struct sgrp *g)
{
    if (!g) {
        printf("%s: NULL\n", what);
        return;
    }
    printf("%s: [%s][%s] admins:", what, g->sg_namp, g->sg_passwd);
    for (char **m = g->sg_adm; *m; m++)
        printf(" [%s]", *m);
    printf(" members:");
    for (char **m = g->sg_mem; *m; m++)
        printf(" [%s]", *m);
    printf("\n");
}

static const char FX_PASSWD[] = "root:x:0:0:root:/root:/bin/bash\n"
                             "alice:x:1000:1000:Alice Liddell,,,:/home/alice:/bin/sh\n"
                             "# a comment\n"
                             "\n"
                             "bob:*:1001:100::/home/bob:\n"
                             "short:x:1002\n"
                             "big:x:4294967295:4294967296:Big:/:/bin/sh\n"
                             "neg:x:-1:-2:Neg:/:/bin/sh\n"
                             "hex:x:0x10:010:Hex:/:/bin/sh\n"
                             "+nisuser::::::\n"
                             "-excluded::::::\n"
                             "+@netgroup::::::\n"
                             "  spaced:x:1003:1003:Spaced:/:/bin/sh\n"
                             "trail:x:1004:1004:Trail:/home/trail:/bin/sh:extra\n"
                             "empty::1005:1005::::\n"
                             "longusername12:x:1007:1007:Long:/:/bin/sh\n"
                             "eight888:x:1008:1008:Eight:/:/bin/sh\n"
                             "nonl:x:1006:1006:No newline:/:/bin/sh";

static const char FX_GROUP[] = "root:x:0:\n"
                            "users:x:100:alice,bob\n"
                            "wheel:x:10:alice\n"
                            "# comment\n"
                            "empty:x:200:\n"
                            "trailing:x:201:alice,\n"
                            "doubled:x:202:alice,,bob\n"
                            "short:x\n"
                            "big:x:4294967296:alice\n"
                            "spaced:x:203: alice , bob \n"
                            "alice:x:1000:\n"
                            "many:x:204:a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y,z,alice\n"
                            "+:::\n";

static const char FX_SHADOW[] = "root:!:19000:0:99999:7:::\n"
                             "alice:$6$salt$hash:19500:1:90:14:30:20000:\n"
                             "bob:*:::::::\n"
                             "short:x:1\n"
                             "flag:x:1:2:3:4:5:6:7\n"
                             "neg:x:-5:-1:-1:-1:-1:-1:\n"
                             "big:x:99999999999999999999:1:2:3:4:5:\n"
                             "# comment\n";

static const char FX_GSHADOW[] = "root:*::\n"
                              "users:!:alice:alice,bob\n"
                              "wheel::admin1,admin2:alice\n"
                              "short:x\n"
                              "empty:::\n";

static void streams(void)
{
    printf("== fgetpwent\n");
    write_file("passwd", FX_PASSWD);
    FILE *f = fopen(path("passwd"), "r");
    struct passwd *p;
    int n = 0;
    errno = 0;
    while ((p = fgetpwent(f)) != NULL && n < 40) {
        show_pw("pw", p);
        n++;
    }
    printf("end errno=%d\n", errno);
    fclose(f);

    printf("== fgetpwent_r buffer sizes\n");
    for (size_t sz = 1; sz <= 64; sz *= 2) {
        f = fopen(path("passwd"), "r");
        struct passwd pw, *res = (struct passwd *)1;
        char *buf = malloc(sz);
        int r = fgetpwent_r(f, &pw, buf, sz, &res);
        printf("size %zu: r=%d res=%s", sz, r, res ? "set" : "NULL");
        if (r == 0 && res)
            printf(" name=[%s]", pw.pw_name);
        long pos = ftell(f);
        printf(" pos=%ld\n", pos);
        free(buf);
        fclose(f);
    }

    printf("== fgetgrent\n");
    write_file("group", FX_GROUP);
    f = fopen(path("group"), "r");
    struct group *g;
    n = 0;
    errno = 0;
    while ((g = fgetgrent(f)) != NULL && n < 40) {
        show_gr("gr", g);
        n++;
    }
    printf("end errno=%d\n", errno);
    fclose(f);
    printf("== fgetgrent_r buffer sizes\n");
    for (size_t sz = 4; sz <= 256; sz *= 4) {
        f = fopen(path("group"), "r");
        struct group gr, *res = (struct group *)1;
        char *buf = malloc(sz);
        int r = 0;
        for (int k = 0; k < 2 && r == 0; k++)
            r = fgetgrent_r(f, &gr, buf, sz, &res);
        printf("size %zu: r=%d res=%s\n", sz, r, res ? "set" : "NULL");
        free(buf);
        fclose(f);
    }

    printf("== fgetspent / sgetspent\n");
    write_file("shadow", FX_SHADOW);
    f = fopen(path("shadow"), "r");
    struct spwd *s;
    n = 0;
    while ((s = fgetspent(f)) != NULL && n < 40) {
        show_sp("sp", s);
        n++;
    }
    fclose(f);
    static const char *lines[] = {"carol:hash:1:2:3:4:5:6:", "carol:hash", "carol:hash:a:b:c:d:e:f:", "", "carol::::::::", "carol:h:1:2:3:4:5:6:7:8"};
    for (size_t i = 0; i < sizeof lines / sizeof lines[0]; i++) {
        char name[64];
        snprintf(name, sizeof name, "sgetspent [%s]", lines[i]);
        show_sp(name, sgetspent(lines[i]));
    }

    printf("== fgetsgent / sgetsgent\n");
    write_file("gshadow", FX_GSHADOW);
    f = fopen(path("gshadow"), "r");
    struct sgrp *sg;
    n = 0;
    while ((sg = fgetsgent(f)) != NULL && n < 40) {
        show_sg("sg", sg);
        n++;
    }
    fclose(f);
    show_sg("sgetsgent [g:p:a1,a2:m1]", sgetsgent("g:p:a1,a2:m1"));
    show_sg("sgetsgent [g:p]", sgetsgent("g:p"));

    printf("== put*\n");
    f = fopen(path("out"), "w");
    struct passwd pw = {"dave", "x", 1010, 1010, "Dave, Jr.", "/home/dave", "/bin/zsh"};
    printf("putpwent: %d\n", putpwent(&pw, f));
    struct passwd plus = {"+nis", "", 0, 0, "", "", ""};
    printf("putpwent +nis: %d\n", putpwent(&plus, f));
    struct passwd colon = {"bad:name", "x", 1, 1, "", "/", "/bin/sh"};
    errno = 0;
    int rc = putpwent(&colon, f);
    printf("putpwent with colon: %d errno=%d\n", rc, errno);
    char *mem[] = {"alice", "bob", NULL};
    struct group gr = {"staff", "x", 50, mem};
    printf("putgrent: %d\n", putgrent(&gr, f));
    char *nomem[] = {NULL};
    struct group gr0 = {"nobody", "", 65534, nomem};
    printf("putgrent empty: %d\n", putgrent(&gr0, f));
    struct spwd sp = {"dave", "$1$x$y", 19000, 0, 99999, 7, -1, -1, (unsigned long)-1};
    printf("putspent: %d\n", putspent(&sp, f));
    struct spwd sp2 = {"eve", "!", 1, 2, 3, 4, 5, 6, 7};
    printf("putspent all: %d\n", putspent(&sp2, f));
    char *adm[] = {"root", NULL};
    struct sgrp sgr = {"staff", "!", adm, mem};
    printf("putsgent: %d\n", putsgent(&sgr, f));
    fclose(f);
    f = fopen(path("out"), "r");
    char line[512];
    while (fgets(line, sizeof line, f))
        printf("out: %s", line);
    fclose(f);
    errno = 0;
    rc = putpwent(NULL, stdout);
    printf("putpwent NULL: %d errno=%d\n", rc, errno);
}

static void show_ut(const char *what, const struct utmp *u)
{
    if (!u) {
        printf("%s: NULL errno=%d\n", what, errno);
        return;
    }
    printf("%s: type=%d pid=%d line=[%.32s] id=[%.4s] user=[%.32s] host=[%.256s] tv=%d\n", what, u->ut_type, u->ut_pid, u->ut_line, u->ut_id,
           u->ut_user, u->ut_host, u->ut_tv.tv_sec);
}

static struct utmp rec(short type, pid_t pid, const char *line, const char *id, const char *user, int sec)
{
    struct utmp u;
    memset(&u, 0, sizeof u);
    u.ut_type = type;
    u.ut_pid = pid;
    strncpy(u.ut_line, line, sizeof u.ut_line);
    strncpy(u.ut_id, id, sizeof u.ut_id);
    strncpy(u.ut_user, user, sizeof u.ut_user);
    strncpy(u.ut_host, "host.example", sizeof u.ut_host);
    u.ut_tv.tv_sec = sec;
    return u;
}

static void utmp_cases(void)
{
    printf("== utmp\n");
    printf("sizeof utmp=%zu utmpx=%zu line=%zu user=%zu host=%zu id=%zu tv=%zu\n", sizeof(struct utmp), sizeof(struct utmpx),
           sizeof(((struct utmp *)0)->ut_line), sizeof(((struct utmp *)0)->ut_user), sizeof(((struct utmp *)0)->ut_host),
           sizeof(((struct utmp *)0)->ut_id), sizeof(((struct utmp *)0)->ut_tv));
    unlink(path("utmp"));
    int fd = creat(path("utmp"), 0644);
    close(fd);
    printf("utmpname: %d\n", utmpname(path("utmp")));
    setutent();
    errno = 0;
    show_ut("empty getutent", getutent());
    struct utmp a = rec(LOGIN_PROCESS, 100, "tty1", "1", "LOGIN", 1000);
    struct utmp b = rec(USER_PROCESS, 200, "pts/0", "ts/0", "alice", 2000);
    struct utmp c = rec(BOOT_TIME, 0, "~", "~~", "reboot", 500);
    setutent();
    show_ut("put a", pututline(&a));
    show_ut("put b", pututline(&b));
    show_ut("put c", pututline(&c));
    struct utmp b2 = rec(DEAD_PROCESS, 200, "pts/0", "ts/0", "", 3000);
    setutent();
    show_ut("put b2 (same id)", pututline(&b2));
    struct stat st;
    stat(path("utmp"), &st);
    printf("file size: %lld\n", (long long)st.st_size);
    setutent();
    int k = 0;
    struct utmp *u;
    while ((u = getutent()) != NULL && k < 10) {
        char nm[16];
        snprintf(nm, sizeof nm, "ent %d", k++);
        show_ut(nm, u);
    }
    struct utmp key;
    memset(&key, 0, sizeof key);
    key.ut_type = USER_PROCESS;
    strncpy(key.ut_id, "1", sizeof key.ut_id);
    setutent();
    errno = 0;
    show_ut("getutid USER id 1", getutid(&key));
    key.ut_type = DEAD_PROCESS;
    strncpy(key.ut_id, "ts/0", sizeof key.ut_id);
    setutent();
    show_ut("getutid DEAD ts/0", getutid(&key));
    key.ut_type = BOOT_TIME;
    setutent();
    show_ut("getutid BOOT_TIME", getutid(&key));
    key.ut_type = 99;
    setutent();
    errno = 0;
    show_ut("getutid bad type", getutid(&key));
    struct utmp lk;
    memset(&lk, 0, sizeof lk);
    strncpy(lk.ut_line, "tty1", sizeof lk.ut_line);
    setutent();
    show_ut("getutline tty1", getutline(&lk));
    strncpy(lk.ut_line, "pts/0", sizeof lk.ut_line);
    setutent();
    errno = 0;
    show_ut("getutline pts/0 (dead)", getutline(&lk));
    struct utmp buf, *res = NULL;
    setutent();
    int r = getutent_r(&buf, &res);
    printf("getutent_r: %d same=%d\n", r, res == &buf);
    endutent();

    printf("== wtmp\n");
    unlink(path("wtmp"));
    fd = creat(path("wtmp"), 0644);
    close(fd);
    updwtmp(path("wtmp"), &a);
    updwtmp(path("wtmp"), &b);
    stat(path("wtmp"), &st);
    printf("wtmp size after 2: %lld\n", (long long)st.st_size);
    unlink(path("nowtmp"));
    updwtmp(path("nowtmp"), &a);
    printf("updwtmp to a missing file creates it: %d\n", access(path("nowtmp"), F_OK) == 0);
    struct utmpx x;
    getutmpx(&b, &x);
    printf("getutmpx: type=%d user=[%s] line=[%s]\n", x.ut_type, x.ut_user, x.ut_line);
    struct utmp back;
    getutmp(&x, &back);
    printf("getutmp: same=%d\n", memcmp(&back, &b, sizeof b) == 0);
    printf("utmpxname: %d\n", utmpxname(path("utmp")));
    setutxent();
    struct utmpx *ux = getutxent();
    printf("getutxent: %s\n", ux ? ux->ut_user : "NULL");
    endutxent();
    errno = 0;
    printf("utmpname long: %d errno=%d\n", utmpname(""), errno);
}

/* D-17.5: the 32-bit ut_tv.tv_sec carries unsigned seconds (to 2106-02-07 06:28:15 UTC). The C
 * member stays a signed int32_t; a caller stores a later time as its unsigned 32-bit value and
 * reads it back as uint32_t. The files written must be byte for byte glibc's, before and after
 * 2038, and the utmp and utmpx interfaces must write the same bytes. */
static unsigned long dump_file(const char *what, const char *name)
{
    static unsigned char b[8192];
    FILE *f = fopen(path(name), "rb");
    size_t n = f ? fread(b, 1, sizeof b, f) : 0;
    if (f)
        fclose(f);
    unsigned long h = 5381;
    for (size_t i = 0; i < n; i++)
        h = (h * 33 + b[i]) & 0xffffffffUL;
    printf("%s: %zu bytes hash=%08lx\n", what, n, h);
    for (size_t i = 0; i + sizeof(struct utmp) <= n; i += sizeof(struct utmp)) {
        printf("  tv bytes:");
        for (size_t j = 340; j < 348; j++)
            printf(" %02x", b[i + j]);
        printf("\n");
    }
    return h;
}

static void y2038_cases(void)
{
    printf("== y2038\n");
    static const uint32_t secs[] = {0u, 1000u, 2147483647u, 2147483648u, 3000000000u, 4294967295u};
    const size_t n = sizeof secs / sizeof secs[0];
    unlink(path("y-utmp"));
    close(creat(path("y-utmp"), 0644));
    unlink(path("y-utmpx"));
    close(creat(path("y-utmpx"), 0644));
    unlink(path("y-wtmp"));
    close(creat(path("y-wtmp"), 0644));
    unlink(path("y-wtmpx"));
    close(creat(path("y-wtmpx"), 0644));
    struct utmp recs[6];
    for (size_t i = 0; i < n; i++) {
        char id[8];
        snprintf(id, sizeof id, "y%zu", i);
        recs[i] = rec(USER_PROCESS, 300 + (int)i, id, id, "late", 0);
        memcpy(&recs[i].ut_tv.tv_sec, &secs[i], sizeof secs[i]);
        recs[i].ut_tv.tv_usec = (int)i * 7;
    }
    utmpname(path("y-utmp"));
    setutent();
    for (size_t i = 0; i < n; i++) {
        pututline(&recs[i]);
        updwtmp(path("y-wtmp"), &recs[i]);
    }
    endutent();
    utmpxname(path("y-utmpx"));
    setutxent();
    for (size_t i = 0; i < n; i++) {
        struct utmpx x;
        getutmpx(&recs[i], &x);
        pututxline(&x);
        updwtmpx(path("y-wtmpx"), &x);
    }
    endutxent();
    unsigned long hu = dump_file("utmp", "y-utmp");
    unsigned long hx = dump_file("utmpx", "y-utmpx");
    unsigned long hw = dump_file("wtmp", "y-wtmp");
    unsigned long hwx = dump_file("wtmpx", "y-wtmpx");
    printf("utmp and utmpx agree: %d %d\n", hu == hx, hw == hwx);
    utmpname(path("y-utmp"));
    setutent();
    struct utmp *u;
    while ((u = getutent()) != NULL) {
        uint32_t s;
        memcpy(&s, &u->ut_tv.tv_sec, sizeof s);
        printf("getutent %.32s: sec=%u usec=%d\n", u->ut_line, (unsigned)s, (int)u->ut_tv.tv_usec);
    }
    endutent();
    utmpxname(path("y-wtmpx"));
    setutxent();
    struct utmpx *x;
    while ((x = getutxent()) != NULL) {
        uint32_t s;
        memcpy(&s, &x->ut_tv.tv_sec, sizeof s);
        printf("getutxent %.32s: sec=%u usec=%d\n", x->ut_line, (unsigned)s, (int)x->ut_tv.tv_usec);
    }
    endutxent();
}

static void nss(void)
{
    printf("== getpwnam / getpwuid\n");
    static const char *names[] = {"root", "alice", "bob", "nobody", "short", "big", "spaced", "+nisuser", "", "empty"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        errno = 0;
        struct passwd *p = getpwnam(names[i]);
        int e = errno;
        char w[64];
        snprintf(w, sizeof w, "getpwnam [%s] errno=%d", names[i], e);
        show_pw(w, p);
    }
    static const uid_t uids[] = {0, 1000, 1001, 4242, 65534};
    for (size_t i = 0; i < sizeof uids / sizeof uids[0]; i++) {
        errno = 0;
        struct passwd *p = getpwuid(uids[i]);
        int e = errno;
        char w[64];
        snprintf(w, sizeof w, "getpwuid %u errno=%d", uids[i], e);
        show_pw(w, p);
    }
    printf("== getpwnam_r ERANGE protocol\n");
    for (size_t sz = 1; sz <= 128; sz *= 2) {
        struct passwd pw, *res = (struct passwd *)1;
        char *buf = malloc(sz);
        int r = getpwnam_r("alice", &pw, buf, sz, &res);
        printf("size %zu: r=%d res=%s\n", sz, r, res ? "set" : "NULL");
        free(buf);
    }
    struct passwd pw, *res = (struct passwd *)1;
    char buf[1024];
    int r = getpwnam_r("nobody-here", &pw, buf, sizeof buf, &res);
    printf("getpwnam_r missing: r=%d res=%s\n", r, res ? "set" : "NULL");
    printf("sysconf GETPW_R_SIZE_MAX=%ld GETGR_R_SIZE_MAX=%ld\n", sysconf(_SC_GETPW_R_SIZE_MAX), sysconf(_SC_GETGR_R_SIZE_MAX));

    printf("== enumeration\n");
    setpwent();
    struct passwd *p;
    int n = 0;
    while ((p = getpwent()) != NULL && n < 40) {
        show_pw("getpwent", p);
        n++;
    }
    /* #409: the end leaves errno as it was, the _r form returns ENOENT. */
    errno = 77;
    printf("getpwent end: %s errno=%d\n", getpwent() ? "rec" : "NULL", errno);
    {
        struct passwd rr, *res = (struct passwd *)1;
        char b[1024];
        errno = 77;
        int rc = getpwent_r(&rr, b, sizeof b, &res);
        printf("getpwent_r end: r=%d res=%s errno=%d\n", rc, res ? "set" : "NULL", errno);
    }
    endpwent();
    setgrent();
    struct group *g;
    n = 0;
    while ((g = getgrent()) != NULL && n < 40) {
        show_gr("getgrent", g);
        n++;
    }
    /* #409: the end leaves errno as it was, the _r form returns ENOENT. */
    errno = 77;
    printf("getgrent end: %s errno=%d\n", getgrent() ? "rec" : "NULL", errno);
    {
        struct group rr, *res = (struct group *)1;
        char b[1024];
        errno = 77;
        int rc = getgrent_r(&rr, b, sizeof b, &res);
        printf("getgrent_r end: r=%d res=%s errno=%d\n", rc, res ? "set" : "NULL", errno);
    }
    endgrent();
    setspent();
    struct spwd *s;
    n = 0;
    while ((s = getspent()) != NULL && n < 40) {
        show_sp("getspent", s);
        n++;
    }
    /* #409: the end leaves errno as it was, the _r form returns ENOENT. */
    errno = 77;
    printf("getspent end: %s errno=%d\n", getspent() ? "rec" : "NULL", errno);
    {
        struct spwd rr, *res = (struct spwd *)1;
        char b[1024];
        errno = 77;
        int rc = getspent_r(&rr, b, sizeof b, &res);
        printf("getspent_r end: r=%d res=%s errno=%d\n", rc, res ? "set" : "NULL", errno);
    }
    endspent();
    setsgent();
    struct sgrp *sg;
    n = 0;
    while ((sg = getsgent()) != NULL && n < 40) {
        show_sg("getsgent", sg);
        n++;
    }
    /* #409: the end leaves errno as it was, the _r form returns ENOENT. */
    errno = 77;
    printf("getsgent end: %s errno=%d\n", getsgent() ? "rec" : "NULL", errno);
    {
        struct sgrp rr, *res = (struct sgrp *)1;
        char b[1024];
        errno = 77;
        int rc = getsgent_r(&rr, b, sizeof b, &res);
        printf("getsgent_r end: r=%d res=%s errno=%d\n", rc, res ? "set" : "NULL", errno);
    }
    endsgent();

    printf("== groups\n");
    show_gr("getgrnam users", getgrnam("users"));
    show_gr("getgrgid 10", getgrgid(10));
    errno = 0;
    g = getgrnam("nonesuch");
    printf("getgrnam missing: %s errno=%d\n", g ? "found" : "NULL", errno);
    show_sp("getspnam alice", getspnam("alice"));
    show_sg("getsgnam wheel", getsgnam("wheel"));
    gid_t groups[64];
    int ng = 64;
    r = getgrouplist("alice", 1000, groups, &ng);
    printf("getgrouplist alice: %d n=%d:", r, ng);
    for (int i = 0; i < ng && r >= 0; i++)
        printf(" %u", groups[i]);
    printf("\n");
    ng = 2;
    r = getgrouplist("alice", 1000, groups, &ng);
    printf("getgrouplist alice n=2: %d n=%d\n", r, ng);
    ng = 64;
    r = getgrouplist("nobody-here", 4242, groups, &ng);
    printf("getgrouplist missing user: %d n=%d first=%u\n", r, ng, groups[0]);
    /* GETGROUPLIST-NAMES-PROBE: lists from a group file with BIG_GROUPS more entries (fixture_etc):
     * the last entries, a wrapped member, base equal to a listed group, 51 groups with room for 64
     * and for 10, and a name that is only a prefix of members. */
    static const struct {
        const char *user;
        unsigned base;
        int room;
    } big[] = {{"bigu49999", 0, 64}, {"bigu00000", 0, 64}, {"bigu49999", 249999, 64}, {"biguall", 100, 64},
               {"biguall", 100, 10}, {"biguall", 200000, 64}, {"bigu", 0, 64}, {"bigu49999", 0, 2}};
    for (size_t i = 0; i < sizeof big / sizeof big[0]; i++) {
        ng = big[i].room;
        r = getgrouplist(big[i].user, big[i].base, groups, &ng);
        printf("getgrouplist %s base %u room %d: %d n=%d:", big[i].user, big[i].base, big[i].room, r, ng);
        for (int k = 0; k < ng && r >= 0; k++)
            printf(" %u", groups[k]);
        printf("\n");
    }
    char line[512];
    errno = 0;
    r = getpw(1000, line);
    printf("getpw 1000: %d [%s]\n", r, r == 0 ? line : "");
    errno = 0;
    r = getpw(4242, line);
    printf("getpw 4242: %d errno=%d\n", r, errno);

    printf("== getusershell\n");
    setusershell();
    char *sh;
    n = 0;
    while ((sh = getusershell()) != NULL && n < 20) {
        printf("shell [%s]\n", sh);
        n++;
    }
    endusershell();

    printf("== lckpwdf\n");
    r = lckpwdf();
    printf("lckpwdf: %d\n", r);
    printf("lock file exists: %d\n", access("/etc/.pwd.lock", F_OK) == 0);
    printf("ulckpwdf: %d\n", ulckpwdf());
    printf("ulckpwdf again: %d\n", ulckpwdf());

    /* cuserid (spec §12.12): the effective user's name, cut to L_cuserid - 1 bytes; NULL or an
     * empty string without an entry. The bytes after the terminator show how much is written. */
    printf("== cuserid (L_cuserid %d)\n", L_cuserid);
    static const uid_t cu[] = {0, 1000, 1007, 1008, 4242};
    for (size_t i = 0; i < sizeof cu / sizeof cu[0]; i++) {
        if (seteuid(cu[i]) != 0) {
            printf("seteuid %u failed\n", cu[i]);
            continue;
        }
        errno = 0;
        char *a = cuserid(NULL);
        int ea = errno;
        char *b = cuserid(NULL);
        char cb[16];
        memset(cb, 'Z', sizeof cb);
        errno = 0;
        char *c = cuserid(cb);
        int ec = errno;
        int back = seteuid(0);
        printf("euid %u: NULL [%s] errno=%d same=%d; buf %s [%s] errno=%d after:", cu[i], a ? a : "(null)", ea, a == b,
               c == cb ? "returned" : c ? "other" : "NULL", c == cb ? cb : "", ec);
        for (size_t k = strlen(cb) + 1; k < sizeof cb; k++)
            printf(" %c", cb[k] == 'Z' ? 'Z' : cb[k] == 0 ? '0' : '?');
        printf(" back=%d\n", back);
    }
}

/* The fixture /etc the `nss` run sees (DIR/etc, bind-mounted over /etc by the harness). */
static void fixture_etc(void)
{
    mkdir(path("etc"), 0755);
    write_file("etc/passwd", FX_PASSWD);
    write_file("etc/group", FX_GROUP);
    /* GETGROUPLIST-NAMES-PROBE: BIG_GROUPS more groups in the shape of the `names` stress profile
     * (group k lists users k and k+1, the last one wrapping to user 0; every 1000th also `biguall`). */
    FILE *f = fopen(path("etc/group"), "a");
    for (unsigned k = 0; k < BIG_GROUPS; k++)
        fprintf(f, "bigg%05u:x:%u:bigu%05u,bigu%05u%s\n", k, 200000 + k, k, (k + 1) % BIG_GROUPS, k % 1000 == 0 ? ",biguall" : "");
    fclose(f);
    write_file("etc/shadow", FX_SHADOW);
    write_file("etc/gshadow", FX_GSHADOW);
    write_file("etc/shells", "/bin/sh\n/bin/bash\n# comment\n\n  /usr/bin/zsh  \n/bin/csh");
    write_file("etc/nsswitch.conf", "passwd: files\ngroup: files\nshadow: files\ngshadow: files\n");
}

int main(int argc, char **argv)
{
    snprintf(dir, sizeof dir, "%s", argc > 1 ? argv[1] : "/tmp");
    if (argc > 2 && strcmp(argv[2], "nss") == 0) {
        nss();
    } else {
        streams();
        utmp_cases();
        y2038_cases();
        fixture_etc();
    }
    printf("done\n");
    return 0;
}
