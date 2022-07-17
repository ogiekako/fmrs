import { Button, Spinner } from 'react-bootstrap';
import * as model from '../../../model';
import * as types from '../types';

export default function SolveButton(props: {
    position: model.Position,
    solving: boolean,
    dispatch: types.Dispatcher,
    onSolved: (jkf: string) => void,
}) {
    return <div className="d-flex">
        <Button disabled={props.solving} onClick={async () => {
            props.dispatch({ ty: 'set-solving', solving: true });
            try {
                for await (let line of solve(model.encodeSfen(props.position))) {
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
                props.dispatch({ ty: 'set-solving', solving: false });
            }
        }}>Solve</Button>
        {
            props.solving ?
                <Spinner animation="border" role="status">
                    <span className="visually-hidden">Solving...</span>
                </Spinner> : <></>
        }
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
