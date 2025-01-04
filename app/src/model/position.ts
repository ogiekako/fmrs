import { Color, Hands, Board } from ".";

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
