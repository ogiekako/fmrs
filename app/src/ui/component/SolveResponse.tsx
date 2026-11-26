import { useMemo } from "react";
import * as model from "../../model";
import * as types from "../types";
import Solution from "./Solution";
import { useWasm } from "../../wasm_main";
import { ogiekakoProblemKif } from "../../ogiekako_problems";

export default function SolveResponse(props: {
  mode: model.SolverMode;
  solveResponse: types.SolveResponse;
  solutionLimit: number;
  oneWayMateMode: boolean;
  dispatch: types.Dispatcher;
  onSolutionLoaded?: (container: HTMLElement) => void;
}) {
  const message = getMessage(
    props.solveResponse,
    props.solutionLimit,
    props.mode
  );
  // 既知作品の KIF 生成と一本道判定は wasm を使うので、読み込み後に表示する。
  const wasm = useWasm();
  const response =
    props.solveResponse.ty === "solved" ? props.solveResponse.response : undefined;
  const kif = useMemo(() => {
    if (!response || response.kif !== undefined) {
      return response?.kif;
    }
    return wasm && ogiekakoProblemKif(wasm, response.sfen);
  }, [response, wasm]);

  let oneWayMessage = null;
  if (
    props.mode === "helpmate" &&
    wasm &&
    props.oneWayMateMode &&
    props.solveResponse.ty === "solved" &&
    props.solveResponse.response.sfen
  ) {
    const result = wasm.check_one_way_mate(props.solveResponse.response.sfen);
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

  const fallbackMessage = props.solveResponse.fallback ? (
    <div className="text-muted" style={{ fontSize: "0.8em" }}>
      サーバーに接続できなかったため、ブラウザ内で解図しました。
    </div>
  ) : null;

  const isKnownProblem =
    props.solveResponse.ty === "solved" &&
    props.solveResponse.response.knownProblem;
  const nodes = getNodes(props.solveResponse);
  const statusDetail = isKnownProblem
    ? "known problem"
    : nodes !== undefined
      ? `${(props.solveResponse.millis / 1000).toFixed(1)}s, ${nodes.toLocaleString()} nodes`
      : `${(props.solveResponse.millis / 1000).toFixed(1)}s`;

  const text = message ? (
    <div>
      {message} ({statusDetail})
      {oneWayMessage}
      {fallbackMessage}
    </div>
  ) : (
    <div>
      {oneWayMessage}
      {fallbackMessage}
    </div>
  );

  if (props.solveResponse.ty !== "solved") {
    return text;
  }
  return (
    <div>
      {text}
      {kif !== undefined ? (
        <Solution
          kif={kif}
          stone={props.solveResponse.stone}
          fromWhite={props.solveResponse.response.fromWhite}
          dispatch={props.dispatch}
          onLoaded={props.onSolutionLoaded}
        />
      ) : wasm ? (
        <div className="text-danger">解答の棋譜を生成できませんでした。</div>
      ) : (
        <div className="text-muted">解答を読み込み中…</div>
      )}
    </div>
  );
}

function getNodes(r: types.SolveResponse): number | undefined {
  switch (r.ty) {
    case "solved":
      return r.response.futsuuNodes;
    case "no-solution":
    case "suspended":
      return r.nodes;
    case "error":
      return undefined;
  }
}

function getMessage(
  r: types.SolveResponse,
  limit: number,
  mode: model.SolverMode
) {
  switch (r.ty) {
    case "error":
      return r.message;
    case "no-solution":
      return mode === "futsuu" ? "不詰" : "No solution";
    case "suspended":
      return "探索を打ち切りました";
    case "solved":
      if (mode === "futsuu" && r.response.futsuuSteps !== undefined) {
        if (!r.response.solutions) {
          return "";
        }
        return r.response.redundant
          ? `${r.response.futsuuSteps}手詰 (駒余り)`
          : `${r.response.futsuuSteps}手詰`;
      }
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
