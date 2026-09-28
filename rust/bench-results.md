# Benchmark results: C++ core vs. Rust prototype

- Date: 2026-09-28 (UTC)
- Machine: windows / x86_64, 16 logical CPUs (add CPU, RAM, disk by hand)
- C++: bench/cpp/build\tc_bench_cpp.exe
- Rust: not run
- Timed cases: median of 3 runs after one load-only warm-up; total runtime 3382 s

## Timing

Times in ms, peak = process peak RSS in MB after the step. Hard limit: Rust/C++ ≤ 1.0 (and peak ≤ C++ for load and sort). Aim: see handoff §6.

| Case | Step | C++ ms | Rust ms | Rust/C++ | C++ peak MB | Rust peak MB | Hard limit | Aim | Same result |
|---|---|---:|---:|---:|---:|---:|---|---|---|
| big-core | load | 33598 | – | – | 3096 | – | – | – | – |
| big-core | save | 70042 | – | – | 3096 | – | – | – | – |
| big-core | find_cs | 10380 | – | – | 3096 | – | – | – | – |
| big-core | find_ci | 138136 | – | – | 3096 | – | – | – | – |
| big-sort-num | load | 37647 | – | – | 3096 | – | – | – | – |
| big-sort-num | sort | 240781 | – | – | 3096 | – | – | – | – |
| big-sort-num | save_sorted | 72773 | – | – | 3096 | – | – | – | – |
| big-sort-str | load | 33731 | – | – | 3096 | – | – | – | – |
| big-sort-str | sort | 30312 | – | – | 3096 | – | – | – | – |
| big-sort-str | save_sorted | 69264 | – | – | 3096 | – | – | – | – |
| big-sort-stri | load | 33026 | – | – | 3096 | – | – | – | – |
| big-sort-stri | sort | 38037 | – | – | 3096 | – | – | – | – |
| big-sort-stri | save_sorted | 67551 | – | – | 3096 | – | – | – | – |
| wide-core | load | 1866 | – | – | 186 | – | – | – | – |
| wide-core | save | 26404 | – | – | 186 | – | – | – | – |
| quoted-regex | load | 788 | – | – | 103 | – | – | – | – |
| quoted-regex | save | 1924 | – | – | 103 | – | – | – | – |
| quoted-regex | find_re | 14761 | – | – | 103 | – | – | – | – |
| quoted-macro | load | 886 | – | – | 103 | – | – | – | – |
| quoted-macro | macro | 1902 | – | – | 104 | – | – | – | – |
