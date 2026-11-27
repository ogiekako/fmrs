# fmrs::futsuu — Benchmark History

- **Latest Commit**: [`70d03a2`](https://github.com/ogiekako/fmrs/commit/70d03a206fcc736fc91862292ed67635b35df48d) (2026-11-27T03:58:46Z)
- **Recorded Runs**: 3
- **Struct Sizes**: `ShtsumePos` = 680 B, `FmrsPos` = 272 B
- **1T Deterministic (`std`, 11 problems, hash=256MB)**: `FmrsPos` **5.583s** (8,275,216 nodes, 1482.2 kNPS) vs `ShtsumePos` 6.312s (9,145,877 nodes, 1449.0 kNPS) — `SH / FMRS` = **1.13x**
- **Long-Mate Parallel (`long`, 4 problems, 4T, hash=1024MB)**: `FmrsPos` **10.854s** (32,813,429 nodes, 3023.2 kNPS)

## Long-Mate Parallel Trends (Independent Y-Axis per Problem)

![Long-Mate Parallel Solve Time](./charts/long_mt_time_grid.svg)

![Long-Mate Parallel Search Nodes](./charts/long_mt_nodes_grid.svg)

## 1T Deterministic Trends (0% Thread-Scheduling Noise)

![1T Total Search Nodes](./charts/std_1t_nodes.svg)

![1T Total Solve Time](./charts/std_1t_time.svg)

![1T Aggregate Throughput](./charts/std_1t_knps.svg)

![1T Long Problems per Problem](./charts/std_1t_long_grid.svg)

## Latest Long-Mate Parallel Suite (`70d03a2`, 4T, hash=1024MB)

| Problem | Note | Result | Nodes | Time | kNPS | GC |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `zuko_01_69` | 69手 (将棋図巧 第1番) | M69 | 5,244,881 (-0.6%) | 1.545s (-26.1%) | 3395.1 | 0 |
| `zuko_99_kemuri_117` | 117手 (将棋図巧 第99番 煙詰) | M117 | 1,787,545 (-27.8%) | 0.549s (-46.5%) | 3255.9 | 0 |
| `zuko_100_kotobuki_611` | 611手 (将棋図巧 第100番 寿) | M611 | 665,864 (+18.0%) | 0.202s (-11.3%) | 3294.8 | 0 |
| `microcosmos_1525` | 1525手 (橋本孝治 ミクロコスモス) | M1545 | 25,115,139 (+0.6%) | 8.558s (-19.4%) | 2934.7 | 0 |

## Latest 1T Deterministic Suite (`70d03a2`, 1T, hash=256MB)

| Problem | Note | SH Res | SH Nodes | SH Time | SH kNPS | FMRS Res | FMRS Nodes | FMRS Time | FMRS kNPS | SH/FMRS |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| `sample_classic_3` | 3手 (shtsume sample) | M3 | 220 | 0.000s (+0.0%) | 1686.7 | M3 | 223 | 0.000s (-25.0%) | 777.0 | 0.33x |
| `yane_2_5` | 5手 (YaneuraOu test) | M5 | 360 | 0.000s (-33.3%) | 1455.4 | M5 | 244 | 0.000s (-60.0%) | 1021.9 | 1.00x |
| `lances_11` | 11手 (shtsume wiki / YaneuraOu) | M11 | 1,079 | 0.001s (-25.0%) | 1871.2 | M11 | 1,447 | 0.001s (-11.1%) | 1815.1 | 0.75x |
| `zuko_05_21` | 21手 (将棋図巧 第5番) | M21 | 3,996 | 0.003s (-16.7%) | 1594.2 | M21 | 3,634 | 0.002s (-21.4%) | 1661.8 | 1.14x |
| `yane_5_fg` | 実戦詰 (YaneuraOu Floodgate #5) | M29(余) | 396,519 | 0.314s (-6.8%) | 1263.6 | M39(余) | 173,002 | 0.113s (-18.9%) | 1536.9 | 2.79x |
| `yane_7_fg` | 実戦詰 (YaneuraOu Floodgate #7) | M27(余) | 625,505 | 0.477s (-14.4%) | 1310.3 | M33(余) | 448,736 | 0.322s (-16.3%) | 1395.7 | 1.48x |
| `yane_10_fg_23` | 23手 (YaneuraOu Floodgate #10) | M25 | 810,600 | 0.569s (-15.8%) | 1424.2 | M25 | 1,162,894 | 0.783s (-16.8%) | 1485.5 | 0.73x |
| `yane_13_fg` | 実戦詰 (YaneuraOu Floodgate #13) | M19 | 894,186 | 0.665s (-13.2%) | 1343.5 | M29(余) | 855,623 | 0.588s (-16.9%) | 1454.3 | 1.13x |
| `zuko_01_69` | 69手 (将棋図巧 第1番) | M69 | 4,907,079 | 3.245s (-15.3%) | 1512.2 | M71 | 3,417,974 | 2.257s (-16.8%) | 1514.5 | 1.44x |
| `zuko_99_kemuri_117` | 117手 (将棋図巧 第99番 煙詰) | M117 | 673,985 | 0.472s (-14.3%) | 1428.0 | M117 | 1,801,959 | 1.258s (-18.6%) | 1432.6 | 0.38x |
| `zuko_100_kotobuki_611` | 611手 (将棋図巧 第100番 寿) | M611 | 832,348 | 0.566s (-15.8%) | 1471.4 | M611 | 409,480 | 0.260s (-20.4%) | 1576.4 | 2.18x |

## Recorded Runs

| Commit | Date (UTC) | 1T FMRS Nodes | 1T FMRS Time | 1T SH/FMRS | Long MT Time (T) | Microcosmos | Reasons / Note |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| [`70d03a2`](https://github.com/ogiekako/fmrs/commit/70d03a206fcc736fc91862292ed67635b35df48d) | 2026-11-27 03:58:46 | 8,275,216 | 5.583s | 1.13x | 10.85s (4T) | 8.56s | Add perfection check option and KIF shortcut branch display for futsuu solver (`1T SH time -14.64%, 1T FMRS time -17.41%, 1T SH kNPS +17.16%, 1T FMRS kNPS +21.08%, long:zuko_01_69 verdict changed, Long MT time -22.29%, Long MT kNPS +26.87%`) |
| [`2324f1e`](https://github.com/ogiekako/fmrs/commit/2324f1e0e7b6b40768464b9bad1ec769d1951d42) | 2026-11-27 01:51:02 | 8,275,216 | 6.760s | 1.09x | 13.97s (4T) | 10.62s | feat(futsuu): add solve_unbounded_pn and without_attacker_hand_piece for sound perfection verification (`1T SH time +19.09%, 1T FMRS time +21.05%, 1T SH kNPS -16.03%, 1T FMRS kNPS -17.39%, long:zuko_01_69 verdict changed, long:microcosmos_1525 verdict changed, Long MT time +6.88%, Long MT nodes -24.85%, Long MT kNPS -29.69%`) |
| [`a6831d6`](https://github.com/ogiekako/fmrs/commit/a6831d6ddbc8b4d249f5f27ee721bdb25b5ad1b9) | 2026-11-26 04:45:07 | 8,275,216 | 5.585s | 1.11x | 13.07s (4T) | 10.83s | Rename fmrs_core::shtsume to futsuu and add 1T/long-mate futsuu benchmark suite (`initial baseline`) |
