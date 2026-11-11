// 自動逆算の Worker。BackwardSearch.advance() は長手数の唯一性検証で
// 1 回に数十秒かかることがある。
import type {
  BackwardSearchRequest,
  BackwardSearchStep,
} from "./wasm_backward_search";
import { serveWorkerTask } from "./worker_task";

const wasm = import("../wasm_api");

// 結果は到達した先手番局面。一本道モードで一本道でなければ undefined。
serveWorkerTask<BackwardSearchRequest, BackwardSearchStep, string | undefined>(
  async ({ sfen, oneWayMateMode }, postStep) => {
    const { BackwardSearch, check_one_way_mate } = await wasm;

    if (oneWayMateMode) {
      const result = check_one_way_mate(sfen);
      const isOneWay = result?.is_one_way ?? false;
      result?.free();
      if (!isOneWay) {
        return undefined;
      }
    }

    // Worker ごと破棄されるので free() はしない (panic 後は free() も失敗する)。
    const bs = new BackwardSearch(sfen, oneWayMateMode);
    let lastBlackSfen = bs.sfen();
    let lastReportedStep: number | undefined;
    while (bs.advance()) {
      const step = bs.step();
      if (step === lastReportedStep) {
        continue;
      }
      const currentSfen = bs.sfen();
      if (step === 0 || step % 2 === 1) {
        lastBlackSfen = currentSfen;
      }
      postStep({ step, sfen: currentSfen, lastBlackSfen });
      lastReportedStep = step;
    }
    return lastBlackSfen;
  }
);
