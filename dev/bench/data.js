window.BENCHMARK_DATA = {
  "lastUpdate": 1778226266947,
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
      }
    ]
  }
}