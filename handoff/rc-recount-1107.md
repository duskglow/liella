# RC-RECOUNT-1107: 30-workload count, next vs main (11-07 pre-release promotion)

State: branch task/rc-recount-1107 (no code). NEW = origin/next 70cb44d76d4ca0c1a6dad7d1f8f393b283ea51ad (1928 commits
ahead of main), BASE = origin/main f3b3584af38cbb6fc4c5be81c0f5ace1d46cd861. Run on keke (AMD EPYC 7551, Zen 1) from
role n2 after `validation/wl_prime_base.sh keke f3b3584af...`:
`WL_BASE_EXACT=1 bash validation/wl_count.sh --base f3b3584af... --rev 70cb44d76... . <the 30 workloads of RC-RECOUNT-1024>`
through jobrun, job `rc-recount-1107-count` (rc 0, 5364 s; both trees built fresh, nothing cached). Full log:
keke:~/logs/wlc-70cb44d76-160512-1625304.log. No timing taken. Ratios are next/main; faults and syscalls are the
median of three runs.

## Table (next vs main)

Verdict column, mechanical: WORSE when instr:u > +0.5%, faults > +2%, or gating syscalls higher by any amount.
The text below gives each WORSE row's classification.

| workload | output new/main | instr:u next | instr:u main | ratio | faults next | main | ratio | syscalls next | main | ratio | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|
| bash-loop | SAME | 18894911974 | 19015321248 | 0.9937 | 211 | 212 | 0.9953 | 158 | 158 | 1.0000 | better |
| cut-tr-uniq | SAME | 825010764 | 825284511 | 0.9997 | 492 | 476 | 1.0336 | 9685 | 9680 | 1.0005 | WORSE: faults,syscalls |
| diff | SAME | 784950318 | 783001667 | 1.0025 | 52586 | 52579 | 1.0001 | 224 | 224 | 1.0000 | no worse |
| du | SAME | 106784468 | 108111319 | 0.9877 | 3659 | 3648 | 1.0030 | 171379 | 171379 | 1.0000 | better |
| gawk-printf | SAME | 6031817751 | 6061479985 | 0.9951 | 258 | 253 | 1.0198 | 6511 | 6511 | 1.0000 | no worse |
| gawk-regex | SAME | 3343437344 | 3361299955 | 0.9947 | 266 | 259 | 1.0270 | 10117 | 10117 | 1.0000 | WORSE: faults |
| gawk-sum | SAME | 1828913137 | 1835917253 | 0.9962 | 259 | 251 | 1.0319 | 10117 | 10117 | 1.0000 | WORSE: faults |
| gawk-utf8 | SAME | 3461858550 | 3474615910 | 0.9963 | 311 | 305 | 1.0197 | 10117 | 10117 | 1.0000 | no worse |
| grep-E | SAME | 466194705 | 466305988 | 0.9998 | 473 | 468 | 1.0107 | 658 | 658 | 1.0000 | no worse |
| grep-F | SAME | 55885523 | 55825015 | 1.0011 | 463 | 459 | 1.0087 | 658 | 658 | 1.0000 | no worse |
| grep-i-utf8 | SAME | 357838259 | 358425989 | 0.9984 | 482 | 480 | 1.0042 | 663 | 663 | 1.0000 | no worse |
| gzip-c | SAME | 9414549516 | 9414561288 | 1.0000 | 261 | 263 | 0.9924 | 1388 | 1388 | 1.0000 | no worse |
| gzip-d | SAME | 1036410356 | 1035975109 | 1.0004 | 212 | 214 | 0.9907 | 1389 | 1389 | 1.0000 | no worse |
| ls-l-enUS | SAME | 1217919818 | 1240042148 | 0.9822 | 3648 | 3731 | 0.9778 | 314370 | 314417 | 0.9999 | better |
| ls-lR | SAME | 1122271150 | 1149055137 | 0.9767 | 3433 | 3512 | 0.9775 | 313850 | 313837 | 1.0000 | WORSE: syscalls |
| make-n | SAME | 757894219 | 763826546 | 0.9922 | 7480 | 7481 | 0.9999 | 41510 | 41509 | 1.0000 | WORSE: syscalls |
| md5sum | SAME | 365722415 | 365720071 | 1.0000 | 146 | 141 | 1.0355 | 1337 | 1337 | 1.0000 | WORSE: faults |
| pipe-mix | SAME | 438681184 | 439146805 | 0.9989 | 1173 | 1166 | 1.0060 | 1824 | 1759 | 1.0370 | WORSE: syscalls |
| printf-loop | SAME | 1836808489 | 1841958256 | 0.9972 | 1340133 | 1237325 | 1.0831 | 737524 | 737636 | 0.9998 | WORSE: faults |
| sed-s | SAME | 9494121133 | 9671464917 | 0.9817 | 245 | 241 | 1.0166 | 19805 | 19805 | 1.0000 | better |
| sed-utf8 | SAME | 2454961075 | 2460019553 | 0.9979 | 244 | 240 | 1.0167 | 20266 | 20266 | 1.0000 | no worse |
| seq | SAME | 2257169458 | 2257139401 | 1.0000 | 129 | 125 | 1.0320 | 31686 | 31686 | 1.0000 | WORSE: faults |
| sort-C | SAME | 1466000551 | 1468448179 | 0.9983 | 39493 | 39491 | 1.0001 | 10188 | 10188 | 1.0000 | no worse |
| sort-enUS | SAME | 6709687886 | 6710821639 | 0.9998 | 39525 | 39525 | 1.0000 | 10191 | 10191 | 1.0000 | no worse |
| sort-n | SAME | 5720118518 | 5722589899 | 0.9996 | 39508 | 39505 | 1.0001 | 10188 | 10188 | 1.0000 | no worse |
| sort-utf8 | SAME | 5187740152 | 5247006650 | 0.9887 | 39511 | 39511 | 1.0000 | 10188 | 10188 | 1.0000 | better |
| tar-c | SAME | 59345308 | 59666120 | 0.9946 | 454 | 460 | 0.9870 | 105450 | 105500 | 0.9995 | better |
| tar-gz | SAME | 9174911905 | 9175272825 | 1.0000 | 561 | 569 | 0.9859 | 96641 | 96357 | 1.0029 | WORSE: syscalls |
| tar-tv | SAME | 168727796 | 169400044 | 0.9960 | 239 | 246 | 0.9715 | 19502 | 19501 | 1.0001 | WORSE: syscalls |
| wc | SAME | 1094395400 | 1094395467 | 1.0000 | 263 | 257 | 1.0233 | 299 | 299 | 1.0000 | WORSE: faults |

## Magnitude summary (distribution of next/main ratios over the 30 workloads)

- Output: next = main for all 30 (ls-l-enUS differs from glibc, and main has the same difference: known).
- instructions:u: min 0.9767 (ls-lR), median 0.9983, max 1.0025 (diff). No workload above +0.5%; 9 better by more
  than 0.5% (ls-lR -2.3%, ls-l-enUS -1.8%, make-n -0.8%, gawk-regex -0.5%, ...).
- instructions:k (not gating): median 0.9981, max 1.0529 (printf-loop; follows its page faults), then sort-enUS
  +2.8%, gawk-printf +2.5%, tar-c +1.5%, gawk-utf8 +1.1%.
- page-faults: median 1.0042. Small static programs gain +2..+8 pages per process (wc +6, seq +4, md5sum +5,
  gawk-sum +8, gawk-regex +7, cut-tr-uniq +16 over 3 processes); ls-l-enUS/ls-lR -2.2%, tar-tv -2.9%.
  printf-loop +8.3% (+102808 = +5.1 per exec of printf over 20000 execs).
- syscalls-gating: 29 of 30 within +-0.5%; pipe-mix +3.7% (+65); tar-gz +0.29% (+284).

## Classification of the WORSE rows

1. Page faults per static start, +4..+6 (printf-loop, wc, seq, md5sum, gawk-*, cut-tr-uniq). Probe (one exec of
   the kit's printf, each tree): page faults 61 (main) vs 66 (next); the syscall set and counts are the same (strace
   -c); `size`: text +25.4 KB, data +5.5 KB, bss -11.1 KB. So: more pages touched at start-up with no extra system
   calls, consistent with layout/size growth of the static image (the 10-24 ruling accepts layout-only fault
   deltas). NOT VERIFIED that it is layout only: printf calls setlocale and bindtextdomain at start-up, and the
   COLDSTART-* landings (SETLOCALE-FIRST-2, BINDTEXTDOMAIN-REBIND-2, TZ-FIRST, STRCOLL-FIRST-2,
   SECOND-LATENCY-4) and DL-BOOT-RELOC-* (rcrt1) changed those paths. No bisect done.
   printf-loop is where it shows: +8.3% faults, +5.3% instr:k, while instr:u -0.3% and syscalls -112.
2. pipe-mix syscalls +65 and tar-gz +284: scheduling, not code. A strace -f -c of one run of each pipeline (both
   trees) moved only `read` (+21 pipe-mix, -472 tar-gz; plus mmap +1, munmap +2 in pipe-mix): the number of reads on
   a pipe depends on how full the pipe is when the reader runs. tar-gz went the other way in the probe.
3. syscalls +1 (make-n, tar-tv), +5 (cut-tr-uniq), +13 (ls-lR, 0.004%): noise level, within the +1 mprotect per
   static start accepted on 10-24, or pipe reads as in 2.

## RULING NEEDED

- +5 page faults per static process start (printf-loop +8.3%). Recommend: accept as layout growth for this
  pre-release only if the owner is content without a bisect; otherwise one bisect story over the COLDSTART-* and
  DL-BOOT-RELOC-* merges (a single printf exec, `perf stat -e page-faults`, 61 vs 66, decides each step in seconds).
- Pipe `read` counts in multi-process workloads: recommend wl_syscls.sh classify `read`/`write` on pipes in
  pipelines as advisory (as futex and threaded mmap already are); the pipe-mix/tar-gz rows are scheduling.

## Not to redo / next step

Main's products and counter rows are cached on keke (~/wl/products and ~/wl/count/f3b3584af...), so a late recount
of a newer next sha only builds and counts next (about half of this job's 89 min). Next step: the coordinator decides
the ruling above; on request, recount the final next sha with the same command (new --rev).

HOLD: printf-loop page faults +8.3% (+5 faults per static process start, 61 -> 66, not proven layout-only, no bisect); everything else identical in output and no worse beyond the tolerances (instr:u max +0.25%, pipe-mix/tar-gz syscall deltas are pipe-read scheduling)
