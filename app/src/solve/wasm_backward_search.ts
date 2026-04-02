import { CancellationToken } from ".";
import { BackwardSearch } from "../../../docs/pkg";

/**
 * @returns solutions or undefined if solution is not found.
 */
export async function backwardSearchWasm(
  sfen: string,
  cancel: CancellationToken,
  oneWayMateMode: boolean,
  onStep: (step: number, sfen: string) => void
): Promise<string | undefined> {
  const bs = new BackwardSearch(sfen, oneWayMateMode);

  try {
    while (bs.advance()) {
      if (cancel.isCanceled()) {
        break;
      }
      onStep(bs.step(), bs.sfen());
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    return bs.sfen();
  } finally {
    bs.free();
  }
}
