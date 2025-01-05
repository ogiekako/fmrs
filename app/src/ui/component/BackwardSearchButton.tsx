import { Button } from "react-bootstrap";
import * as model from "../../model";
import * as types from "../types";
import { CancellationToken } from "../../solve";
import { backwardSearchWasm } from "../../solve/wasm_backward_search";

export function BackwardSearchButton(props: {
  position: model.Position;
  solveResponse: types.SolveResponse | undefined;
  dispatch: types.Dispatcher;
}) {
  const sfen = model.encodeSfen(props.position);
  const unique =
    props.solveResponse?.ty === "solved" &&
    props.solveResponse.response.redundant === false &&
    props.solveResponse.response.solutions === 1 &&
    props.solveResponse.response.sfen === sfen;
  const button = unique ? (
    <Button
      variant="secondary"
      onClick={async () => {
        const cancelToken = new CancellationToken();
        props.dispatch({
          ty: "set-solving",
          solving: { cancelToken, step: 0 },
        });
        const newSfen = await backwardSearchWasm(sfen, cancelToken, (step) => {
          props.dispatch({
            ty: "set-solving",
            solving: { cancelToken, step },
          });
        });
        if (sfen === newSfen) {
          alert("これ以上逆算できません");
        }
        props.dispatch({ ty: "set-solving", solving: undefined });
        if (newSfen) {
          props.dispatch({
            ty: "set-position",
            position: model.decodeSfen(newSfen),
          });
        }
      }}
      title="唯一解のままできるだけ逆算"
    >
      自動逆算
    </Button>
  ) : null;

  return <div>{button}</div>;
}
