#!/usr/bin/env python3
"""Exact max-reachable-pieces value DP over recorded parent->child edges.

Reads the binary edge file from a run with FMRS_EDGE_FILE set (records: parent
digest u64, child digest u64, parent pieces u8, child pieces u8, little-endian;
parent = position at step s, child = its surviving predecessor at step s+2, which
has >= pieces). Computes value(d) = max pieces over all positions reachable by
continuing the backward search from d (= max over the subtree of descendants),
via a fixpoint: value init = own pieces, then repeatedly value[parent] =
max(value[parent], value[child]) until stable. This is the *exact* label (within
the recorded DAG), replacing the trace-sampled lower bound.

Output: binary digest(u64) value(u8) pairs to --out (loaded by
smoke_cone_analysis when FMRS_EDGE_VALUE_FILE is set).
"""
import argparse
import struct
import sys

import numpy as np


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--edges", required=True)
    ap.add_argument("--out")
    args = ap.parse_args()

    raw = np.fromfile(args.edges, dtype=np.uint8)
    n = len(raw) // 18
    raw = raw[: n * 18].reshape(n, 18)
    parent = raw[:, 0:8].copy().view(np.uint64).ravel()
    child = raw[:, 8:16].copy().view(np.uint64).ravel()
    ppc = raw[:, 16].astype(np.int32)
    cpc = raw[:, 17].astype(np.int32)
    print(f"edges={n}", file=sys.stderr)

    # Own piece count per digest (consistent across records).
    piece = {}
    for arr_d, arr_p in ((parent, ppc), (child, cpc)):
        for d, p in zip(arr_d.tolist(), arr_p.tolist()):
            piece[d] = max(piece.get(d, 0), p)  # should be equal; max guards
    value = dict(piece)

    # Fixpoint: value[parent] = max(value[parent], value[child]).
    par = parent.tolist()
    chi = child.tolist()
    passes = 0
    while True:
        changed = 0
        for p, c in zip(par, chi):
            vc = value[c]
            if vc > value[p]:
                value[p] = vc
                changed += 1
        passes += 1
        if changed == 0:
            break
    print(f"DP converged in {passes} passes; distinct positions={len(value)}", file=sys.stderr)

    # --- invariant checks ---
    bad = 0
    for d in value:
        if value[d] < piece[d]:
            bad += 1
    assert bad == 0, "value < own piece"
    edge_bad = sum(1 for p, c in zip(par, chi) if value[p] < value[c])
    assert edge_bad == 0, "value[parent] < value[child]"
    # deepest leaves (never a parent) must have value == own piece
    parents = set(par)
    leaves = [d for d in value if d not in parents]
    leaf_bad = sum(1 for d in leaves if value[d] != piece[d])
    print(
        f"invariants OK: value>=piece, parent>=child; leaves={len(leaves)} "
        f"(value==piece mismatches={leaf_bad})",
        file=sys.stderr,
    )
    vmax = max(value.values())
    print(f"max value (reachable pieces) = {vmax}", file=sys.stderr)
    # distribution
    from collections import Counter

    c = Counter(value.values())
    print("value histogram (top):", {k: c[k] for k in sorted(c, reverse=True)[:8]}, file=sys.stderr)

    if args.out:
        with open(args.out, "wb") as f:
            for d, v in value.items():
                f.write(struct.pack("<QB", d, v))
        print(f"wrote {len(value)} (digest,value) to {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
