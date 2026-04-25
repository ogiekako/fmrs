import * as types from "../types";
import Solution from "./Solution";
import { check_one_way_mate } from "../../wasm_api";

export default function SolveResponse(props: {
  solveResponse: types.SolveResponse;
  solutionLimit: number;
  oneWayMateMode: boolean;
  dispatch: types.Dispatcher;
}) {
  const message = getMessage(props.solveResponse, props.solutionLimit);

  let oneWayMessage = null;
  if (props.oneWayMateMode && props.solveResponse.ty === "solved" && props.solveResponse.response.sfen) {
    const result = check_one_way_mate(props.solveResponse.response.sfen);
    if (result !== undefined) {
      if (result.is_one_way) {
        oneWayMessage = <div>一本道詰将棋です ({result.steps}手)</div>;
      } else {
        oneWayMessage = <div>一本道詰将棋ではありません ({result.steps}手目に変化あり)</div>;
      }
      result.free();
    } else {
      oneWayMessage = <div>一本道詰将棋ではありません</div>;
    }
  }

  const text = message ? (
    <div>
      {message} ({(props.solveResponse.millis / 1000).toFixed(1)}s)
      {oneWayMessage}
    </div>
  ) : (
    <div>{oneWayMessage}</div>
  );

  return props.solveResponse.ty === "solved" ? (
    <div>
      {text}
      <Solution
        kif={props.solveResponse.response.kif}
        stone={props.solveResponse.stone}
        fromWhite={props.solveResponse.response.fromWhite}
        dispatch={props.dispatch}
      />
    </div>
  ) : (
    text
  );
}

function getMessage(r: types.SolveResponse, limit: number) {
  switch (r.ty) {
    case "error":
      return r.message;
    case "no-solution":
      return "No solution";
    case "solved":
      const count = r.response.solutions;
      if (!count) {
        return "";
      }
      if (count > limit) {
        return `More than ${limit} solutions found`;
      } else if (count > 1) {
        return `${count} solutions found`;
      } else if (r.response.redundant) {
        return `${count} solution found (駒余り)`;
      } else {
        return `${count} solution found`;
      }
  }
}
