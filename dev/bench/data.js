window.BENCHMARK_DATA = {
  "lastUpdate": 1778324545409,
  "repoUrl": "https://github.com/ogiekako/fmrs",
  "entries": {
    "Rust Benchmark": [
      {
        "commit": {
          "author": {
            "name": "Keigo Oka",
            "username": "ogiekako",
            "email": "ogiekako@gmail.com"
          },
          "committer": {
            "name": "Keigo Oka",
            "username": "ogiekako",
            "email": "ogiekako@gmail.com"
          },
          "id": "30cee831ec891007cd5cc45ac944e6fc0f199d87",
          "message": "fix(ci): serialize rust_bench after gh-pages deploy to prevent data.js overwrite\n\nRace condition: both workflows triggered on push to main simultaneously.\nbenchmark-action force-pushes with 1-run data when its push fails due to\ngh-pages.yaml pushing first, wiping all benchmark history.\n\nFix: trigger rust_bench via workflow_run after gh-pages deploy completes.\n\nCo-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>",
          "timestamp": "2026-05-09T10:50:30Z",
          "url": "https://github.com/ogiekako/fmrs/commit/30cee831ec891007cd5cc45ac944e6fc0f199d87"
        },
        "date": 1778324542893,
        "tool": "cargo",
        "benches": [
          {
            "name": "black_advance",
            "value": 848,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "white_advance",
            "value": 3563,
            "range": "± 35",
            "unit": "ns/iter"
          },
          {
            "name": "black_pinned",
            "value": 264,
            "range": "± 40",
            "unit": "ns/iter"
          },
          {
            "name": "solve3",
            "value": 892,
            "range": "± 3267",
            "unit": "ns/iter"
          },
          {
            "name": "oneway",
            "value": 34539,
            "range": "± 449",
            "unit": "ns/iter"
          },
          {
            "name": "reachable",
            "value": 1556,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "pinned300",
            "value": 5008,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "bench_solve97",
            "value": 1916629,
            "range": "± 290",
            "unit": "ns/iter"
          },
          {
            "name": "attacker",
            "value": 11688,
            "range": "± 653",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish",
            "value": 182,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonicalize_attacker_goldish_heavy",
            "value": 111,
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
            "value": 145,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_heavy",
            "value": 39,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "canonical_digest_for_smoke_empty",
            "value": 15,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "bench_jugemu",
            "value": 37445,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1965",
            "value": 4478,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "bench_1461",
            "value": 23255,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "bench_bataco",
            "value": 76979,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search",
            "value": 41763,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "bench_backward_search_seed_sfen",
            "value": 90100,
            "range": "± 783",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}