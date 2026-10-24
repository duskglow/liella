/* LEDGER-COREUTILS black-box probe: fflush on a seekable input stream after ungetc.
 * POSIX fflush: for a seekable input stream, the file offset of the descriptor is set to the
 * stream's position, and ungetc pushback is discarded. */
#include <stdio.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "probe.txt";
    FILE *w = fopen(path, "w");
    if (!w) return 10;
    fputs("abcdefghijklmnopqrstuvwxyz", w);
    fclose(w);
    FILE *f = fopen(path, "r");
    if (!f) return 11;
    int fd = fileno(f);
    char buf[5];
    if (fread(buf, 1, 5, f) != 5) return 12;
    int r1 = fflush(f);
    long off1 = (long)lseek(fd, 0, SEEK_CUR);
    /* backup ungetc: push back the byte just read */
    int c = fgetc(f);
    ungetc(c, f);
    fflush(f);
    int b = fgetc(f);
    /* non-backup ungetc: push back a different byte */
    int c2 = fgetc(f);
    ungetc('@', f);
    int r3 = fflush(f);
    long off3 = (long)lseek(fd, 0, SEEK_CUR);
    long tell3 = ftell(f);
    int n = fgetc(f);
    printf("fflush_after_read rc=%d fd_off=%ld\n", r1, off1);
    printf("backup_ungetc read=%c after_fflush=%c\n", c, b);
    printf("nonbackup_ungetc read=%c fflush=%d fd_off=%ld ftell=%ld next=%c\n", c2, r3, off3, tell3, n);
    fclose(f);
    unlink(path);
    return 0;
}
