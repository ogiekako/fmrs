import { Board, Color, Hands, Kind, Piece, Position } from "..";
import { is_white_in_check } from "../../wasm_api";

export function encodeSfen(position: Position): string {
  const board = encodeBoard(position.board);
  let hands =
    encodeHands("black", position.hands["black"]) +
    encodeHands("white", position.hands["white"]);
  if (!hands) {
    hands = "-";
  }
  const sfenBlack = [board, "b", hands].join(" ") + " 1";
  try {
    if (is_white_in_check(sfenBlack)) {
      return [board, "w", hands].join(" ") + " 1";
    }
  } catch {
    // wasm not yet initialized
  }
  return sfenBlack;
}

function encodeBoard(b: Board): string {
  let res = "";
  for (let row = 0; row < 9; row++) {
    let emptyCount = 0;
    for (let col = 8; col >= 0; col--) {
      let p = b[row][col];
      if (p) {
        if (emptyCount) {
          res += emptyCount;
        }
        emptyCount = 0;
        res += encodePiece(p);
      } else {
        emptyCount++;
      }
    }
    if (emptyCount) {
      res += emptyCount;
    }
    if (row < 8) {
      res += "/";
    }
  }
  return res;
}

function encodePiece(p: Piece): string {
  if (p === "O") {
    return "O";
  }
  let res = "";
  if (p.promoted) {
    res += "+";
  }
  res += p.color === "black" ? p.kind : p.kind.toLowerCase();
  return res;
}

const HAND_KIND: Kind[] = ["R", "B", "G", "S", "N", "L", "P"];
function encodeHands(c: Color, h: Hands): string {
  let res = "";
  for (const k of HAND_KIND) {
    if (h[k] > 1) {
      res += h[k];
    }
    if (h[k]) {
      res += encodePiece({
        color: c,
        kind: k,
        promoted: false,
      });
    }
  }
  return res;
}
