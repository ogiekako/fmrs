# Instructions

- Use `cargo t` instead of `cargo test` (runs `--all` by default).

# Domain

- This project solves **helpmate-tsume (協力詰)**, NOT adversarial tsume. Both
  sides cooperate toward mate, so the search has no AND/OR minimax structure.
  Consequently adversarial-search techniques like **df-pn / proof-number search
  do NOT apply**. "Uniqueness" of a position means there is exactly **one**
  cooperative move sequence to mate — i.e. it is a **solution-path-count == 1**
  problem over the DAG of positions, for which BFS / topological path-count DP is
  the natural framing (not best-first AND/OR search).
