// Solve の wasm 解図の Worker。難しい問題では Solver.advance() 1 回 (1 手分)
// が数秒かかる。
import type { Response } from ".";
import type { SolveRequest } from "./wasm_solver";
import { serveWorkerTask } from "./worker_task";

const wasm = import("../wasm_api");

/** 進捗を送る間隔。毎手送るとメインスレッドの再描画が追いつかない。 */
const STEP_INTERVAL_MS = 50;

// 結果は解図結果。詰みがなければ undefined。
serveWorkerTask<SolveRequest, number, Response | undefined>(
  async ({ sfen, n }, postStep) => {
    const { Algorithm, Solver } = await wasm;
    // Worker ごと破棄されるので free() はしない (panic 後は free() も失敗する)。
    const solver = new Solver(sfen, n + 1, Algorithm.Standard);
    let lastPost = 0;
    for (;;) {
      const step = solver.advance();
      if (solver.no_solution()) {
        return undefined;
      }
      if (solver.solutions_found()) {
        return {
          sfen,
          redundant: solver.redundant(),
          solutions: solver.solutions_count(),
          kif: solver.solutions_kif(),
          fromWhite: solver.is_from_white(),
        };
      }
      const now = Date.now();
      if (now - lastPost >= STEP_INTERVAL_MS) {
        postStep(step);
        lastPost = now;
      }
    }
  }
);
