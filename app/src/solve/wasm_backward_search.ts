import { CancellationToken } from ".";

export type BackwardSearchRequest = {
  sfen: string;
  oneWayMateMode: boolean;
};

export type BackwardSearchMessage =
  | {
      ty: "step";
      step: number;
      /** 現在の局面 (後手番のこともある) */
      sfen: string;
      /** これまでに到達した先手番局面 */
      lastBlackSfen: string;
    }
  | { ty: "done"; sfen: string | undefined }
  | { ty: "error"; message: string };

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
  const worker = new Worker(
    new URL("./backward_search_worker.ts", import.meta.url)
  );
  let lastBlackSfen = sfen;
  let unsubscribe = () => {};
  try {
    return await new Promise<string | undefined>((resolve, reject) => {
      unsubscribe = cancel.onCancel(() => resolve(lastBlackSfen));
      worker.onmessage = (event: MessageEvent<BackwardSearchMessage>) => {
        const message = event.data;
        switch (message.ty) {
          case "step":
            lastBlackSfen = message.lastBlackSfen;
            onStep(message.step, message.sfen);
            return;
          case "done":
            resolve(message.sfen);
            return;
          case "error":
            reject(
              new BackwardSearchError(lastBlackSfen, new Error(message.message))
            );
            return;
        }
      };
      worker.onerror = (event) => {
        event.preventDefault();
        reject(
          new BackwardSearchError(lastBlackSfen, new Error(event.message))
        );
      };
      const request: BackwardSearchRequest = { sfen, oneWayMateMode };
      worker.postMessage(request);
    });
  } finally {
    unsubscribe();
    // キャンセル後に届いた step で盤面が書き換わらないようにする。
    worker.onmessage = null;
    worker.onerror = null;
    worker.terminate();
  }
}
