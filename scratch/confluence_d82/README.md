# 40枚候補は39枚系列の局面を通るか（2026-10-17）

論文 5.3節の「2618個の解手順は，詰みから82手の時点でいずれも盤上25枚または26枚であり，
系列Aの合流点（27枚）も，系列B，Cの解手順上の同じ手数の局面（いずれも29枚）も通らない」の根拠。

各局面の最短解（fmrs `solve standard`）をたどって、詰みから82手の局面の盤上枚数を数えた。
盤上枚数は金類の正規化で変わらないので、枚数が違えば同じ局面ではない。

| 集合 | 手数 | 詰みから82手の局面の盤上枚数 |
|---|---|---|
| 系列Aの合流点（`scratch/zenkoma_smoke_journey.md` 付録A） | — | 27 |
| 系列B（`lineage_bc_roots.txt` 1行目、journey 付録Aの new2） | 103 | 29 |
| 系列C（同2行目、new3） | 105 | 29 |
| 2618候補（`paper/main/verification/sources/40piece_solutions.txt`） | 113 | 25（680局）、26（1938局） |

系列Aの合流点: `8G/6K1p/7S1/3k+P1P+pn/4PP1pN/8P/7N+P/3P2NP+P/G+pL1LL1S+p w 2P2r2b2g2sl2p 1`

## 再現

```sh
python3 scratch/confluence_d82/d_from_mate.py scratch/confluence_d82/lineage_bc_roots.txt 82 > lineage_bc_d82.jsonl
python3 scratch/confluence_d82/d_from_mate.py paper/main/verification/sources/40piece_solutions.txt 82 > 40piece_2618_d82.jsonl
```

リポジトリ直下で実行する。fmrs は `rust/target/release/fmrs`（第3引数で変えられる）、
python-shogi 1.1.1 は `scratch/tofu_smoke/pydeps` に入れるか `PYTHONPATH` で渡す。
