"""各局面の最短解（fmrs solve standard）をたどり、詰みから d 手の局面を JSON Lines で出す。

usage: python3 d_from_mate.py <SFEN または URL の一覧> <d> [fmrs バイナリ] > out.jsonl
python-shogi は ../tofu_smoke/pydeps に置く（verify_bfs.py と同じ）。
"""
import json, subprocess, sys
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, str(Path(__file__).parent.parent / "tofu_smoke" / "pydeps"))
import shogi
FMRS = sys.argv[3] if len(sys.argv) > 3 else "rust/target/release/fmrs"
def at(sfen, d):
    p = subprocess.run([FMRS, "solve", "standard", sfen], capture_output=True, text=True)
    out = p.stdout.strip().split()
    n = len(out)
    b = shogi.Board(sfen)
    for m in out[: n - d]:
        b.push_usi(m)
    return n, b.sfen()
srcs = [l.strip().replace("https://ogiekako.github.io/fmrs/", "").replace("_", " ") for l in open(sys.argv[1]) if l.strip()]
d = int(sys.argv[2])
with ThreadPoolExecutor(32) as ex:
    for s, (n, r) in zip(srcs, ex.map(lambda s: at(s, d), srcs)):
        print(json.dumps({"src": s, "len": n, "at": r}))
