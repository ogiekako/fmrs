# 全駒煙詰(40枚ユニーク協力詰)到達までの技術的軌跡

**達成日:** 2026-06-04
**到達:** 盤上40駒・持駒ゼロのユニーク協力詰(煙詰)を発見・厳密検証。40 は将棋一組(王2・飛2・角2・金4・銀4・桂4・香4・歩18 = 計40)の全駒であり、煙詰の初期盤上駒数として**理論上の最大**。

本書は、エンジン基盤・探索空間の性質・機械学習誘導 beam・分散インフラ・そして最終的に40へ到達した「合流点(gateway)からの深い分岐探索」までを、再現と検証に必要なすべての事実とともに記録する。将来の論文化を見越し、読者が疑問に思いうる点を先回りして明示する。

---

## 0. 用語と問題設定

### 0.1 協力詰(helpmate)であって通常詰将棋ではない

fmrs が解くのは **協力詰(helpmate-tsume)**。攻方・受方の双方が協力して最短で詰みに向かう。通常の詰将棋(攻方=詰ます側 vs 受方=逃げる側)と違い、探索木に **AND/OR(min/max)構造がない**。

**帰結(重要):**
- **df-pn / 証明数探索(proof-number search)は原理的に当たらない。** 証明数探索は「攻方 min・受方 max」の AND/OR 構造を前提にするが、協力詰にはそれがない。一意性検証に pn 系を持ち込まない。
- ここでの「一意性(uniqueness)」= **協力手順がちょうど 1 本** = 局面 DAG 上の **解経路数(solution path count)== 1**。BFS / トポロジカル経路数 DP が自然な枠組み(後述する DP 路線は別途検討され、最終的に非実用と確定している。§7.4)。

### 0.2 煙詰(smoke mate)と「全駒煙」

煙詰は、初期に多数の駒が盤上にあり、協力手順の進行とともに駒が「煙のように」消えていき、最終的に **玉+1駒の 2駒局面**で詰む、というジャンル。

- **本問題 = 煙詰の初期盤上駒数を最大化する。** 盤上40駒(将棋一組全部、持駒ゼロ)で始まる煙詰 = **全駒煙**。40 が上限。
- 発見された40枚局面はすべて持駒表記が `-`(空)= **40駒すべてが盤上**。これが全駒煙の定義条件を満たす証左。

### 0.3 探索の向き・終端・best の定義

- **逆算(backward search):** 詰み(2駒)局面から逆向きに、駒を増やしながら初期方向へたどる。`enumerate_final_2_positions` が終端2駒を列挙、`BackwardSearch` が逆算。
- **step:** mate からの距離(ply 数)。step が大きい局面ほど深い=駒数が多い傾向。
- **best の決定 = (#pieces, steps) の辞書順最大。** 同じ駒数なら**手数(step)が長い**解を優先。`log_global_best_if_improved` は `(piece_count<<32 | step)` を atomic にパックして「改善時のみ」1行ログする。
- **early-exit は既定 OFF。** 理論最大駒数に到達しても探索は止まらない(より長手数の同駒数解を探し続けるため)。停止は `--early-exit` 指定時のみ。
- コマンド: `single-king-smoke ideal-backward`。本問題は双玉・白駒可の特殊 seed を使う(後述)。

---

## 1. 結果サマリ

- **40駒・持駒ゼロのユニーク協力詰(全駒煙)を 2618 局面発見、厳密検証済み。** すべて 113手(plies)。
- 直前の到達点: 38枚(過去の大計算で既知)→ 本系統で **39枚**(本セッション)→ **40枚**(本セッション)。
- 40 は **早期終了ではなく**、beam frontier が自然に枯渇(Completed)した最終端 step113 で出現。
- 発見の鍵は **「全 39解が通る最深の合流点(gateway, step42, 14駒)」から深部に厚い beam を張って探索**したこと。39 の壁は exact 検証で「制約 regime に内在する構造的上限」と分かっており、40 はその壁より**深く分岐する別系列**に在った。

**留保(完全性について):** 40 の発見は **beam(frontier を絞る近似)**による。各局面の一意性は厳密検証されるので **見つかった40は本物**だが、「113 が真の最大手数」「2618 が全ての40」であることは exact では証明されていない(§13)。

---

## 1.5. 発見の時系列(経緯) ― どの順で何が起きたか

本研究は一足跳びに 40 に到達したのではなく、以下の段階を踏んだ。技術的事実(§2 以降)を「いつ・なぜ」その順で導入したかの記録。

1. **単玉の貧乏煙(31枚→3枚)の発見が出発点。** まず単玉(玉1枚)の煙詰を逆算探索し、**金・銀を含まない**単玉煙詰(初形31枚→3枚で詰む)を発見。これを **WFP(Web Fairy Paradise)に出題**した。この時点で「**この逆算は、もっと多くの駒数まで伸ばせるのでは**」という手応えを得た。← 全研究の心理的・技術的な起点。
2. **単玉 → 双玉への転換。** 双玉(玉2枚=`--double-king`)にすると、**より多様な詰め上がり(収束)が生まれる**と見立てた。様々な双玉の詰め上がりから逆算を試す。
3. **収束(詰め上がり位置)の目視選定。** 各収束からの逆算を回し、**ログを目視**して「**この収束は逆算の枝が広い(深く多枚数まで伸びそう)**」と判断。**注力する詰め上がり位置を確定**した。← ここは人間の目によるパターン認識。後の gateway 概念の萌芽。
4. **大規模探索＋人手シード＋コーディングエージェントによる高速化 → 38枚。** ある程度の大規模探索を回し、再びログを見ながら **promising そうな「初期十数手たった局面」を目視で与え(人手シード)**、再度大規模探索。並行して **AIエージェントによるコーディング(agentic coding=自分でコードを書き修正し試す生成AI)で高速化アルゴリズムを実装**したことも寄与。これらで **38枚の煙詰**まで到達。
5. **機械学習の発想の登場。** 30〜38枚の煙詰が **ある程度の多様性を持って存在**していたので、**それをデータセットとして利用**できると気づいた。人間的な特徴量を導入し、「**多枚数の煙詰に至る逆算の"勘"**」を機械学習で学習。
6. **学習モデルを beam に投入 → 39枚。** 学習した scorer で beam を誘導し、**39枚**を達成。
7. **39解の合流点(step=42)の発見 → 40枚。** 39枚解が **すべてある step=42 の局面から派生**していることを確認。**その盤面を開始点として再び beam** を回し、ついに **全駒煙(40枚)を発見**。

注: ここには **2種類の別物の「AI」** が登場する。混同させないこと(講演でも対句で区別する):
- **コードを書くAI(コーディングエージェント / agentic coding)** = 4 の探索エンジン自体の高速化実装を AI と協働で行ったこと。「道具づくり」。
- **勘を学ぶ機械学習** = 5〜7 の探索を誘導する scorer の学習。「勘の移植」。

---

## 2. エンジン基盤

### 2.1 逆算 BFS と一意性検証(2 フェーズ)

`BackwardSearch`(`fmrs_core/src/search/backward.rs`)は層別(step ごと)BFS:

- **Phase 1(候補生成):** 各 frontier 局面の逆向き合法手(undo move)で predecessor を生成、smoke 制約でフィルタ。raw-digest で完全同一局面のみ dedup。
- **Phase 2(一意性検証 = 支配コスト):** 各候補について「mate への協力解がちょうど 1 本か」を `solutions_overlay`(深さ制限 DFS + memo)で判定。memo は `StepRange`(距離+一意性レンジ)を持つ transposition cache。

**核心事実:** 一意性は memo には載っていない。memo は cache に過ぎず、miss 時は full 計算する。これが後述の checkpoint-resume / split / gateway-seed がすべて exact である根拠(cold memo で再開しても結果不変)。

### 2.2 Phase 2 DFS は near-oracle(高速化の天井)

詳細計測(max-step 31, 深部 step)で:
- **nocut ≈ 97.5%:** ユニーク局面の全幅検証(2本目の解が無いことの証明 = 全手確認が原理的に必要)。**順序/枝刈り/oracle の余地ゼロ = irreducible。**
- pass2_cut ≈ 2.5%(非ユニーク反証, 平均展開数 1.15 ≈ oracle 1.0)、pass1_cut ≈ 0.1%(memo のみ)。

→ **move ordering は既に near-oracle**(killer/history が反証子をほぼ最初に選ぶ)。perf profile でも ~74% が intrinsic な per-node コストで、ordering 自体も floor(K/H を減らすと全部 CPU 増)。**「DFS 効率」では速くならない。高速化は「何を探すか(探索空間を狭める制約・admissible 上界)」側。** これが本問題が**速度でなく探索誘導の問題**である理由。

補足(並列の代償): Phase 2 は候補を並列処理し worker は thread-local memo を持つため、同一 step 内で複数候補が同じ深い局面に達すると互いを知らず再検証(within-step redo, nocut の ~2/3〜3/4)。step-scope の sharded concurrent memo(`SharedDfs`, commit 35463b5)で共有して削減済み。

### 2.3 frontier 正規化(`--canonicalize-attacker-goldish`)の不変条件

攻方 goldish(金と同働きの成駒)同値を畳む正規化。**設計不変条件(誤ると探索空間崩壊):**
- frontier には**元の局面(全 distinct)を残す。** Phase-1 dedup は「完全同一」のみ畳む。
- canonical 化は **uniqueness 判定(memo lookup + solutions_overlay)だけ**に使う。
- やってはいけない: Phase-1 の dedup/shard キーを canonical digest にすること(盤面として別物の distinct 局面が脱落し崩壊。実測 regression あり)。
- V 重複削減は wave スコープの sharded canonical キャッシュで行い、frontier は raw-digest dedup のまま(出力 byte 一致)。

### 2.4 メモリの lever(OOM 安全性が必須制約)

煙詰の探索空間は実質無限に膨らみうる。インスタンスの RAM は可変。**「メモリを OS に返さない」系の最適化は OOM するため禁止**(圧力時にカーネルが回収できることが必須)。
- `--max-memo-entries`(既定 auto = mem/cores): memo の主 lever。
- `--memo-retain-from-step N`(既定10): 閾値未満は memo を毎 step discard(memo は cache なので exact、hit 率が下がるだけ)。
- mmap pool は `MADV_DONTNEED`(即時物理解放+ゼロフィル保証)を採用(memset 6.61% を削除、累計 8.0s→5.0s)。

---

## 3. 探索空間の壁:なぜ exact 全幅では届かないか

逆算 frontier は **約 1.7〜1.9×/2step** で増える。
- 実測: 標準 seed で step37 に約 406万局面。
- 40枚は概ね step≈2×(40-3)≈74 必要 → frontier は 10^10〜10^11 規模。
- exact 全幅は 128GB で step38-39 OOM、768GB でも step49(最大22枚)あたりが限界。

**結論:** 40 は exact 全幅では到達不能。**速度でなく「探索誘導(どの frontier 局面を残すか)」の問題**。ここから機械学習誘導 beam の開発に進む。

---

## 4. cone 分析:探索構造の解明

「各 step の最大駒数 best 局面の一意解を mate 方向へたどった経路 = best cone」を全幅 frontier と比較(canonical dedup で揃える)。

主知見(標準 5min seed, max-step 37):
- **best cone は frontier の 0.002〜0.1% の極薄スライバ。** 中盤で 1〜2 の canonical 局面に強く合流(strong convergence)。
- **live 率**(step s frontier のうち、より深い step で max-piece best になる子孫を持つ割合): step11 で **2.6%**、~95% は dead。深いほど低下。ただし descriptive であり pruning rule ではない(どの 3% が live かは結果論、dead も一意性証明には必要)。
- **駒数は best cone 上で mate へ単調**、best cone は各 step の最大駒数集合と一致 → **駒数は強い guidance signal**(高駒数を優先する beam が live cone をたどる)。

→ 「高 live な薄いスライバを落とさず保つ scoring 関数」を学習できれば beam で深部に到達できる、という戦略が立つ。

---

## 5. 機械学習誘導 beam の構築

### 5.1 特徴量(作家の直感を数値化)

詰将棋作家の「行けそう/行けなさそう感」を `smoke_features.rs` の ~85 特徴量に:
- **玉の自由/希望:** king_liberties, safe_flights, flight_cov_avg, escape_depth, ray_freedom, net_frac, white_mobility ほか。
- **図の広がり/配置(dispersion):** bbox_area, board_density, occupied_files/ranks, row_std/col_std, centroid, promoted_total。
- **phase:** step 自体(前半/後半で評価が変わる)。
- opt-in heavy: black_check_moves(王手生成数)— kiki と冗長で寄与僅少、既定 off。

(性能注: `FMRS_FEAT_HEAVY` の env 読みは per-局面で呼ぶと env-lock 競合で並列スコアリングが直列化し ~17% 低速化する。`OnceLock` 化で解消済み。black_kiki の二重計算も排除。)

### 5.2 データセットとラベル(ここが品質の律速だった)

- 行 = canonical な output-valid frontier 局面。列に step, piece_count, live_deeper, max_best_depth, **best_piece_reachable**(回帰ターゲット), sfen。
- 当初ラベル = **トレース(サンプル経路)による下界**。GBDT の within-cell(同 step×駒数セル内)Spearman ≈ 0.225。
- full-trace(深部 frontier を大量後方トレース)で promise rows 15.9k→30.7k、0.322 に改善。
- **direction 1(LambdaRank):** cell-group の per-cell 0.32 = 回帰と同等(超えず)→ **within-cell 天井 ~0.32 は目的関数でなく特徴量/ラベルの情報量律速**と判明。
- **direction 2(厳密 value-DP ラベル, 決定打):** 探索の親子リンク(`cand.frontier_idx`)から親→子辺を `FMRS_EDGE_FILE` で記録(gated, off時ゼロコスト)、後方 DAG 上で `max_reachable_pieces` を fixpoint DP で**厳密**算出(不変条件 value≥piece, parent≥child, leaf==piece を検証)。1570万辺・762万局面で GBDT 再学習し **per-cell Spearman 0.264 → 0.494(+87%)= 0.32 天井を突破。** これが現行 SOTA モデル(`models/cone_dp_gbdt.json`、`--beam-sota` に埋め込み)。

GroupKFold(group = 解経路の max_best_depth)で経路漏洩を防いで評価。

### 5.3 選別ルール:value×多様性が真のレバー

重要な発見: **モデル品質を上げても top-K greedy 選別は beam では random に負ける**(高スコア局面が似通って崩壊 or 低駒数偏り)。選別ルールこそ本質。

- **温度サンプリング `--beam-temperature T`:** Gumbel-top-K トリック(score + T·Gumbel の top-K = exp(score/T) 比例の非復元サンプリング)。T=0 で greedy、T 大で random に漸近。**T が決定的:** T=5 で greedy 崩壊、T=40 で random 寄り、**T=15 が最適**。width100k で random 30 vs **GBDT T=15 = 35**。
- **stratified 選別 `--beam-stratify`:** scored pool を駒数 bucket で round-robin、高駒数フロントが低駒数 line を crowd out するのを防ぐ(温度より構造的)。
- **GBDT-in-beam:** sklearn HistGradientBoosting を Rust に移植(`GbdtModel`, 葉 value 合計+baseline、sklearn predict と 6e-14 一致を検証)。`--beam-model` が `"trees"` を含めば GBDT、`"weights"` なら線形を自動判別。線形 per-cell 0.20 → GBDT 0.32 の非線形分を回収。
- **scorer 無効化バグ(歴史的教訓):** `--beam-width` が candidates_limit(digest 下位 K 切詰)も設定し、apply_beam が no-op になっていた時期があった。scorer 有効時は pool = width×BEAM_SCORE_POOL(=16)に広げて score 選別するよう修正。

### 5.4 幾何的 width ランプと決定的 RNG

- **`--beam-width-at STEP:WIDTH` + `--beam-width-max`:** width を step に対し**幾何補間**(frontier が幾何的に増えるので kept-fraction の減衰を制御できる)。アンカーを超えても幾何的に増え、max で頭打ち。
- **決定的 per-局面 RNG:** Gumbel の乱数を `SmallRng::seed_from_u64(digest ⊕ step ⊕ seed)` で局面ごとに seed。→ 再現可能(`--random-seed` 固定で bit 一致)・並列安全(共有 RNG なし)・`from_entropy` の OS エントロピー読みを除去するので速度犠牲なし。**checkpoint/resume を bit 一致で継続する土台。**

### 5.5 ML beam の到達点(40 直前まで)

- ML 誘導 beam(GBDT+温度+stratify)が **random 30 → 37枚**(+7枚)に到達。全幅 38枚を遥かに小資源で近似 = 効率の勝利。
- ただし**単一 seed・production 系 44 regime では ~36-37 が robust ceiling**(温度・width・pool・min-pawn-pct・stratify を掃いても 37 分散支配で頭打ち、iterative 再学習はむしろ過適合で悪化)。geometric ramp 単体も 38 への lever ではなかった(fixed=36 vs ramp 28/35)。

> ここまでで「ML beam machinery は完成・検証済(最大の再利用資産)」だが、**単純な beam チューニングだけでは 38→40 は出ない**ことも判明していた。突破は次章以降の**構造解析**から来る。

---

## 6. 分散インフラと運用堅牢性

40 は 768GB 級マシンで時間をかけて回す必要がある。そのための運用基盤を本セッションで整備した。

### 6.1 GCP spot fleet

`scripts/gcp-spot.sh`(`up/push/run-bg/tail/status/stop`、ID で fmrs-spot-N を指定)。
- fmrs-spot, fmrs-spot-2(本番, m3-ultramem 系), **fmrs-spot-3(実験, n2d-highmem-96 = 96 vCPU / 768GB, asia-northeast1-c)**。
- `run-bg` は tmux session "job" でバックグラウンド実行(ログ `/tmp/fmrs-job.log`)。spot は preempt されうる。

### 6.2 beam checkpoint / auto-resume(本セッション実装)

spot preempt や OOM kill からの救済が必要。beam run を**同一 config なら無フラグで自動再開**(forget-proof)にした。
- `BeamConfig::checkpoint_key()` = width/anchor/temperature/stratify/rng_seed/scorer 指紋の安定 hash。
- checkpoint ファイル名に `_beam{key}` segment を付与。**exact run は beam_key=None で path 完全不変**(回帰テスト済)。exact ckpt・beam ckpt・異なる beam config 同士は完全 isolation。
- 書き込み gate を緩和し beam も checkpoint を書く。load は beam key 優先、無ければ **exact/split prefix の checkpoint を warm-start として fallback**。
- `--fresh` で全 checkpoint を無視。
- 決定的 RNG(§5.4)のおかげで再開後も bit 一致で継続。

### 6.3 split モード(exact かつメモリ有界)

`--split-start-step / --split-chunk-size / --split-seed`。prefix BFS を split step まで exact に走らせ、frontier F を 88byte 表現で sort(並列順序の非決定性を消す)→ shuffle → chunk 分割し、各 chunk を cold memo で順に exact 探索して best を merge。
- **なぜ exact か:** 一意性は memo に載らず(§2.1)、cold memo 再構築は checkpoint resume と同種。partition の逆算展開 = 各 chunk 展開の和なので取りこぼし無し。重複は merge 時 digest dedup。
- 本セッションで **split + beam を compatible 化**(旧来は排他)。両指定時は「exact prefix(step < split_start_step)→ beam(chunking なし)」モード(`BeamConfig.activate_step`)。これにより **OOM kill された split job を width 制限付きで救済**できる(3TB OOM 事例が発端)。

---

## 7. 38 → 39:gateway-seed レシピの確立

### 7.1 39 到達コマンド

```
cargo r -r single-king-smoke ideal-backward --parallel 32 --allow-white-pieces \
  --slack 100 --double-king \
  --seed-sfen '8k/6K+P1/9/9/9/9/9/9/9 w 2r2b4g4s4n4l17p 1' \
  --canonicalize-attacker-goldish --min-pawn-pct 44 \
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority \
  --lance-knight-allow-start 8 --lance-knight-allow-step 2 \
  --beam-width 200000 --beam-sota --beam-width-at 75:2000000 --beam-width-max 5000000
```

- **seed** は煙詰の終端(盤上 k, K, +P の 2-3駒、白持駒が将棋一組の残り全部)。逆算で持駒が盤に出て 40駒へ向かう。
- **44 regime の制約**(意図的に選択された問題クラス): min-pawn-pct 44(歩比率の下限)、rook-bishop / lance-knight の許容枚数スケジュール、goldish-priority。

### 7.2 制約は step ではなく「局面内在の駒数」で効く(重要)

`rook-bishop-allow-start` 等は探索 step ではなく `pieces_in_play`(= 盤上駒数 + 黒持駒)で許容枚数を決める(`satisfies_family_allowance`)。min-pawn-pct も after-step=0 なら局面内在。→ **後述の gateway-seed でも同じフラグがそのまま正しく効く**(step オフセットの心配が要らない)。

### 7.3 39解の構造:単一 spine + 浅い crown(合流解析)

到達した **438個のユニーク39解**をトレースし、mate からの距離 d ごとに canonical 局面数を数えた(`fmrs_core/tests/smoke_cone_analysis.rs` の `smoke_confluence`、`FMRS_CONFLUENCE_SFENS` に SFEN/URL 列のファイルを渡す):

- 全 438解は**ちょうど 107手**(min=max)。
- **d=0〜82 は distinct=1**(mate 側 83手は完全に単一の forced spine、全解同一)。
- **分岐は root 側の top ~25手(d≥83)のみ**: d83→3, d85→6, d91→8, d95→16, d104→136, d107(root)→438。
- = 39の多様性は **1本の 82手 spine の上に乗った浅い "crown"**(cosmetic な top variation)。
- **全解が通る唯一の funnel(gateway, d=82, 27駒, 白番):**
  `8G/6K1p/7S1/3k+P1P+pn/4PP1pN/8P/7N+P/3P2NP+P/G+pL1LL1S+p w 2P2r2b2g2sl2p 1`

これが「39 到達コマンドの `--beam-width-at 75:2000000` が効いた理由」を説明する: width ramp が step75(=分岐開始 d83 の直前)で立ち上がり、ちょうど crown に広い beam を与えていた。

### 7.4 gateway-seed:探索を合流点の cone に集中させる手法

**backward search の step は seed の解長から始まる**(`new_with_parallel`: `step = solution.len()`)。gateway を `--seed-sfen` に渡すと step=82 から開始し、その上の crown だけを探索する。
- `--max-step` は seed 解長より大きく設定すること(小さいと即終了する。当初これで踏んだ)。
- 制約は局面内在(§7.2)なので同フラグでそのまま正しい。beam ramp も step が 82 から始まるので最初から width 最大付近。
- **本質的利得:** 元コマンドは step0〜82 の frontier(gateway 以外の dead 枝を含む)にも beam 予算を食われるが、gateway-seed は **beam 予算100%を gateway の cone に集中**できる。

**そして決定的に重要だったのは、これが高速な exact オラクルにもなること:** 27駒 gateway/neck から **exact 全幅探索**すると、~25手の cone を数秒〜数十秒で**完全検証**できる(crown は frontier max 24万程度で tractable)。これで「制約 Y のもとで gateway X の上に N 駒が在るか」を確定的に判定できる。

### 7.5 39 は構造的天井である(exact 証明)

gateway-seed exact オラクルで、複数の独立系列を検証:
- 元 gateway(全438の真の合流, d=82)cone → **exact 全幅で 39@107 で自然終了**(40 は存在しないと exact 証明)。
- ユーザ提供の別系列2本(new2 root `8G/6K1p/3+S4G/...`、new3 root `8G/6K1p/3+P3S1/3G+p...`)は、合流解析で **mate 側 ~43手だけ共通 spine を共有し d≈43 で分岐する genuinely 別 lineage**(440個合流で d43→2系列, d80→3系列)。だが両者の ~27駒 neck から **exact 全幅で、やはり 39 で自然終了**。

→ **独立3系列がすべて exact で 39 天井 = 39 の壁は lineage 固有でなく、min-pawn-pct 44 + 現許容の regime に内在する構造的上限。** 同 regime 内で別の39を探し続けても 40 は出ない公算が大。

ここで戦略判断が分岐した。選択肢は (A) 制約 regime を緩和する(問題クラスが変わる)か、(B) 44 regime のまま **もっと深く分岐する 40** を探すか。**(B) を選択。**

---

## 8. 39 → 40:合流点(d=42 gateway)からの深い分岐探索

### 8.1 着想

39の3系列は mate 側 d≤42 でのみ共通(その先で分岐)。**全系列が通る最深の共通点 = d=42 gateway(14駒, 白番):**
```
8G/6K1p/9/3+P2P+p+S/4PP1pk/9/8P/6NP1/9 w N2r2b3g3s2n4l9p 1
```
ここから逆算すれば **d≥42 の全系列の深い分岐すべてを、beam 予算を spine に食われずカバー**できる。前回(focused neck, d~78開始)が取りこぼした「d<78 で分岐する 40」を拾える可能性。

### 8.2 投入(GCP spot-3, 96 vCPU / 768GB)

```
cargo r -r single-king-smoke ideal-backward --parallel 96 --allow-white-pieces \
  --slack 100 --double-king \
  --seed-sfen '8G/6K1p/9/3+P2P+p+S/4PP1pk/9/8P/6NP1/9 w N2r2b3g3s2n4l9p 1' \
  --canonicalize-attacker-goldish --min-pawn-pct 44 \
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority \
  --lance-knight-allow-start 8 --lance-knight-allow-step 2 \
  --beam-width 1000000 --beam-sota --beam-width-at 75:3000000 --beam-width-max 10000000 \
  --seed-result-log /tmp/deep42/log.jsonl
```

- width を前回より深部に厚く配分(base 1M、step75 で 3M、max 10M)。
- preempt 耐性は §6.2 の beam checkpoint で担保。
- メモリは peak 68GB / 755GB(余裕)、memo は ~9億エントリまで成長。

### 8.3 結果:40 到達

step ごとの最大駒数の進行(各 step 1行のログ、`global_best_pieces=N steps=S positions=M 代表URL`):

| step | 最大駒数 | その駒数の局面数 |
|---|---|---|
| 103 | 39 | — |
| 105 | 39 | 334 |
| 107 | 39 | 1664 |
| 109 | 39 | 6653 |
| 111 | 39 | 4801 |
| **113** | **40** | **2618** |

- **40 は step113 で初出**、そこで frontier が枯れて Completed(`DONE exit=0`、`best_pieces=40 best_steps=113: positions=2618`)。
- frontier は crown で単調収縮(step107:223k → 109:106k → 111:55k → 113:26k → 115で消滅)。
- **`--early-exit` は付けていない** → 「40達成で打ち切り」ではなく、**走りきって到達した最終端の40を全出力**。
- step111 の最大は 39(40ではない)→ **40 はこの探索では step113 にのみ存在**(2618個以外の別手数40はこの run には無かった)。

40 が step113(=39 の step107 より深い)で出たことは、**40 が「39 より深く分岐する別系列」に在る**という (B) の仮説の的中を意味する。focused neck 探索(d~78開始)はこの深い分岐を取りこぼしていた。

---

## 9. 検証

- 取得した 2618 局面はすべて **盤上40駒・持駒ゼロ**(`b - 1`)。
- スポット検証(1番目/1300番目/2618番目): いずれも 40駒・**unique 解**(`standard_solve` が解1本、`smoke_dump_path` の `sols==1` assert 通過)・**113手**。
- エンジン側も `--canonicalize-attacker-goldish` 経由で `finalize_seed_best` が各 best を `standard_solve` 再検証し、一意解のもののみ保持(canonical false positive を除去済)。2618 は canonical-distinct。
- **beam は frontier を絞るだけで、各局面の一意性は厳密検証される。** よって発見された40は本物のユニーク協力詰。

成果物:
- 40局面 全2618件: `40piece_solutions.txt`(repo 直下, URL 形式, sort 済)。
- 代表(検証済):
  ```
  8G/6K1p/3+S4+S/1B1+Pp+PP+p+n/3+pPP1pL/3Bkg+l1P/1RRg+l+p1NG/2+SPNpNP+P/+P1L4S+p b - 1
  8G/6K1p/4S3+S/3Gp+PP+p+n/2B+pPP1p1/3Bkg+p1P/RR1s+p+lLN+P/2+PPN+lNPG/+P+pL4S1 b - 1
  ```

---

## 10. 何が効いたか(寄与の総括)

40 到達は単一の工夫ではなく、以下の積み重ねの帰結:

1. **問題の正しい framing:** 速度ではなく探索誘導の問題と見抜いた(cone 分析)。協力詰なので df-pn 不適、一意性=経路数==1。
2. **ML 誘導 beam machinery:** 作家直感の特徴量 + 厳密 value-DP ラベル(per-cell 0.49)+ 温度(Gumbel-top-K)+ stratify + GBDT-in-beam。random 30 → 37。
3. **構造解析(decisive):** 合流(confluence)解析で「39解 = 単一 spine + 浅い crown」を発見。gateway という概念。
4. **gateway-seed = 高速 exact オラクル:** 「39 の壁は regime 内在の構造的上限」を exact 証明。これにより「別の39を探す」という無駄を排し、「**より深く分岐する 40**」へ方針を定めた。
5. **深部合流点(d=42 gateway)からの集中探索:** beam 予算を全系列の深い分岐に集中。これが 40 を拾った。
6. **運用基盤:** GCP spot 768GB + beam checkpoint/resume + split で、長時間・preempt 耐性のある探索を可能に。

---

## 11. 再現手順(まとめ)

- **環境:** GCP fmrs-spot-3(n2d-highmem-96, 96 vCPU / 768GB, asia-northeast1-c)。`scripts/gcp-spot.sh push 3` → `run-bg 3 '<コマンド>'`。ローカル検証は 8〜16 コアで数十秒〜数分。
- **40 探索コマンド:** §8.2。
- **gateway/neck の取得:** `FMRS_CONFLUENCE_SFENS=<SFEN列> cargo t -p fmrs_core smoke_confluence`(合流点 = distinct が 1 になる最深 d の局面)。`FMRS_DUMP_PATH_SFEN=<SFEN> cargo t -p fmrs_core smoke_dump_path`(解パスを d・駒数・SFEN で出力)。
- **exact オラクル(40 が在るかの確定判定):** 27駒 gateway を `--seed-sfen` に、beam なし、`--max-step` を seed 解長より大きく。数秒〜数十秒で Completed。
- **モデル:** `--beam-sota`(埋め込み SOTA = `models/cone_dp_gbdt.json`, per-cell 0.49)。温度は SOTA 既定値。

---

## 12. オープンな問い・今後

- **41 はあるか:** 40 が将棋一組全部なので 41 は不可能(理論最大)。**40 が真の上限。** よって残る問いは「40 を**何手で**、**何通り**作れるか」。
- **真の最大手数:** 本 run は 113手の40を出した。だがこれは **beam が到達した最深 step**。中盤(step83-95, frontier ~7M)で beam が枝刈りした系列に、より深い step(115+)の40が在る可能性は exact では排除できていない。
- **全40の完全列挙(1つ残らず):** 現状の 2618 は step113 の40のみ(engine が (#pieces,steps) 辞書順最大しか保持しないため、もし別手数の40があれば破棄されている。ただし本 run では step111 の最大が 39 だったので、この探索が見た40は全部 113)。**真に網羅するには exact が必要**(d=42 gateway から exact 全幅 or split exact)。crown 頂上の frontier は小さい(step113 で 2.6万)が、中盤 frontier が exact では巨大になるため、split(exact・メモリ有界)が適切。これは未実施。
- **制約 regime の役割:** 40 は min-pawn-pct 44 + 現許容で発見。別 regime(緩和/別構造)での40の存在・多寡は未探索。
- **経路数 DP は不適:** 一意性は無制約 forward グラフで定義され、DP は「smoke 非フィルタの無制約 backward 到達集合」の materialize を要し、実 seed で約×10/層の指数爆発(layer7=4450万、OOM 寸前)。**DFS(深さ制限+cutoff+制約付候補の遅延探索)が本問題に本質的に適切**で、今後 smoke 高速化で経路数 DP を再提案しない(独立 shadow 検証器・dp_pair 等の検証資産は env-gated で保持)。

---

## 13. 留保と正確性に関する注記(論文化の際の必読)

- **「40 = ユニーク協力詰」は厳密に正しい**(各局面 `standard_solve` 検証, solutions==1)。煙詰条件(盤上40・持駒0)も満たす。
- **「2618 が全ての40」「113 が真の最大手数」は exact 証明されていない**(beam による近似探索の到達範囲)。論文では「beam 探索で発見・検証した40の集合」と表現すべきで、「全40」と断定しない(網羅性を主張するなら exact run が要る)。
- best の (#pieces, steps) 辞書順最大ポリシーにより、**より短手数の40が在ったとしても保持・出力されない**(この run では存在しなかったことを step111=39 から確認済だが、一般には注意)。
- 数値・局面はすべて 2026-06-04 の run(`/tmp/fmrs-job.log` on fmrs-spot-3)に基づく。

---

## 14. 「美しい全駒煙」の選定と最終形

40駒の解は2618局面ある。発表用に「最も整った全駒煙」を1つ選ぶため、煙詰の作品性に効く指標で辞書順ランク付けした(すべて最小化)。ツールは `fmrs_core/tests/smoke_cone_analysis.rs` の `smoke_beautiful`(第1段)・`smoke_beautiful2`(第2段)、env でSFEN列を渡す。

### 14.1 第1段ランク(`smoke_beautiful`):2618 → 72

1. **初手が駒取りでない**ほどよい。
2. 盤上の **成銀・成桂・成香**が少ないほどよい(まず現れる種類数を最小化、次に合計枚数)。
3. 盤上の **成角・成飛**が少ないほどよい(同上)。

判明した構造的事実:
- **2618 全局面が初手で駒を取る**(全駒盤上・黒番では初手から捕獲が不可避)→ 基準1は区別せず。
- **成銀・成桂・成香の3種は全局面で必ず1枚以上現れる**(最小種類数=3)→ 基準2の種類数も改善不可。
- 実質の決め手は「成S/N/L が各1枚ずつ(合計3=最小)」かつ「成角・成飛がゼロ」。これを満たす **72局面**に絞られる。

### 14.2 第2段タイブレーク(`smoke_beautiful2`):72 → 3

1. **成銀・成桂・成香が敵陣外にあるもの**の数(黒=1–3段/白=7–9段の自陣側=敵陣で成るのは自然=ペナルティ0、敵陣外に居るものを数えて最小化)。
2. **総成駒数(と金を含む)**を最小化。
3. **初手がと金以外**だとよい(初手がと金の捕獲ならペナルティ)。
4. **相手玉(=被詰の白玉)への全駒マンハッタン距離の総和**を最小化(駒が玉に密集しているほどよい)。

72局面内の挙動:
- 総成駒数は **全72で12**(区別せず)。初手は **全72がと金の捕獲**(区別せず)。
- 効いたのは「成L/N/S の敵陣外個数(最小=1、0は存在しない)」と、その上での「玉への距離和(最小=153)」。
- 結果 **3局面**でタイ(これ以上のタイブレークは規定せず=全出力):
  ```
  8G/6K1p/4S3+S/1B1Gp+PP+p+n/3+pPP1p1/3Bk+p+p1P/1RRs+lgLN+P/2+PPNlNPG/+P+pL4S1 b - 1
  8G/6K1p/4S3+S/1B1Gp+PP+p+n/3+pPP1p1/3Bkg+p1P/1RRs+l+pLN+P/2+PPNlNPG/+P+pL4S1 b - 1
  8G/6K1p/4S3+S/1B1Gp+PP+p+n/3+pPP1p1/3Bkg+p1P/1RRs+p+lLN+P/2+PPNlNPG/+P+pL4S1 b - 1
  ```

### 14.3 手動仕上げによる最終形

上位3局面に対し、**攻方の(不成)角を1マスだけ被詰玉に近づける手動修正**を施したところ、ユニーク性・40駒・113手を保ったまま、玉へのマンハッタン距離和が 153 → **151** に下がり、第2段基準でさらに良い最終形が得られた:

```
8G/6K1p/4S3+S/3Gp+PP+p+n/2B+pPP1p1/3Bk+p+p1P/1RRs+lgLN+P/2+PPNlNPG/+P+pL4S1 b - 1
```
(URL: https://ogiekako.github.io/fmrs/8G/6K1p/4S3+S/3Gp+PP+p+n/2B+pPP1p1/3Bk+p+p1P/1RRs+lgLN+P/2+PPNlNPG/+P+pL4S1_b_-_1 / `standard_solve` で一意・113手を確認済み)

**この局面は beam が見つけた2618集合には含まれていなかった**(角の配置違いは canonicalize-attacker-goldish の対象外なので別物として取りこぼされていた)。第1段キーは上位3局面と同一([1,3,3,0,0])で72に入る資格を持ち、第2段の距離和が 151 と上回る。これは §13 の留保(2618は非網羅)の具体例であり、同時に「ランク付けで上位を出し、最後に作家が1マス調整して仕上げる」という実務的フローで最終形に到達したことを示す。網羅的な真の最適は exact 列挙を要するが、本研究の目的(全駒煙の発見)は達成済みのため、ここで一段落とする。

成果物(repo 直下): 発見された全2618局面=`40piece_solutions.txt`、ランク上位3=`40piece_beautiful_final.txt`。本質169類・美しさ72局面の各リストは中間生成物のため非保存だが、上記テスト(`smoke_essential_classes` / `smoke_beautiful` / `smoke_beautiful2`)に `40piece_solutions.txt` を渡せば再生成できる。

---

## 付録 A:鍵となる局面(SFEN)

- **40 例(検証済, 全2618は `40piece_solutions.txt`):**
  `8G/6K1p/3+S4+S/1B1+Pp+PP+p+n/3+pPP1pL/3Bkg+l1P/1RRg+l+p1NG/2+SPNpNP+P/+P1L4S+p b - 1`
- **39 真の合流 gateway(d=82, 27駒):**
  `8G/6K1p/7S1/3k+P1P+pn/4PP1pN/8P/7N+P/3P2NP+P/G+pL1LL1S+p w 2P2r2b2g2sl2p 1`
- **全系列の最深共通点 = 40 探索の seed(d=42, 14駒):**
  `8G/6K1p/9/3+P2P+p+S/4PP1pk/9/8P/6NP1/9 w N2r2b3g3s2n4l9p 1`
- **煙詰の終端 seed(39/元コマンドの seed):**
  `8k/6K+P1/9/9/9/9/9/9/9 w 2r2b4g4s4n4l17p 1`
- **39 の3系列の root(別 lineage の例):**
  `8G/6K1p/3+S4G/3G+p+PP+pn/3pPP1pN/4l1B1S/3L1B1NP/+R+R+l+P+PSNPG/+P+pL+p+pk1S1 b p 1`（new2）
  `8G/6K1p/3+P3S1/3G+p+PP+pn/3+pPP1pN/6B1S/3L1+B1N+P/+R+R+lG+PSNP+p/G+pL+p+lk1SP b p 1`（new3）

## 付録 B:主要コード位置

- 逆算・一意性: `fmrs_core/src/search/backward.rs`(`new_with_parallel` の `step=solution.len()` が gateway-seed の鍵)。
- 探索ループ・beam 適用・per-step dump hook: `src/command/single_king_smoke/search.rs`。
- beam 設定(checkpoint_key, width_at, activate_step, scorer): `src/command/single_king_smoke/beam.rs`。
- 特徴量・GBDT/Linear モデル: `src/command/smoke_features.rs`。
- 制約(局面内在の許容枚数): `src/command/smoke_constraints.rs`。
- checkpoint/split 永続化: `src/command/smoke_persistence.rs`。
- 合流/パス解析テスト: `fmrs_core/tests/smoke_cone_analysis.rs`(`smoke_confluence`, `smoke_dump_path`)。
- 分析パイプライン: `analysis/smoke_cone/`(REPORT.md, run.sh, train_*.py, export_gbdt.py, edge_value_dp.py)。
- SOTA モデル: `models/cone_dp_gbdt.json`(per-cell 0.49, `--beam-sota` に埋め込み)。

## 付録 C:CLI フラグ早見(本問題で使うもの)

| フラグ | 役割 |
|---|---|
| `--seed-sfen` | 単一 seed(終端 or gateway)を指定。step は解長から開始 |
| `--double-king` / `--allow-white-pieces` | 双玉・白駒可(本問題の枠組み) |
| `--canonicalize-attacker-goldish` | 攻方 goldish 同値で一意性判定を正規化(frontier は distinct 維持) |
| `--min-pawn-pct` | 歩比率の下限(regime 定義) |
| `--rook-bishop-allow-start/-step`, `--lance-knight-allow-start/-step` | 駒数(局面内在)に応じた飛角・香桂の許容枚数スケジュール |
| `--goldish-priority` | goldish 優先 |
| `--beam-width`, `--beam-width-at STEP:WIDTH`, `--beam-width-max` | beam 幅と幾何的ランプ |
| `--beam-sota` / `--beam-model` | 埋め込み SOTA GBDT / 任意モデル |
| `--beam-temperature`, `--beam-stratify` | 選別の多様性(Gumbel-top-K / 駒数 bucket) |
| `--split-start-step/-chunk-size/-seed` | exact・メモリ有界の split(beam 併用で「exact prefix→beam」) |
| `--max-memo-entries`, `--memo-retain-from-step` | memo メモリ lever(cache なので exact 不変) |
| `--fresh` | 既存 checkpoint を無視して最初から |
| `--early-exit` | 最大駒数到達で全体停止(既定 OFF) |
| `--random-seed` | 決定的 RNG の seed(bit 一致再現) |
