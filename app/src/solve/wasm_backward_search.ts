import { CancellationToken } from ".";
import { runWorkerTask } from "./worker_task";

export type BackwardSearchRequest = {
  sfen: string;
  oneWayMateMode: boolean;
};

export type BackwardSearchStep = {
  step: number;
  /** 現在の局面 (後手番のこともある) */
  sfen: string;
  /** これまでに到達した先手番局面 */
  lastBlackSfen: string;
};

/** 自動逆算が途中で失敗したことを表す。lastSfen はそれまでに到達した先手番局面。 */
export class BackwardSearchError extends Error {
  constructor(
    readonly lastSfen: string,
    readonly original: unknown
  ) {
    super(
      "自動逆算中にエラーが発生しました" +
        (original instanceof Error && original.message
          ? `: ${original.message}`
          : "。")
    );
    this.name = "BackwardSearchError";
  }
}

/**
 * 自動逆算を Web Worker で実行する (backward_search_worker.ts)。
 * キャンセルされたらその時点で Worker を止め、到達済みの先手番局面を返す。
 *
 * @returns 到達した先手番局面。一本道モードで一本道でなければ undefined。
 * @throws BackwardSearchError if the search fails midway.
 */
export async function backwardSearchWasm(
  sfen: string,
  cancel: CancellationToken,
  oneWayMateMode: boolean,
  onStep: (step: number, sfen: string) => void
): Promise<string | undefined> {
  let lastBlackSfen = sfen;
  try {
    return await runWorkerTask<
      BackwardSearchRequest,
      BackwardSearchStep,
      string | undefined
    >(
      new Worker(new URL("./backward_search_worker.ts", import.meta.url)),
      { sfen, oneWayMateMode },
      cancel,
      (s) => {
        lastBlackSfen = s.lastBlackSfen;
        onStep(s.step, s.sfen);
      },
      () => lastBlackSfen
    );
  } catch (e) {
    throw new BackwardSearchError(lastBlackSfen, e);
  }
}
