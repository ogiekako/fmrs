"""Independent full-SFEN replay/BFS with python-shogi 1.1.1 (not fmrs code)."""
import sys, json, time, collections, argparse
from pathlib import Path
ROOT=Path(__file__).parent
sys.path.insert(0,str(ROOT/'python_deps'))
import shogi
PREFIX='noroshi'

def key(b): return ' '.join(b.sfen().split()[:3])
def replay(data):
    b=shogi.Board(data['sfen']); states=[]
    moves=data['moves'].split()
    for ply in range(len(moves)+1):
        types=collections.Counter(p.piece_type for s in shogi.SQUARES if (p:=b.piece_at(s)))
        kings=sum(p.piece_type==shogi.KING for s in shogi.SQUARES if (p:=b.piece_at(s)))
        assert kings==2, (ply,kings)
        states.append({'ply':ply,'remaining_plies':len(moves)-ply,'sfen':b.sfen(),'board_pieces':sum(types.values()),'in_check':b.is_check(),'black_hand':dict(b.pieces_in_hand[shogi.BLACK]),'white_hand':dict(b.pieces_in_hand[shogi.WHITE])})
        if ply==len(moves):break
        m=shogi.Move.from_usi(moves[ply]); side=b.turn
        assert m in b.legal_moves, (ply+1,moves[ply],b.sfen())
        assert b.piece_type_at(m.to_square)!=shogi.KING
        b.push(m)
        if side==shogi.BLACK:assert b.is_check(),(ply+1,'attacker did not check')
    assert b.turn==shogi.WHITE and b.is_checkmate()
    assert states[-1]['board_pieces']==3
    assert not b.pieces_in_hand[shogi.BLACK]
    out={'library':'python-shogi 1.1.1','plies':len(moves),'all_moves_legal':True,'all_black_moves_check':True,'both_kings_preserved':True,'final_checkmate':True,'final_sfen':b.sfen(),'states':states}
    (ROOT/(PREFIX+'_independent_replay.json')).write_text(json.dumps(out,ensure_ascii=False,indent=2))
    print('REPLAY',out['plies'],out['final_sfen'],flush=True)

def bfs(sfen):
    start=time.monotonic(); initial=key(shogi.Board(sfen))
    distance={initial:0}; layer={initial:(1,())}; depth=0; stats=[]
    while layer:
        mates=[]
        for state,(paths,path) in layer.items():
            b=shogi.Board(state+' 1')
            if b.turn==shogi.WHITE and b.is_checkmate():mates.append((state,paths,path))
        stats.append({'depth':depth,'states':len(layer),'shortest_mates':len(mates)})
        print('BFS',depth,len(layer),'total',len(distance),'seconds',round(time.monotonic()-start,2),flush=True)
        if mates:
            out={'library':'python-shogi 1.1.1','sfen':sfen,'position_key':'full SFEN board, turn, both hands; move number excluded; no hash-only equivalence','shortest_plies':depth,'shortest_solution_count_capped_at_2':min(2,sum(x[1] for x in mates)),'terminal_position_count':len(mates),'visited_states':len(distance),'time_seconds':time.monotonic()-start,'solution':' '.join(mates[0][2]),'layers':stats}
            (ROOT/(PREFIX+'_independent_bfs.json')).write_text(json.dumps(out,ensure_ascii=False,indent=2))
            return
        nxt={}
        for state,(paths,path) in layer.items():
            b=shogi.Board(state+' 1'); side=b.turn
            for m in list(b.legal_moves):
                if b.piece_type_at(m.to_square)==shogi.KING:continue
                b.push(m)
                if side==shogi.BLACK and not b.is_check(): b.pop();continue
                target=key(b); b.pop()
                old=distance.get(target)
                if old is not None and old<depth+1:continue
                if target in nxt:
                    n,p=nxt[target];nxt[target]=(min(2,n+paths),p)
                else:
                    nxt[target]=(paths,path+(m.usi(),));distance[target]=depth+1
        layer=nxt;depth+=1
    raise RuntimeError('No mate')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--bfs',action='store_true');parser.add_argument('--input',default='noroshi_fmrs_verification.json');parser.add_argument('--prefix',default='noroshi');args=parser.parse_args()
    PREFIX=args.prefix
    data=json.loads((ROOT/args.input).read_text())
    replay(data)
    if args.bfs:bfs(data['sfen'])
