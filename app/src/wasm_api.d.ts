declare module "../../docs/pkg" {
  export const Algorithm: typeof import("../../docs/pkg").Algorithm;
  export const BackwardSearch: typeof import("../../docs/pkg").BackwardSearch;
  export const FutsuuSolveResult: typeof import("../../docs/pkg").FutsuuSolveResult;
  export const Solver: typeof import("../../docs/pkg").Solver;
  export const check_one_way_mate: typeof import("../../docs/pkg").check_one_way_mate;
  export const moves_to_kif: (sfen: string, moves_sfen: string) => string | undefined;
  export const solve_futsuu: typeof import("../../docs/pkg").solve_futsuu;
}
