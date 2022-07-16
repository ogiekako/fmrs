import { useState } from 'react';
import Board from './Board';
import Hands from './Hands';
import { newState, update, updateOnRightClick } from './state/state';
import * as types from './types';
import * as model from '../../model';
import { decode } from '../../model/sfen/decode';
import Button from 'react-bootstrap/Button';

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

    const sfen = model.sfen(state.position);

    return <div>
        <Hands hands={state.position.hands['white']} selected={whiteHandSelected} onClick={k => setState(state => update(state, { ty: 'hand', color: 'white', kind: k }))} />
        <Board pieces={state.position.board} selected={boardSelected} onClick={pos => setState(state => update(state, { ty: 'board', pos }))} onRightClick={pos => setState(state => updateOnRightClick(state, pos))} />
        <Hands hands={state.position.hands['black']} selected={blackHandSelected} onClick={k => setState(state => update(state, { ty: 'hand', color: 'black', kind: k }))} />
        <div>SFEN <input type="text" value={sfen} onChange={e => {
            if (e.target.value === sfen) {
                return;
            }
            setState({
                position: decode(e.target.value),
                selected: undefined,
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
