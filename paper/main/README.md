# GPW 2026 本論文：協力詰における全駒煙詰の発見

岡 圭吾「協力詰における全駒煙詰の発見：探索手法の段階的な設計と改良による計算機支援創作」の著者レビュー用原稿です。

- [本文PDF（8ページ）](gpw2026_cooperative_mate.pdf)
- [前版（ea890a2）から2026年10月24日版までの変更箇所](review/diff_since_ea890a2.pdf)（削除は灰色の取り消し線、追加は青。`../tools/tex_linediff.py` で生成）
- [GPW優秀論文賞（2021〜2025年）の分析と本稿への反映](review/gpw_award_analysis.md)
- [LaTeXソース](gpw2026_cooperative_mate.tex)
- [検証結果と再現手順](verification/README.md)
- [投稿用 extended abstract](../extended-abstract/)

本文・PDFは2026年10月25日更新の原稿です（初版は10月9日）。
3.1節で探索中の候補判定と最終候補の厳密検証を区別し、4.2節でGumbel乱択の抽出重みの説明を明確にしました。

## 構成

- `figures/`：「狼煙」の初形・詰上がり・着手表と，豆腐煙の初形3図
- `generate_figures.py`：図と着手表の再生成（Python標準ライブラリのみ。盤面図は `../tools/sfen2tikz.py` を呼ぶ）
- `build.sh`：XeLaTeXによる本文PDFのビルド
- `verification/`：発表形「狼煙」、保存候補2,618局、169類の分類、保存GBDTの構造的特徴指標の再検証資料
- `THIRD_PARTY_NOTICES.txt`：同梱ファイルと依存関係の権利・出典
- `SHA256SUMS`：公開ファイル一式のチェックサム

## PDFのビルド

必要な環境：XeLaTeX、xeCJK、fontspec、PGF/TikZ、一般的なAMS/LaTeXパッケージ、
Latin Modern、Noto Serif CJK JPおよびNoto Sans CJK JP。
TeX Liveの日本語/CJKとXeTeXパッケージを導入した環境を想定しています。
XeTeXはフォントを名前で探すので、Latin ModernとNoto CJKがfontconfigから見える必要があります
（`fc-list | grep -E "Latin Modern|Noto (Serif|Sans) CJK JP"` で確認）。
TeX Live付属のLatin Modernが見えない場合は、TeX Liveの `fonts/opentype/public/lm` を
`<dir>` に加えたfonts.confを用意し、`FONTCONFIG_FILE` で指定してビルドしてください。

IPSJの公式スタイルから **UTF8版の `ipsj.cls` v4.1** を取得し、
このディレクトリへ置いてください。クラスファイルはこのリポジトリには同梱していません。

- [公式配布ページ](https://www.ipsj.or.jp/journal/submit/style.html)
- [公式 v4.1 ZIP](https://www.ipsj.or.jp/journal/submit/9faeag0000001nsz-att/ipsj_v4-1.zip) の `UTF8/ipsj.cls`
- 使用したクラスのSHA-256：`e00eed9718f0961ba71d12b21454dddfeead82e9c5c002fbd5f7a28f3b0c7d50`

このディレクトリで実行します。

```sh
sh build.sh
```

XeLaTeXを2回実行します。BibTeXは不要です。
本文はIPSJクラスを `english,submit` で読み込み、xeCJKで日本語を組みます。
隣のextended abstractはLuaLaTeXを使う別稿です。

## 図の再生成

```sh
python3 generate_figures.py
```

盤面図は extended abstract と同じ `../tools/sfen2tikz.py` で生成します（マス5.2mm×5.6mm、詰上がり図の受方持駒は「残り全部」と表示）。
入力は `verification/noroshi_fmrs_verification.json` と
`verification/noroshi_independent_replay.json` です。
豆腐煙の初形3図は `generate_figures.py` 内の検証済みSFENを使います（検証記録は `../../scratch/tofu_smoke/verify_independent*.jsonl`）。
図を変更するときは、手で駒文字を動かさず、入力SFENと検証結果をそろえてください。

## 検証の範囲

- 「狼煙」：fmrsと独立な合法手生成を用いた幅優先探索で、最短113手・最短解数1を確認
- 保存候補2,618局：全件最短113手・最短解数1、標準40枚の初形と盤上3枚の詰上がり
- 着手列の3射影による分類：169類
- 保存モデルの特徴量集計：構造的な利用指標。因果寄与やpermutation importanceではありません

「解数1」は最短手順についての結果です。より長い迂回手順の全否定や、
候補生成の完全性を意味しません。この資料は保存結果の再検証用であり、
大規模な発見探索そのものを再実行するものではありません。

資料照合時のfmrsは
[`546167f38447f7d322bc41a29048b3e05d94a313`](https://github.com/ogiekako/fmrs/tree/546167f38447f7d322bc41a29048b3e05d94a313)。
モデル・入力データのスナップショットと、公式公開WASMの固定版を同梱しています。
詳細な固定版とハッシュは [verification_provenance.json](verification/verification_provenance.json) を参照してください。
python-shogiは同梱していません。依存関係の導入と全再実行コマンドは
[verification/README.md](verification/README.md) に記載しています。

チェックサムはこのディレクトリで確認できます。

```sh
sha256sum --check SHA256SUMS
```

