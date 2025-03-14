import { cloneSelected, cloneState } from "../clone";
import * as types from "../types";
import * as position from "./position";
import * as model from "../../model";
import { KINDS } from "../../model";
import { PRESET_PROBLEMS } from "../../problem";
import { positionPieceBox } from "../../model/position";

export function newState(): types.State {
  const url = new URL(window.location.href);
  const sfen = url.searchParams.get("sfen");

  const initialPosition = sfen ? model.decodeSfen(sfen) : position.create();
  return {
    position: initialPosition,
    selected: {
      shown: false,
      ty: "board",
      pos: [4, 4],
      typed: false,
    },
    solving: undefined,
    problems: PRESET_PROBLEMS.map(([sfen, name]) => [
      model.decodeSfen(sfen),
      name,
    ]),
    solveResponse: undefined,
    solutionLimit: 5,
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
      state.selected = {
        shown: false,
        ty: "board",
        pos: [4, 4],
        typed: false,
      };
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
    case "set-solution-limit":
      state = cloneState(orig);
      state.solutionLimit = event.n;
      return state;
    case "shift":
      return shifted(orig, event.dir);
  }
}

function handleClick(
  orig: types.State,
  event: types.ClickBoardEvent | types.ClickHandEvent
): types.State {
  const state = cloneState(orig);
  maybeClearSolveResponse(state);

  if (
    !state.selected.shown ||
    (state.selected.ty === "board" && state.selected.typed)
  ) {
    if (event.ty === "click-hand") {
      state.selected = {
        shown: true,
        ty: "hand",
        color: event.color,
        kind: event.kind,
      };
      return state;
    }
    state.selected = {
      shown: true,
      ty: "board",
      pos: event.pos,
      typed: false,
    };
    return state;
  }

  const isSelected =
    (state.selected.ty === "board" &&
      !!state.position.board[state.selected.pos[0]][state.selected.pos[1]]) ||
    (state.selected.ty === "hand" && !!state.selected.kind);

  if (!isSelected) {
    switch (event.ty) {
      case "click-hand":
        state.selected = {
          shown: true,
          ty: "hand",
          color: event.color,
          kind: event.kind,
        };
        break;
      case "click-board":
        state.selected = {
          shown: true,
          ty: "board",
          pos: event.pos,
          typed: false,
        };
        break;
      default:
        ((_: never) => {})(event);
    }
    return state;
  }

  if (
    state.selected.ty === "board" &&
    state.position.board[state.selected.pos[0]][state.selected.pos[1]] === "O"
  ) {
    switch (event.ty) {
      case "click-hand":
        state.position.board[state.selected.pos[0]][state.selected.pos[1]] =
          undefined;
        break;
      case "click-board":
        tryMove(
          state,
          {
            ty: "board",
            pos: state.selected.pos,
          },
          {
            ty: "board",
            pos: event.pos,
            color: "black",
            promoted: false,
          }
        );
        break;
      default:
        ((_: never) => {})(event);
    }
    state.selected.shown = false;
    return state;
  }

  const from =
    state.selected.ty === "hand"
      ? {
          ty: "hand" as const,
          color: state.selected.color,
          kind: state.selected.kind!,
        }
      : {
          ty: "board" as const,
          pos: state.selected.pos,
        };

  let to: Dest;
  switch (event.ty) {
    case "click-hand":
      to = { ty: "hand", color: event.color };
      break;
    case "click-board":
      if (from.ty === "hand") {
        to = {
          ty: "board",
          pos: event.pos,
          color: "black",
          promoted: false,
        };
      } else {
        const piece = state.position.board[from.pos[0]][from.pos[1]];
        if (!piece || piece === "O") throw new Error("BUG: unreachable");
        to = {
          ty: "board",
          pos: event.pos,
          color: piece.color,
          promoted: piece.promoted,
        };
      }
      break;
    default:
      ((_: never) => {})(event);
      throw new Error("BUG: unreachable");
  }

  tryMove(state, from, to);
  state.selected.shown = false;
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
  if (mutablePiece === "O") {
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
  hands: { [c in model.Color | "pieceBox"]: model.Hands },
  selected: types.Selected,
  direction: Direction
): types.Selected {
  switch (selected.ty) {
    case "hand":
      switch (direction) {
        case "ArrowUp":
          if (selected.color === "pieceBox") {
            return selected;
          }
          if (selected.color === "white") {
            const kinds = KINDS.filter((k) => hands["white"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            const pieceBoxKinds = KINDS.filter((k) => hands["pieceBox"][k]);
            const kind = pieceBoxKinds[Math.min(pieceBoxKinds.length - 1, i)];
            return { shown: true, ty: "hand", color: "pieceBox", kind };
          } else {
            const kinds = KINDS.filter((k) => hands["black"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            return { shown: true, ty: "board", pos: [8, 8 - i], typed: false };
          }
        case "ArrowDown":
          if (selected.color === "black") {
            return selected;
          } else if (selected.color === "pieceBox") {
            const kinds = KINDS.filter((k) => hands["pieceBox"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            const whiteKinds = KINDS.filter((k) => hands["white"][k]);
            const kind = whiteKinds[Math.min(whiteKinds.length - 1, i)];
            return { shown: true, ty: "hand", color: "white", kind };
          } else {
            const kinds = KINDS.filter((k) => hands["white"][k]);
            const i = selected.kind ? kinds.indexOf(selected.kind) : 0;
            return { shown: true, ty: "board", pos: [0, 8 - i], typed: false };
          }
        case "ArrowLeft":
          return {
            shown: true,
            ty: "hand",
            color: selected.color,
            kind:
              selected.kind &&
              nextKind(hands[selected.color], selected.kind, "left", false),
          };
        case "ArrowRight":
          return {
            shown: true,
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
          shown: true,
          ty: "hand",
          color: "white",
          kind,
        };
      } else if (pos[0] > 8) {
        const kinds = KINDS.filter((k) => hands["black"][k]);
        const kind = kinds[Math.min(kinds.length - 1, 8 - pos[1])];
        return {
          shown: true,
          ty: "hand",
          color: "black",
          kind,
        };
      }
      return { shown: true, ty: "board", pos, typed: false };
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

function handleKeyDown(orig: types.State, key: string) {
  const state = cloneState(orig);

  if (key.startsWith("Arrow")) {
    state.selected = nextSelection(
      {
        ...state.position.hands,
        pieceBox: positionPieceBox(state.position),
      },
      orig.selected,
      key as Direction
    );
    return state;
  }

  if (!state.selected.shown) return orig;

  const oppositeOrWhite = {
    black: ["white", "pieceBox"] as const,
    white: ["pieceBox", "black"] as const,
    pieceBox: ["white", "black"] as const,
  } as const;

  if (state.selected.ty == "hand") {
    if (key == " " || key == "-") {
      if (!state.selected.kind) return state;
      for (const color of oppositeOrWhite[state.selected.color]) {
        if (
          tryMove(
            state,
            {
              ty: "hand",
              color: state.selected.color,
              kind: state.selected.kind,
            },
            {
              ty: "hand",
              color,
            }
          )
        ) {
          break;
        }
      }
    } else if (key == "+") {
      if (!state.selected.kind) return state;
      for (const color of oppositeOrWhite[state.selected.color]) {
        if (
          tryMove(
            state,
            {
              ty: "hand",
              color,
              kind: state.selected.kind,
            },
            {
              ty: "hand",
              color: state.selected.color,
            }
          )
        ) {
          break;
        }
      }
    } else {
      const piece = keyToPiece(key);
      if (piece && piece !== "O") {
        for (const color of oppositeOrWhite[state.selected.color]) {
          if (
            tryMove(
              state,
              {
                ty: "hand",
                color,
                kind: piece.kind,
              },
              {
                ty: "hand",
                color: state.selected.color,
              }
            )
          ) {
            break;
          }
        }
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

  if (piece === "O") {
    const dest =
      state.position.board[state.selected.pos[0]][state.selected.pos[1]];
    if (dest && dest !== "O") {
      state.position.hands["white"][dest.kind]++;
    }
    state.position.board[state.selected.pos[0]][state.selected.pos[1]] = piece;
    state.selected.typed = true;
  } else {
    for (const color of ["white", "pieceBox", "black"] as const) {
      if (
        tryMove(
          state,
          {
            ty: "hand",
            color,
            kind: piece.kind,
          },
          {
            ty: "board",
            pos: state.selected.pos,
            color: piece.color,
            promoted: piece.promoted,
          }
        )
      ) {
        state.selected.typed = true;
        break;
      }
    }
  }

  return state;
}

function keyToPiece(key: string) {
  let upper = key.toUpperCase();
  if (upper === "E") return "O" as const;
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
      color: model.Color | "pieceBox";
      kind: model.Kind;
    }
  | {
      ty: "board";
      pos: [number, number];
    };

type Dest =
  | {
      ty: "hand";
      color: model.Color | "pieceBox";
    }
  | {
      ty: "board";
      pos: [number, number];
      color: model.Color;
      promoted: boolean;
    };

function tryMove(state: types.State, from: Source, to: Dest): boolean {
  const pieceBox = positionPieceBox(state.position);
  const hands = {
    black: state.position.hands["black"],
    white: state.position.hands["white"],
    pieceBox: pieceBox,
  };

  if (from.ty === "hand") {
    if (to.ty === "hand") {
      if (from.color === to.color) return false;
      if (hands[from.color][from.kind] === 0) return false;
      hands[from.color][from.kind]--;
      hands[to.color][from.kind]++;
    } else {
      let dest = state.position.board[to.pos[0]][to.pos[1]];
      if (dest === "O") {
        state.position.board[to.pos[0]][to.pos[1]] = undefined;
        dest = undefined;
      }
      if (dest?.kind !== from.kind && hands[from.color][from.kind] === 0) {
        return false;
      }

      state.position.board[to.pos[0]][to.pos[1]] = {
        color: to.color,
        kind: from.kind,
        promoted: to.promoted,
      };
      hands[from.color][from.kind]--;
      if (dest) {
        hands[from.color][dest.kind]++;
      }
    }
  } else {
    const source = state.position.board[from.pos[0]][from.pos[1]];
    if (!source) return false;
    if (to.ty === "hand") {
      state.position.board[from.pos[0]][from.pos[1]] = undefined;
      if (source !== "O") {
        hands[to.color][source.kind]++;
      }
    } else {
      if (from.pos[0] === to.pos[0] && from.pos[1] === to.pos[1]) {
        if (source === "O") return false;
        state.position.board[from.pos[0]][from.pos[1]] = {
          color: to.color,
          kind: source.kind,
          promoted: to.promoted,
        };
      } else {
        const dest = state.position.board[to.pos[0]][to.pos[1]];
        state.position.board[to.pos[0]][to.pos[1]] = source;
        if (dest && dest !== "O") {
          hands[source === "O" ? "white" : source.color][dest.kind]++;
        }
        state.position.board[from.pos[0]][from.pos[1]] = undefined;
      }
    }
  }
  hands["black"]["K"] = 0;
  hands["white"]["K"] = 0;

  // Update selected.
  if (!state.selected.shown) return true;
  if (state.selected.ty === "hand") {
    state.selected.kind = nextKind(
      getHands(state.position, state.selected.color),
      state.selected.kind ?? "P",
      "left",
      true
    );
  }
  return true;
}

function shifted(orig: types.State, dir: "up" | "down" | "left" | "right") {
  const state = cloneState(orig);
  for (let i = 0; i < 9; i++) {
    for (let j = 0; j < 9; j++) {
      const piece = orig.position.board[i][j];
      let [ni, nj] = {
        up: [i - 1, j],
        down: [i + 1, j],
        left: [i, j + 1],
        right: [i, j - 1],
      }[dir];
      if (ni < 0) ni += 9;
      if (ni >= 9) ni -= 9;
      if (nj < 0) nj += 9;
      if (nj >= 9) nj -= 9;

      state.position.board[ni][nj] = piece;
    }
  }
  return state;
}

function getHands(
  position: model.Position,
  color: model.Color | "pieceBox"
): model.Hands {
  if (color === "pieceBox") {
    return positionPieceBox(position);
  }
  return position.hands[color];
}
