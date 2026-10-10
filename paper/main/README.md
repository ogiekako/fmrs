# GPW 2026 本論文：協力詰における全駒煙詰の発見

岡 圭吾「協力詰における全駒煙詰の発見：探索手法の段階的な設計と改良による計算機支援創作」の著者レビュー用原稿です。

- [本文PDF（6ページ）](gpw2026_cooperative_mate.pdf)
- [LaTeXソース](gpw2026_cooperative_mate.tex)
- [検証結果と再現手順](verification/README.md)
- [投稿用 extended abstract](../extended-abstract/)

本文・PDFは2026年10月9日作成のレビュー稿です。最終提出版ではありません。

## 構成

- `figures/`：SFENと検証済み113手から生成した初形・詰上がり・着手表
- `generate_figures.py`：図と着手表の再生成（Python標準ライブラリのみ）
- `build.sh`：XeLaTeXによる本文PDFのビルド
- `verification/`：発表形「狼煙」、保存候補2,618局、169類の分類、保存GBDTの構造的特徴指標の再検証資料
- `THIRD_PARTY_NOTICES.txt`：同梱ファイルと依存関係の権利・出典
- `SHA256SUMS`：公開ファイル一式のチェックサム

## PDFのビルド

必要な環境：XeLaTeX、xeCJK、fontspec、PGF/TikZ、一般的なAMS/LaTeXパッケージ、
Latin Modern、Noto Serif CJK JPおよびNoto Sans CJK JP。
TeX Liveの日本語/CJKとXeTeXパッケージを導入した環境を想定しています。

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

入力は `verification/noroshi_fmrs_verification.json` と
`verification/noroshi_independent_replay.json` です。
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

