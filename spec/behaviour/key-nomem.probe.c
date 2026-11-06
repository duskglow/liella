/* pthread_setspecific/getspecific under an exhausted address space: which keys succeed (table) */
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

#define NKEYS 1024
static pthread_key_t keys[NKEYS];
static int nkeys;
static int mode_seq;   /* 1: set keys 0.. in creation order until one fails */
static int target;     /* mode one: the creation index to set */

static void say(const char *s) { (void)!write(1, s, strlen(s)); }

static long vm_bytes(void) {
  char b[128];
  int fd = open("/proc/self/statm", O_RDONLY);
  if (fd < 0) return -1;
  long n = read(fd, b, sizeof b - 1);
  close(fd);
  if (n <= 0) return -1;
  b[n] = 0;
  return atol(b) * 4096L;
}

static void exhaust(void) {
  struct rlimit r;
  getrlimit(RLIMIT_AS, &r);
  r.rlim_cur = (rlim_t)vm_bytes() + 65536;
  setrlimit(RLIMIT_AS, &r);
  static const size_t sizes[] = {1 << 20, 65536, 4096, 256, 16};
  for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; i++)
    for (int n = 0; n < 1000000 && malloc(sizes[i]) != NULL; n++) {}
  for (int n = 0; n < 100000 && mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) != MAP_FAILED; n++) {}
}

static void *body(void *arg) {
  char line[160];
  (void)arg;
  exhaust();
  if (mode_seq) {
    int i;
    for (i = 0; i < nkeys; i++) {
      int rc = pthread_setspecific(keys[i], (void *)(long)(i + 1));
      if (rc != 0) {
        void *g = pthread_getspecific(keys[i]);
        snprintf(line, sizeof line, " first-fail idx=%d rc=%s get=%s", i, rc == ENOMEM ? "ENOMEM" : "other",
                 g ? "nonnull" : "NULL");
        say(line);
        break;
      }
    }
    if (i == nkeys) say(" all-set");
    int bad = 0;
    for (int j = 0; j < i; j++) bad += pthread_getspecific(keys[j]) != (void *)(long)(j + 1);
    snprintf(line, sizeof line, " get-mismatch=%d\n", bad);
    say(line);
  } else {
    int rc = pthread_setspecific(keys[target], (void *)0x1234);
    void *g = pthread_getspecific(keys[target]);
    snprintf(line, sizeof line, " idx=%d set=%s get=%s\n", target, rc == 0 ? "0" : rc == ENOMEM ? "ENOMEM" : "other",
             g == (void *)0x1234 ? "value" : g ? "other" : "NULL");
    say(line);
  }
  return 0;
}

static void run(int threaded, int seq, int tgt) {
  pthread_t t;
  say(threaded ? "thread" : "main  ");
  fflush(stdout);
  pid_t p = fork();
  if (p == 0) {
    mode_seq = seq;
    target = tgt;
    if (threaded) {
      if (pthread_create(&t, 0, body, 0) != 0) { say(" create-failed\n"); _exit(1); }
      pthread_join(t, 0);
    } else {
      body(0);
    }
    _exit(0);
  }
  int st;
  waitpid(p, &st, 0);
  if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) say(" child-abnormal\n");
}

int main(void) {
  for (nkeys = 0; nkeys < NKEYS; nkeys++)
    if (pthread_key_create(&keys[nkeys], 0) != 0) break;
  printf("keys created %d\n", nkeys);
  fflush(stdout);
  static const int idx[] = {0, 1, 15, 16, 30, 31, 32, 33, 63, 64, 500, 1000};
  for (int th = 0; th < 2; th++) {
    run(th, 1, 0);
    for (unsigned i = 0; i < sizeof idx / sizeof idx[0]; i++)
      if (idx[i] < nkeys) run(th, 0, idx[i]);
  }
  return 0;
}
