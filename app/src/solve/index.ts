import * as model from "../model";
import { ServerUnavailableError, solveServer } from "./server_solver";
import { solveWasm } from "./wasm_solver";

export class CancellationToken {
  private canceled = false;
  constructor() {}
  cancel() {
    this.canceled = true;
  }
  isCanceled(): boolean {
    return this.canceled;
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
  kif: string;
  sfen: string;
  fromWhite: boolean;
};

export async function solve(
  position: model.Position,
  n: number,
  cancelToken: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  const sfen = model.encodeSfen(position);
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
      if (e instanceof ServerUnavailableError) {
        if (requireServer) {
          throw e;
        }
        // 進捗が出たあとに切れた場合は step 0 からやり直しになる。
        console.warn("server solve unavailable, falling back to wasm", e);
      } else {
        throw e;
      }
    }
  } else if (requireServer) {
    throw new Error(
      "ローカル解図サーバーに接続できませんでした。npm run dev を起動し直してください。"
    );
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
