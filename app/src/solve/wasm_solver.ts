import { CancellationToken, Response } from ".";
import { Algorithm, Solver } from "../../../docs/pkg";

/**
 * @returns solutions or undefined if solution is not found.
 */
export async function solveWasm(
  sfen: string,
  n: number,
  cancel: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  const solver = Solver.new(sfen, n + 1, Algorithm.Standard);
  try {
    return await solveWasmInner(solver, cancel, onStep, sfen);
  } catch (e) {
    console.error(e);
    throw e;
  } finally {
    solver.free();
  }
}

async function solveWasmInner(
  solver: Solver,
  cancel: CancellationToken,
  onStep: (step: number) => void,
  sfen: string
): Promise<Response | undefined> {
  let step = 0;
  let nextAwaitStep = nextAwait(step);
  while (!cancel.isCanceled()) {
    try {
      step = solver.advance();
    } catch (e) {
      console.error(e);
      throw e;
    }
    onStep(step);
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
    if (step >= nextAwaitStep) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      nextAwaitStep = nextAwait(step);
    }
  }
  return undefined;
}

function nextAwait(step: number) {
  if (step < 100) {
    return step + 1;
  }
  if (step < 1000) {
    return step + 10;
  }
  return step + 100;
}
