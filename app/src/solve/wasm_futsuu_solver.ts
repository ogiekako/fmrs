import { CancellationToken, FutsuuResponse } from ".";
import { WarmWorker } from "./worker_task";

export type FutsuuSolveRequest = {
  sfen: string;
  checkPerfection: boolean;
};

const futsuuWorker = new WarmWorker(
  () => new Worker(new URL("./futsuu_worker.ts", import.meta.url))
);

export function warmFutsuuWorker() {
  futsuuWorker.warm();
}

export function disposeFutsuuWorker() {
  futsuuWorker.dispose();
}

/**
 * wasm の普通詰将棋ソルバーを Web Worker で実行する (futsuu_worker.ts)。
 * キャンセルされたらその時点で Worker を止めて undefined を返す。
 */
export async function solveFutsuuWasm(
  sfen: string,
  cancel: CancellationToken,
  onNodes: (nodes: number) => void,
  checkPerfection: boolean = false
): Promise<FutsuuResponse | undefined> {
  try {
    return await futsuuWorker.run<
      FutsuuSolveRequest,
      number,
      FutsuuResponse | undefined
    >({ sfen, checkPerfection }, cancel, onNodes, () => undefined);
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
    message.includes("受方玉にすでに王手がかかっています") ||
    message.includes("普通詰将棋では石") ||
    message.includes("初形が不正です") ||
    message.includes("局面の読み込みに失敗しました") ||
    message.includes("オフラインのため")
  ) {
    return message;
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
