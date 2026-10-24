// C++ exceptions thrown through the C library's frames (CXX-THROW-THROUGH-LIBC). Each case
// throws from a callback the library calls (a comparator, an action, an init routine, a signal
// handler, a stream cookie function, an exit handler) and catches in the caller, around the
// library call. Unwinding is done by the compiler's unwinder (libgcc), which reads the
// call-frame information of every frame it passes, the library's included.
//
// Every case runs in its own child (fork), with an alarm, so a case that aborts, hangs or
// corrupts the stack is reported as such and the next case still runs. A case prints one
// line: the exception that arrived (its code and text), how many destructors ran between the
// throw and the catch (the callback's guard and the caller's guard: 2), and what the case
// observes after the catch. The output names no path, address, descriptor or PID, so the
// host's C library and Liella print the same text.
//
// Library calls go through a function pointer of a type without `noexcept` (the headers mark
// many functions `noexcept`, and a call to one is not covered by the caller's handler).
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <ftw.h>
#include <link.h>
#include <malloc.h>
#include <pthread.h>
#include <search.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

struct Boom {
    int code;
    char text[24];
};

static int dtors;
struct Guard {
    ~Guard() { dtors++; }
};

[[noreturn]] static void boom(int code) {
    Guard g;
    Boom b{code, "through the library"};
    throw b;
}

// The pointer is read through a volatile object: the compiler cannot see the callee, so the
// call may throw.
template <class R, class... A> static R (*mt(R (*f)(A...)))(A...) {
    R (*volatile p)(A...) = f;
    return p;
}

static void report(const char *name, const Boom &b, const char *after) {
    std::printf("%s: caught code=%d text=\"%s\" dtors=%d%s%s\n", name, b.code, b.text, dtors,
                after[0] ? " " : "", after);
}

#define CASE(name, call, after)                                                                    \
    do {                                                                                           \
        dtors = 0;                                                                                 \
        try {                                                                                      \
            Guard caller;                                                                          \
            call;                                                                                  \
            std::printf("%s: returned without the exception\n", name);                           \
        } catch (const Boom &b) {                                                                  \
            report(name, b, after);                                                                \
        } catch (...) {                                                                            \
            std::printf("%s: caught another exception\n", name);                                  \
        }                                                                                          \
    } while (0)

// Comparators and actions that throw on their `at`-th call.
static int calls, at;
// State after a case: the heap in use, the open descriptors (OD-6 revised: the library gives
// back what it held when a callback threw).
static long heap() { return (long)mallinfo2().uordblks; }
static int fds() {
    int n = 0;
    for (int i = 0; i < 256; i++) n += fcntl(i, F_GETFD) != -1;
    return n;
}
static int cmp(const void *a, const void *b) {
    if (++calls == at) boom(calls);
    int x = *static_cast<const int *>(a), y = *static_cast<const int *>(b);
    return (x > y) - (x < y);
}
static int cmp_r(const void *a, const void *b, void *) { return cmp(a, b); }
static int ints[1000];
static void fill(int n) {
    for (int i = 0; i < n; i++) ints[i] = (i * 7919) % n;
}

static void c_qsort_small() {
    fill(8), calls = 0, at = 5;
    CASE("qsort 8 elements", mt(qsort)(ints, 8, sizeof(int), cmp), "");
}
static void c_qsort_large() {
    fill(1000), calls = 0, at = 3000;
    long h = heap();
    CASE("qsort 1000 elements", mt(qsort)(ints, 1000, sizeof(int), cmp), "");
    // The array is still a permutation of its elements, and qsort works again.
    int heap_back = heap() == h;
    long sum = 0;
    for (int i = 0; i < 1000; i++) sum += ints[i];
    calls = 0, at = -1;
    mt(qsort)(ints, 1000, sizeof(int), cmp);
    int sorted = 1;
    for (int i = 1; i < 1000; i++) sorted &= ints[i - 1] <= ints[i];
    std::printf("  after: sum=%ld sorted again=%d heap back=%d\n", sum, sorted, heap_back);
}
// A throw at every comparison of a 1000-element sort in turn: each time the array is still a
// permutation of its elements (a merge's elements in flight go back), and no heap is kept.
static void c_qsort_sweep() {
    long h = heap();
    int points = 0, perm = 1;
    for (int t = 1;; t++) {
        fill(1000), calls = 0, at = t;
        try {
            mt(qsort)(ints, 1000, sizeof(int), cmp);
        } catch (const Boom &) {
            points++;
        }
        static char seen[1000];
        std::memset(seen, 0, sizeof seen);
        for (int i = 0; i < 1000; i++) seen[ints[i]] = 1;
        for (int i = 0; i < 1000; i++) perm &= seen[i];
        if (calls < t) break;  // the sort finished before the throw point
    }
    std::printf("  after a throw at each of >1000 comparisons=%d: permutation kept=%d heap back=%d\n",
                points > 1000, perm, heap() == h);
}
static void c_qsort_r() {
    fill(64), calls = 0, at = 40;
    CASE("qsort_r 64 elements", mt(qsort_r)(ints, 64, sizeof(int), cmp_r, nullptr), "");
}
static void c_bsearch() {
    for (int i = 0; i < 100; i++) ints[i] = i;
    int key = 3;
    calls = 0, at = 4;
    CASE("bsearch", mt(bsearch)(&key, ints, 100, sizeof(int), cmp), "");
}
static void c_lfind() {
    for (int i = 0; i < 10; i++) ints[i] = i;
    int key = 9;
    size_t n = 10;
    calls = 0, at = 6;
    CASE("lfind", mt(lfind)(&key, ints, &n, sizeof(int), cmp), "");
    calls = 0, at = 6;
    CASE("lsearch", mt(lsearch)(&key, ints, &n, sizeof(int), cmp), "");
}

static int keys[64];
static void *root;
static void build_tree() {
    root = nullptr;
    calls = 0, at = -1;
    for (int i = 0; i < 64; i++) {
        keys[i] = (i * 37) % 64;
        tsearch(&keys[i], &root, cmp);
    }
}
static void act(const void *, VISIT v, int) {
    if (v == leaf && ++calls == at) boom(calls);
}
static void act_r(const void *, VISIT v, void *) {
    if (v == leaf && ++calls == at) boom(calls);
}
static void free_node(void *) {
    if (++calls == at) boom(calls);
}
static void noop(void *) {}
static void c_tsearch() {
    build_tree();
    int key = 64;
    calls = 0, at = 3;
    CASE("tsearch", mt(tsearch)(&key, &root, cmp), "");
    key = 5, calls = 0, at = 3;
    CASE("tfind", mt(tfind)(&key, &root, cmp), "");
    calls = 0, at = 3;
    CASE("tdelete", mt(tdelete)(&key, &root, cmp), "");
    // The tree is intact: every key is still found.
    calls = 0, at = -1;
    int found = 0;
    for (int i = 0; i < 64; i++) found += tfind(&keys[i], &root, cmp) != nullptr;
    std::printf("  after: %d of 64 keys found\n", found);
    calls = 0, at = 5;
    CASE("twalk", mt(twalk)(root, act), "");
    calls = 0, at = 5;
    CASE("twalk_r", mt(twalk_r)(root, act_r, nullptr), "");
    calls = 0, at = 1;
    CASE("tdestroy", mt(tdestroy)(root, free_node), "");
}

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int once_runs;
static void init_throws() {
    if (++once_runs == 1) boom(once_runs);
}
static void c_pthread_once() {
    CASE("pthread_once", mt(pthread_once)(&once, init_throws), "");
    // C++ call_once semantics: an init routine left by an exception has not run; the next call
    // runs it again.
    int rc = pthread_once(&once, init_throws);
    std::printf("  after: second pthread_once=%d init runs=%d\n", rc, once_runs);
}

static int phdr_cb(struct dl_phdr_info *, size_t, void *) {
    boom(7);
}
static void c_dl_iterate_phdr() {
    CASE("dl_iterate_phdr", mt(dl_iterate_phdr)(phdr_cb, nullptr), "");
    // The library's lock (if any) was not left held: a second walk returns.
    int n = 0;
    dl_iterate_phdr([](struct dl_phdr_info *, size_t, void *d) { return ++*static_cast<int *>(d), 0; }, &n);
    std::printf("  after: second walk returned, objects seen=%s\n", n > 0 ? "yes" : "no");
}

static char tree[4096];
static int ftw_cb(const char *, const struct stat *, int) {
    if (++calls == at) boom(calls);
    return 0;
}
static int nftw_cb(const char *, const struct stat *, int, struct FTW *) {
    if (++calls == at) boom(calls);
    return 0;
}
static int scan_filter(const struct dirent *d) {
    if (std::strcmp(d->d_name, ".") && std::strcmp(d->d_name, "..") && ++calls == at) boom(calls);
    return 1;
}
static int scan_cmp(const struct dirent **a, const struct dirent **b) {
    if (++calls == at) boom(calls);
    return std::strcmp((*a)->d_name, (*b)->d_name);
}
static void make_tree() {
    std::snprintf(tree, sizeof tree, "%s/cxxthrowXXXXXX", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
    if (!mkdtemp(tree)) std::printf("mkdtemp failed\n"), _exit(1);
    for (int i = 0; i < 6; i++) {
        char p[4200];
        std::snprintf(p, sizeof p, "%s/f%d", tree, i);
        close(open(p, O_CREAT | O_WRONLY, 0600));
    }
}
static void remove_tree() {
    for (int i = 0; i < 6; i++) {
        char p[4200];
        std::snprintf(p, sizeof p, "%s/f%d", tree, i);
        unlink(p);
    }
    rmdir(tree);
}
static void c_ftw() {
    make_tree();
    calls = 0, at = 3;
    int f0 = fds();
    CASE("ftw", mt(ftw)(tree, ftw_cb, 4), "");
    std::printf("  after: fds back=%d\n", fds() == f0);
    remove_tree();
}
static void c_nftw() {
    make_tree();
    calls = 0, at = 3;
    int f0 = fds();
    char cwd0[4096], cwd1[4096];
    if (!getcwd(cwd0, sizeof cwd0)) cwd0[0] = 0;
    CASE("nftw", mt(nftw)(tree, nftw_cb, 4, FTW_PHYS | FTW_CHDIR), "");
    if (!getcwd(cwd1, sizeof cwd1)) cwd1[0] = 0;
    std::printf("  after: fds back=%d cwd back=%d\n", fds() == f0, !std::strcmp(cwd0, cwd1));
    remove_tree();
}
static struct dirent **list;
static void c_scandir_filter() {
    make_tree();
    calls = 0, at = 3;
    int f0 = fds();
    long h = heap();
    CASE("scandir filter", mt(scandir)(tree, &list, scan_filter, nullptr), "");
    std::printf("  after: fds back=%d heap back=%d\n", fds() == f0, heap() == h);
    remove_tree();
}
static void c_scandir_compar() {
    make_tree();
    calls = 0, at = 3;
    int f0 = fds();
    long h = heap();
    CASE("scandir compar", mt(scandir)(tree, &list, nullptr, scan_cmp), "");
    std::printf("  after: fds back=%d heap back=%d\n", fds() == f0, heap() == h);
    remove_tree();
}

// A throw at every comparison of scandir's sort in turn (in each pass of the merge, also while
// the sort's spare copy holds the list): each time the directory is closed and the entries freed.
static void c_scandir_sweep() {
    make_tree();
    int f0 = fds(), points = 0, back = 1;
    long h = heap();
    for (int t = 1;; t++) {
        calls = 0, at = t;
        int n = -1;
        bool thrown = false;
        try {
            n = mt(scandir)(tree, &list, nullptr, scan_cmp);
        } catch (const Boom &) {
            thrown = true;
        }
        // Checked once the catch has ended: the exception object is heap until then.
        if (thrown) {
            points++;
            back &= fds() == f0 && heap() == h;
            continue;
        }
        for (int i = 0; i < n; i++) std::free(list[i]);
        std::free(list);
        break;
    }
    std::printf("  after a throw at each of >8 comparisons=%d: fds and heap back=%d\n", points > 8, back);
    remove_tree();
}
static void on_usr1(int) {
    boom(SIGUSR1);
}
static void on_usr2(int, siginfo_t *, void *) {
    boom(SIGUSR2);
}
static void mask_after(char *out, int sig) {
    sigset_t now;
    sigprocmask(SIG_SETMASK, nullptr, &now);
    std::snprintf(out, 64, "after: signal %s blocked", sigismember(&now, sig) ? "still" : "not");
}
static void c_signal() {
    struct sigaction sa {};
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, nullptr);
    char after[64] = "";
    try {
        Guard caller;
        mt(raise)(SIGUSR1);
        std::printf("signal handler via raise: returned without the exception\n");
    } catch (const Boom &b) {
        mask_after(after, SIGUSR1);
        report("signal handler via raise", b, after);
    }
    struct sigaction si {};
    si.sa_sigaction = on_usr2;
    si.sa_flags = SA_SIGINFO | SA_NODEFER;
    sigaction(SIGUSR2, &si, nullptr);
    dtors = 0;
    try {
        Guard caller;
        mt(kill)(getpid(), SIGUSR2);
        std::printf("SA_SIGINFO handler via kill: returned without the exception\n");
    } catch (const Boom &b) {
        mask_after(after, SIGUSR2);
        report("SA_SIGINFO handler via kill", b, after);
    }
}

static ssize_t ck_read(void *, char *, size_t) {
    boom(11);
}
static ssize_t ck_write(void *, const char *, size_t) {
    boom(12);
}
static void c_fopencookie() {
    cookie_io_functions_t io{};
    io.read = ck_read;
    io.write = ck_write;
    FILE *f = fopencookie(nullptr, "r+", io);
    char buf[8];
    CASE("fopencookie read via fread", mt(fread)(buf, 1, sizeof buf, f), "");
    dtors = 0;
    CASE("fopencookie read via fgetc", mt(fgetc)(f), "");
    dtors = 0;
    FILE *w = fopencookie(nullptr, "w", io);
    setvbuf(w, nullptr, _IONBF, 0);
    CASE("fopencookie write via fputs", mt(fputs)("x", w), "");
}

static void exit_handler() {
    boom(13);
}
static void c_exit() {
    atexit(exit_handler);
    std::fflush(stdout);
    CASE("atexit handler via exit", mt(exit)(0), "");
}

static void *thread_main(void *) {
    fill(1000), calls = 0, at = 2000;
    CASE("qsort in a second thread", mt(qsort)(ints, 1000, sizeof(int), cmp), "");
    return nullptr;
}
static void c_thread() {
    pthread_t t;
    pthread_create(&t, nullptr, thread_main, nullptr);
    pthread_join(t, nullptr);
}

static const struct {
    const char *name;
    void (*run)();
} cases[] = {
    {"qsort small", c_qsort_small},  {"qsort large", c_qsort_large},
    {"qsort sweep", c_qsort_sweep},
    {"qsort_r", c_qsort_r},          {"bsearch", c_bsearch},
    {"lfind", c_lfind},              {"tsearch", c_tsearch},
    {"pthread_once", c_pthread_once}, {"dl_iterate_phdr", c_dl_iterate_phdr},
    {"ftw", c_ftw},                  {"nftw", c_nftw},
    {"scandir filter", c_scandir_filter}, {"scandir compar", c_scandir_compar},
    {"scandir sweep", c_scandir_sweep},
    {"signal", c_signal},
    {"fopencookie", c_fopencookie},  {"exit", c_exit},
    {"thread", c_thread},
};

// With arguments, only the cases they name run (in table order).
int main(int argc, char **argv) {
    int bad = 0;
    for (const auto &c : cases) {
        bool named = argc < 2;
        for (int i = 1; i < argc; i++) named |= !std::strcmp(argv[i], c.name);
        if (!named) continue;
        std::printf("== %s\n", c.name);
        std::fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
            alarm(20);
            dtors = 0;
            c.run();
            std::fflush(stdout);
            _exit(0);
        }
        int st = 0;
        while (waitpid(pid, &st, 0) < 0 && errno == EINTR) {
        }
        if (WIFSIGNALED(st))
            std::printf("-- %s: killed by signal %d\n", c.name, WTERMSIG(st)), bad = 1;
        else if (WEXITSTATUS(st))
            std::printf("-- %s: exit %d\n", c.name, WEXITSTATUS(st)), bad = 1;
    }
    return bad;
}
