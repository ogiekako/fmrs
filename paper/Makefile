LATEX := lualatex -interaction=nonstopmode -halt-on-error

# 一号局「狼煙」。盤面図はこの SFEN から機械生成する（手写しによる誤りを防ぐため）
NOROSHI_SFEN := 8+P/6K1p/4S3+P/3+Pp+PP+pn/2BpPP1p1/3Bk+p+p1P/1RRs+lgLNG/2SPNlNPG/G+pL4S1 b - 1

.PHONY: all abstract clean distclean

all: abstract

abstract: extended-abstract/extended-abstract.pdf

extended-abstract/noroshi_board.tex: tools/sfen2tikz.py
	python3 tools/sfen2tikz.py '$(NOROSHI_SFEN)' 5.6 6.0 > $@

# lualatex を2回: 1回目で相互参照(\ref, 文献番号)を .aux に書き、2回目で解決する
extended-abstract/extended-abstract.pdf: \
		extended-abstract/extended-abstract.tex \
		extended-abstract/noroshi_board.tex
	cd extended-abstract && $(LATEX) extended-abstract.tex >/dev/null \
		&& $(LATEX) extended-abstract.tex >/dev/null
	@pdfinfo $@ | grep '^Pages:' || true

clean:
	rm -f */*.aux */*.log */*.out */*.toc */*.fdb_latexmk */*.fls */*.synctex.gz

distclean: clean
	rm -f */*.pdf
