import * as model from "../model";
import { solveServer } from "./server_solver";
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

const ALIVE_URL = "/fmrs_alive";
export async function isServerAvailable(): Promise<boolean> {
  try {
    const resp = await fetch(ALIVE_URL, { cache: "no-store" });
    return resp.ok;
  } catch {
    return false;
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
  if (await isServerAvailable()) {
    try {
      return await solveServer(sfen, n, cancelToken, onStep);
    } catch (e) {
      if (e instanceof Error && e.message === "サーバーに接続できませんでした。") {
        if (requireServer) {
          throw e;
        }
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
