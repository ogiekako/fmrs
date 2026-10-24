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
board(r['sfen'],OUT/'noroshi_initial.tex','--coords')
board(s['final_sfen'],OUT/'noroshi_final.tex','--coords','--gote-hand','残り全部')
# 豆腐煙の最大初形．scratch/tofu_smoke/verify_independent*.jsonl で
# 盤上枚数（11，16，12），最短手数（29，53，27），最短解数1を確認済み．
tofu_initials={
 'tofu_single_king_initial': '9/9/9/+P8/9/P+P7/+P8/9/1PPPPPk1+P b 2r2b4g4s4n4l8p 1',
 'tofu_two_kings_initial': '4+P3+P/6K1p/4P3+p/5+P1+pk/5P3/6PP+P/6+PpP/9/9 b 2r2b4g4s4n4l4p 1',
 'tofu_two_kings_center_initial': '9/9/4K4/9/4+P4/9/+P3+P4/4P4/PPPP1Pk1+P b 2r2b4g4s4n4l8p 1',
}
for name,sfen in tofu_initials.items():
 board(sfen,OUT/(name+'.tex'),'--coords','--gote-hand','残り全部')
m=[]
for line in r['kif'].splitlines():
 p=re.match(r'\s+([0-9]+)\s+(\S+)',line)
 if p and int(p[1]) <= 113 and re.match(r'[１２３４５６７８９]', p[2]):
  v=re.sub(r'\(\d+\)','',p[2]);m.append(v)
assert len(m)==113
# 直前の着手と同じ地点への着手は「同」と書く（詰将棋の棋譜の慣習）
for i in range(len(m)-1,0,-1):
 if m[i][:2]==m[i-1][:2]: m[i]='同'+m[i][2:]
rows=[r'\begin{tabular}{r@{\quad}l}',r'\toprule']
for i in range(0,len(m),10): rows.append(f'{i+1} & '+r'\quad '.join(m[i:i+10])+r'\\')
rows.extend([r'\bottomrule',r'\end{tabular}'])
(OUT/'noroshi_moves.tex').write_text('\n'.join(rows)+'\n')
