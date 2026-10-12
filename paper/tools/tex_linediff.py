#!/usr/bin/env python3
"""1文1行で書いた LaTeX 原稿の新旧を行（文）単位で比べ、変更箇所を示す LaTeX を出力する。

削除した文は灰色の取り消し線（xeCJKfntef の \\CJKsout）、追加した文は青で示す。
latexdiff は和文を1文字ずつ比べるため変更が細切れになるので、その代わりに使う。
XeLaTeX + xeCJK の原稿を想定している。

usage: python3 tex_linediff.py old.tex new.tex > diff.tex
"""
import difflib
import re
import sys

PREAMBLE = r"""
% --- tex_linediff.py: 削除は灰色の取り消し線，追加は青 ---
\usepackage{xcolor}
\usepackage{xeCJKfntef}
\newcommand{\DIFadd}[1]{{\color{blue}#1}}
\newcommand{\DIFdel}[1]{{\color{gray}\CJKsout{#1}}}
\newcommand{\DIFdelcmd}[1]{{\color{gray}#1}}
"""

# 取り消し線の中に入れると壊れやすいものは、取り消し線の外で灰色にする
PROTECT = re.compile(
    r"(\\cite\{[^}]*\}|\\ref\{[^}]*\}|\\eqref\{[^}]*\}|\$[^$]*\$|\\textsuperscript\{[^}]*\}"
    r"|\\,|\\%|\\\\(?:\[[^\]]*\])?)")
LABEL = re.compile(r"\\label\{[^}]*\}")


def is_text(line):
    t = line.strip()
    if not t or t.startswith("%"):
        return False
    if t.startswith("\\"):
        return t.startswith(("\\ref{", "\\eqref{", "\\cite{"))
    return True


def strike(line):
    line = LABEL.sub("", line)
    out = []
    for part in PROTECT.split(line):
        if not part:
            continue
        if PROTECT.fullmatch(part):
            out.append(r"\DIFdelcmd{" + part + "}")
        else:
            out.append(r"\DIFdel{" + part + "}")
    return "".join(out)


def caption_inner(line):
    m = re.match(r"^(\s*\\caption\{)(.*)(\}\s*)$", line)
    return m.groups() if m else None


def main():
    old = open(sys.argv[1], encoding="utf-8").read().split("\n")
    new = open(sys.argv[2], encoding="utf-8").read().split("\n")
    out = []
    sm = difflib.SequenceMatcher(None, old, new, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        o, n = old[i1:i2], new[j1:j2]
        if tag == "equal":
            out.extend(n)
            continue
        # 変更のある塊。キャプションは新旧を1つのキャプションにまとめる
        oc = [caption_inner(x) for x in o]
        nc = [caption_inner(x) for x in n]
        if len(o) == len(n) == 1 and oc[0] and nc[0]:
            pre, oi, post = oc[0]
            _, ni, _ = nc[0]
            out.append(pre + strike(oi) + r"\DIFadd{" + ni + "}" + post)
            continue
        for line in o:
            if is_text(line):
                out.append(strike(line) + "%")  # 行末の改行が空白にならないように
            # 構造の行（見出し・ラベル・表の行など）の削除は表示しない
        for line in n:
            out.append(r"\DIFadd{" + line + "}%" if is_text(line) else line)
    text = "\n".join(out)
    # プリアンブルの末尾（\begin{document} の直前）に色と取り消し線の定義を入れる
    text = text.replace(r"\begin{document}", PREAMBLE + r"\begin{document}", 1)
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
