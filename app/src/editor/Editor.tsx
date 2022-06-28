import { useState } from 'react';
import Board from './Board';
import Hands from './Hands';
import { newState, updatedState, updateStateOnRightClick } from './state';
import * as types from './types';

export function Editor(props: {
    onSolve: (position: types.Position) => void,
}) {
    const [state, setState] = useState<types.State>(() => newState());

    let boardSelected = undefined;
    let whiteHandSelected = undefined;
    let blackHandSelected = undefined;
    if (state.selected) {
        if (state.selected.ty === 'board') {
            boardSelected = state.selected.pos;
        } else if (state.selected.color === 'white') {
            whiteHandSelected = state.selected.kind
        } else {
            blackHandSelected = state.selected.kind
        }
    }

    return <div>
        <Hands hands={state.position.hands['white']} selected={whiteHandSelected} onClick={k => setState(state => updatedState(state, { ty: 'hand', color: 'white', kind: k }))} />
        <Board pieces={state.position.pieces} selected={boardSelected} onClick={pos => setState(state => updatedState(state, { ty: 'board', pos }))} onRightClick={pos => setState(state => updateStateOnRightClick(state, pos))} />
        <Hands hands={state.position.hands['black']} selected={blackHandSelected} onClick={k => setState(state => updatedState(state, { ty: 'hand', color: 'black', kind: k }))} />
    </div>
}
