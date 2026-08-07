# fmrs 全駒煙探索：特徴量の寄与度（SOTA モデル `beam_sota.json` に基づく）

各特徴の定義は [`FEATURES.md`](FEATURES.md)、探索側の文脈は [`REPORT.md`](REPORT.md) を参照。

## 0. 何を測ったか（重要な注意）

`rust/models/beam_sota.json`（= `cone_dp_gbdt.json`、`--beam-sota` に埋め込まれた現行 SOTA モデル。
sklearn `HistGradientBoostingRegressor`、300 木、85 特徴）の**木構造そのもの**から寄与度を算出した。

- 指標 **gain%**：各内部ノードについて `|左部分木の平均予測値 − 右部分木の平均予測値| × (部分木の葉数 / 木の葉数)` を、
  その分岐に使われた特徴に加算し、全体で正規化した値。**予測値をどれだけ動かしたか**の構造的な代理量。
- エクスポート JSON にはサンプル数が含まれないため、これは sklearn の gain/permutation importance そのものではない。
  ただし `REPORT.md` に記録された著者自身の GBDT permutation importance の上位
  （`step`, `total_black_kiki`, `king_flight_cov_avg`, `king_escape_depth`, `king_ray_freedom`,
  `king_liberties`, `row_std`, `king_centroid_cheby`, `white_mobility`）と順位がほぼ一致しており、
  相互に裏付けが取れている。
- 学習データ上の permutation importance を取り直す場合は、`train_cone_model.py` の
  `permutation_importance` を DP ラベル版データセットで再実行して差し替えるのが望ましい
  （REPORT.md §6 には順位のみが記録され、数値が残っていない）。
- 頑健性確認のため、別ラベル・別条件で学習された `cone_44_gbdt.json` / `cone_pct32_gbdt.json` でも同じ計算を行い、
  3 モデル平均を併記した（上位の顔ぶれはほぼ不変）。

**`board_total` が突出する理由**：回帰ターゲット `best_piece_reachable`（その局面から到達可能な最大駒数）は
定義上その局面の駒数以上なので、現在の駒数が予測値の大半を説明してしまう。
`REPORT.md` の通り、駒数だけで並べても step 内 Spearman は 0.95 に達する。
探索が実際に使う信号は **同一 (step, 駒数) セル内の順位**であり、そこでは駒数は無意味になる。
よって表には「`board_total`・`step` を除いて再正規化した寄与率」の列を設け、
**この列こそが「作家の直感を数値化した特徴が効いているか」を表す**。
なお SOTA モデルのセル内 Spearman は **0.494**（トレース下界ラベル時代は 0.225 → 0.322）。

---

## 1. 採用すべき特徴（上位 45、累積 gain 99.1%）

カテゴリ記号：A 位相 / B 駒種別枚数 / C 持駒 / D 玉の包囲 / E 盤の広がり / F 玉の自由度 / G 配置の散らばり

| # | 特徴 | 分類 | gain% | 除 `board_total`,`step` 後の% | 3モデル平均 gain% | 意味 |
|---:|---|---|---:|---:|---:|---|
| 1 | `board_total` | E 広がり | 22.71 | — | 22.92 | 盤上総駒数 |
| 2 | `step` | A 位相 | 7.80 | — | 8.48 | 詰みからの逆算手数（位相） |
| 3 | `king_centroid_cheby` | F 玉自由度 | 3.69 | 5.30 | 3.65 | 玉と駒重心のチェビシェフ距離 |
| 4 | `king_escape_depth` | F 玉自由度 | 3.68 | 5.29 | 3.50 | 安全な逃げ道の2手先の広さ |
| 5 | `king_white_neighbors_attacked` | D 玉包囲 | 3.50 | 5.04 | 2.85 | 玉8近傍のうち攻方利きのあるマス数 |
| 6 | `total_black_kiki` | D 玉包囲 | 3.18 | 4.57 | 3.13 | 攻方の利きの盤面総和 |
| 7 | `king_flight_cov_avg` | F 玉自由度 | 3.16 | 4.54 | 3.52 | 玉8近傍の空マスの平均被利き数 |
| 8 | `king_ray_freedom` | F 玉自由度 | 3.15 | 4.53 | 3.71 | 玉から8方向の空マス総数 |
| 9 | `hand_white_total` | C 持駒 | 3.04 | 4.38 | 2.55 | 玉方の持駒総数 |
| 10 | `row_std` | G 配置 | 2.93 | 4.21 | 3.63 | 占有マスの段の標準偏差 |
| 11 | `white_mobility` | F 玉自由度 | 2.79 | 4.02 | 2.81 | 玉方全駒の利き総和 |
| 12 | `king_white_row` | D 玉包囲 | 2.78 | 4.01 | 1.95 | 玉の段 |
| 13 | `bbox_area` | G 配置 | 2.39 | 3.44 | 1.29 | 占有駒の外接矩形面積 |
| 14 | `col_std` | G 配置 | 2.32 | 3.34 | 2.41 | 占有マスの筋の標準偏差 |
| 15 | `king_liberties` | F 玉自由度 | 2.22 | 3.20 | 1.95 | 玉8近傍の空マス数 |
| 16 | `board_max_row` | E 広がり | 2.03 | 2.92 | 1.66 | 駒が存在する最大段 |
| 17 | `board_density` | G 配置 | 1.95 | 2.81 | 2.09 | 外接矩形内の駒密度 |
| 18 | `board_black_pawn` | B 駒種 | 1.90 | 2.73 | 2.15 | 盤上の攻方歩数 |
| 19 | `king_white_col` | D 玉包囲 | 1.83 | 2.63 | 1.37 | 玉の筋 |
| 20 | `king_white_neighbors_white` | D 玉包囲 | 1.70 | 2.45 | 1.15 | 玉8近傍の玉方駒数 |
| 21 | `king_overcovered_flights` | F 玉自由度 | 1.64 | 2.37 | 1.55 | 玉8近傍で利き2以上の空マス数 |
| 22 | `occupied_ranks` | G 配置 | 1.60 | 2.30 | 1.49 | 駒がある段の数 |
| 23 | `overcovered_squares` | D 玉包囲 | 1.49 | 2.14 | 1.54 | 盤面で利き2以上のマス数 |
| 24 | `king_white_neighbors_black` | D 玉包囲 | 1.35 | 1.94 | 1.13 | 玉8近傍の攻方駒数 |
| 25 | `king_net_frac` | F 玉自由度 | 1.31 | 1.88 | 1.07 | 玉8近傍の包囲率 |
| 26 | `board_black_silver` | B 駒種 | 1.27 | 1.82 | 1.98 | 盤上の攻方銀数 |
| 27 | `king_white_2ring_black` | D 玉包囲 | 1.26 | 1.82 | 1.54 | 玉中心5x5の攻方駒数 |
| 28 | `board_black` | E 広がり | 1.02 | 1.47 | 1.15 | 盤上の攻方駒数 |
| 29 | `black_far_from_king` | G 配置 | 0.91 | 1.31 | 1.28 | 玉から距離>2の攻方駒数 |
| 30 | `king_safe_flights` | F 玉自由度 | 0.85 | 1.22 | 0.70 | 利き0の逃げ道数 |
| 31 | `board_min_row` | E 広がり | 0.80 | 1.15 | 0.78 | 駒が存在する最小段 |
| 32 | `board_max_col` | E 広がり | 0.75 | 1.08 | 0.58 | 駒が存在する最大筋 |
| 33 | `hand_white_lance` | C 持駒 | 0.66 | 0.95 | 0.32 | 玉方持駒の香数 |
| 34 | `board_black_lance` | B 駒種 | 0.65 | 0.94 | 0.80 | 盤上の攻方香数 |
| 35 | `occupied_files` | G 配置 | 0.64 | 0.92 | 0.66 | 駒がある筋の数 |
| 36 | `white_nonking_far` | G 配置 | 0.61 | 0.88 | 0.94 | 玉から距離>2の玉方駒数 |
| 37 | `board_white` | E 広がり | 0.61 | 0.87 | 0.93 | 盤上の玉方駒数 |
| 38 | `king_white_min_edge_dist` | D 玉包囲 | 0.59 | 0.85 | 0.72 | 玉の盤端までの最小距離 |
| 39 | `hand_white_knight` | C 持駒 | 0.50 | 0.73 | 0.35 | 玉方持駒の桂数 |
| 40 | `board_white_ppawn` | B 駒種 | 0.49 | 0.71 | 0.60 | 盤上の玉方と金数 |
| 41 | `board_row_spread` | E 広がり | 0.41 | 0.60 | 0.30 | 段方向の広がり |
| 42 | `hand_white_pawn` | C 持駒 | 0.27 | 0.39 | 0.63 | 玉方持駒の歩数 |
| 43 | `hand_white_silver` | C 持駒 | 0.27 | 0.38 | 0.18 | 玉方持駒の銀数 |
| 44 | `board_white_pawn` | B 駒種 | 0.22 | 0.32 | 0.15 | 盤上の玉方歩数 |
| 45 | `promoted_total` | G 配置 | 0.18 | 0.26 | 0.25 | 成駒の総数 |

### 読み取れること

- **駒数と位相を除くと、上位は F（玉の自由度）と D（玉の包囲）と G（配置の散らばり）でほぼ占められる。**
  すなわち、詰将棋作家の「まだ余裕がある／もう窮屈だ」「駒が散っていて煙らしい」という感覚を数値化した特徴が、
  実際に予測を駆動している。これは本手法の主張（専門家知識の機械学習による近似）を支持する定量的証拠になる。
- **B（駒種別枚数）はほぼ全滅で、例外は攻方の歩・銀・香のみ。** 個別の駒種構成より、
  盤上の総量と玉まわりの幾何のほうが「深く伸びるか」を決めている。
- **玉方の持駒総数 `hand_white_total` が単独で 9 位**（除外後 4.4%）に来るのは、
  協力詰の逆算では玉方持駒＝これから盤上へ戻せる在庫であり、伸びしろの直接の指標になっているため。


### カテゴリ別集計

| 分類 | 採用次元 | gain% | 除 `board_total`,`step` 後の% |
|---|---:|---:|---:|
| A 位相（`step`） | 1 | 7.8 | — |
| E 盤の広がり（うち `board_total`） | 7 (1) | 28.3 (22.7) | 8.1 |
| F 玉の自由度 | 9 | 22.5 | 32.4 |
| D 玉の包囲 | 9 | 17.7 | 25.5 |
| G 配置の散らばり | 9 | 13.5 | 19.5 |
| C 持駒 | 5 | 4.7 | 6.8 |
| B 駒種別枚数 | 5 | 4.5 | 6.5 |
| H 余詰（`black_check_moves`） | 0 | 0.0 | 0.0 |

---

## 2. 落としてよい特徴（40 個、累積 gain 0.9%）

### 2.1 3 モデルすべてで一度も分岐に使われなかった（20 個）— 定数または完全共線

| 特徴 | 使われなかった理由（推定） |
|---|---|
| `empty_squares` | `81 − board_total` の一次従属。木は `board_total` を選ぶ |
| `black_check_moves` | `FMRS_FEAT_HEAVY` が既定 off のため常に 0.0（定数）。REPORT でも「kiki と冗長で寄与僅少」と記録 |
| `king_white_attackers` | この探索の frontier 局面では実質定数 |
| `hand_black_total`, `hand_black_{pawn,lance,knight,silver,gold,bishop,rook}`（8個） | 攻方持駒はこの探索設定では動かず定数 |
| `board_black_king`, `board_white_king` | 玉の枚数は定数 |
| `board_{black,white}_rook`, `board_{black,white}_bishop` 系, `board_white_{lance,knight}` 等（7個） | `--rook-bishop-allow-start 31` 等の制約下、データの駒数帯（〜22枚）では盤上に現れず定数 0 |
| `black_pawn_columns` | 歩の筋数は `board_black_pawn` と強く共線 |

### 2.2 使われたが寄与が無視できる（20 個、いずれも gain < 0.16%）

`board_black_ppawn`, `board_black_knight`, `board_white_gold`, `board_min_col`,
`board_white_psilver`, `board_black_psilver`, `board_col_spread`, `board_black_gold`,
`board_white_pknight`, `hand_white_bishop`, `hand_white_gold`, `board_white_plance`,
`board_black_plance`, `board_white_silver`, ほか成駒・大駒の枚数系。

これらを落とすと **85 → 45 次元**（−47%）。gain ベースでは 0.9% の損失で、
特徴抽出のうち重い部分（`black_check_moves` の王手生成）も不要になる。

---

## 3. 再現方法

本ファイルの数値は次のコマンドで再現できる：

```
cd rust && python3 analysis/smoke_cone/gbdt_structural_importance.py \
    models/beam_sota.json models/cone_44_gbdt.json models/cone_pct32_gbdt.json
```
