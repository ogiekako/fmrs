import { useReducer } from 'react';
import { newState, reduce } from './state/state';
import { Button } from 'react-bootstrap';
import { Info } from './Info';
import { Position } from './Position';
import Sfen from './Sfen';
import { sfen } from '../../model';

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
        <Button disabled={state.solving} onClick={async (e) => {
            dispatch({ ty: 'set-solving', solving: true });
            try {
                for await (let line of solve(sfen(state.position))) {
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
