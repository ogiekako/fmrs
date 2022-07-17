import { useReducer } from 'react';
import { newState, reduce } from './state/state';
import Info from './Info';
import Position from './Position';
import Sfen from './Sfen';
import SolveButton from './SolveButton';

export function Editor(props: {
    onSolved: (jkf: string) => void,
}) {
    const [state, dispatch] = useReducer(reduce, newState());

    return <div>
        <div className="d-flex">
            <Position position={state.position} selected={state.selected} dispatch={dispatch} />
            <Info />
        </div>
        <Sfen position={state.position} dispatch={dispatch} />
        <SolveButton position={state.position} disabled={state.solving} dispatch={dispatch} onSolved={props.onSolved} />
    </div>
}
