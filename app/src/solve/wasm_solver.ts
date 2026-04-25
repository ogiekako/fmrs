import { CancellationToken, Response } from ".";
import { Algorithm, Solver } from "../wasm_api";

/**
 * @returns solutions or undefined if solution is not found.
 */
export async function solveWasm(
  sfen: string,
  n: number,
  cancel: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  let solver: Solver | undefined;
  try {
    solver = new Solver(sfen, n + 1, Algorithm.Standard);
    return await solveWasmInner(solver, cancel, onStep, sfen);
  } catch (e) {
    console.error(e);
    throw new Error(toJapaneseErrorMessage(e));
  } finally {
    if (solver) {
      try {
        solver.free();
      } catch (e) {
        console.warn("failed to free solver", e);
      }
    }
  }
}

async function solveWasmInner(
  solver: Solver,
  cancel: CancellationToken,
  onStep: (step: number) => void,
  sfen: string
): Promise<Response | undefined> {
  let step = 0;
  let nextAwaitStep = nextAwait(step);
  while (!cancel.isCanceled()) {
    step = solver.advance();
    onStep(step);
    if (solver.no_solution()) {
      return undefined;
    }
    if (solver.solutions_found()) {
      return {
        sfen,
        redundant: solver.redundant(),
        solutions: solver.solutions_count(),
        kif: solver.solutions_kif(),
        fromWhite: solver.is_from_white(),
      };
    }
    if (step >= nextAwaitStep) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      nextAwaitStep = nextAwait(step);
    }
  }
  return undefined;
}

function nextAwait(step: number) {
  if (step < 100) {
    return step + 1;
  }
  if (step < 1000) {
    return step + 10;
  }
  return step + 100;
}

function toJapaneseErrorMessage(error: unknown): string {
  const message = extractErrorMessage(error);

  if (
    message.includes("memory") ||
    message.includes("overflow") ||
    message.includes("allocation") ||
    message.includes("out of bounds") ||
    message.includes("unreachable") ||
    message.includes("borrowed")
  ) {
    return "ブラウザのメモリ不足により探索を継続できませんでした。";
  }
  if (
    message.includes("両方の玉に王手がかかっています") ||
    message.includes("初形が不正です") ||
    message.includes("局面の読み込みに失敗しました")
  ) {
    return message;
  }
  if (message === "both checked") {
    return "両方の玉に王手がかかっています。";
  }
  if (message === "Illegal initial position") {
    return "初形が不正です。";
  }
  if (message === "double pawns") {
    return "初形が不正です: 二歩があります。";
  }
  if (message === "unmovable") {
    return "初形が不正です: 行きどころのない駒があります。";
  }

  return `内部エラー: ${message}`;
}

function extractErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === "string" && error.length > 0) {
    return error;
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as { message: unknown }).message === "string"
  ) {
    return (error as { message: string }).message;
  }
  return "不明なエラーが発生しました。";
}
