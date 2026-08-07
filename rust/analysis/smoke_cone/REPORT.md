# 煙詰 best-cone 解析

single-king-smoke の逆算探索において、frontier 全体のうち*実際に役に立っている*部分が
どれだけを占めるのか、そしてそれがプロジェクトの目標——
**最多枚数の協力詰煙詰（目標 40 枚）の構成**——にとって何を意味するのかの調査。

再現は [`run.sh`](run.sh)（`--max-step 37` の実行で step ごとの最多枚数「best」局面を出力し、
続けて [`fmrs_core/tests/smoke_cone_analysis.rs`](../../fmrs_core/tests/smoke_cone_analysis.rs)
を実行する）。生の入力は [`data/`](data/) にコミット済み。実行設定は標準の 5 分 seed
`8k/6K+P1/...` に `--min-pawn-pct 60 --rook-bishop-allow-start 31
--lance-knight-allow-start 8 --max-file 7 --canonicalize-attacker-goldish` を付けたもの。

局面数はすべて frontier の重複排除に合わせた **canonical**（goldish 縮約後）の値。
`step` = 詰みまでの残り手数（frontier は奇数 step に存在する）。

- 特徴量 85 次元の定義は [`FEATURES.md`](FEATURES.md)。
- そのうちどれが効いているかは [`FEATURE_IMPORTANCE.md`](FEATURE_IMPORTANCE.md)。

## 1. 「best cone」は frontier の中の極めて薄い切片でしかない

最深 step（37）における最多枚数（18 枚）の best 局面 352 個を取り、それぞれの一意な解を
再構成して詰み方向へ辿る。これらの経路上に現れる相異なる局面（「cone」）の step ごとの数を、
frontier 全体と比べる：

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

18 枚の結果は、中盤全体を通じて **1〜2 個の canonical 局面**に収束している。
step 37 の frontier の 99.998% は、最深 best へ至るいかなる経路上にも乗っていない。

## 2. 「live 率」：frontier のうち best に寄与するのはどれだけか

中心的な問い：浅い step の frontier のうち、*より深い step の最多枚数 best の祖先*である
ものはどれだけか（残りは無駄な重み）。

`live_deep` = **厳密により深い** step で最多枚数 best として現れる子孫を持つ canonical
frontier 局面（これが元々の問い）。`live` はこれに加えて、その step 自体で最多枚数 best で
あるものも数える。

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

**step 11 の問いへの答え：** step 11 の frontier にある canonical 局面 1,816 個のうち、
より深い step で最多枚数 best になる子孫を持つものは **47 個（2.6%）**のみ。step 11 自体で
best であるものを含めても **83 個（4.6%）**。**約 95% は dead** であり、いかなる最多枚数の
結果にも寄与しない。

live 率は深くなるほど*下がる*（step 11: 2.6% → step 31: 0.02%）。探索が深くなるほど、
最多枚数の答えにとって——後から見れば——無関係な frontier 上の作業の割合が増えていく。

> 注意：これは**記述的な観察であって枝刈り規則ではない**。どの 3% が live なのかを事前に
> 知ることはできない（それこそが探索の結果である）。また dead な局面は*正しさ*の観点では
> 「無駄」ではない。live な局面の一意性を*証明*するために依然として必要である。
> （step 1〜5 で 100% を超えるのは、そこでの frontier が seed 初期化の副産物であり、
> 真の一意局面数ではないため。）

## 3. 駒数の推移——「煙」の形

最深 cone に沿って、駒数は詰みに向かって単調である（cone 上の各 step で min == max、
すなわち cone は駒数について一価）：

```
step  37 35 33 31 29 27 25 23 21 19 17 15 13 11  9  7  5  3  1
枚数  18 16 14 15 14 12 13 12 11 11 11 10  9  8  7  6  5  4  3
```

40 枚の目標にとって重要な観察が 2 つある：

- cone は**浅いうち**は許容上界 `max = step/2 + 3` に張り付いている（step 11 では
  8 枚 = 上界）が、**深部では上界から離れる**（step 37 で 18 枚に対し上界 21——3 枚の差）。
  この seed と制約のもとでは、2 手ごとに 1 枚ずつ増やし続けることはできず、
  達成可能な最大値は上界に対して劣線形にしか伸びない。
- step ごとの最多枚数局面が*そのまま* cone になっている（大域最良解は、各 step で
  利用可能な最多枚数の局面を通る）。したがって**駒数は強い誘導信号**であり、
  高駒数の局面を優先する探索は自然と live cone を辿ることになる。

## 4. 40 枚到達に向けた含意

- **厳密な全幅探索は不可能。** 40 枚には step ≈ `2·(40−3) = 74` が必要（上記の劣線形な
  ずれを考えるとおそらくもっと深い）。frontier は **2 step ごとに約 1.7 倍**に増える
  （step 37 で 406 万）。step 74 まで外挿すると 1.7^18 × 400 万 ≈ **10^10〜10^11 局面**——
  メモリの限界を遥かに超える（実際 128 GB では step 38〜39 付近で OOM する）。
- **問題は探索の*速度*ではなく*誘導*である。** §1〜2 が示す通り、関係する cone は
  frontier の 0.002〜5% であり、しかも強く収束している。live cone を保持できる
  beam やヒューリスティクスがあれば、ごく僅かなコストで深部の best に到達できるはずである。
  エンジンには既に beam モードがある。未解決なのは、*最終的な best を落とさずに
  step ごとの live な約 3% を保持するスコア関数*である。
- **駒数が最初の beam 特徴として自明な候補**（§3）。best cone は step ごとの最多枚数集合と
  一致するため。次の一手として有望なのは、（駒数, 成駒・歩の構成）を鍵にした beam を評価し、
  beam 幅に対する cone の保持率を測ること——つまり、どこまで beam を絞っても
  18 枚（およびそれより深い）の best を復元できるかを調べることである。

## 5. ラベル付き ML データセット（`data/dataset.csv`）

「どの局面が live か」を*学習可能*にするため（40 枚に向けた beam スコア関数のため）、
`run.sh` はラベル付きデータセットを出力する。各行は 1 つの canonical な output-valid
frontier 局面（canonical digest で重複排除済み）。

| 列 | 意味 |
|---|---|
| `step` | 詰みまでの残り手数 |
| `piece_count` | 盤上の駒数（特徴） |
| `live_deeper` | **厳密により深い** step の最多枚数 best の祖先なら 1（cone ベース、厳しい条件で稀） |
| `max_best_depth` | 最多枚数 best の祖先となる最深の step（該当なしなら 0） |
| `best_piece_reachable` | **回帰ターゲット**：そこから到達可能な深部終点の最大駒数（下界。step ごとの best と深部 frontier の部分標本を詰み方向へ辿って算出） |
| `sfen` | 局面（特徴量化は下流で行う） |

行の出所は、step ごとの最多枚数 best すべて（識別に効く母集団）と、frontier からの一様標本
（広い負例）。canonical digest で重複排除。

コミットされている `data/dataset.csv` は **step 49 までの GCP 上の深い実行**によるもの
（厳密・非分割、`--memo-retain-from-step 999` によりメモリが frontier に追随する設定、
n2d-highmem-96 / 768 GB。ローカルの 128 GB では frontier は step 38〜39 付近で OOM する）：

- **282,088 行**。`best_piece_reachable` は **3〜22 枚**に分布（質量は 11〜15 に集中、
  19〜22 に達するのは約 3,650 行）。`live_deeper == 1` は 769 行（厳しい cone ラベル）。
- `best_piece_reachable` の下限はその局面自身の駒数（局面は自明に自分の駒数に到達でき、
  深部の子孫は駒を増やすだけ）であり、辿った深部終点によって引き上げられる。よってこれは
  真の到達値の**下界**であり、実行の深さ（49）によっても頭打ちになる。トレースは部分標本
  （`FMRS_BEST_TRACE_CAP`, `FMRS_TRACE_CAP`）なので、深い `max_best_depth` のラベル自体も
  下界である。

30 枚以上に厳密に到達するのは不可能（frontier は 2 step ごとに約 1.9 倍 → step 58 付近で
10^9）。次の拡張は、厳密探索の深さを超えた先での **beam**（top-K）サンプリングである（§4）。

## 6. beam スコアラの学習（「なぜその局面は live なのか」）

目標は、ある step において frontier の局面を「その子孫がどれだけ深く・高駒数まで届くか」で
順位付けするモデル——40 枚に向けた beam スコア関数である。

パイプライン：`single-king-smoke cone-features --dataset data/dataset.csv -o train.csv`
（局面ごとに `extract_features` を実行）→ `train_cone_model.py` → Rust の beam 用
（`--beam-model`）の `LinearModel` JSON。`extract_features` には作家の直感に相当する特徴を
追加した（玉の liberties / safe flights / flight coverage / escape depth / ray freedom /
net tightness、玉方の mobility、盤面の分散と重心、成駒数、opt-in の `black_check_moves`）。
加えて `step`（局面の位相）。

方法論上の要点：`best_piece_reachable` は**現在の駒数に支配される**（駒数だけで順位付けしても
step 内 Spearman は 0.95）。興味深い信号は **gain = 到達値 − 現在の駒数**であり、
**(step, 駒数) のセル内**で評価すべきものである——「同じ駒数の局面のうち、どれがより深く
伸びるか」。GroupKFold（group = `max_best_depth`。1 つの解経路が 1 つの fold に収まるようにし、
dead 行は自由に分割される）で評価した結果：

| モデル | セル内 Spearman（gain） |
|---|---|
| Ridge（線形） | 0.15 |
| GBDT（HistGradientBoosting） | **0.225** |

つまり有望度は確かに予測可能だが、**弱〜中程度であり、かつ非線形**である。上位の駆動要因
（GBDT の permutation importance）は、まさに人間の直感に対応する特徴である：
`step`（位相）、`total_black_kiki`、`king_flight_cov_avg`、`king_escape_depth`、
`king_ray_freedom`、`king_liberties`、`row_std` などの分散、`king_centroid_cheby`、
`white_mobility`。駒数はセル内では（当然ながら）無関係になる。
`black_check_moves` は単独では有力（2 位）だが `total_black_kiki` と冗長であり、
アンサンブルへの寄与はごく僅かなので既定では無効にしてある
（`FMRS_FEAT_HEAVY=1` で有効化。データセット全体で計算しても約 2 秒）。

### beam による検証——モデルよりも選別規則が効く

beam（`--beam-width` と深い `--max-step`）を実行してスコアラを比較したところ、
2 つのことが分かった。

1. **ラベルの精度がモデルの質を決める。** step ごとの最多枚数の背骨だけでなく、標本全体を
   遡ってトレースすることで「有望」行が倍増し（15.9k → 30.7k、到達値 > 現在の駒数）、
   セル内 GBDT Spearman が **0.225 → 0.322** に向上した。駆動要因は依然として直感系の特徴
   （step、kiki、玉の escape depth / liberties / flight coverage / ray freedom / net、分散）。
2. **top-K 選別は多様性を殺し、random に負ける。** 幅 50,000 において、貪欲な value beam は
   *崩壊*する（高スコア局面は互いに似ており、探索が狭まって step 51・21 枚で行き詰まる）。
   一方、一様・ランダムな beam は多様な筋を保ち、step 71 で 28 枚に到達する。
   局面ごとのスコアラを厳密な top-K として使う限り、どれだけモデルが良くても random には
   勝てない。

解決策は **value × 多様性**：`--beam-temperature T` はスコアを `T·Gumbel` で摂動させてから
top-K を取る——すなわち exp(score/T) に比例した非復元 K 個サンプリング（T=0 で貪欲、
T→∞ で random）。full-trace の到達値モデルを **T=5** で使うと、beam は
**厳密最適に一致する（step 49 で 22 枚。random は 19 枚止まり）**うえ、あらゆる深い step で
より高い駒数を辿り、step 67 で 28 枚に到達する（random は step 71 を要し、貪欲は 21 枚で崩壊）。
つまり、選別が多様性を保ちさえすれば、学習した value モデルは**確かに** random や駒数ヒューリスティクスに勝つ。

今後の課題：T=5 をさらに深く押し進めて 30 枚以上を狙うこと、(T, 幅) の調整、そして
線形↔GBDT の差（セル内 0.20 対 0.32）を GBDT-in-beam や交互作用特徴で埋めること。

## ファイル

- [`FEATURES.md`](FEATURES.md) — `extract_features` の 85 列の定義とカテゴリ分け。
- [`FEATURE_IMPORTANCE.md`](FEATURE_IMPORTANCE.md) — そのうち実際に SOTA モデルを駆動しているのはどれか。
- [`gbdt_structural_importance.py`](gbdt_structural_importance.py) — 上記の寄与度を
  コミット済みのモデル JSON から再生成する（学習データ不要）。
- [`run.sh`](run.sh) — `data/` と上記の表を再生成する。
- [`data/best_step_<S>.txt`](data/) — step S における最多枚数局面の canonical URL。
- [`data/frontier.txt`](data/frontier.txt) — `<step> <frontier_size>`。
- [`fmrs_core/tests/smoke_cone_analysis.rs`](../../fmrs_core/tests/smoke_cone_analysis.rs) — 解析本体。
- データ収集フック：`src/command/single_king_smoke/search.rs` の `FMRS_PERSTEP_BEST_DIR`
  （env で制御、未設定ならコストゼロ）。
