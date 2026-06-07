# 40枚全駒煙 サルベージ (d=34 seed run, 2026-06-06)

「若い step (d=34) から beam を開始すれば、より多様・より長手数の40枚が出る」という仮説の検証 run。
fmrs-spot (m3-ultramem-128) で実行、**正常終了 (DONE exit=0)**。

## 実行コマンド (再現可能・決定的)

seed = d=42 gateway を unique solution に沿って 8手戻した d=34 局面:
`8G/6K1p/9/3+P2P+p1/4PP3/7kP/7P1/9/9 w P2r2b3g4s4n4l9p 1`

```
cargo r -r single-king-smoke ideal-backward --parallel 128 --allow-white-pieces \
  --slack 100 --double-king \
  --seed-sfen '8G/6K1p/9/3+P2P+p1/4PP3/7kP/7P1/9/9 w P2r2b3g4s4n4l9p 1' \
  --canonicalize-attacker-goldish --min-pawn-pct 44 \
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority \
  --lance-knight-allow-start 8 --lance-knight-allow-step 2 \
  --beam-width 2000000 --beam-sota --beam-width-at 75:10000000 --beam-width-max 30000000 \
  --seed-result-log /tmp/fmrs-d34-beam.jsonl
```

## 結果: 40枚が3つの手数で出現

| 手数 | 局面数 | 回収状況 |
|---|---|---|
| **115手** | 67 | **全67局を完全回収** (`40piece_115ply.sfen` / `.url.txt`) ← **新記録 (従来113手)** |
| 113手 | 6765 | **代表1局のみ** (`40piece_113ply_representative.sfen`) |
| 109手 | 92 | **代表1局のみ** (`40piece_109ply_representative.sfen`) |

### なぜ 113手・109手は代表1局しか無いのか
engine は `(駒数, 手数)` の**辞書順 max の best 集合のみ**を最終 dump する。
40@115 が 40@113・40@109 を辞書順で上回るため、**最終 best = 40@115 (67局) だけ**が完全出力された。
113手(6765)・109手(92)は「改善時ログ」に count と代表URL 1件しか残っておらず、
**checkpoint も空** (`/tmp/.checkpoints` に何も無し。rolling 上書き式でファイル名に step を含まないため最終状態のみ) だった。
→ **109手・113手の全局を得るには再 run が必要** (下記)。

## 検証 (報告前に実施済み)
- 115手 先頭局 / 109手代表 / 113手代表 を `smoke_dump_path` (standard_solve) で検証:
  - **3局とも plies 一致 (115/109/113)・root 40枚・解が一意 (sols==1)**。
- 115手 全67局: 盤上40枚・持駒空 (`b - 1`) を機械的に確認 (NG 0件)。

## ファイル
- `40piece_115ply.sfen` — 115手・全67局 (SFEN, sort 済, unique)
- `40piece_115ply.url.txt` — 同上の URL 形式
- `40piece_109ply_representative.sfen` — 109手の代表1局
- `40piece_113ply_representative.sfen` — 113手の代表1局
- `d34_run.log` — 実行ログ全体 (285行, 揮発前に退避)

## confluence 解析: smallest unique 祖先 (gateway)

115手・全67局を `smoke_confluence` で解析 (`confluence_115ply.txt`)。
各 smoke の一意解を mate からの距離 d ごとに canonical 重複除去して数えた。

- **全67局が d=0(mate)〜d=42 まで完全に単一の forced spine を共有**。**d=43 で初分岐**。
- **共通祖先 (gateway, d=42, 盤上14枚):**
  `8G/6K1p/9/3+P2P+pG/4PP1pk/9/8P/6NP1/9 w N2r2b2g4s2n4l9p 1`
  - これが67局すべてが必ず通る最深の単一局面 = この 115手 cone の「funnel」。
- 分岐プロファイル(d: distinct): ~d42:**1** → d43–49:2 → d50–77:4 → d78–96:5 → d97–98:19 → d99–114:40 → **d115(root):67**。
  = 長い42手の単一 spine の上に、上位 ~73手で開く crown。多様性の大半は最後 ~18手(d≥97)に集中。

**注目点:** この d=42 gateway は `...3+P2P+pG/...`(金)で、当初の113手 run の seed だった d=42 gateway
`...3+P2P+p+S/...`(成銀)とは**別局面**。つまり 115手系列は、113手系列とは **d=35 以降で分かれる別 lineage**
(両者が共有するのは d≤34 = 今回の seed `8G/6K1p/9/3+P2P+p1/4PP3/7kP/7P1/9/9 w P2r2b3g4s4n4l9p 1` まで)。
→ 「若い step(d=34)から始めると、別 lineage の・より長手数(115)の40が拾える」という今回の仮説の裏付け。

### 全手数横断 (109/113/115) の最小共通祖先

109・113 は代表1局ずつで十分(同一 lineage のメンバーは合流点まで同じ spine を通るため、
合流点の特定には1局あれば足りる)。67(115)+1(113)+1(109)=69局をまとめて confluence した
(`confluence_all_109_113_115.txt`):

- **3手数すべてが d=0〜d=42 まで単一 spine を共有、d=43 で初分岐**(d43:2 → d44:3 → d50+:5 …)。
- **全40枚(109/113/115)の smallest unique 共通祖先 = 115手単独と全く同じ d=42・14枚局面:**
  `8G/6K1p/9/3+P2P+pG/4PP1pk/9/8P/6NP1/9 w N2r2b2g4s2n4l9p 1`
- つまり **この d=34 run が出した40枚はすべて、この単一の d=42 gateway(金型)から派生**している。
  当初 run の d=42 gateway(成銀型 `...+p+S...`)とは別物 = **別 lineage** であることが確定。
- 我々の seed (d=34, 11枚) はこの gateway の更に下の spine 上(d=34<42)にあり、
  forced backward で d=42 gateway に到達する。

## 109手・113手の全局を回収するには (未実施)
RNG は決定的 (per-局面 seed) なので、**同一コマンドの再 run で同じ 40@109/113/115 を bit 一致で再現**できる。
ただし最終 best しか dump されない問題は同じ。全手数の40を取りこぼさず保存するには:
- **per-step dump を有効化して再 run** (search.rs の per-step dump hook。step ごとに 40枚を別ファイルへ)、
- または `--early-exit` を使わず、目的手数 (113) を seed 解長として gateway-seed で exact / 厚い beam を当てる、
等が必要。所要は d=34→115 の beam 全体 (fmrs-spot で数時間規模)。
