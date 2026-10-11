"""fmrs とは別の合法手生成（python-shogi 1.1.1）で、協力詰の最短手数と最短解の数を幅優先で求める。

usage: python3 -I verify_bfs.py <out.jsonl> <sfen>...
python-shogi は同じディレクトリの pydeps/ に入れておく
（python3 -m pip install --target pydeps python-shogi==1.1.1）。
各局面について、最短手数・最短解数（2以上は2に丸める）・解手順・解の各局面の
盤上枚数と駒種を出力する。局面の同一視は盤面・手番・双方持駒の SFEN（手数を除く）。
"""
import collections
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "pydeps"))
import shogi  # noqa: E402

PAWN_ONLY = {shogi.KING, shogi.PAWN, shogi.PROM_PAWN}


def key(b):
    return " ".join(b.sfen().split()[:3])


def board_types(b):
    return [p.piece_type for s in shogi.SQUARES if (p := b.piece_at(s))]


def bfs(sfen):
    start = time.monotonic()
    initial = key(shogi.Board(sfen))
    distance = {initial: 0}
    layer = {initial: (1, ())}
    depth = 0
    while layer:
        mates = []
        for state, (paths, path) in layer.items():
            b = shogi.Board(state + " 1")
            if b.turn == shogi.WHITE and b.is_checkmate():
                mates.append((state, paths, path))
        if mates:
            return {
                "shortest_plies": depth,
                "shortest_solution_count_capped_at_2": min(2, sum(m[1] for m in mates)),
                "visited_states": len(distance),
                "seconds": round(time.monotonic() - start, 2),
                "solution": list(mates[0][2]),
            }
        nxt = {}
        for state, (paths, path) in layer.items():
            b = shogi.Board(state + " 1")
            side = b.turn
            for m in list(b.legal_moves):
                if b.piece_type_at(m.to_square) == shogi.KING:
                    continue
                b.push(m)
                if side == shogi.BLACK and not b.is_check():
                    b.pop()
                    continue
                target = key(b)
                b.pop()
                old = distance.get(target)
                if old is not None and old < depth + 1:
                    continue
                if target in nxt:
                    n, p = nxt[target]
                    nxt[target] = (min(2, n + paths), p)
                else:
                    nxt[target] = (paths, path + (m.usi(),))
                    distance[target] = depth + 1
        layer = nxt
        depth += 1
    return {"shortest_plies": None, "shortest_solution_count_capped_at_2": 0,
            "visited_states": len(distance), "seconds": round(time.monotonic() - start, 2)}


def replay(sfen, moves):
    b = shogi.Board(sfen)
    pieces = [len(board_types(b))]
    pawn_only = all(t in PAWN_ONLY for t in board_types(b))
    for u in moves:
        b.push(shogi.Move.from_usi(u))
        pieces.append(len(board_types(b)))
        pawn_only = pawn_only and all(t in PAWN_ONLY for t in board_types(b))
    return {
        "initial_board_pieces": pieces[0],
        "final_board_pieces": pieces[-1],
        "final_sfen": b.sfen(),
        "board_pawn_only_throughout": pawn_only,
        "black_hand_empty_at_start": not shogi.Board(sfen).pieces_in_hand[shogi.BLACK],
        "board_pieces_by_ply": pieces,
    }


def main():
    out = Path(sys.argv[1])
    with out.open("a") as f:
        for sfen in sys.argv[2:]:
            r = {"sfen": sfen, "library": "python-shogi 1.1.1", **bfs(sfen)}
            if r.get("solution"):
                r.update(replay(sfen, r["solution"]))
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
            print(r["sfen"], r["shortest_plies"], r["shortest_solution_count_capped_at_2"],
                  r.get("initial_board_pieces"), r.get("final_board_pieces"), r["seconds"], flush=True)


if __name__ == "__main__":
    main()
