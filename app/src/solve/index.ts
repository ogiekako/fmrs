import * as model from "../model";
import { getPrecomputedSolveResponseBySfen } from "../ogiekako_problems";
import { ServerUnavailableError, solveServer } from "./server_solver";
import { solveFutsuuServer } from "./server_futsuu_solver";
import {
  disposeSolveWorker,
  solveWasm,
  warmSolveWorker,
} from "./wasm_solver";
import {
  disposeFutsuuWorker,
  solveFutsuuWasm,
  warmFutsuuWorker,
} from "./wasm_futsuu_solver";

/**
 * 現在開いているモードの WASM Worker を先読みし、もう一方の待機 Worker を解放する。
 * ページ表示後にオフラインになってもブラウザ内解図を開始できるようにする。
 */
export function warmWorkerForMode(mode: model.SolverMode) {
  if (mode === "futsuu") {
    disposeSolveWorker();
    warmFutsuuWorker();
  } else {
    disposeFutsuuWorker();
    warmSolveWorker();
  }
}

export class CancellationToken {
  private canceled = false;
  private listeners: Array<() => void> = [];
  constructor() {}
  cancel() {
    if (this.canceled) {
      return;
    }
    this.canceled = true;
    for (const listener of this.listeners.splice(0)) {
      listener();
    }
  }
  isCanceled(): boolean {
    return this.canceled;
  }
  /** キャンセル時に一度だけ呼ばれる。返り値の関数で登録を解除する。 */
  onCancel(listener: () => void): () => void {
    if (this.canceled) {
      listener();
      return () => {};
    }
    this.listeners.push(listener);
    return () => {
      this.listeners = this.listeners.filter((l) => l !== listener);
    };
  }
}

export enum Algorithm {
  Wasm,
  Server,
}

declare const FMRS_API_BASE_URL: string;

const API_BASE_URL = resolveApiBaseUrl();
const ALIVE_URL = apiUrl("/fmrs_alive");

/**
 * サーバーの応答をここまで待ち、超えたら wasm に切り替える。
 * 疎通確認と /solve のヘッダ受信で共有する予算なので、
 * 「Solve を押してから探索が始まるまで」がおおよそこの時間で頭打ちになる。
 */
const SERVER_RESPONSE_BUDGET_MS = 2000;
/** ローカル開発時はフォールバックできない (エラーになる) ので長めに待つ。 */
const LOCAL_SERVER_RESPONSE_BUDGET_MS = 10000;

export async function isServerAvailable(
  timeoutMs: number = SERVER_RESPONSE_BUDGET_MS
): Promise<boolean> {
  if (timeoutMs <= 0) {
    return false;
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const resp = await fetch(ALIVE_URL, {
      cache: "no-store",
      signal: controller.signal,
    });
    return resp.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

export type Response = {
  redundant: boolean;
  solutions: number;
  // 既知作品 (knownProblem) では持たず、表示時に生成する (ogiekakoProblemKif)。
  kif?: string;
  sfen: string;
  fromWhite: boolean;
  knownProblem?: boolean;
  /** 普通詰将棋の詰手数（協力詰では undefined）。 */
  futsuuSteps?: number;
  /** 普通詰将棋の探索ノード数（協力詰では undefined）。 */
  futsuuNodes?: number;
};

export type FutsuuResponse =
  | {
      ty: "mate";
      steps: number;
      redundant: boolean;
      nodes: number;
      kif: string;
      sfen: string;
    }
  | {
      ty: "no-mate";
      nodes: number;
    }
  | {
      ty: "suspended";
      nodes: number;
    };

export async function solveFutsuu(
  position: model.Position,
  cancelToken: CancellationToken,
  onNodes: (nodes: number) => void,
  onFallback?: () => void,
  useServer: boolean = true
): Promise<FutsuuResponse | undefined> {
  const sfen = model.encodeSfen(position);
  if (!useServer) {
    return await solveFutsuuWasm(sfen, cancelToken, onNodes);
  }
  const requireServer = isLocalDevServerBackedPage();
  const deadline =
    Date.now() +
    (requireServer
      ? LOCAL_SERVER_RESPONSE_BUDGET_MS
      : SERVER_RESPONSE_BUDGET_MS);
  if (await isServerAvailable(deadline - Date.now())) {
    try {
      return await solveFutsuuServer(
        sfen,
        cancelToken,
        onNodes,
        deadline - Date.now()
      );
    } catch (e) {
      if (!(e instanceof ServerUnavailableError)) {
        throw e;
      }
      if (requireServer) {
        throw e;
      }
      console.warn("server futsuu solve unavailable, falling back to wasm", e);
      onFallback?.();
    }
  } else if (requireServer) {
    throw new Error(
      "ローカル解図サーバーに接続できませんでした。npm run dev を起動し直してください。"
    );
  } else {
    onFallback?.();
  }
  return await solveFutsuuWasm(sfen, cancelToken, onNodes);
}

export async function solve(
  position: model.Position,
  n: number,
  cancelToken: CancellationToken,
  onStep: (step: number) => void,
  onFallback?: () => void,
  useServer: boolean = true
): Promise<Response | undefined> {
  const sfen = model.encodeSfen(position);
  if (n >= 1) {
    const precomputed = getPrecomputedSolveResponseBySfen(sfen);
    if (precomputed) {
      return precomputed;
    }
  }
  if (!useServer) {
    return await solveWasm(sfen, n, cancelToken, onStep);
  }
  const requireServer = isLocalDevServerBackedPage();
  const deadline =
    Date.now() +
    (requireServer ? LOCAL_SERVER_RESPONSE_BUDGET_MS : SERVER_RESPONSE_BUDGET_MS);
  if (await isServerAvailable(deadline - Date.now())) {
    try {
      return await solveServer(
        sfen,
        n,
        cancelToken,
        onStep,
        deadline - Date.now()
      );
    } catch (e) {
      if (!(e instanceof ServerUnavailableError)) {
        throw e;
      }
      if (requireServer) {
        throw e;
      }
      // 進捗が出たあとに切れた場合は step 0 からやり直しになる。
      console.warn("server solve unavailable, falling back to wasm", e);
      onFallback?.();
    }
  } else if (requireServer) {
    throw new Error(
      "ローカル解図サーバーに接続できませんでした。npm run dev を起動し直してください。"
    );
  } else {
    onFallback?.();
  }
  return await solveWasm(sfen, n, cancelToken, onStep);
}

function isLocalDevServerBackedPage(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return (
    window.location.port === "3000" &&
    (window.location.hostname === "localhost" ||
      window.location.hostname === "127.0.0.1")
  );
}

export function apiUrl(path: string): string {
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  return `${API_BASE_URL}${normalizedPath}`;
}

function resolveApiBaseUrl(): string {
  const configured = FMRS_API_BASE_URL.trim();
  if (configured) {
    return configured.replace(/\/+$/, "");
  }
  return "";
}
