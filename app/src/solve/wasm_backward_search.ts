import { CancellationToken } from ".";
import { BackwardSearch, check_one_way_mate } from "../wasm_api";

/** 自動逆算が途中で失敗したことを表す。lastSfen はそれまでに到達した先手番局面。 */
export class BackwardSearchError extends Error {
  constructor(
    readonly lastSfen: string,
    readonly original: unknown
  ) {
    super(
      "自動逆算中にエラーが発生しました" +
        (original instanceof Error && original.message
          ? `: ${original.message}`
          : "。")
    );
    this.name = "BackwardSearchError";
  }
}

/**
 * @returns solutions or undefined if solution is not found.
 * @throws BackwardSearchError if the search fails midway.
 */
export async function backwardSearchWasm(
  sfen: string,
  cancel: CancellationToken,
  oneWayMateMode: boolean,
  onStep: (step: number, sfen: string) => void
): Promise<string | undefined> {
  if (oneWayMateMode) {
    const result = check_one_way_mate(sfen);
    try {
      if (!result?.is_one_way) {
        return undefined;
      }
    } finally {
      result?.free();
    }
  }

  const bs = new BackwardSearch(sfen, oneWayMateMode);

  let lastBlackSfen = sfen;
  try {
    lastBlackSfen = bs.sfen();
    let lastReportedStep: number | undefined;
    while (bs.advance()) {
      if (cancel.isCanceled()) {
        break;
      }
      const currentStep = bs.step();
      if (currentStep !== lastReportedStep) {
        const currentSfen = bs.sfen();
        if (currentStep === 0 || currentStep % 2 === 1) {
          lastBlackSfen = currentSfen;
        }
        onStep(currentStep, currentSfen);
        lastReportedStep = currentStep;
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    return lastBlackSfen;
  } catch (e) {
    throw new BackwardSearchError(lastBlackSfen, e);
  } finally {
    // Rust 側が panic して trap すると借用フラグが立ったまま残り、free() も
    // "attempted to take ownership of Rust value while it was borrowed" で
    // 失敗する。それで元のエラーが隠れないよう、free の失敗は警告に留める。
    try {
      bs.free();
    } catch (e) {
      console.warn("failed to free backward search", e);
    }
  }
}
