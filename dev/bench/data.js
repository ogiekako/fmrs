window.BENCHMARK_DATA = {
  "lastUpdate": 1778322527322,
  "repoUrl": "https://github.com/ogiekako/fmrs",
  "entries": {
    "Rust Benchmark": [
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "e81aef2b84213742d45492f30c9636b620daa618",
          "message": "chore(bench): dashmap_vs_logic/dashmap_100_insert_get を削除\n\nCo-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>",
          "timestamp": "2026-05-08T15:57:08+09:00",
          "tree_id": "3eb8dd22df4bcd87d2ed66ab89ece2db2ad3c7db",
          "url": "https://github.com/ogiekako/fmrs/commit/e81aef2b84213742d45492f30c9636b620daa618"
        },
        "date": 1778224101987,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 899,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3446,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 270,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 979,
            "range": "± 1748",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 30559,
            "range": "± 113",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1526,
            "range": "± 143",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 4268,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1707177,
            "range": "± 47",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11220,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "map_ops/dashmap_insert_get",
            "value": 188710,
            "range": "± 2089",
            "unit": "ns/iter"
          },
          {
            "name": "map_ops/hashmap_nohash_insert_get",
            "value": 82137,
            "range": "± 618",
            "unit": "ns/iter"
          },
          {
            "name": "map_ops/dashmap_get_existing",
            "value": 76412,
            "range": "± 1915",
            "unit": "ns/iter"
          },
          {
            "name": "map_ops/hashmap_nohash_get_existing",
            "value": 23986,
            "range": "± 131",
            "unit": "ns/iter"
          },
          {
            "name": "dashmap_vs_logic/advance_aux_100",
            "value": 63326,
            "range": "± 1630",
            "unit": "ns/iter"
          },
          {
            "name": "dashmap_vs_logic/previous_100",
            "value": 18801,
            "range": "± 71",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 33337,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4049,
            "range": "± 103",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 20834,
            "range": "± 264",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 75471,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 38461,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 81356,
            "range": "± 122",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "3cf45c314051043e821b63c749298ab958a4b786",
          "message": "chore(bench): remove dashmap microbenchmarks\n\nDelete bench_dashmap_overhead and bench_dashmap_vs_logic — these were\ninvestigating a dashmap-vs-logic tradeoff that was resolved; the remaining\nbenches regressed into unmaintained stubs with no ongoing value.\n\nAlso removes the dashmap dev-dependency (no longer used anywhere).\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-08T16:06:21+09:00",
          "tree_id": "9e7788321f852ea344ff64bc5a3c0172f2e7415a",
          "url": "https://github.com/ogiekako/fmrs/commit/3cf45c314051043e821b63c749298ab958a4b786"
        },
        "date": 1778224582873,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 721,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 2914,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 265,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 972,
            "range": "± 2222",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 27928,
            "range": "± 68",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1528,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 4136,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1689614,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 10469,
            "range": "± 31",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 33760,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4128,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 21276,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 82766,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 37735,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 79119,
            "range": "± 29",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "6cac3258749c779e8654b31884930da1cccd5f0b",
          "message": "feat(smoke): seed × step ごとの trajectory log を常時 emit\n\n`<seed_result_log>.trajectory.jsonl` に 1 行 / seed / step で構造特徴を\n追記する。advance 成功直後に emit:\n\n  {\"cond\":\"<hash>\",\"seed\":N,\"step\":K,\"frontier\":F,\"memo\":M,\"inner\":I,\"ms\":T}\n\n- frontier dynamics (peak ではなく時系列) を捕えるための baseline 用\n- shogi 特徴量を出す既存 feature_log とは独立ストリーム\n- フラグなしで常時オン (1 行 ~100 byte、step 内 advance に対し\n  serde 不要の writeln 1 回でオーバーヘッド無視できる程度)\n- `cond` 列で同じ trajectory log に複数条件が混ざっても join 可能\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-08T16:21:25+09:00",
          "tree_id": "8bb33b5df46a1dd19c7f2013f6c4eadb3d350a78",
          "url": "https://github.com/ogiekako/fmrs/commit/6cac3258749c779e8654b31884930da1cccd5f0b"
        },
        "date": 1778225329055,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 723,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 2838,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 273,
            "range": "± 50",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 975,
            "range": "± 2129",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 26191,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1498,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 4105,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1698790,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 10507,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 32092,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 3933,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 20202,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 80574,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 37190,
            "range": "± 4",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "072ea217f6d85dc384bd3f5945b57f2869aa32b9",
          "message": "fix(smoke): trajectory_log_path が /dev/null に .trajectory.jsonl を付加して Permission denied になる問題を修正\n\nseed_result_log が /dev/null のときは trajectory path も /dev/null を返すようにした。\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-08T16:35:08+09:00",
          "tree_id": "4d99b81b089aaa1295d66b748bf31e86d55afaf1",
          "url": "https://github.com/ogiekako/fmrs/commit/072ea217f6d85dc384bd3f5945b57f2869aa32b9"
        },
        "date": 1778226264459,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 935,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3641,
            "range": "± 84",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 267,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 899,
            "range": "± 3264",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 34032,
            "range": "± 214",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1532,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 5015,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1780995,
            "range": "± 1809",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11827,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 35873,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4244,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 22222,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 76208,
            "range": "± 28",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 41935,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 88846,
            "range": "± 1206",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "79268019450dd5b000fc0d1ff9b90c5ed704447d",
          "message": "fix(core): sfen_to_image_url を path 形式に変更、from_image_url も旧形式互換\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-08T21:33:57+09:00",
          "tree_id": "8332887283e30ba36d6c498e66fcbe1058390f2b",
          "url": "https://github.com/ogiekako/fmrs/commit/79268019450dd5b000fc0d1ff9b90c5ed704447d"
        },
        "date": 1778244203399,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 812,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3430,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 270,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 883,
            "range": "± 3075",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 30580,
            "range": "± 916",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1489,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 4347,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1742833,
            "range": "± 174",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11547,
            "range": "± 62",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 33561,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4067,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 21276,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 77212,
            "range": "± 218",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 39241,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 80750,
            "range": "± 107",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "04c8caeb644382e1a0b3d807619e51226e85bc99",
          "message": "perf(core): backward search の delta マージを wave 単位に分割してピークメモリを削減\n\n全候補を一括処理してから delta をまとめてマージする代わりに、\nwave_size (parallel * 8 chunks) ごとに処理→即時マージ→解放を繰り返す。\nピーク delta メモリを O(全チャンク数) から O(parallel * 8) に抑える。\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T14:32:09+09:00",
          "tree_id": "f1f0fead8f6b6ef122e4d4baf7a7d7024deeaee7",
          "url": "https://github.com/ogiekako/fmrs/commit/04c8caeb644382e1a0b3d807619e51226e85bc99"
        },
        "date": 1778305288372,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 850,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3552,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 266,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 895,
            "range": "± 3372",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 34341,
            "range": "± 195",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1551,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 5015,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1828030,
            "range": "± 1009",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11905,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 37271,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4424,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 23335,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 76977,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 41897,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 90906,
            "range": "± 10",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "9baba05eadfe5ac3b10f33af9b15348313afb60f",
          "message": "feat(smoke): canonicalize-attacker-goldish オプションで memo cache hit 率向上\n\nbackward 探索の uniqueness 判定境界で「黒 goldish (≠ ProPawn) を ProPawn 化、\n駒種情報を白手駒に対称的に移す」正規化を適用する opt-in flag を追加。総駒数固定\n(Gold/Silver/Knight/Lance=4, Pawn=18) の制約下では同じ goldish 占有マス集合を持つ\n局面群がすべて 1 canonical に潰れ、典型 5^K の collapse が memo 共有を生む。\n\n主な変更:\n- fmrs_core/src/search/canonicalize.rs を新設\n  - canonicalize_attacker_goldish: per-kind bb 反復 + Phase A/B 分割で 99 ns\n  - canonical_digest_for_smoke: mutation なしで canonical digest を差分計算 (24 ns/heavy)\n- BackwardSearch に canonicalize_attacker_goldish flag、memo lookup は digest 先行\n  hit ならは mutation 不要、miss 時のみ clone + canonicalize して solutions 呼び出し\n- KindBitBoard::change_kind / Position::change_kind / PositionAux::change_kind を新設\n  unset+set の cancel-pair (黒 bb / occupied) と digest XOR をまとめて削減\n- single-king-smoke ideal-backward に --canonicalize-attacker-goldish CLI flag\n- search_single_seed / scheduler.finalize_task で best_positions を standard_solve で\n  再検証 (false positive を除外)\n- benches/bench.rs に canonicalize / canonical_digest 用 bench 追加\n- canonicalize.rs に 15 unit tests (collapse 確認 + property test)\n\n実走計測 (--allowed-kinds gold,silver,knight,lance --max-step 13 --seed-limit 5):\n  OFF: 17.4s, memo 2.85M    ON: 10.7s, memo 0.81M (3.5x 縮小、wall 1.6x 速)\n  best_pieces=9, positions=170 で OFF/ON 一致\n\nCo-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T16:53:52+09:00",
          "tree_id": "a55f16138819c4ec35621b70aed1be8f46ad2609",
          "url": "https://github.com/ogiekako/fmrs/commit/9baba05eadfe5ac3b10f33af9b15348313afb60f"
        },
        "date": 1778313774482,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 662,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 2822,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 218,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 714,
            "range": "± 2142",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 25738,
            "range": "± 278",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1196,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 3761,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1435884,
            "range": "± 38",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 9041,
            "range": "± 54",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 141,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_heavy",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_empty",
            "value": 40,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke",
            "value": 113,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_heavy",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_empty",
            "value": 11,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 28907,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 3521,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 18699,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 62095,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 32610,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 71342,
            "range": "± 86",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "60236f2fdc79fc0b9f2d87fabc9afb1b3ae7fa0a",
          "message": "chore: .gitignore に .claude/ を追加\n\nClaude Code のローカル設定ディレクトリを ignore。\n\nCo-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T17:13:41+09:00",
          "tree_id": "51609d74e42b2916f83eb185e44862bac0331521",
          "url": "https://github.com/ogiekako/fmrs/commit/60236f2fdc79fc0b9f2d87fabc9afb1b3ae7fa0a"
        },
        "date": 1778315146568,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 662,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 2895,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 205,
            "range": "± 40",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 714,
            "range": "± 2043",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 26190,
            "range": "± 174",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1200,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 3780,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1358850,
            "range": "± 210",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 9083,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 142,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_heavy",
            "value": 86,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_empty",
            "value": 40,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke",
            "value": 113,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_heavy",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_empty",
            "value": 11,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 27179,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 3236,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 17241,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 60640,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 31816,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 70388,
            "range": "± 77",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "e256bd06d112d7f0329c831651c7f5a446b4d48f",
          "message": "perf(smoke): canonical-equivalent な seed 群を 1 BackwardSearch にバッチ化\n\ncanonicalize-attacker-goldish ON のとき、enumerate_final_2_positions の seed を\ncanonical_digest_for_smoke でグループ化し、各グループを 1 BackwardSearch で\n処理する。memo は canonical-keyed で seed 間で共有されるため、同じ canonical\nclass に属する seed (典型 5〜16 個) の predecessor 探索の重複作業が大幅に\n削減される。\n\n主な変更:\n- BackwardSearch::new_canonical_group: 複数 seed を受けて memo に各 seed の\n  canonical_digest を `StepRange::exact(0)` で seed する。初期 frontier は全\n  seed の core を並べる (predecessor 生成は seed 個別に走るが canonical-keyed\n  memo で共有)。0-step 詰局面のみサポート (smoke seeds 用途)。\n- ideal_backward: enumerate → grouping → shuffle → fleet_partition → truncate\n  の順に変更。canonicalize ON では seed_limit が group 数を意味する。\n- scheduler::TaskState::Cold: seed: PositionAux → seeds: Vec<PositionAux>。\n- search_single_seed: seed: &PositionAux → seeds: &[PositionAux]。canonicalize ON\n  では new_canonical_group、OFF では従来の new_with_parallel + checkpoint。\n- canon vs non-canon は record/checkpoint 互換なし (canon ON では log 読み書き\n  をスキップ)。\n\n実測 (--allowed-kinds=pawn,lance,silver,gold --parallel 8 --mate-square 99\n--max-step 11):\n              OFF              ON                改善\n  Wall:       22.3s            14.0s             1.59x\n  memo peak:  2.7M             0.15M             18x 縮小\n  prev_memo:  4.5M             0.05M             90x 縮小\n  RSS peak:   10 GB            0.5 GB            20x 削減\n  searches:   56 (24 succeed)  5 groups (11.2avg) 11.2x 集約\n  positions:  23338            23338             完全一致\n\nbest_pieces=8 で OFF/ON 一致。\n\nCo-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T17:59:10+09:00",
          "tree_id": "31a35dfff9d5fa3cbe70b539d9ee030ce7102daf",
          "url": "https://github.com/ogiekako/fmrs/commit/e256bd06d112d7f0329c831651c7f5a446b4d48f"
        },
        "date": 1778317738747,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 786,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3283,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 275,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 861,
            "range": "± 2904",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 33148,
            "range": "± 130",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1487,
            "range": "± 43",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 5199,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1783385,
            "range": "± 856",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11174,
            "range": "± 794",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 174,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_heavy",
            "value": 105,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_empty",
            "value": 49,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke",
            "value": 147,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_heavy",
            "value": 37,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_empty",
            "value": 16,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 33333,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4018,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 20000,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 74624,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 39999,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 80736,
            "range": "± 72",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "committer": {
            "email": "ogiekako@gmail.com",
            "name": "Keigo Oka",
            "username": "ogiekako"
          },
          "distinct": true,
          "id": "6a1a3bcbdd8ba43551e187ec7731fd5e256097a6",
          "message": "chore: sfen.rs の未使用 percent_encoding インポートを削除\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T19:15:58+09:00",
          "tree_id": "2f59c5de2e78890a3a6983acf7810361352e3f6c",
          "url": "https://github.com/ogiekako/fmrs/commit/6a1a3bcbdd8ba43551e187ec7731fd5e256097a6"
        },
        "date": 1778322523647,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 664,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 2739,
            "range": "± 70",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 204,
            "range": "± 41",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 713,
            "range": "± 2094",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 27410,
            "range": "± 65",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1204,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 3887,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1508374,
            "range": "± 884",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 9014,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 141,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_heavy",
            "value": 86,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_empty",
            "value": 40,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke",
            "value": 113,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_heavy",
            "value": 30,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_empty",
            "value": 11,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 28098,
            "range": "± 199",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 3307,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 17858,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 61707,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 32609,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 70342,
            "range": "± 48",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}