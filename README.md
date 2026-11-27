# fmrs::futsuu — Benchmark History

- **Latest Commit**: [`2331b31`](https://github.com/ogiekako/fmrs/commit/2331b31bb7ce7a3552c820415dd5f67e43dc5a7f) (2026-11-27T12:33:36Z)
- **Recorded Runs**: 6
- **Struct Sizes**: `ShtsumePos` = 680 B, `FmrsPos` = 272 B
- **1T Deterministic (`std`, 11 problems, hash=256MB)**: `FmrsPos` **6.105s** (8,720,331 nodes, 1428.5 kNPS) vs `ShtsumePos` 6.999s (9,210,231 nodes, 1316.0 kNPS) — `SH / FMRS` = **1.15x**
- **Long-Mate Parallel (`long`, 4 problems, 4T, hash=1024MB)**: `FmrsPos` **14.184s** (35,727,491 nodes, 2518.8 kNPS)

## Long-Mate Parallel Trends (Independent Y-Axis per Problem)

![Long-Mate Parallel Solve Time](./charts/long_mt_time_grid.svg)

![Long-Mate Parallel Search Nodes](./charts/long_mt_nodes_grid.svg)

## 1T Deterministic Trends (0% Thread-Scheduling Noise)

![1T Total Search Nodes](./charts/std_1t_nodes.svg)

![1T Total Solve Time](./charts/std_1t_time.svg)

![1T Aggregate Throughput](./charts/std_1t_knps.svg)

![1T Long Problems per Problem](./charts/std_1t_long_grid.svg)

## Latest Long-Mate Parallel Suite (`2331b31`, 4T, hash=1024MB)

| Problem | Note | Result | Nodes | Time | kNPS | GC |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `zuko_01_69` | 69手 (将棋図巧 第1番) | M69 | 5,294,121 (+6.2%) | 1.973s (+7.5%) | 2683.0 | 0 |
| `zuko_99_kemuri_117` | 117手 (将棋図巧 第99番 煙詰) | M117 | 1,484,144 (-35.8%) | 0.584s (-36.7%) | 2542.4 | 0 |
| `zuko_100_kotobuki_611` | 611手 (将棋図巧 第100番 寿) | M611 | 571,787 (-10.7%) | 0.219s (-11.6%) | 2611.0 | 0 |
| `microcosmos_1525` | 1525手 (橋本孝治 ミクロコスモス) | M1533 | 28,377,439 (+16.2%) | 11.409s (+16.0%) | 2487.4 | 0 |

## Latest 1T Deterministic Suite (`2331b31`, 1T, hash=256MB)

| Problem | Note | SH Res | SH Nodes | SH Time | SH kNPS | FMRS Res | FMRS Nodes | FMRS Time | FMRS kNPS | SH/FMRS |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| `sample_classic_3` | 3手 (shtsume sample) | M3 | 254 | 0.000s (+0.0%) | 1556.1 | M3 | 223 | 0.000s (+0.0%) | 702.6 | 0.67x |
| `yane_2_5` | 5手 (YaneuraOu test) | M5 | 376 | 0.000s (+33.3%) | 1068.5 | M5 | 244 | 0.000s (+0.0%) | 933.7 | 1.33x |
| `lances_11` | 11手 (shtsume wiki / YaneuraOu) | M11 | 1,079 | 0.001s (+0.0%) | 1447.9 | M11 | 1,447 | 0.001s (+22.2%) | 1359.5 | 0.64x |
| `zuko_05_21` | 21手 (将棋図巧 第5番) | M21 | 3,996 | 0.004s (+26.7%) | 1055.0 | M21 | 3,634 | 0.003s (+8.0%) | 1337.4 | 1.41x |
| `yane_5_fg` | 実戦詰 (YaneuraOu Floodgate #5) | M29(余) | 396,519 | 0.317s (-5.3%) | 1252.1 | M39(余) | 173,002 | 0.122s (-7.1%) | 1414.5 | 2.59x |
| `yane_7_fg` | 実戦詰 (YaneuraOu Floodgate #7) | M27(余) | 625,505 | 0.521s (-4.4%) | 1199.6 | M33(余) | 447,576 | 0.342s (-3.0%) | 1307.7 | 1.52x |
| `yane_10_fg_23` | 23手 (YaneuraOu Floodgate #10) | M25 | 810,600 | 0.637s (-3.6%) | 1273.4 | M25 | 1,162,769 | 0.814s (-5.4%) | 1428.9 | 0.78x |
| `yane_13_fg` | 実戦詰 (YaneuraOu Floodgate #13) | M19 | 894,186 | 0.711s (-6.8%) | 1257.5 | M29(余) | 856,012 | 0.614s (-6.7%) | 1394.0 | 1.16x |
| `zuko_01_69` | 69手 (将棋図巧 第1番) | M71 | 4,971,383 | 3.633s (-3.9%) | 1368.4 | M71 | 4,001,947 | 2.722s (-5.2%) | 1470.3 | 1.33x |
| `zuko_99_kemuri_117` | 117手 (将棋図巧 第99番 煙詰) | M117 | 673,985 | 0.526s (-4.2%) | 1281.4 | M119 | 1,664,848 | 1.204s (-6.5%) | 1383.0 | 0.44x |
| `zuko_100_kotobuki_611` | 611手 (将棋図巧 第100番 寿) | M611 | 832,348 | 0.649s (-5.2%) | 1282.7 | M611 | 408,629 | 0.282s (-5.4%) | 1447.3 | 2.30x |

## Recorded Runs

| Commit | Date (UTC) | 1T FMRS Nodes | 1T FMRS Time | 1T SH/FMRS | Long MT Time (T) | Microcosmos | Reasons / Note |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| [`2331b31`](https://github.com/ogiekako/fmrs/commit/2331b31bb7ce7a3552c820415dd5f67e43dc5a7f) | 2026-11-27 12:33:36 | 8,720,331 | 6.105s | 1.15x | 14.18s (4T) | 11.41s | fix(futsuu): harden tsume futile-interposition, PV stabilization, perfection verification, and test suite (`1T FMRS nodes -0.01%, 1T SH time -4.39%, 1T FMRS time -5.58%, 1T SH kNPS +4.59%, 1T FMRS kNPS +5.91%, long:microcosmos_1525 verdict changed, Long MT time +10.45%, Long MT nodes +10.40%`) |
| [`db0e90a`](https://github.com/ogiekako/fmrs/commit/db0e90a6aef1ce633a1aeabcca849037e4345ee4) | 2026-11-27 08:50:49 | 8,721,230 | 6.466s | 1.13x | 12.84s (4T) | 9.84s | Cherry-pick validated futsuu soundness fixes and safety guards from tsumegen/fmrs (`std:zuko_01_69/shtsume verdict changed, std:zuko_99_kemuri_117/fmrs verdict changed, 1T SH nodes +0.70%, 1T FMRS nodes +5.39%, 1T FMRS time +4.41%, long:microcosmos_1525 verdict changed`) |
| [`5c7d270`](https://github.com/ogiekako/fmrs/commit/5c7d27038a3eba53ea71fef805b1304a041b91ad) | 2026-11-27 08:25:44 | 8,275,216 | 6.192s | 1.18x | 13.23s (4T) | 10.38s | fix(futsuu): parallelize solve_unbounded_pn and optimize perfection verification (`1T SH time +15.92%, 1T FMRS time +10.91%, 1T SH kNPS -13.73%, 1T FMRS kNPS -9.84%, Long MT time +21.87%, Long MT kNPS -19.66%`) |
| [`70d03a2`](https://github.com/ogiekako/fmrs/commit/70d03a206fcc736fc91862292ed67635b35df48d) | 2026-11-27 03:58:46 | 8,275,216 | 5.583s | 1.13x | 10.85s (4T) | 8.56s | Add perfection check option and KIF shortcut branch display for futsuu solver (`1T SH time -14.64%, 1T FMRS time -17.41%, 1T SH kNPS +17.16%, 1T FMRS kNPS +21.08%, long:zuko_01_69 verdict changed, Long MT time -22.29%, Long MT kNPS +26.87%`) |
| [`2324f1e`](https://github.com/ogiekako/fmrs/commit/2324f1e0e7b6b40768464b9bad1ec769d1951d42) | 2026-11-27 01:51:02 | 8,275,216 | 6.760s | 1.09x | 13.97s (4T) | 10.62s | feat(futsuu): add solve_unbounded_pn and without_attacker_hand_piece for sound perfection verification (`1T SH time +19.09%, 1T FMRS time +21.05%, 1T SH kNPS -16.03%, 1T FMRS kNPS -17.39%, long:zuko_01_69 verdict changed, long:microcosmos_1525 verdict changed, Long MT time +6.88%, Long MT nodes -24.85%, Long MT kNPS -29.69%`) |
| [`a6831d6`](https://github.com/ogiekako/fmrs/commit/a6831d6ddbc8b4d249f5f27ee721bdb25b5ad1b9) | 2026-11-26 04:45:07 | 8,275,216 | 5.585s | 1.11x | 13.07s (4T) | 10.83s | Rename fmrs_core::shtsume to futsuu and add 1T/long-mate futsuu benchmark suite (`initial baseline`) |
