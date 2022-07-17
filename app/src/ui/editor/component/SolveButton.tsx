import { Button, Spinner } from 'react-bootstrap';
import { Solver } from '../../../../../dist/pkg';
import * as model from '../../../model';
import * as types from '../types';

const USE_WASM = true;

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
                if (USE_WASM) {
                    const n = 10;
                    const solutions = solveWasm(model.encodeSfen(props.position), n);
                    if (solutions) {
                        props.onSolved(solutions)
                    }
                } else { // request to server
                    for await (let line of solve(model.encodeSfen(props.position))) {
                        const obj = JSON.parse(line);
                        if (obj['Solved']) {
                            props.onSolved(JSON.stringify(obj['Solved']))
                        } else {
                            console.log(line);
                        }
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

/**
 * @returns array of sfen moves representing a solution.
 * If more than n solutions are found, returns n + 1 solutions.
 */
function solveWasm(sfen: string, n: number): string {
    const solver = Solver.new(sfen, n + 1);
    try {
        return solveWasmInner(solver);
    } catch (e: any) {
        console.error(e)
        return ""
    } finally {
        solver.free();
    }
}

function solveWasmInner(solver: Solver): string {
    for (; ;) {
        console.log('.');
        const error = solver.advance();
        if (error) {
            console.error(error);
            return "";
        }
        if (solver.no_solution()) {
            return ""
        }
        if (solver.solutions_found()) {
            return solver.solutions_json();
        }
    }
}