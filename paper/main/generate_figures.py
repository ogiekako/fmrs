#!/usr/bin/env python3
import json,re,pathlib,subprocess,sys
HERE=pathlib.Path(__file__).resolve().parent
DATA=HERE/'verification'
OUT=HERE/'figures';OUT.mkdir(exist_ok=True)
r=json.load(open(DATA/'noroshi_fmrs_verification.json'))
s=json.load(open(DATA/'noroshi_independent_replay.json'))
SFEN2TIKZ=HERE.parent/'tools'/'sfen2tikz.py'
def board(sfen,path,*opts):
 # 盤面図は extended abstract と同じ paper/tools/sfen2tikz.py で生成する（マス 5.2mm×5.6mm）
 out=subprocess.run([sys.executable,str(SFEN2TIKZ),sfen,'5.2','5.6',*opts],check=True,capture_output=True,text=True).stdout
 path.write_text(out)
board(r['sfen'],OUT/'noroshi_initial.tex')
board(s['final_sfen'],OUT/'noroshi_final.tex','--gote-hand','残り全部')
m=[]
for line in r['kif'].splitlines():
 p=re.match(r'\s+([0-9]+)\s+(\S+)',line)
 if p and int(p[1]) <= 113 and re.match(r'[１２３４５６７８９]', p[2]):
  v=re.sub(r'\(\d+\)','',p[2]);m.append(v)
assert len(m)==113
rows=[r'\begin{tabular}{r@{\quad}l}',r'\toprule']
for i in range(0,len(m),10): rows.append(f'{i+1} & '+r'\quad '.join(m[i:i+10])+r'\\')
rows.extend([r'\bottomrule',r'\end{tabular}'])
(OUT/'noroshi_moves.tex').write_text('\n'.join(rows)+'\n')
