# paper

協力詰の全駒煙詰（盤上40枚・持駒なし・詰上がり3枚）に関する原稿。

## 構成

| ディレクトリ | 内容 |
|---|---|
| `extended-abstract/` | ゲームプログラミングワークショップ (GPW) 2026 投稿用の **extended abstract**（A4縦・2ページ以内） |
| `tools/` | 原稿共通のツール |
| `main/` | 本論文（未着手） |

`extended-abstract/` はあくまで投稿用の要旨であり、本論文とは別稿。
GPW 2026 の投稿規定は A4縦2ページ以内・フォント10pt以上・PDF・EasyChair (GPW-26) 経由。

## ビルド

```
make -C paper abstract     # extended-abstract/extended-abstract.pdf
```

TeX Live の LuaLaTeX（`ltjsarticle`）と Python 3 が必要。

VSCode の LaTeX Workshop から `Ctrl+Alt+B` でもビルドできる（レシピは
`.vscode/settings.json` で LuaLaTeX に設定済み）。**pdflatex では通らない**ので、
既定のレシピのままだと `luatexja-core Error: This package requires Lua(HB)(La)TeX.`
で失敗する。ただし LaTeX Workshop は盤面図 `noroshi_board.tex` を生成しないので、
SFEN を変えたときは `make` を通すこと。

## 盤面図

盤面は `tools/sfen2tikz.py` が SFEN から TikZ を生成する（`make` が自動で実行）。
手で書き写さないこと。図面の体裁は詰将棋界で標準的な寸法（マス 4.8mm×5.2mm、
外枠 0.4mm）に倣い、後手駒は180度回転、持駒は盤の左右に縦書きする。

```
python3 tools/sfen2tikz.py '<sfen>' [マス幅mm マス高mm] > board.tex
```

生成側では `\koma` と `\mochi` の2つのコマンドを定義しておく（`extended-abstract.tex` を参照）。

## 一号局「狼煙」

```
8+P/6K1p/4S3+P/3+Pp+PP+pn/2BpPP1p1/3Bk+p+p1P/1RRs+lgLNG/2SPNlNPG/G+pL4S1 b - 1
```

協力詰113手・盤上40枚・持駒なし・解が一意。`cargo run -r solve standard '<sfen>'` で再検証できる。
『詰将棋パラダイス』2026年8月号に特別懸賞出題（ogiekako 名義）。

探索の技術的経緯は [`../scratch/zenkoma_smoke_journey.md`](../scratch/zenkoma_smoke_journey.md) に記録がある。
