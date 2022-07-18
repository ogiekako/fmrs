import { CancellationToken } from ".";
import { Solver } from "../../../dist/pkg";

/**
 * @returns JSON string representing solutions or an empty string if solution
 * is not found or operation is canceled.
 */
export async function solveWasm(sfen: string, n: number, cancel: CancellationToken, onStep: (step: number) => void): Promise<string> {
    const solver = Solver.new(sfen, n + 1);
    try {
        const res = await solveWasmInner(solver, cancel, onStep);
        solver.free();
        return res
    } catch (e: any) {
        console.error(e)
        return ""
    }
}

async function solveWasmInner(solver: Solver, cancel: CancellationToken, onStep: (step: number) => void): Promise<string> {
    let step = 0;
    while (!cancel.isCanceled()) {
        const error = solver.advance();
        if (error) {
            console.error(error);
            return "";
        }
        onStep(++step);
        if (solver.no_solution()) {
            return ""
        }
        if (solver.solutions_found()) {
            return solver.solutions_json();
        }
        await new Promise(resolve => setTimeout(resolve, 0));
    }
    return "";
}
