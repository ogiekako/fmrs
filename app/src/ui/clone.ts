import * as types from "./types";
import * as model from "../model";

export function cloneState(state: types.State): types.State {
  return {
    position: model.clonePosition(state.position),
    selected: cloneSelected(state.selected),
    solving: state.solving && Object.assign({}, state.solving),
    problems: cloneProblems(state.problems),
    solveResponse: state.solveResponse && Object.freeze(state.solveResponse),
    solutionLimit: state.solutionLimit,
  };
}

export function cloneSelected(selected: types.Selected): types.Selected {
  return selected.ty === "board"
    ? {
        shown: selected.shown,
        ty: "board",
        pos: [selected.pos[0], selected.pos[1]],
        typed: selected.typed,
      }
    : {
        shown: selected.shown,
        ty: "hand",
        color: selected.color,
        kind: selected.kind,
      };
}

function cloneProblems(
  problems: Array<[model.Position, string]>
): Array<[model.Position, string]> {
  return problems.map(([position, name]) => [
    model.clonePosition(position),
    name,
  ]);
}
