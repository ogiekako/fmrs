/* tslint:disable */
/* eslint-disable */

export enum Algorithm {
    Standard = 0,
    Parallel = 1,
}

export class BackwardSearch {
    free(): void;
    [Symbol.dispose](): void;
    advance(): boolean;
    constructor(sfen: string);
    sfen(): string;
    step(): number;
}

export class Solver {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns non-empty string in case of an error.
     */
    advance(): number;
    is_from_white(): boolean;
    constructor(problem_sfen: string, solutions_upto: number, algo: Algorithm);
    no_solution(): boolean;
    redundant(): boolean;
    solutions_count(): number;
    solutions_found(): boolean;
    solutions_kif(): string;
    /**
     * Newline-delimited sfen moves
     */
    solutions_sfen(): string;
}

export function greet(): void;
