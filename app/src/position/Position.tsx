import { useState } from 'react';
import { newState } from './state';
import * as types from './types';

function Square(props: { piece: types.Piece | undefined, selected: boolean }) {
    return <span className="Square" style={{
        width: 20,
        height: 20,
        border: "1px solid black",
        display: "inline-block",
    }}>{
            props.piece ? props.piece.toString() : ''
        }</span>
}

export function Position(props: {
    onChange: (board: types.Board) => void,
}) {
    const [state, setState] = useState<types.State>(() => newState());

    const board = [];
    for (let row = 0; row < 9; row++) {
        const rowPieces = []
        for (let col = 8; col >= 0; col--) {
            rowPieces.push(<Square key={col} piece={state.board.pieces[col][row]} selected={[row, col] === state.selected} />);
        }
        board.push(<div key={row} className="row">{rowPieces}</div>)
    }
    return <div>{board}</div>;
}
