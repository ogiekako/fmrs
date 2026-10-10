import sys,json,collections
from pathlib import Path
ROOT=Path(__file__).parent;sys.path.insert(0,str(ROOT/'python_deps'))
import shogi
DEMOTE={shogi.PROM_PAWN:shogi.PAWN,shogi.PROM_LANCE:shogi.LANCE,shogi.PROM_KNIGHT:shogi.KNIGHT,shogi.PROM_SILVER:shogi.SILVER,shogi.PROM_BISHOP:shogi.BISHOP,shogi.PROM_ROOK:shogi.ROOK}
EXPECTED={shogi.KING:2,shogi.ROOK:2,shogi.BISHOP:2,shogi.GOLD:4,shogi.SILVER:4,shogi.KNIGHT:4,shogi.LANCE:4,shogi.PAWN:18}
classes=collections.Counter(); kings=set(); atks=set(); defs=set(); finals=set(); inputs=set(); bydist=collections.defaultdict(set)
for i,line in enumerate((ROOT/'all_2618_verification.jsonl').open()):
    row=json.loads(line);b=shogi.Board(row['sfen']);inputs.add(row['sfen'])
    material=collections.Counter(DEMOTE.get(p.piece_type,p.piece_type) for s in shogi.SQUARES if(p:=b.piece_at(s)))
    assert dict(material)==EXPECTED and not any(b.pieces_in_hand), (i,material)
    king=[b.king_squares[shogi.WHITE]];atk=[];df=[]
    bydist[113].add(' '.join(b.sfen().split()[:3]))
    for ply,usi in enumerate(row['moves'].split()):
        m=shogi.Move.from_usi(usi);cap=b.piece_type_at(m.to_square)
        atk.append(cap if b.turn==shogi.BLACK and cap else -1)
        df.append(m.to_square if b.turn==shogi.WHITE and cap else -1)
        b.push(m);king.append(b.king_squares[shogi.WHITE])
        bydist[112-ply].add(' '.join(b.sfen().split()[:3]))
    assert sum(bool(b.piece_type_at(s)) for s in shogi.SQUARES)==3
    assert not b.pieces_in_hand[shogi.BLACK]
    sig=(tuple(king),tuple(atk),tuple(df));classes[sig]+=1;kings.add(sig[0]);atks.add(sig[1]);defs.add(sig[2]);finals.add(' '.join(b.sfen().split()[:3]))
out={'positions':i+1,'distinct_initial_sfen':len(inputs),'all_standard_40_piece_inventory':True,'all_final_board_count_3':True,'all_final_black_hands_empty':True,'classes':len(classes),'king_trajectory_classes':len(kings),'attacker_capture_type_classes':len(atks),'defender_capture_square_classes':len(defs),'final_sfen_count':len(finals),'final_sfens':sorted(finals),'class_size_histogram':dict(sorted(collections.Counter(classes.values()).items())),'literal_distinct_positions_by_distance':{k:len(v) for k,v in sorted(bydist.items())}}
(ROOT/'all_2618_classification.json').write_text(json.dumps(out,ensure_ascii=False,indent=2))
print(json.dumps({k:v for k,v in out.items() if k!='literal_distinct_positions_by_distance'},ensure_ascii=False,indent=2))
