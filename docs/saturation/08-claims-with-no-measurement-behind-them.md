## 8. Claims with no measurement behind them (all rows marked C above)

1. `qsort`: glibc "about 22" library instructions per comparison; Liella's side is counted, glibc's is the number in the note.
2. `readdir`: "already at parity" is a time ratio; the instruction ratio was 1.17 at the time.
3. `memcpy`/`memset` 16 and 64: no stated limit; gap of 1 to 3 instr unexplained after MEM-ENTRY-2.
4. `strlen_64`, `strnlen_64`, `rawmemchr_64`: above glibc, no explanation in the limits file.
5. `gettext`, `strftime`, `mbrtowc`, `regex capture`, `wordexp`: each has a rejected-attempt note but no stated floor.
