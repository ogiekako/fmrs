/* tslint:disable */
/* eslint-disable */
export function greet(): void;
export enum Algorithm {
  Standard = 0,
  Parallel = 1,
}
export class BackwardSearch {
  free(): void;
  constructor(sfen: string);
  advance(): boolean;
  step(): number;
  sfen(): string;
}
export class Solver {
  free(): void;
  constructor(problem_sfen: string, solutions_upto: number, algo: Algorithm);
  /**
   * Returns non-empty string in case of an error.
   */
  advance(): number;
  no_solution(): boolean;
  solutions_found(): boolean;
  /**
   * Newline-delimited sfen moves
   */
  solutions_sfen(): string;
  solutions_kif(): string;
  solutions_count(): number;
  redundant(): boolean;
  is_from_white(): boolean;
}
