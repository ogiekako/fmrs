import { useEffect, useReducer } from "react";
import { newState, reduce } from "../state/state";
import Info from "./Info";
import Position from "./Position";
import Problems from "./Problems";
import Sfen from "./Sfen";
import SolveButton from "./SolveButton";
import { decodeSfen, encodeSfen } from "../../model";

export function Editor(props: {}) {
  const [state, dispatch] = useReducer(reduce, newState());

  const sfen = encodeSfen(state.position);
  const url = new URL(window.location.href);
  if (url.searchParams.get("sfen") !== sfen) {
    url.searchParams.set("sfen", sfen);
    window.history.pushState({}, "", url);
  }
  useEffect(() => {
    const onPopState = () => {
      const url = new URL(window.location.href);
      const sfen = url.searchParams.get("sfen");
      if (sfen) {
        dispatch({
          ty: "set-position",
          position: decodeSfen(sfen),
        });
      }
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  });

  return (
    <div>
      <div className="d-flex">
        <div>
          <Position
            position={state.position}
            selected={state.selected}
            dispatch={dispatch}
            disabled={!!state.solving}
          />
        </div>
        <Info />
        <div className="p-3">
          <Problems
            position={state.position}
            problems={state.problems}
            dispatch={dispatch}
            disabled={!!state.solving}
          />
        </div>
      </div>
      <Sfen
        position={state.position}
        dispatch={dispatch}
        disabled={!!state.solving}
      />
      <SolveButton
        position={state.position}
        solving={state.solving}
        solveResponse={state.solveResponse}
        solutionLimit={state.solutionLimit}
        dispatch={dispatch}
      />
    </div>
  );
}
