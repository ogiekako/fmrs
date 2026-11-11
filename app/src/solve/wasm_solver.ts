import { CancellationToken, Response } from ".";
import { runWorkerTask } from "./worker_task";

export type SolveRequest = {
  sfen: string;
  /** 求める解の数。n + 1 個まで数えて余詰を判定する。 */
  n: number;
};

/**
 * wasm の Solver を Web Worker で実行する (solve_worker.ts)。
 * キャンセルされたらその時点で Worker を止めて undefined を返す。
 *
 * @returns solutions or undefined if solution is not found.
 */
export async function solveWasm(
  sfen: string,
  n: number,
  cancel: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  try {
    return await runWorkerTask<SolveRequest, number, Response | undefined>(
      new Worker(new URL("./solve_worker.ts", import.meta.url)),
      { sfen, n },
      cancel,
      onStep,
      () => undefined
    );
  } catch (e) {
    console.error(e);
    throw new Error(toJapaneseErrorMessage(e));
  }
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
