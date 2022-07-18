/* tslint:disable */
/* eslint-disable */
/**
*/
export function greet(): void;
/**
*/
export class Solver {
  free(): void;
/**
* @param {string} problem_sfen
* @param {number} solutions_upto
* @returns {Solver}
*/
  static new(problem_sfen: string, solutions_upto: number): Solver;
/**
* Returns non-empty string in case of an error.
* @returns {string}
*/
  advance(): string;
/**
* @returns {boolean}
*/
  no_solution(): boolean;
/**
* @returns {boolean}
*/
  solutions_found(): boolean;
/**
* Newline-delimited sfen moves
* @returns {string}
*/
  solutions_sfen(): string;
/**
* @returns {string}
*/
  solutions_json(): string;
}
