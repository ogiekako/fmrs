// SFEN の手番判定 (王手がかかっていれば後手番) は、wasm の読み込みを待たずに
// 決めるため TS (src/model/check.ts) と Rust (PositionAux::checked_slow) の
// 二重管理になっている。両者がずれないことを、Rust 実装 (wasm の Node 向けビルド)
// を正解として突き合わせて確かめる。
//
// 実行: npm run test:web (wasm を build/wasm-node にビルドしてから走らせる)
import { test } from "node:test";
import assert from "node:assert";
import * as fs from "fs";
import * as path from "path";
import { decodeSfen, encodeSfen, Position, Piece, Kind } from "../src/model";
import { PRESET_PROBLEMS } from "../src/problem";

type Oracle = { is_white_in_check(sfen: string): boolean };
// eslint-disable-next-line @typescript-eslint/no-var-requires
const wasm: Oracle = require(
  process.env.FMRS_WASM_NODE ?? path.resolve("build/wasm-node/wasm.js"),
);

// TS 版 (encodeSfen が付ける手番)
function tsTurn(position: Position): string {
  return encodeSfen(position).split(" ")[1];
}

// Rust 版。手番以外は encodeSfen の出力をそのまま使う。
function rustTurn(position: Position): string {
  const [board, , hands] = encodeSfen(position).split(" ");
  return wasm.is_white_in_check(`${board} b ${hands} 1`) ? "w" : "b";
}

// 全局面を流して、ずれた局面を最初の数件だけ報告する。
function assertAgree(label: string, positions: Iterable<Position>): number {
  let count = 0;
  const mismatches: string[] = [];
  for (const p of positions) {
    count++;
    const ts = tsTurn(p);
    const rust = rustTurn(p);
    if (
      ts !== rust &&
      mismatches.push(`${encodeSfen(p)} (TS: ${ts}, Rust: ${rust})`) >= 5
    ) {
      break;
    }
  }
  assert.deepStrictEqual(
    mismatches,
    [],
    `${label}: TS と Rust の手番判定が食い違う`,
  );
  return count;
}

const emptyHands = () => ({ P: 0, L: 0, N: 0, S: 0, G: 0, B: 0, R: 0, K: 0 });
function emptyPosition(): Position {
  return {
    board: Array.from({ length: 9 }, () => Array(9).fill(undefined)),
    hands: { black: emptyHands(), white: emptyHands() },
  };
}
function sfen(board: string): Position {
  return decodeSfen(`${board} b - 1`);
}

const KINDS: Kind[] = ["P", "L", "N", "S", "G", "B", "R", "K"];
const PROMOTABLE = new Set<Kind>(["P", "L", "N", "S", "B", "R"]);
// 成駒を含む先手の全駒種 (14 種)
const BLACK_PIECES: Piece[] = KINDS.flatMap((kind) =>
  [false, true]
    .filter((promoted) => !promoted || PROMOTABLE.has(kind))
    .map((promoted) => ({ color: "black" as const, kind, promoted })),
);
const SLIDERS: Piece[] = BLACK_PIECES.filter(
  (p) =>
    p !== "O" &&
    ((p.kind === "L" && !p.promoted) || p.kind === "B" || p.kind === "R"),
);
const WHITE_KING: Piece = { color: "white", kind: "K", promoted: false };

test("仕様例 (期待値どおりで、Rust とも一致する)", () => {
  const cases: [string, string, boolean][] = [
    ["歩", "9/4k4/4P4/9/9/9/9/9/9", true],
    ["歩は 2 マス先に利かない", "4k4/9/4P4/9/9/9/9/9/9", false],
    ["桂", "4k4/9/3N5/9/9/9/9/9/9", true],
    ["香", "4k4/9/9/9/4L4/9/9/9/9", true],
    ["香は石で止まる", "4k4/9/4O4/9/4L4/9/9/9/9", false],
    ["角", "4k4/9/9/9/B8/9/9/9/9", true],
    ["角は駒で止まる", "4k4/9/2P6/9/B8/9/9/9/9", false],
    ["角は縦に利かない", "4k4/4B4/9/9/9/9/9/9/9", false],
    ["馬は縦に 1 マス利く", "4k4/4+B4/9/9/9/9/9/9/9", true],
    ["飛は斜めに利かない", "4k4/3R5/9/9/9/9/9/9/9", false],
    ["龍は斜めに 1 マス利く", "4k4/3+R5/9/9/9/9/9/9/9", true],
    ["銀は斜め後ろに利く", "3S5/4k4/9/9/9/9/9/9/9", true],
    ["金は斜め後ろに利かない", "3G5/4k4/9/9/9/9/9/9/9", false],
    ["成銀は金と同じ", "3+S5/4k4/9/9/9/9/9/9/9", false],
    ["後手の駒では王手にならない", "9/4k4/4p4/9/9/9/9/9/9", false],
    ["後手玉が無い", "9/9/9/9/9/9/9/9/4K4", false],
    [
      "後手玉が 2 枚なら SFEN で最後の玉を見る (王手なし)",
      "9/4k4/4P4/9/9/9/9/9/4k4",
      false,
    ],
    [
      "後手玉が 2 枚なら SFEN で最後の玉を見る (王手あり)",
      "4k4/9/9/9/9/9/4k4/4P4/9",
      true,
    ],
  ];
  for (const [name, board, expected] of cases) {
    const p = sfen(board);
    assert.strictEqual(tsTurn(p), expected ? "w" : "b", `TS: ${name}`);
    assert.strictEqual(rustTurn(p), expected ? "w" : "b", `Rust: ${name}`);
  }
});

test("実在の問題の初形", () => {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (p.endsWith(".sfen") && !p.endsWith(".ans.sfen")) files.push(p);
    }
  };
  walk(path.resolve("rust/problems"));
  const positions = [
    ...files.map((f) => decodeSfen(fs.readFileSync(f, "utf8"))),
    ...PRESET_PROBLEMS.map(([s]) => decodeSfen(s)),
  ];
  assert.ok(positions.length >= 40, `問題数 ${positions.length}`);
  assertAgree("実在の問題", positions);
});

test("先手の駒 1 枚の利き (玉・駒種・駒の位置の全組み合わせ)", () => {
  function* gen() {
    for (let k = 0; k < 81; k++) {
      for (const piece of BLACK_PIECES) {
        for (let a = 0; a < 81; a++) {
          if (a === k) continue;
          const p = emptyPosition();
          p.board[Math.floor(k / 9)][k % 9] = WHITE_KING;
          p.board[Math.floor(a / 9)][a % 9] = piece;
          yield p;
        }
      }
    }
  }
  const n = assertAgree("駒 1 枚", gen());
  assert.strictEqual(n, 81 * BLACK_PIECES.length * 80);
});

test("走り駒と、間を遮る駒 1 枚 (石・後手の駒・先手の駒)", () => {
  const BLOCKERS: Piece[] = [
    "O",
    { color: "white", kind: "P", promoted: false },
    { color: "black", kind: "G", promoted: false },
  ];
  const KING_SQUARES = [0, 4, 40, 44, 80, 22, 58]; // 隅・端・中央付近
  function* gen() {
    for (const k of KING_SQUARES) {
      for (const slider of SLIDERS) {
        for (let a = 0; a < 81; a++) {
          if (a === k) continue;
          for (let b = 0; b < 81; b++) {
            if (b === k || b === a) continue;
            const p = emptyPosition();
            p.board[Math.floor(k / 9)][k % 9] = WHITE_KING;
            p.board[Math.floor(a / 9)][a % 9] = slider;
            p.board[Math.floor(b / 9)][b % 9] =
              BLOCKERS[(a + b) % BLOCKERS.length];
            yield p;
          }
        }
      }
    }
  }
  assertAgree("走り駒と遮る駒", gen());
});

test("ランダム局面 (駒の多い局面・玉の周りに寄せた局面・玉 0〜2 枚)", () => {
  // mulberry32 (シード固定で再現可能)
  let seed = 20261112;
  const rnd = (n: number) => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) % n;
  };
  const randomPiece = (color?: "black" | "white"): Piece => {
    if (rnd(15) === 0) return "O";
    const kind = KINDS[rnd(KINDS.length)];
    return {
      color: color ?? (rnd(2) ? "black" : "white"),
      kind,
      promoted: PROMOTABLE.has(kind) && rnd(3) === 0,
    };
  };
  function* gen() {
    for (let i = 0; i < 200000; i++) {
      const p = emptyPosition();
      const mode = rnd(4);
      if (mode === 0) {
        const n = rnd(40);
        for (let j = 0; j < n; j++) p.board[rnd(9)][rnd(9)] = randomPiece();
      } else {
        const kr = rnd(9);
        const kc = rnd(9);
        p.board[kr][kc] = WHITE_KING;
        const n = 1 + rnd(8);
        for (let j = 0; j < n; j++) {
          const r = kr + rnd(9) - 4;
          const c = kc + rnd(9) - 4;
          if (r < 0 || r > 8 || c < 0 || c > 8 || (r === kr && c === kc))
            continue;
          p.board[r][c] = randomPiece(rnd(3) ? "black" : undefined);
        }
        if (mode === 2) {
          for (let j = 0; j < 3; j++)
            p.board[rnd(9)][rnd(9)] = SLIDERS[rnd(SLIDERS.length)];
        }
        if (mode === 3 && rnd(2)) p.board[rnd(9)][rnd(9)] = WHITE_KING;
      }
      yield p;
    }
  }
  assertAgree("ランダム局面", gen());
});
