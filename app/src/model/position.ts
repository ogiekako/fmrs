import { Color, Hands, Board } from ".";

export type Position = {
  board: Board;
  hands: { [C in Color]: Hands };
};
