window.BENCHMARK_DATA = {
  "lastUpdate": 1778224104715,
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
      }
    ]
  }
}