import { Color, Hands, Board, Kind } from ".";

export type Position = {
  board: Board;
  hands: { [C in Color]: Hands };
};

export function positionStone(position: Position): boolean[][] {
  const res = new Array(9).fill(null).map(() => new Array(9).fill(false));
  for (let i = 0; i < 9; i++) {
    for (let j = 0; j < 9; j++) {
      res[i][j] = position.board[i][j] === "O";
    }
  }
  return res;
}

export function positionPieceBox(position: Position): Hands {
  return {
    P: Math.max(0, 18 - count(position, "P")),
    L: Math.max(0, 4 - count(position, "L")),
    N: Math.max(0, 4 - count(position, "N")),
    S: Math.max(0, 4 - count(position, "S")),
    G: Math.max(0, 4 - count(position, "G")),
    B: Math.max(0, 2 - count(position, "B")),
    R: Math.max(0, 2 - count(position, "R")),
    K: Math.max(0, 2 - count(position, "K")),
  };
}

function count(position: Position, kind: Kind): number {
  let res = 0;
  for (let i = 0; i < 9; i++) {
    for (let j = 0; j < 9; j++) {
      const piece = position.board[i][j];
      if (piece && piece !== "O" && piece.kind === kind) {
        res++;
      }
    }
  }
  return res + position.hands["black"][kind] + position.hands["white"][kind];
}
