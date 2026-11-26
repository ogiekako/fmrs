// 普通詰将棋 (futsuu) の wasm 解図 Worker。
import type { FutsuuResponse } from ".";
import type { FutsuuSolveRequest } from "./wasm_futsuu_solver";
import { serveWorkerTask } from "./worker_task";

const wasm = import("../wasm_api");

/** 進捗を送る間隔。頻繁に送るとメインスレッドの再描画が追いつかない。 */
const PROGRESS_INTERVAL_MS = 50;

serveWorkerTask<FutsuuSolveRequest, number, FutsuuResponse>(
  async ({ sfen }, postNodes) => {
    const { solve_futsuu } = await wasm;
    let lastPost = 0;
    const res = solve_futsuu(sfen, 64, (nodes: number) => {
      const now = Date.now();
      if (now - lastPost >= PROGRESS_INTERVAL_MS) {
        postNodes(nodes);
        lastPost = now;
      }
    });
    if (res.is_mate()) {
      return {
        ty: "mate",
        steps: res.steps(),
        redundant: res.redundant(),
        nodes: res.nodes(),
        kif: res.kif(),
        sfen: res.sfen(),
      };
    }
    if (res.is_no_mate()) {
      return {
        ty: "no-mate",
        nodes: res.nodes(),
      };
    }
    return {
      ty: "suspended",
      nodes: res.nodes(),
    };
  }
);
