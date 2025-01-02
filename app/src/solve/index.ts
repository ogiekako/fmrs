import * as model from "../model";
import { solveWasm } from "./wasm_solver";

export class CancellationToken {
  private canceled = false;
  constructor() {}
  cancel() {
    this.canceled = true;
  }
  isCanceled(): boolean {
    return this.canceled;
  }
}

export enum Algorithm {
  Wasm,
  Server,
}

const ALIVE_URL = "/fmrs_alive";
export async function isServerAvailable(): Promise<boolean> {
  const resp = await fetch(ALIVE_URL);
  return resp.ok;
}

export type Response = {
  solutions: number;
  kif: string;
};

export async function solve(
  position: model.Position,
  n: number,
  cancelToken: CancellationToken,
  onStep: (step: number) => void
): Promise<Response | undefined> {
  // TODO: use server when available
  return await solveWasm(model.encodeSfen(position), n, cancelToken, onStep);
}
