import { CancellationToken, Response } from ".";
import { Algorithm, JsonResponse, Solver } from "../../../docs/pkg";

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
  let response;
  try {
    response = await solveWasmInner(solver, cancel, onStep);
  } catch (e) {
    console.error(e);
    throw e;
  } finally {
    solver.free();
  }
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
    let delta;
    try {
      delta = solver.advance();
    } catch (e) {
      console.error(e);
      throw e;
    }
    step += delta;
    onStep(step);
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
