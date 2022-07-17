import { useReducer, useState } from 'react';
import { newState, reduce } from './state/state';
import * as model from '../../model';
import { Button } from 'react-bootstrap';
import { Info } from './Info';
import { Position } from './Position';
import { decode } from '../../model/sfen/decode';

export function Editor(props: {
    onSolved: (jkf: string) => void,
}) {
    const [state, dispatch] = useReducer(reduce, newState());

    const sfen = model.sfen(state.position);

    return <div>
        <div className="d-flex">
            <Position position={state.position} selected={state.selected} dispatch={dispatch} />
            <Info />
        </div>
        <div>SFEN <input type="text" value={sfen} onChange={e => {
            if (e.target.value === sfen) {
                return;
            }
            dispatch({
                ty: 'set-position',
                position: decode(e.target.value),
            });
        }} style={{ width: 250 }} /></div>
        <Button disabled={state.solving} onClick={async (e) => {
            dispatch({ ty: 'set-solving', solving: true });
            try {
                for await (let line of solve(sfen)) {
                    const obj = JSON.parse(line);
                    if (obj['Solved']) {
                        props.onSolved(JSON.stringify(obj['Solved']))
                    } else {
                        console.log(line);
                    }
                }
            } catch (e: any) {
                console.error(e)
            } finally {
                dispatch({ ty: 'set-solving', solving: false });
            }
        }}>Solve</Button>
    </div>
}

async function* solve(sfen: string) {
    const utf8Decoder = new TextDecoder('utf-8');
    const response = await fetch("http://localhost:1234/solve", {
        method: 'POST',
        body: sfen,
    });
    const reader = response.body!.getReader();

    let line = "";
    for (; ;) {
        let { value, done } = await reader.read();
        if (done) {
            if (line) {
                yield line;
            }
            return;
        }
        const s = utf8Decoder.decode(value!);
        for (let i = 0; i < s.length; i++) {
            if (s[i] === '\n') {
                yield line;
                line = "";
                continue;
            }
            line += s[i];
        }
    }
}
