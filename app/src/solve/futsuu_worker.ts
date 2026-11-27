// 普通詰将棋 (futsuu) の wasm 解図 Worker。
import type {
  FutsuuPerfectionInfo,
  FutsuuPerfectionStatus,
  FutsuuResponse,
} from ".";
import type { FutsuuSolveRequest } from "./wasm_futsuu_solver";
import { serveWorkerTask } from "./worker_task";

const wasm = import("../wasm_api");

/** 進捗を送る間隔。頻繁に送るとメインスレッドの再描画が追いつかない。 */
const PROGRESS_INTERVAL_MS = 50;

serveWorkerTask<FutsuuSolveRequest, number, FutsuuResponse>(
  async ({ sfen, checkPerfection }, postNodes) => {
    const { solve_futsuu } = await wasm;
    let lastPost = 0;
    const res = solve_futsuu(sfen, 64, checkPerfection, (nodes: number) => {
      const now = Date.now();
      if (now - lastPost >= PROGRESS_INTERVAL_MS) {
        postNodes(nodes);
        lastPost = now;
      }
    });
    try {
      if (res.is_mate()) {
        const rawStatus = res.perfection_status();
        const perfection: FutsuuPerfectionInfo | undefined = rawStatus
          ? {
              status: rawStatus as FutsuuPerfectionStatus,
              shortcutPly: res.shortcut_ply() || undefined,
              shortcutTotalSteps: res.shortcut_total_steps() || undefined,
              shortcutMoveKif: res.shortcut_move_kif() || undefined,
              intendedMoveKif: res.intended_move_kif() || undefined,
              minorFlawCount: res.minor_flaw_count(),
            }
          : undefined;
        return {
          ty: "mate",
          steps: res.steps(),
          redundant: res.redundant(),
          nodes: res.nodes(),
          kif: res.kif(),
          sfen: res.sfen(),
          perfection,
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
    } finally {
      res.free();
    }
  }
);
