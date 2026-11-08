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

export function is_white_in_check(sfen: string): boolean;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_solver_free: (a: number, b: number) => void;
    readonly solver_advance: (a: number, b: number) => void;
    readonly solver_is_from_white: (a: number) => number;
    readonly solver_new: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly solver_no_solution: (a: number) => number;
    readonly solver_redundant: (a: number) => number;
    readonly solver_solutions_count: (a: number) => number;
    readonly solver_solutions_found: (a: number) => number;
    readonly solver_solutions_kif: (a: number, b: number) => void;
    readonly solver_solutions_sfen: (a: number, b: number) => void;
    readonly __wbg_get_onewaymateresult_is_one_way: (a: number) => number;
    readonly __wbg_get_onewaymateresult_steps: (a: number) => number;
    readonly __wbg_onewaymateresult_free: (a: number, b: number) => void;
    readonly __wbg_set_onewaymateresult_is_one_way: (a: number, b: number) => void;
    readonly __wbg_set_onewaymateresult_steps: (a: number, b: number) => void;
    readonly check_one_way_mate: (a: number, b: number) => number;
    readonly is_white_in_check: (a: number, b: number) => number;
    readonly __wbg_backwardsearch_free: (a: number, b: number) => void;
    readonly backwardsearch_advance: (a: number) => number;
    readonly backwardsearch_new: (a: number, b: number, c: number) => number;
    readonly backwardsearch_sfen: (a: number, b: number) => void;
    readonly backwardsearch_step: (a: number) => number;
    readonly Square_relative_file: (a: number, b: number) => number;
    readonly Square_rank: (a: number) => number;
    readonly Square_relative_rank: (a: number, b: number) => number;
    readonly Square_file: (a: number) => number;
    readonly PartialPosition_side_to_move: (a: number) => number;
    readonly Bitboard_count: (a: number) => number;
    readonly PartialPosition_piece_at: (a: number, b: number) => number;
    readonly Piece_piece_kind: (a: number) => number;
    readonly Piece_color: (a: number) => number;
    readonly PartialPosition_hand_of_a_player: (a: number, b: number, c: number) => void;
    readonly Position_initial_position: (a: number) => number;
    readonly Bitboard_is_empty: (a: number) => number;
    readonly display_single_compactmove: (a: number, b: number, c: number) => number;
    readonly display_single_compactmove_kansuji: (a: number, b: number, c: number) => number;
    readonly PartialPosition_vacant_bitboard: (a: number, b: number) => void;
    readonly Bitboard_contains: (a: number, b: number) => number;
    readonly Color_flip: (a: number) => number;
    readonly PartialPosition_player_bitboard: (a: number, b: number, c: number) => void;
    readonly PartialPosition_piece_bitboard: (a: number, b: number, c: number) => void;
    readonly PartialPosition_piece_kind_bitboard: (a: number, b: number, c: number) => void;
    readonly Hand_new: (a: number) => void;
    readonly PartialPosition_occupied_bitboard: (a: number, b: number) => void;
    readonly Position_inner: (a: number) => number;
    readonly all_drop_checks_partial: (a: number, b: number, c: number) => void;
    readonly is_in_check_partial_lite: (a: number) => number;
    readonly Square_index: (a: number) => number;
    readonly legality_normal_check: (a: number, b: number, c: number, d: number) => number;
    readonly Bitboard_bitand: (a: number, b: number, c: number) => void;
    readonly Bitboard_bitand_assign: (a: number, b: number) => void;
    readonly Bitboard_bitor: (a: number, b: number, c: number) => void;
    readonly Bitboard_bitor_assign: (a: number, b: number) => void;
    readonly Bitboard_bitxor: (a: number, b: number, c: number) => void;
    readonly Bitboard_bitxor_assign: (a: number, b: number) => void;
    readonly Bitboard_empty: (a: number) => void;
    readonly Bitboard_flip: (a: number, b: number) => void;
    readonly Bitboard_not: (a: number, b: number) => void;
    readonly Bitboard_pop: (a: number) => number;
    readonly Bitboard_single: (a: number, b: number) => void;
    readonly CompactMove_drop: (a: number, b: number) => number;
    readonly CompactMove_from: (a: number) => number;
    readonly CompactMove_is_drop: (a: number) => number;
    readonly CompactMove_is_promoting: (a: number) => number;
    readonly CompactMove_normal: (a: number, b: number, c: number) => number;
    readonly CompactMove_to: (a: number) => number;
    readonly GameResolution_from_u8_unchecked: (a: number) => number;
    readonly Hand_add: (a: number, b: number) => number;
    readonly Hand_count: (a: number, b: number) => number;
    readonly Hand_is_hand_piece: (a: number) => number;
    readonly Hand_remove: (a: number, b: number) => number;
    readonly IllegalMoveKind_from_u8_unchecked: (a: number) => number;
    readonly PieceKind_from_u8: (a: number) => number;
    readonly PieceKind_from_u8_unchecked: (a: number) => number;
    readonly PieceKind_promote: (a: number) => number;
    readonly PieceKind_unpromote: (a: number) => number;
    readonly Piece_new: (a: number, b: number) => number;
    readonly Piece_promote: (a: number) => number;
    readonly Piece_unpromote: (a: number) => number;
    readonly Square_flip: (a: number) => number;
    readonly Square_from_u8: (a: number) => number;
    readonly Square_from_u8_unchecked: (a: number) => number;
    readonly Square_new: (a: number, b: number) => number;
    readonly Square_new_relative: (a: number, b: number, c: number) => number;
    readonly Square_shift: (a: number, b: number, c: number) => number;
    readonly Game_position: (a: number) => number;
    readonly Game_resolution: (a: number) => number;
    readonly Game_resolve: (a: number, b: number) => void;
    readonly Game_unresolve: (a: number) => void;
    readonly PartialGame_position: (a: number) => number;
    readonly PartialGame_resolution: (a: number) => number;
    readonly PartialGame_resolve: (a: number, b: number) => void;
    readonly PartialGame_unresolve: (a: number) => void;
    readonly PartialPosition_last_compact_move: (a: number) => number;
    readonly PartialPosition_make_compact_move: (a: number, b: number) => number;
    readonly PartialPosition_ply: (a: number) => number;
    readonly PartialPosition_startpos: (a: number) => void;
    readonly PartialPosition_to_sfen_c: (a: number, b: number) => void;
    readonly Position_destruct: (a: number) => void;
    readonly Position_hand_of_a_player: (a: number, b: number, c: number) => void;
    readonly Position_last_compact_move: (a: number) => number;
    readonly Position_make_compact_move: (a: number, b: number) => number;
    readonly Position_occupied_bitboard: (a: number, b: number) => void;
    readonly Position_piece_at: (a: number, b: number) => number;
    readonly Position_piece_bitboard: (a: number, b: number, c: number) => void;
    readonly Position_player_bitboard: (a: number, b: number, c: number) => void;
    readonly Position_ply: (a: number) => number;
    readonly Position_side_to_move: (a: number) => number;
    readonly Position_startpos: () => number;
    readonly Position_to_sfen_c: (a: number, b: number) => void;
    readonly Position_vacant_bitboard: (a: number, b: number) => void;
    readonly __wbindgen_export: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export2: (a: number, b: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
