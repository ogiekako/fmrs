window.BENCHMARK_DATA = {
  "lastUpdate": 1778323366791,
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
          "id": "8ec778793569c9e1049e8161f530ab7ef52cef42",
          "message": "fix(smoke): multi-step seed を new_canonical_group で受理し step を解の長さに揃える\n\n0-step (即詰み) seed のみ受け付けていた制約を撤廃し、任意の N-step seed を\ncanonical group 構築に渡せるようにした。memo/prev_memo を代表 seed の解路で\ncanonical digest ごとに初期化し、BackwardSearch の step フィールドを\ngroup_step に設定することで探索の起点を正しく揃える。\nリグレッションテスト (new_canonical_group_accepts_n_step_seed) を追加。\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T19:32:51+09:00",
          "tree_id": "56debdf7d27d4b566df7eb03b2778fd4407cb25d",
          "url": "https://github.com/ogiekako/fmrs/commit/8ec778793569c9e1049e8161f530ab7ef52cef42"
        },
        "date": 1778323363818,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 781,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3294,
            "range": "± 17",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 266,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 857,
            "range": "± 3000",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 32065,
            "range": "± 188",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1452,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 5130,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1755567,
            "range": "± 129",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 10950,
            "range": "± 73",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 174,
            "range": "± 0",
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
            "value": 51,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke",
            "value": 150,
            "range": "± 3",
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
            "value": 17,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 33714,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4117,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 21055,
            "range": "± 1263",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 73581,
            "range": "± 21",
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
            "value": 81989,
            "range": "± 30",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}