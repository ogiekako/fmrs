import { useReducer } from "react";
import { newState, reduce } from "../state/state";
import Info from "./Info";
import Position from "./Position";
import Problems from "./Problems";
import Sfen from "./Sfen";
import SolveButton from "./SolveButton";

export function Editor(props: {}) {
  const [state, dispatch] = useReducer(reduce, newState());

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
        dispatch={dispatch}
      />
    </div>
  );
}
