import { Board, emptyHands, Hands, Kind, Piece, Position } from "..";

export function extractSfenLine(sfenOrFileContent: string): string {
  for (const rawLine of sfenOrFileContent.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.length > 0 && !line.startsWith("#")) {
      return line;
    }
  }
  return sfenOrFileContent.trim();
}

export interface ParsedProblemSfen {
  id: string;
  file: string;
  sfen: string;
  title: string | null;
  author: string;
  source: string;
  publishedAt: string;
  moves: number;
  rule: string;
  defenderHand: string | null;
  awards: string[];
  url: string | null;
  notes: string | null;
  selected: boolean;
}

export function parseProblemSfen(
  id: string,
  file: string,
  content: string
): ParsedProblemSfen {
  let title: string | null = null;
  let author = "";
  let source = "";
  let publishedAt = "";
  let moves = 0;
  let rule = "協力詰";
  let defenderHand: string | null = null;
  const awards: string[] = [];
  let url: string | null = null;
  let notes: string | null = null;
  let selected = false;
  let sfen = "";

  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (line.startsWith("#")) {
      const rest = line.slice(1);
      const colonIdx = rest.indexOf(":");
      if (colonIdx === -1) continue;
      const key = rest.slice(0, colonIdx).trim();
      const val = rest.slice(colonIdx + 1).trim();
      switch (key) {
        case "作品名":
          title = val;
          break;
        case "作者":
          author = val;
          break;
        case "発表誌":
          source = val;
          break;
        case "発表年月":
          publishedAt = val;
          break;
        case "手数":
          moves = parseInt(val, 10);
          break;
        case "ルール": {
          const marker = "（受方持駒：";
          const mIdx = val.indexOf(marker);
          if (mIdx !== -1) {
            rule = val.slice(0, mIdx).trim();
            defenderHand = val
              .slice(mIdx + marker.length)
              .replace(/）$/, "")
              .trim();
          } else {
            rule = val;
          }
          break;
        }
        case "受賞":
          for (const a of val.split(" / ")) {
            if (a.trim()) awards.push(a.trim());
          }
          break;
        case "出典URL":
          url = val;
          break;
        case "備考":
          notes = val;
          break;
        case "セレクション":
          selected = val === "true";
          break;
      }
    } else if (!sfen) {
      sfen = line;
    }
  }

  return {
    id,
    file,
    sfen,
    title,
    author,
    source,
    publishedAt,
    moves,
    rule,
    defenderHand,
    awards,
    url,
    notes,
    selected,
  };
}

export function decodeSfen(sfen: string): Position {
  const cleanSfen = extractSfenLine(sfen);
  const [boardStr, , handsStr] = cleanSfen.split(" ");
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
  if (c === "O") {
    return "O";
  }
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
    if (piece === "O") {
      throw new Error("stone in hand");
    }
    if (piece.color === "black") {
      black[piece.kind] = n;
    } else {
      white[piece.kind] = n;
    }
  }
  return { black, white };
}

export function sfenEqualsModuloColor(sfen1: string, sfen2: string): boolean {
  const [board1, , hands1] = sfen1.split(" ");
  const [board2, , hands2] = sfen2.split(" ");
  return board1 === board2 && hands1 === hands2;
}
