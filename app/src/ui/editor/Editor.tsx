import { useReducer, useState } from 'react';
import Board from './Board';
import Hands from './Hands';
import { newState, reduce } from './state/state';
import * as types from './types';
import * as model from '../../model';
import { decode } from '../../model/sfen/decode';
import { Button } from 'react-bootstrap';
import { Info } from './Info';
import { Position } from './Position';

export function Editor(props: {
    onSolved: (jkf: string) => void,
}) {
    const [state, dispatch] = useReducer(reduce, newState());
    const [solving, setSolving] = useState<boolean>(() => false);

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
        <Button disabled={solving} onClick={async (e) => {
            setSolving(true);
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
                setSolving(false);
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
