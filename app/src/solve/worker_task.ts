// 重い wasm 計算を Web Worker で 1 回だけ実行するための共通部品。
// wasm の 1 呼び出しが数秒〜数十秒かかることがあり、メインスレッドで回すと
// 画面も Cancel も止まるため。Worker 側は serveWorkerTask、メインスレッド側は
// runWorkerTask を使う。Worker は 1 タスクごとに作って捨てるので、wasm が
// 確保したメモリも panic 後の壊れた状態も、終了時にまとめて破棄される。
//
// Worker からも import されるので、型以外の重い依存 (wasm など) を持たないこと。
import type { CancellationToken } from ".";

export type WorkerTaskMessage<S, R> =
  | { ty: "step"; step: S }
  | { ty: "done"; result: R }
  | { ty: "error"; message: string };

/**
 * メインスレッド側: worker に request を送り、結果を待つ。
 * キャンセルされたらその時点で worker を止め、onCanceled() の値を返す。
 * 終了時 (完了・キャンセル・エラー) には必ず worker を terminate する。
 *
 * @throws Error worker 内で例外が起きた場合 (message はその例外のもの)。
 */
export async function runWorkerTask<Req, S, R>(
  worker: Worker,
  request: Req,
  cancel: CancellationToken,
  onStep: (step: S) => void,
  onCanceled: () => R
): Promise<R> {
  let unsubscribe = () => {};
  try {
    return await new Promise<R>((resolve, reject) => {
      unsubscribe = cancel.onCancel(() => resolve(onCanceled()));
      worker.onmessage = (event: MessageEvent<WorkerTaskMessage<S, R>>) => {
        const message = event.data;
        switch (message.ty) {
          case "step":
            onStep(message.step);
            return;
          case "done":
            resolve(message.result);
            return;
          case "error":
            reject(new Error(message.message));
            return;
        }
      };
      worker.onerror = (event) => {
        event.preventDefault();
        reject(new Error(event.message || "Worker の起動に失敗しました。"));
      };
      worker.postMessage(request);
    });
  } finally {
    unsubscribe();
    // キャンセル後に届いた step で状態が書き換わらないようにする。
    worker.onmessage = null;
    worker.onerror = null;
    worker.terminate();
  }
}

/**
 * Worker 側: 要求を受け取って handler で処理し、結果か例外を返す。
 * worker のトップレベルで同期的に呼ぶこと (要求の取りこぼしを防ぐため)。
 * wasm は handler の中で動的 import して待つ。
 */
export function serveWorkerTask<Req, S, R>(
  handler: (request: Req, postStep: (step: S) => void) => Promise<R>
) {
  const scope = self as unknown as {
    onmessage: ((event: MessageEvent<Req>) => void) | null;
    postMessage(message: WorkerTaskMessage<S, R>): void;
  };
  scope.onmessage = async (event) => {
    try {
      const result = await handler(event.data, (step) =>
        scope.postMessage({ ty: "step", step })
      );
      scope.postMessage({ ty: "done", result });
    } catch (e) {
      scope.postMessage({
        ty: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  };
}
