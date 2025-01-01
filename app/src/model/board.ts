import { Piece } from ".";

/**
 * (row, col)
 */
export type Board = (Piece | undefined)[][];

export function emptyBoard(): Board {
  return new Array(9).fill(null).map(() => new Array(9));
}
