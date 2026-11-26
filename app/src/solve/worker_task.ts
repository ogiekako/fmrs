// 重い wasm 計算を Web Worker で実行するための共通部品。
// wasm の 1 呼び出しが数秒〜数十秒かかることがあり、メインスレッドで回すと
// 画面も Cancel も止まるため。Worker 側は serveWorkerTask、メインスレッド側は
// runWorkerTask / WarmWorker を使う。
//
// Worker からも import されるので、型以外の重い依存 (wasm など) を持たないこと。
import type { CancellationToken } from ".";

export type WorkerTaskMessage<S, R> =
  | { ty: "step"; step: S }
  | { ty: "done"; result: R }
  | { ty: "error"; message: string; fatal?: boolean };

function workerErrorMessage(event: ErrorEvent): string {
  if (event.message) {
    return event.message;
  }
  if (typeof navigator !== "undefined" && !navigator.onLine) {
    return "オフラインのためブラウザ内ソルバー (Worker) を起動できませんでした。";
  }
  return "Worker の起動に失敗しました。";
}

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
        reject(new Error(workerErrorMessage(event)));
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
 * Worker を 1 個だけ先読み・待機させて再利用するラッパー。
 *
 * - ページ表示時 (オンライン時) に warm() で Worker と内部 wasm を先読みして
 *   おくことで、その後オフラインになってもネットワーク通信なしで解図できる。
 * - 正常完了または入力バリデーション等の非致命エラー後は同じ Worker を再利用する。
 * - キャンセル・onerror・WASM クラッシュ (fatal) 時は Worker を terminate して
 *   破棄し、オンラインであれば次回用に再ウォームアップする。
 */
export class WarmWorker {
  private standby: Worker | undefined;

  constructor(private readonly createWorker: () => Worker) {}

  warm() {
    if (this.standby || typeof Worker === "undefined") {
      return;
    }
    try {
      const w = this.createWorker();
      w.onerror = (event) => {
        event.preventDefault();
        if (this.standby === w) {
          this.standby = undefined;
        }
        w.onerror = null;
        w.terminate();
      };
      this.standby = w;
    } catch {
      this.standby = undefined;
    }
  }

  dispose() {
    const w = this.standby;
    this.standby = undefined;
    if (w) {
      w.onmessage = null;
      w.onerror = null;
      w.terminate();
    }
  }

  async run<Req, S, R>(
    request: Req,
    cancel: CancellationToken,
    onStep: (step: S) => void,
    onCanceled: () => R
  ): Promise<R> {
    const worker = this.standby ?? this.createWorker();
    this.standby = undefined;
    let keepAlive = false;
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
              keepAlive = true;
              resolve(message.result);
              return;
            case "error":
              keepAlive = !message.fatal;
              reject(new Error(message.message));
              return;
          }
        };
        worker.onerror = (event) => {
          event.preventDefault();
          keepAlive = false;
          reject(new Error(workerErrorMessage(event)));
        };
        worker.postMessage(request);
      });
    } finally {
      unsubscribe();
      worker.onmessage = null;
      if (keepAlive && !cancel.isCanceled() && !this.standby) {
        worker.onerror = (event) => {
          event.preventDefault();
          if (this.standby === worker) {
            this.standby = undefined;
          }
          worker.onerror = null;
          worker.terminate();
        };
        this.standby = worker;
      } else {
        worker.onerror = null;
        worker.terminate();
        if (typeof navigator === "undefined" || navigator.onLine) {
          this.warm();
        }
      }
    }
  }
}

/**
 * Rust の Result::Err(String) は wasm-bindgen が文字列として throw するため
 * 非致命 (WASM インスタンスは健全)。一方 Error / WebAssembly.RuntimeError
 * 等はパニック・OOM・import 失敗等の可能性があるため致命扱いにして破棄する。
 */
function isFatalWorkerError(error: unknown): boolean {
  return typeof error !== "string";
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
        fatal: isFatalWorkerError(e),
      });
    }
  };
}
