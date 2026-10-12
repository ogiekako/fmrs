"""煙詰の初形で、各駒が「飾り駒」かを調べる。

飾り駒の判定（論文と同じ定義）: その駒を盤から取り除いて受方の持駒に移しても、
最短解が一意で、元と同じ手順のままなら飾り駒とする。
求解は fmrs の `solve standard`（解が複数なら "Multiple solutions" を出す）。

usage: python3 kazari.py <fmrs binary> <out.jsonl> <sfen-or-url file> [jobs]
"""
import json
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

HAND_ORDER = "RBGSNLP"


def to_sfen(line):
    s = line.strip()
    s = s.replace("https://ogiekako.github.io/fmrs/", "")
    if " " not in s:
        s = s.replace("_", " ")
    return s


def parse_board(board):
    rows = []
    for r in board.split("/"):
        row, i = [], 0
        while i < len(r):
            c = r[i]
            if c.isdigit():
                row += [None] * int(c)
                i += 1
            elif c == "+":
                row.append("+" + r[i + 1])
                i += 2
            else:
                row.append(c)
                i += 1
        assert len(row) == 9, (board, r)
        rows.append(row)
    return rows


def board_str(rows):
    out = []
    for row in rows:
        s, e = "", 0
        for p in row:
            if p is None:
                e += 1
            else:
                if e:
                    s += str(e)
                    e = 0
                s += p
        if e:
            s += str(e)
        out.append(s)
    return "/".join(out)


def parse_hand(h):
    res = {}
    if h == "-":
        return res
    n = ""
    for c in h:
        if c.isdigit():
            n += c
        else:
            res[c] = res.get(c, 0) + (int(n) if n else 1)
            n = ""
    return res


def hand_str(hand):
    s = ""
    for side in (str.upper, str.lower):
        for k in HAND_ORDER:
            c = hand.get(side(k), 0)
            if c:
                s += (str(c) if c > 1 else "") + side(k)
    return s or "-"


def solve(fmrs, sfen):
    p = subprocess.run([fmrs, "solve", "standard", sfen], capture_output=True, text=True)
    err = p.stderr
    m = re.search(r"Solved in (\d+) steps", err)
    if not m:
        return {"steps": None, "multiple": False, "moves": None, "error": err.strip()[-200:]}
    return {"steps": int(m.group(1)), "multiple": "Multiple solutions" in err,
            "moves": p.stdout.strip()}


def removals(sfen):
    board, turn, hand = sfen.split()[:3]
    rows = parse_board(board)
    for r in range(9):
        for c in range(9):
            p = rows[r][c]
            if p is None or p.upper() == "K":
                continue
            new = [row[:] for row in rows]
            new[r][c] = None
            h = parse_hand(hand)
            base = p[-1].lower()  # 受方の持駒（成駒は元の駒に戻す）
            h[base] = h.get(base, 0) + 1
            sq = f"{9 - c}{'一二三四五六七八九'[r]}"
            yield sq, p, f"{board_str(new)} {turn} {hand_str(h)} 1"


def main():
    fmrs, out, src = sys.argv[1:4]
    jobs = int(sys.argv[4]) if len(sys.argv) > 4 else 32
    sfens = [to_sfen(l) for l in open(src) if l.strip()]
    with ThreadPoolExecutor(jobs) as ex, open(out, "w") as f:
        for sfen in sfens:
            base = solve(fmrs, sfen)
            rems = list(removals(sfen))
            results = list(ex.map(lambda t: (t, solve(fmrs, t[2])), rems))
            kazari = [
                {"square": sq, "piece": p}
                for (sq, p, _), r in results
                if r["steps"] == base["steps"] and not r["multiple"] and r["moves"] == base["moves"]
            ]
            rec = {"sfen": sfen, "steps": base["steps"], "unique": not base["multiple"],
                   "pieces_tested": len(rems), "kazari": kazari}
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            f.flush()


if __name__ == "__main__":
    main()
