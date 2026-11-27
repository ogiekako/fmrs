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
    constructor(sfen: string, one_way_mate_mode: boolean);
    sfen(): string;
    step(): number;
}

export class FutsuuSolveResult {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    intended_move_kif(): string;
    is_mate(): boolean;
    is_no_mate(): boolean;
    is_suspended(): boolean;
    kif(): string;
    minor_flaw_count(): number;
    moves_sfen(): string;
    nodes(): number;
    perfection_status(): string;
    redundant(): boolean;
    sfen(): string;
    shortcut_move_kif(): string;
    shortcut_ply(): number;
    shortcut_total_steps(): number;
    steps(): number;
}

export class OneWayMateResult {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    is_one_way: boolean;
    steps: number;
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

export function check_one_way_mate(sfen: string): OneWayMateResult | undefined;

/**
 * アプリ本体は TS 版 (app/src/model/check.ts) を使う。これはその正解として
 * テスト (app/test/check.test.ts) から呼ばれる。
 */
export function is_white_in_check(sfen: string): boolean;

export function moves_to_kif(sfen: string, moves_sfen: string): string | undefined;

export function solve_futsuu(problem_sfen: string, hash_mb: number, check_perfection?: boolean | null, on_progress?: Function | null): FutsuuSolveResult;
