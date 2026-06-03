# Smoke best-cone analysis

Investigation into how the *useful* part of the single-king-smoke backward
search relates to the full frontier, and what that implies for the project goal:
**construct the maximum-piece cooperative smoke-mate (target 40 pieces).**

Reproduce with [`run.sh`](run.sh) (drives a `--max-step 37` run that dumps the
per-step max-piece "best" positions, then runs
[`fmrs_core/tests/smoke_cone_analysis.rs`](../../fmrs_core/tests/smoke_cone_analysis.rs)).
Raw inputs are committed under [`data/`](data/). Run config: the standard 5-min
seed `8k/6K+P1/...` with `--min-pawn-pct 60 --rook-bishop-allow-start 31
--lance-knight-allow-start 8 --max-file 7 --canonicalize-attacker-goldish`.

All position counts are **canonical** (goldish-collapsed) to match how the
frontier dedups; `step` = plies remaining to mate (frontier exists at odd steps).

## 1. The "best cone" is a vanishingly thin sliver of the frontier

Take the 352 max-piece (18-piece) best positions at the deepest step (37),
reconstruct each unique solution, and trace it toward mate. The distinct
positions on these paths ("cone") per step, vs the full frontier:

| step | cone | frontier | cone/frontier |
|---:|---:|---:|---:|
| 37 | 96 | 4,063,882 | 0.0024% |
| 31 | 36 | 810,222 | 0.0044% |
| 25 | 12 | 146,756 | 0.0082% |
| 19 | 2 | 34,798 | 0.0057% |
| 13 | 2 | 7,856 | 0.025% |
| 11 | 2 | 1,816 | 0.11% |
| 7 | 2 | 112 | 1.79% |
| 1 | 1 | 1 | 100% |

The whole 18-piece result funnels through **1–2 canonical positions** across the
entire mid-game. 99.998% of the step-37 frontier is not on any deepest-best path.

## 2. "Live fraction": how much frontier ever contributes to a best

The central question: of the frontier at a shallow step, how much is an
*ancestor of a max-piece best at some deeper step* (vs dead weight)?

`live_deep` = canonical frontier positions whose descendants appear as the
max-piece best at some **strictly deeper** step (this is the user's question);
`live` additionally counts being max-piece best at the step itself.

| step | live_deep | live | frontier | live_deep/frontier | live/frontier |
|---:|---:|---:|---:|---:|---:|
| 37 | 0 | 96 | 4,063,882 | 0% | 0.0024% |
| 31 | 184 | 256 | 810,222 | 0.023% | 0.032% |
| 25 | 140 | 172 | 146,756 | 0.095% | 0.117% |
| 19 | 36 | 48 | 34,798 | 0.104% | 0.138% |
| 17 | 30 | 54 | 61,382 | 0.049% | 0.088% |
| 15 | 13 | 109 | 24,964 | 0.052% | 0.437% |
| 13 | 39 | 135 | 7,856 | 0.496% | 1.72% |
| **11** | **47** | **83** | **1,816** | **2.59%** | **4.57%** |
| 9 | 20 | 33 | 298 | 6.71% | 11.1% |
| 7 | 14 | 14 | 112 | 12.5% | 12.5% |

**Answer to the step-11 question:** of the 1,816 canonical positions in the
step-11 frontier, only **47 (2.6%)** have a descendant that ever becomes the
max-piece best deeper; **83 (4.6%)** are live including being best at step 11
itself. **~95% are dead** — they never contribute to any max-piece result.

The live fraction *falls* with depth (step 11: 2.6% → step 31: 0.02%). The
deeper the search, the larger the share of frontier work that is, in hindsight,
irrelevant to the maximal-piece answer.

> Caveat: this is **descriptive, not a pruning rule** — we cannot know a priori
> which 3% is live (that is the search result). Dead positions are also not
> "wasted" for *correctness*: they are still needed to *prove* uniqueness of the
> live ones. (The shallow steps 1–5 show >100% because the frontier there is a
> seed-initialization artifact, not a true unique-position count.)

## 3. Piece-count trajectory — the "smoke" shape

Along the deepest cone, piece count is monotone toward mate (min==max at every
step on the cone, i.e. the cone is single-valued in piece count):

```
step  37 35 33 31 29 27 25 23 21 19 17 15 13 11  9  7  5  3  1
pieces 18 16 14 15 14 12 13 12 11 11 11 10  9  8  7  6  5  4  3
```

Two observations matter for the 40-piece goal:

- The cone rides the admissible bound `max = step/2 + 3` **early** (step 11:
  8 pieces = the bound) but **falls below it at depth** (step 37: 18 vs bound
  21 — a 3-piece gap). For this seed/constraints you cannot keep adding a piece
  every 2 plies; the achievable maximum grows sub-linearly in the bound.
- The per-step max-piece positions *are* the cone (the global-best solution
  passes through the highest-piece position available at each step). So **piece
  count is a strong guidance signal**: a search biased toward high-piece
  positions naturally follows the live cone.

## 4. Implications for reaching 40 pieces

- **Exact full-width is infeasible.** 40 pieces needs step ≈ `2·(40−3) = 74`
  (and likely deeper, given the sub-linear gap above). The frontier grows
  **~1.7× per 2 steps** (4.06M @ 37). Extrapolating to step 74 gives
  ~1.7^18 × 4M ≈ **10^10–10^11 positions** — far beyond memory (the run already
  OOMs near step 38–39 on 128 GB).
- **The problem is search *guidance*, not search *speed*.** Sections 1–2 show the
  relevant cone is ~0.002–5% of the frontier and strongly convergent. A beam /
  heuristic that retains the live cone could reach deep bests at a tiny fraction
  of the cost; the engine already has a beam mode. The open problem is a *scoring
  function that keeps the live ~3% per step* without dropping the eventual best.
- **Piece count is the obvious first beam feature** (§3): the best cone coincides
  with the per-step max-piece set. Promising next step: evaluate a beam keyed on
  (piece count, promoted/pawn structure) and measure cone retention vs beam width
  — i.e. how narrow a beam still recovers the 18-piece (and deeper) bests.

## Files

- [`run.sh`](run.sh) — regenerate `data/` and the tables above.
- [`data/best_step_<S>.txt`](data/) — canonical-URL max-piece positions at step S.
- [`data/frontier.txt`](data/frontier.txt) — `<step> <frontier_size>`.
- [`fmrs_core/tests/smoke_cone_analysis.rs`](../../fmrs_core/tests/smoke_cone_analysis.rs) — the analysis.
- Data-collection hook: `FMRS_PERSTEP_BEST_DIR` in
  `src/command/single_king_smoke/search.rs` (env-gated, zero cost when unset).
