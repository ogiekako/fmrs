// 自動逆算を Web Worker で実行する。BackwardSearch.advance() は長手数の
// 唯一性検証で 1 回に数十秒かかることがあり、メインスレッドで回すと UI が
// 固まるため。キャンセルはメインスレッド側が Worker を terminate して行う。
import type {
  BackwardSearchMessage,
  BackwardSearchRequest,
} from "./wasm_backward_search";

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<BackwardSearchRequest>) => void) | null;
  postMessage(message: BackwardSearchMessage): void;
};

// wasm の読み込みは非同期。静的 import だとハンドラ登録前に届いた要求を
// 取りこぼし得るので、ハンドラは同期的に登録し、読み込みは中で待つ。
const wasm = import("../wasm_api");

scope.onmessage = async (event) => {
  try {
    scope.postMessage({ ty: "done", sfen: await run(event.data) });
  } catch (e) {
    scope.postMessage({
      ty: "error",
      message: e instanceof Error ? e.message : String(e),
    });
  }
};

/**
 * @returns 到達した先手番局面。一本道モードで一本道でなければ undefined。
 */
async function run({
  sfen,
  oneWayMateMode,
}: BackwardSearchRequest): Promise<string | undefined> {
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
    scope.postMessage({ ty: "step", step, sfen: currentSfen, lastBlackSfen });
    lastReportedStep = step;
  }
  return lastBlackSfen;
}
