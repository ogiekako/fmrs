import { Color, Kind } from ".";

export type Piece = {
  color: Color;
  kind: Kind;
  promoted: boolean;
};
