import type { Piece, Position } from ".";

// 後手玉に先手の駒が利いているか。SFEN の手番 (王手がかかっていれば後手番) を
// これで決める。
//
// Rust 側の PositionAux::checked_slow(WHITE) と同じ判定。手番は URL・SFEN 表示・
// 解図要求のすべてに使うので、wasm の読み込みを待たずに同期的に求められるよう
// TS で持っている。後手玉が複数あるときは Rust と同じく、SFEN で最後に現れる
// 玉だけを見る (Rust は SFEN の読み込み時に最後に置いた玉の位置を覚えている)。
export function isWhiteInCheck(position: Position): boolean {
  const king = whiteKing(position);
  if (!king) {
    return false;
  }
  const board = position.board;
  for (let row = 0; row < 9; row++) {
    for (let col = 0; col < 9; col++) {
      const p = board[row][col];
      if (!p || p === "O" || p.color !== "black") {
        continue;
      }
      for (const [dr, dc] of steps(p)) {
        if (row + dr === king[0] && col + dc === king[1]) {
          return true;
        }
      }
      for (const [dr, dc] of slides(p)) {
        let r = row + dr;
        let c = col + dc;
        while (0 <= r && r < 9 && 0 <= c && c < 9) {
          if (r === king[0] && c === king[1]) {
            return true;
          }
          if (board[r][c]) {
            break; // 駒か石で止まる
          }
          r += dr;
          c += dc;
        }
      }
    }
  }
  return false;
}

function whiteKing(position: Position): [number, number] | undefined {
  // SFEN の並び (一段目から、9 筋 → 1 筋) の逆順で探して最初の玉 = 最後の玉
  for (let row = 8; row >= 0; row--) {
    for (let col = 0; col < 9; col++) {
      const p = position.board[row][col];
      if (p && p !== "O" && p.color === "white" && p.kind === "K") {
        return [row, col];
      }
    }
  }
  return undefined;
}

// [段の差, 筋の差]。先手の駒なので前は段が減る向き。
type Dir = readonly [number, number];

const GOLD: Dir[] = [
  [-1, -1],
  [-1, 0],
  [-1, 1],
  [0, -1],
  [0, 1],
  [1, 0],
];
const SILVER: Dir[] = [
  [-1, -1],
  [-1, 0],
  [-1, 1],
  [1, -1],
  [1, 1],
];
const ORTH: Dir[] = [
  [-1, 0],
  [1, 0],
  [0, -1],
  [0, 1],
];
const DIAG: Dir[] = [
  [-1, -1],
  [-1, 1],
  [1, -1],
  [1, 1],
];
const KING: Dir[] = [...ORTH, ...DIAG];

// 1 マス先 (桂は 2 段先) に利く方向
function steps(p: Exclude<Piece, "O">): Dir[] {
  if (p.promoted) {
    // 馬・龍は走り (slides) に加えて玉の動き、それ以外の成駒は金
    return p.kind === "B" || p.kind === "R" ? KING : GOLD;
  }
  switch (p.kind) {
    case "P":
      return [[-1, 0]];
    case "N":
      return [
        [-2, -1],
        [-2, 1],
      ];
    case "S":
      return SILVER;
    case "G":
      return GOLD;
    case "K":
      return KING;
    default:
      return []; // 香・角・飛は slides で扱う
  }
}

// 遮られるまで利く方向
function slides(p: Exclude<Piece, "O">): Dir[] {
  switch (p.kind) {
    case "L":
      return p.promoted ? [] : [[-1, 0]];
    case "B":
      return DIAG;
    case "R":
      return ORTH;
    default:
      return [];
  }
}
