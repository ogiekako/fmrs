import { CancellationToken, Response } from ".";
import { JsonResponse, Solver } from "../../../docs/pkg";

/**
 * @returns solutions or undefined if solution is not found.
 */
export async function solveWasm(
  sfen: string,
  n: number,
  cancel: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  const solver = Solver.new(sfen, n + 1);
  const response = await solveWasmInner(solver, cancel, onStep);
  solver.free();
  return response && { solutions: response.solutions(), kif: response.kif() };
}

async function solveWasmInner(
  solver: Solver,
  cancel: CancellationToken,
  onStep: (step: number) => void
): Promise<JsonResponse | undefined> {
  let step = 0;
  let nextAwaitStep = nextAwait(step);
  while (!cancel.isCanceled()) {
    const error = solver.advance();
    if (error) {
      console.error(error);
      throw new Error(error);
    }
    onStep(++step);
    if (solver.no_solution()) {
      return undefined;
    }
    if (solver.solutions_found()) {
      return solver.solutions_json();
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
