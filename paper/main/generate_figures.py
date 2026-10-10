#!/usr/bin/env python3
import json,re,pathlib
HERE=pathlib.Path(__file__).resolve().parent
DATA=HERE/'verification'
OUT=HERE/'figures';OUT.mkdir(exist_ok=True)
r=json.load(open(DATA/'noroshi_fmrs_verification.json'))
s=json.load(open(DATA/'noroshi_independent_replay.json'))
pieces={'P':'歩','L':'香','N':'桂','S':'銀','G':'金','B':'角','R':'飛','K':'王','+P':'と','+L':'杏','+N':'圭','+S':'全','+B':'馬','+R':'龍'}
def board(sfen,path):
 lines=[r'\begin{tikzpicture}[x=5.2mm,y=-5.2mm]',r'\draw[line width=.4pt] (0,0) grid (9,9);',r'\draw[line width=.8pt] (0,0) rectangle (9,9);']
 for i in range(9):
  lines.append(r'\node[font=\footnotesize] at (%s,-.55) {%d};'%(i+.5,9-i))
  lines.append(r'\node[font=\footnotesize] at (9.5,%s) {%s};'%(i+.5,'一二三四五六七八九'[i]))
 for row,line in enumerate(sfen.split()[0].split('/')):
  col=0
  for piece in re.findall(r'\+?[A-Za-z]|[1-9]',line):
   if piece.isdigit(): col+=int(piece);continue
   text=pieces[piece.upper()]
   if piece=='k':text='玉'
   rotate=',rotate=180' if piece[-1].islower() else ''
   lines.append(r'\node[inner sep=0pt%s] at (%s,%s) {\koma %s};'%(rotate,col+.5,row+.5,text));col+=1
 lines.append(r'\end{tikzpicture}')
 path.write_text('\n'.join(lines)+'\n')
board(r['sfen'],OUT/'noroshi_initial.tex')
board(s['final_sfen'],OUT/'noroshi_final.tex')
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
