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

## 5. Labeled ML dataset (`data/dataset.csv`)

To make "which positions are live" *learnable* (a beam scoring function toward
40 pieces), `run.sh` emits a labeled dataset. Each row is one canonical
output-valid frontier position (deduped by canonical digest).

| column | meaning |
|---|---|
| `step` | plies remaining to mate |
| `piece_count` | board pieces (feature) |
| `live_deeper` | 1 if an ancestor of a max-piece best at a STRICTLY deeper step (cone-based; strict, rare) |
| `max_best_depth` | deepest step at which it is an ancestor of a max-piece best (0 if none) |
| `best_piece_reachable` | **regression target**: max piece count of any deep endpoint reachable from it (lower bound, from tracing per-step bests + a subsample of deep frontier positions back toward mate) |
| `sfen` | the position (featurize downstream) |

Row sources: all per-step max-piece bests (the discriminative population) plus a
uniform frontier sample (broad negatives). Dedup by canonical digest.

The committed `data/dataset.csv` is from a **deep GCP run to step 49** (exact,
non-split with `--memo-retain-from-step 999` so memory tracks the frontier, on
n2d-highmem-96 / 768 GB; the frontier OOMs near step 38–39 on 128 GB locally):

- **282,088 rows**; `best_piece_reachable` spans **3–22 pieces** (mass at 11–15;
  ~3,650 rows reach 19–22). `live_deeper == 1`: 769 (strict cone label).
- `best_piece_reachable` floor = the position's own piece count (a position
  trivially reaches its own count; deeper descendants only add pieces), raised
  by traced deep endpoints — so it is a **lower bound** on true reachable value,
  also capped by the run depth (49). Tracing is subsampled
  (`FMRS_BEST_TRACE_CAP`, `FMRS_TRACE_CAP`) so deep `max_best_depth` labels are
  themselves lower bounds.

Reaching 30+ pieces exactly is infeasible (frontier grows ~1.9×/2 steps → ~10^9
at step ~58); the next extension is **beam** (top-K) sampling past the exact
depth (see §4).

## 6. Learning a beam scorer ("why does a position live?")

Goal: a model that, at a given step, ranks frontier positions by how deep/
high-piece their descendants reach — a beam scoring function toward 40 pieces.

Pipeline: `single-king-smoke cone-features --dataset data/dataset.csv -o train.csv`
(runs `extract_features` per position) → `train_cone_model.py` → a `LinearModel`
JSON for the Rust beam (`--beam-model`). Composer-intuition features were added
to `extract_features` (king liberties / safe flights / flight coverage / escape
depth / ray freedom / net tightness, white mobility, board dispersion &
centroid, promoted count, and an opt-in `black_check_moves`), plus `step`
(phase).

Key methodological point: `best_piece_reachable` is **dominated by the current
piece count** (rank-by-piece-count alone gives per-step Spearman 0.95). The
*interesting* signal is the **gain** = reachable − current pieces, evaluated
**within (step, piece-count) cells** — "which of the same-piece positions extend
deeper". Evaluated with GroupKFold (group = `max_best_depth`, so a whole
solution path stays in one fold; dead rows split freely):

| model | within-cell Spearman (gain) |
|---|---|
| Ridge (linear) | 0.15 |
| GBDT (HistGradientBoosting) | **0.225** |

So the promise IS predictable but **weak–moderate and nonlinear**. The top
drivers (GBDT permutation importance) are exactly the human-intuition features:
`step` (phase), `total_black_kiki`, `king_flight_cov_avg`, `king_escape_depth`,
`king_ray_freedom`, `king_liberties`, `row_std`/dispersion, `king_centroid_cheby`,
`white_mobility` — piece count is irrelevant within a cell (as it should be).
`black_check_moves` is individually informative (#2) but redundant with
`total_black_kiki`, so it barely helps the ensemble and is left off by default
(opt-in via `FMRS_FEAT_HEAVY=1`; ~2 s to compute over the whole dataset).

### Beam validation — the selection rule matters more than the model

Running the beam (`--beam-width`, deep `--max-step`) and comparing scorers
revealed two things:

1. **Label accuracy drives model quality.** Tracing the full sample back (not
   just the per-step max-piece spine) doubled the "promise" rows (15.9k → 30.7k
   with reachable > current pieces) and lifted within-cell GBDT Spearman
   **0.225 → 0.322**. Drivers stay the intuition features (step, kiki, king
   escape depth / liberties / flight coverage / ray freedom / net, dispersion).
2. **Top-K selection kills diversity → loses to random.** At width 50 000 the
   greedy value beam *collapses* (high-value positions are similar; the search
   narrows and dies at step 51, 21 pieces), while a uniform/random beam keeps
   diverse lines and reaches 28 pieces at step 71. A per-position scorer, used
   as strict top-K, can't beat random no matter how good.

The fix is **value × diversity**: `--beam-temperature T` perturbs scores by
`T·Gumbel` and takes top-K — i.e. samples K without replacement ∝ exp(score/T)
(T=0 greedy, T→∞ random). With the full-trace reachable model at **T=5**, the
beam **matches the exact optimum (22 pieces at step 49, where random reaches only
19)** and tracks higher pieces at every deep step, reaching 28 by step 67 (random
needs step 71; greedy collapses at 21). So a learned value model **does** beat
random / piece-count once selection preserves diversity.

Open next: push T=5 deeper toward 30+; tune (T, width); and close the linear↔
GBDT gap (per-cell 0.20 vs 0.32) with GBDT-in-beam or interaction features.

## Files

- [`run.sh`](run.sh) — regenerate `data/` and the tables above.
- [`data/best_step_<S>.txt`](data/) — canonical-URL max-piece positions at step S.
- [`data/frontier.txt`](data/frontier.txt) — `<step> <frontier_size>`.
- [`fmrs_core/tests/smoke_cone_analysis.rs`](../../fmrs_core/tests/smoke_cone_analysis.rs) — the analysis.
- Data-collection hook: `FMRS_PERSTEP_BEST_DIR` in
  `src/command/single_king_smoke/search.rs` (env-gated, zero cost when unset).
