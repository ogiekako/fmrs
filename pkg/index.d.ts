/* tslint:disable */
/* eslint-disable */
export function greet(): void;
export enum Algorithm {
  Standard = 0,
  Parallel = 1,
}
export class Solver {
  private constructor();
  free(): void;
  static new(problem_sfen: string, solutions_upto: number, algo: Algorithm): Solver;
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
}
