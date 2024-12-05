/* tslint:disable */
/* eslint-disable */
export function greet(): void;
export class JsonResponse {
  private constructor();
  free(): void;
  solutions(): number;
  jkf(): string;
}
export class Solver {
  private constructor();
  free(): void;
  static new(problem_sfen: string, solutions_upto: number): Solver;
  /**
   * Returns non-empty string in case of an error.
   */
  advance(): string;
  no_solution(): boolean;
  solutions_found(): boolean;
  /**
   * Newline-delimited sfen moves
   */
  solutions_sfen(): string;
  solutions_json(): JsonResponse;
}
