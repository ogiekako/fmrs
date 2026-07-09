import { CancellationToken } from ".";
import { BackwardSearch, check_one_way_mate } from "../wasm_api";

/**
 * @returns solutions or undefined if solution is not found.
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

  try {
    let lastBlackSfen = bs.sfen();
    while (bs.advance()) {
      if (cancel.isCanceled()) {
        break;
      }
      const currentStep = bs.step();
      const currentSfen = bs.sfen();
      if (currentStep === 0 || currentStep % 2 === 1) {
        lastBlackSfen = currentSfen;
      }
      onStep(currentStep, currentSfen);
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    return lastBlackSfen;
  } finally {
    bs.free();
  }
}
