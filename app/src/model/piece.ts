import { Color, Kind } from ".";

export const STONE = "O" as const;

export type Piece =
  | {
      color: Color;
      kind: Kind;
      promoted: boolean;
    }
  | typeof STONE;
