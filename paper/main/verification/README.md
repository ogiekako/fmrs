# 「狼煙」および保存候補の再検証

2026-10-09に実行した検証を再現するためのファイルです。大規模な逆算による発見探索を再実行するものではありません。

## 結果

- 発表形「狼煙」: 独立した2系統で最短113手・解数1を確認。
- 公開fmrsの保存候補2,618局: 全件最短113手・解数1。標準40枚の初形、詰上がり3枚。
- 3つの手順射影による分類: 169類。
- 300木・85特徴の保存GBDTの構造的特徴指標: 原資料の表と一致。

「解数1」は最短手順に対するものです。より長い迂回手順の全否定や、候補生成の完全性を意味しません。盤上3枚への減少後、残り37枚は受方の持駒です。

## 構成と由来

- `wasm/index_bg.wasm`, `wasm/index_bg.js`: fmrsの公式公開成果物。固定版とハッシュは `verification_provenance.json`。
- `wasm/solve.mjs`: 発表形をfmrsで求解。解数上限を2にして、一意性が検査できるようにしています。
- `wasm/verify_set.mjs`: 保存2,618局を全て求解。
- `independent_verification.py`: python-shogi 1.1.1による別系統の合法手生成を用いた再生と幅優先探索。
- `classify_set.py`: 保存候補の手順を再構成し、駒種内訳と分類を検査。
- `noroshi_solution.kif`, `noroshi_solution.usi.txt`: 発表形の全113手。
- `noroshi_independent_replay.json`: 114局面すべてのSFEN、盤上枚数、持駒。
- `all_2618_verification.jsonl`: 各保存候補の求解結果と手順。
- `sources/`: 当該検証に必要なリポジトリ上のデータとモデル。

## 必要な環境

Python 3.10以降、Node.js（WebAssembly対応版）、python-shogi 1.1.1。Python依存は同ディレクトリに限定してインストールできます。

```sh
python -m pip install --target python_deps python-shogi==1.1.1
```

python-shogiは https://github.com/gunyarakun/python-shogi の別実装で、GPL-3.0-onlyライセンスです。依存ライブラリ自体はこの検証一式には同梱していません。fmrsはMITライセンスで、同梱ライセンスを参照してください。

## 再実行

作業ディレクトリをこのREADMEのある場所にします。
再実行コマンドは同梱の結果ファイルを上書きします。保存結果をそのまま保つ場合はコピーで実行してください。
実行時間の値は環境ごとに変わるため、再実行後はチェックサムも変わることがあります。

```sh
# 公開fmrsによる発表形の最短手順・解数
(cd wasm && node solve.mjs) > noroshi_fmrs_verification.json

# 別実装による全手再生と、全局面をキーとした最短経路数のBFS
python independent_verification.py --bfs

# 保存された2,618候補を全件fmrsで求解
(cd wasm && node verify_set.mjs) > all_2618_summary.json

# 手順の3射影による169類と標準駒内訳の確認
python classify_set.py

# 保存モデルの構造的特徴指標（因果寄与・permutation importanceではありません）
python sources/rust__analysis__smoke_cone__gbdt_structural_importance.py \
  sources/rust__models__beam_sota.json \
  sources/rust__models__cone_44_gbdt.json \
  sources/rust__models__cone_pct32_gbdt.json
```

## 独立BFSの対象

通常の合法手生成のうち、攻方は王手になる手だけを採用し、受方は全合法応手を採用します。双方の玉を取る手は許しません。状態キーはSFENの盤面・手番・双方の持駒で、手数番号は除外します。最初に詰みが見つかった層で、そこに到達する全最短経路数を合計します。経路数は2以上を2に丸めます。

7,250状態を生成した結果、113手より短い詰みはなく、113手の経路数は1でした。全ての着手の合法性、毎回の王手、両玉保持、詰上がりを別途逐次確認しています。

これはこの具体的な作品の再検証であり、ライブラリやfmrs全体の形式的正当性証明ではありません。また、この検証の実行時間を、発見に要した計算量やソルバーの一般性能と同一視しないでください。

## 3七香の除去による補足確認

初形の攻方3七香を取り除き、その香を受方持駒へ移す変更も検査しました。盤上は39枚になりますが、駒一組の総数は維持します。fmrsと独立BFSの両方で、最短113手・解数1・元の作品と完全に同じ着手列を確認しました。結果は `noroshi_without_lance_fmrs.json` と `noroshi_without_lance_independent_bfs.json` にあります。これは発表形を改作したという意味ではなく、40枚全てが一意性に必須とは限らないことを調べる補足検証です。

香を外した補足検証も、次のコマンドで再実行できます。

```sh
python independent_verification.py --bfs --input noroshi_without_lance_fmrs.json --prefix noroshi_without_lance
```
