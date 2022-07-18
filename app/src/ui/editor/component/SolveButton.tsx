import { Button, Spinner } from 'react-bootstrap';
import * as model from '../../../model';
import * as types from '../types';
import * as solve from '../../../solve';
import { solveServer } from '../../../solve/server_solver';

const USE_WASM = true;

export default function SolveButton(props: {
    position: model.Position,
    solving: types.Solving | undefined,
    dispatch: types.Dispatcher,
    onSolved: (jkf: string) => void,
}) {
    const buttonText = props.solving ? "Cancel" : "Solve";
    const buttonVariant = props.solving ? "danger" : "primary"
    return <div className="d-flex" style={{ gap: "5px" }}>
        <Button variant={buttonVariant} onClick={async e => {
            e.currentTarget.blur();
            if (props.solving) {
                props.solving.cancelToken.cancel();
                return;
            }
            const cancelToken = new solve.CancellationToken();
            props.dispatch({ ty: 'set-solving', solving: { cancelToken, step: 0 } });
            try {
                if (USE_WASM) {
                    const n = 10;
                    const cancelToken = new solve.CancellationToken();
                    const onStep = (step: number) => {
                        props.dispatch({ ty: 'set-solving', solving: { cancelToken, step } });
                    };
                    const solutions = await solve.solve(props.position, n, cancelToken, onStep);
                    if (solutions) {
                        props.onSolved(solutions)
                    }
                    return
                }
                // request to server
                for await (let line of solveServer(model.encodeSfen(props.position))) {
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
                props.dispatch({ ty: 'set-solving', solving: undefined });
            }
        }}>{buttonText}</Button>
        {
            props.solving ?
                <>
                    <span style={{ fontSize: "0.8em" }}>Step<br />{props.solving.step}</span>
                    <Spinner animation="border" role="status">
                        <span className="visually-hidden">Solving...</span>
                    </Spinner>
                </> : <></>
        }
    </div >
}
