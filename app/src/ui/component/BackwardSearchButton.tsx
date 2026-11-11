import { Button } from "react-bootstrap";
import * as model from "../../model";
import * as types from "../types";
import { CancellationToken } from "../../solve";
import {
  BackwardSearchError,
  backwardSearchWasm,
} from "../../solve/wasm_backward_search";
import { sfenEqualsModuloColor } from "../../model/sfen/decode";

export function BackwardSearchButton(props: {
  position: model.Position;
  solveResponse: types.SolveResponse | undefined;
  oneWayMateMode: boolean;
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
          solving: { cancelToken, step: 0, sfen },
        });
        const start = Date.now();
        let newSfen: string | undefined;
        try {
          newSfen = await backwardSearchWasm(
            sfen,
            cancelToken,
            props.oneWayMateMode,
            (step, sfen) => {
              props.dispatch({
                ty: "set-solving",
                solving: { cancelToken, step, sfen },
              });
              props.dispatch({
                ty: "set-position",
                position: model.decodeSfen(sfen),
              });
            }
          );
        } catch (e) {
          console.error(e);
          // 途中の後手番局面が盤に残らないよう、最後に到達した先手番局面に戻す。
          const lastSfen = e instanceof BackwardSearchError ? e.lastSfen : sfen;
          props.dispatch({
            ty: "set-position",
            position: model.decodeSfen(lastSfen),
          });
          props.dispatch({
            ty: "set-solve-response",
            response: {
              ty: "error",
              message:
                e instanceof Error && e.message
                  ? e.message
                  : "自動逆算中にエラーが発生しました。",
              millis: Date.now() - start,
            },
          });
          return;
        } finally {
          props.dispatch({ ty: "set-solving", solving: undefined });
        }
        if (newSfen && sfenEqualsModuloColor(sfen, newSfen)) {
          return;
        }
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
