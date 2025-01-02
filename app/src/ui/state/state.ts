import { cloneState } from "../clone";
import * as types from "../types";
import * as position from "./position";
import * as model from "../../model";
import { KINDS } from "../../model";
import { colorOpposite } from "../../model/color";
import { PRESET_PROBLEMS } from "../../problem";

export function newState(): types.State {
  const initialPosition = position.create();
  return {
    position: initialPosition,
    selected: undefined,
    solving: undefined,
    problems: PRESET_PROBLEMS.map(([sfen, name]) => [
      model.decodeSfen(sfen),
      name,
    ]),
    solveResponse: undefined,
  };
}

export function reduce(orig: types.State, event: types.Event): types.State {
  if (
    orig.solving &&
    event.ty !== "set-solving" &&
    event.ty !== "set-solve-response"
  ) {
    return orig;
  }
  let state;
  switch (event.ty) {
    case "click-board":
    case "click-hand":
      return handleClick(orig, event);
    case "right-click-board":
      return handleRightClick(orig, event.pos);
    case "set-position":
      state = cloneState(orig);
      state.position = event.position;
      state.selected = undefined;
      maybeClearSolveResponse(state);
      return state;
    case "set-solving":
      state = cloneState(orig);
      state.solving = event.solving;
      if (event.solving) {
        maybeClearSolveResponse(state);
      }
      return state;
    case "set-problems":
      state = cloneState(orig);
      state.problems = event.problems;
      return state;
    case "set-solve-response":
      state = cloneState(orig);
      state.solveResponse = event.response;
      return state;
    case "key-down":
      return handleKeyDown(orig, event.key);
  }
}

function handleClick(
  orig: types.State,
  event: types.ClickBoardEvent | types.ClickHandEvent
): types.State {
  const state = cloneState(orig);
  maybeClearSolveResponse(state);

  if (!state.selected) {
    if (event.ty === "click-hand") {
      state.selected = {
        ty: "hand",
        color: event.color,
        kind: event.kind,
      };
      return state;
    }
    state.selected = {
      ty: "board",
      pos: event.pos,
    };
    return state;
  }

  const selectedPiece =
    (state.selected.ty === "board" &&
      state.position.board[state.selected.pos[0]][state.selected.pos[1]]) ||
    (state.selected.ty === "hand" &&
      state.selected.kind && {
        color: state.selected.color,
        kind: state.selected.kind,
        promoted: false,
      });

  if (!selectedPiece) {
    switch (event.ty) {
      case "click-hand":
        state.selected = {
          ty: "hand",
          color: event.color,
          kind: event.kind,
        };
        break;
      case "click-board":
        state.selected = {
          ty: "board",
          pos: event.pos,
        };
        break;
      default:
        ((_: never) => {})(event);
    }
    return state;
  }

  const from =
    state.selected.ty === "hand"
      ? {
          ty: "hand" as const,
          color: selectedPiece.color,
          kind: selectedPiece.kind,
        }
      : {
          ty: "board" as const,
          pos: state.selected.pos,
        };
  const to =
    event.ty === "click-hand"
      ? { ty: "hand" as const, color: event.color }
      : {
          ty: "board" as const,
          pos: event.pos,
          color: state.selected.ty === "hand" ? "black" : selectedPiece.color,
          promoted: selectedPiece.promoted,
        };

  tryMove(state, from, to);
  state.selected = undefined;
  return state;
}

function handleRightClick(
  orig: types.State,
  pos: [number, number]
): types.State {
  const state = cloneState(orig);
  maybeClearSolveResponse(state);
  const mutablePiece = state.position.board[pos[0]][pos[1]];
  if (!mutablePiece) {
    return state;
  }
  if (mutablePiece.kind === "G" || mutablePiece.kind === "K") {
    mutablePiece.color = mutablePiece.color === "black" ? "white" : "black";
    return state;
  }
  if (!mutablePiece.promoted) {
    mutablePiece.promoted = true;
    return state;
  }
  mutablePiece.color = mutablePiece.color === "black" ? "white" : "black";
  mutablePiece.promoted = false;
  return state;
}

function maybeClearSolveResponse(state: types.State) {
  if (!state.solveResponse || state.solveResponse.ty !== "solved") {
    state.solveResponse = undefined;
    return;
  }
  state.solveResponse.response.solutions = 0;
}

type Direction = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

function nextSelection(
  hands: { [c in model.Color]: model.Hands },
  selected: types.Selected | undefined,
  direction: Direction
): types.Selected | undefined {
  if (!selected) {
    return {
      ty: "board",
      pos: [4, 4],
    };
  }
  switch (selected.ty) {
    case "hand":
      switch (direction) {
        case "ArrowUp":
          if (selected.color === "white") {
            return selected;
          } else {
            const kinds = KINDS.filter((k) => hands["black"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            return { ty: "board", pos: [8, 8 - i] };
          }
        case "ArrowDown":
          if (selected.color === "black") {
            return selected;
          } else {
            const kinds = KINDS.filter((k) => hands["white"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            return { ty: "board", pos: [0, 8 - i] };
          }
        case "ArrowLeft":
          return {
            ty: "hand",
            color: selected.color,
            kind:
              selected.kind &&
              nextKind(hands[selected.color], selected.kind, "left", false),
          };
        case "ArrowRight":
          return {
            ty: "hand",
            color: selected.color,
            kind:
              selected.kind &&
              nextKind(hands[selected.color], selected.kind, "right", false),
          };
      }
    case "board":
      const pos: [number, number] = [...selected.pos];
      switch (direction) {
        case "ArrowUp":
          pos[0]--;
          break;
        case "ArrowDown":
          pos[0]++;
          break;
        case "ArrowLeft":
          pos[1] = Math.min(8, pos[1] + 1);
          break;
        case "ArrowRight":
          pos[1] = Math.max(0, pos[1] - 1);
          break;
      }
      if (pos[0] < 0) {
        const kinds = KINDS.filter((k) => hands["white"][k]);
        const kind = kinds[Math.min(kinds.length - 1, 8 - pos[1])];
        return {
          ty: "hand",
          color: "white",
          kind,
        };
      } else if (pos[0] > 8) {
        const kinds = KINDS.filter((k) => hands["black"][k]);
        const kind = kinds[Math.min(kinds.length - 1, 8 - pos[1])];
        return {
          ty: "hand",
          color: "black",
          kind,
        };
      }
      return { ty: "board", pos };
  }
}

function nextKind(
  hands: model.Hands,
  kind: model.Kind,
  direction: "left" | "right",
  searchNonZero: boolean
): model.Kind | undefined {
  const index = KINDS.indexOf(kind);
  const mult = direction == "left" ? -1 : 1;

  for (let i = searchNonZero ? 0 : 1; i < KINDS.length; i++) {
    let j = index + i * mult;
    if (j < 0) {
      if (!searchNonZero) break;
      j = index - j;
    } else if (j >= KINDS.length) {
      if (!searchNonZero) break;
      j = index - (j - (KINDS.length - 1));
    }
    if (hands[KINDS[j]] > 0) {
      return KINDS[j];
    }
  }
  return hands[kind] > 0 ? kind : undefined;
}

function firstKind(hands: model.Hands) {
  return nextKind(hands, "P", "right", true);
}

function handleKeyDown(orig: types.State, key: string) {
  const state = cloneState(orig);

  if (key.startsWith("Arrow")) {
    state.selected = nextSelection(
      orig.position.hands,
      orig.selected,
      key as Direction
    );
    return state;
  }

  if (!state.selected) return orig;

  if (state.selected.ty == "hand") {
    if (key == " " || key == "-") {
      if (!state.selected.kind) return state;
      tryMove(
        state,
        {
          ty: "hand",
          color: state.selected.color,
          kind: state.selected.kind,
        },
        {
          ty: "hand",
          color: colorOpposite(state.selected.color),
        }
      );
    } else if (key == "+") {
      if (!state.selected.kind) return state;
      tryMove(
        state,
        {
          ty: "hand",
          color: colorOpposite(state.selected.color),
          kind: state.selected.kind,
        },
        {
          ty: "hand",
          color: state.selected.color,
        }
      );
    } else {
      const piece = keyToPiece(key);
      if (piece) {
        tryMove(
          state,
          {
            ty: "hand",
            color: colorOpposite(state.selected.color),
            kind: piece.kind,
          },
          {
            ty: "hand",
            color: state.selected.color,
          }
        );
      }
    }
    return state;
  }

  if (key == " ") {
    tryMove(
      state,
      {
        ty: "board",
        pos: state.selected.pos,
      },
      {
        ty: "hand",
        color: "white",
      }
    );
    return state;
  }

  const piece = keyToPiece(key);
  if (!piece) return state;

  tryMove(
    state,
    {
      ty: "hand",
      color: "white",
      kind: piece.kind,
    },
    {
      ty: "board",
      pos: state.selected.pos,
      color: piece.color,
      promoted: piece.promoted,
    }
  );
  return state;
}

function keyToPiece(key: string) {
  let upper = key.toUpperCase();
  const color = key == upper ? ("white" as const) : ("black" as const);
  const kind = (
    {
      Q: ["K", false],
      A: ["R", false],
      S: ["B", false],
      D: ["G", false],
      F: ["S", false],
      G: ["N", false],
      H: ["L", false],
      J: ["P", false],
      Z: ["R", true],
      X: ["B", true],
      C: ["S", true],
      V: ["N", true],
      B: ["L", true],
      N: ["P", true],
    } as const
  )[upper];

  if (!kind) {
    return undefined;
  }

  return {
    color,
    kind: kind[0],
    promoted: kind[1],
  };
}

type Source =
  | {
      ty: "hand";
      color: model.Color;
      kind: model.Kind;
    }
  | {
      ty: "board";
      pos: [number, number];
    };

type Dest =
  | {
      ty: "hand";
      color: model.Color;
    }
  | {
      ty: "board";
      pos: [number, number];
      color: model.Color;
      promoted: boolean;
    };

function tryMove(state: types.State, from: Source, to: Dest) {
  if (from.ty === "hand") {
    if (to.ty === "hand") {
      if (from.color === to.color) return;
      if (state.position.hands[from.color][from.kind] === 0) return;
      state.position.hands[from.color][from.kind]--;
      state.position.hands[to.color][from.kind]++;
    } else {
      const source = state.position.board[to.pos[0]][to.pos[1]];
      if (
        source?.kind !== from.kind &&
        state.position.hands[from.color][from.kind] === 0
      ) {
        return;
      }

      state.position.board[to.pos[0]][to.pos[1]] = {
        color: to.color,
        kind: from.kind,
        promoted: to.promoted,
      };
      state.position.hands[from.color][from.kind]--;
      if (source) {
        state.position.hands[from.color][source.kind]++;
      }
    }
  } else {
    const source = state.position.board[from.pos[0]][from.pos[1]];
    if (!source) return;
    if (to.ty === "hand") {
      state.position.board[from.pos[0]][from.pos[1]] = undefined;
      state.position.hands[to.color][source.kind]++;
    } else {
      if (from.pos[0] === to.pos[0] && from.pos[1] === to.pos[1]) {
        state.position.board[from.pos[0]][from.pos[1]] = {
          color: to.color,
          kind: source.kind,
          promoted: to.promoted,
        };
      } else {
        const dest = state.position.board[to.pos[0]][to.pos[1]];
        state.position.board[to.pos[0]][to.pos[1]] = source;
        if (dest) {
          state.position.hands[source.color][dest.kind]++;
        }
        state.position.board[from.pos[0]][from.pos[1]] = undefined;
      }
    }
  }
  state.position.hands["white"]["K"] += state.position.hands["black"]["K"];
  state.position.hands["black"]["K"] = 0;

  // Update selected.
  if (!state.selected) return;
  if (state.selected.ty === "hand") {
    state.selected.kind = nextKind(
      state.position.hands[state.selected.color],
      state.selected.kind ?? "P",
      "left",
      true
    );
  }
}
