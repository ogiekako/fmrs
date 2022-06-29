import { useState } from 'react';
import Board from './Board';
import Hands from './Hands';
import { newState, updatedState, updateStateOnRightClick } from './state';
import * as types from './types';

export function Editor(props: {
    onSolved: (jkf: string) => void,
}) {
    const [state, setState] = useState<types.State>(() => newState());
    const [solving, setSolving] = useState<boolean>(() => false);

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
        <button disabled={solving} onClick={async (e) => {
            setSolving(true);
            try {
                const problem = 'ggssn2p1/lgssn3l/+R8/5N2k/4bn3/3l5/9/2g4L1/KBr3PP1 b 11P4p 1';
                for await (let line of solve(problem)) {
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
        }}>Solve</button>
    </div>
}

async function* solve(sfen: string) {
    const utf8Decoder = new TextDecoder('utf-8');
    const response = await fetch("http://localhost:1234/solve", {
        method: 'POST',
        body: 'ggssn2p1/lgssn3l/+R8/5N2k/4bn3/3l5/9/2g4L1/KBr3PP1 b 11P4p 1',
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
