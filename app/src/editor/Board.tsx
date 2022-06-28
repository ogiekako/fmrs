import { SELECTED_COLOR } from './constants';
import * as types from './types';

export default function Board(props: { pieces: (types.Piece | undefined)[][], selected: [number, number] | undefined, onClick: (pos: [number, number]) => void, onRightClick: (pos: [number, number]) => void }) {
    const board = [];
    for (let row = 0; row < 9; row++) {
        const rowPieces = []
        for (let col = 8; col >= 0; col--) {
            rowPieces.push(<Square key={col} onRightClick={() => props.onRightClick([row, col])} onClick={() => props.onClick([row, col])} piece={props.pieces[row][col]} selected={!!props.selected && row === props.selected[0] && col === props.selected[1]} />);
        }
        board.push(<tr key={row}>{rowPieces}</tr>)
    }
    return <table style={{ borderCollapse: "collapse" }}>
        <tbody>
            {board}
        </tbody>
    </table>;
}

function Square(props: { piece: types.Piece | undefined, selected: boolean, onClick: () => void, onRightClick: () => void }) {
    return <td onClick={_e => props.onClick()} onContextMenu={e => { e.preventDefault(); e.stopPropagation(); props.onRightClick() }} style={{
        width: 32,
        height: 36,
        padding: 0,
        border: "1px solid black",
        backgroundColor: props.selected ? SELECTED_COLOR : "white",
        fontSize: '1.5em',
        verticalAlign: 'middle',
    }}>{
            props.piece ? pieceString(props.piece) : ''
        }</td>
}

function pieceString(p: types.Piece) {
    const letter = MAPPING[p.kind][p.promoted ? 1 : 0];
    return <div style={{ transform: p.color === 'black' ? "rotate(0)" : "rotate(180deg)", textAlign: "center" }}>{letter}</div>
}

const MAPPING: { [K in types.Kind]: [string, string] } = {
    'P': ['歩', 'と'],
    'L': ['香', '杏'],
    'N': ['桂', '圭'],
    'S': ['銀', '全'],
    'G': ['金', ''],
    'B': ['角', '馬'],
    'R': ['飛', '龍'],
    'K': ['玉', ''],
}
