import { Board, emptyHands, Hands, Kind, Piece, Position } from "..";

export function decodeSfen(sfen: string): Position {
  const [boardStr, , handsStr] = sfen.split(" ");
  const board = decodeBoard(boardStr);
  const hands = decodeHands(handsStr);
  return {
    board,
    hands,
  };
}

function decodeBoard(s: string): Board {
  const rows = s.split("/");
  if (rows.length !== 9) {
    throw new Error(`Invalid number of rows`);
  }
  const res = [];
  for (let i = 0; i < 9; i++) {
    res.push(decodeRow(rows[i]));
  }
  return res;
}

function decodeRow(s: string): (Piece | undefined)[] {
  const res = Array(9);
  let col = 8;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if ("0" <= c && c <= "9") {
      col -= parseInt(c, 10);
      continue;
    }
    let promoted = false;
    if (c === "+") {
      promoted = true;
      i++;
    }
    res[col--] = decodePiece(s[i], promoted);
  }
  return res;
}

function decodePiece(c: string, promoted: boolean): Piece {
  const color = "A" <= c && c <= "Z" ? "black" : "white";
  let kind: Kind;
  switch (c.toUpperCase()) {
    case "P": {
      kind = "P";
      break;
    }
    case "L": {
      kind = "L";
      break;
    }
    case "N": {
      kind = "N";
      break;
    }
    case "S": {
      kind = "S";
      break;
    }
    case "G": {
      kind = "G";
      break;
    }
    case "B": {
      kind = "B";
      break;
    }
    case "R": {
      kind = "R";
      break;
    }
    case "K": {
      kind = "K";
      break;
    }
    default:
      throw new Error(`Unknown piece ${c}`);
  }
  return {
    color,
    kind,
    promoted,
  };
}

function decodeHands(s: string): { black: Hands; white: Hands } {
  if (s === "-") {
    return { black: emptyHands(), white: emptyHands() };
  }
  let black = emptyHands();
  let white = emptyHands();
  for (let i = 0; i < s.length; i++) {
    let n = 0;
    while ("0" <= s[i] && s[i] <= "9") {
      n = n * 10 + parseInt(s[i], 10);
      i++;
    }
    if (n === 0) {
      n = 1;
    }
    let piece = decodePiece(s[i], false);
    if (piece.color === "black") {
      black[piece.kind] = n;
    } else {
      white[piece.kind] = n;
    }
  }
  return { black, white };
}
