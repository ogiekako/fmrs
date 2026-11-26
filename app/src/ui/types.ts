import { Color, Kind, Position, SolverMode } from "../model";
import * as solve from "../solve";

export type State = {
  mode: SolverMode;
  position: Position;
  selected: Selected;
  solving: Solving | undefined;
  problems: Array<Problem>;
  solveResponse: SolveResponse | undefined;
  solutionLimit: number;
  oneWayMateMode: boolean;
  useServer: boolean;
};

export type Problem = [Position, /* name */ string];

export type Selected = {
  shown: boolean;
} & (
  | {
      ty: "hand";
      color: Color | "pieceBox";
      kind: Kind | undefined;
    }
  | {
      ty: "board";
      pos: [number, number]; // zero-origin
      typed: boolean;
    }
);

export type Solving = {
  cancelToken: solve.CancellationToken;
  step: number;
  nodes?: number;
  sfen?: string;
  /** サーバーが使えず wasm に切り替えた場合に true。 */
  fallback?: boolean;
};

export type SolveResponse = { millis: number; fallback?: boolean } & (
  | {
      ty: "solved";
      response: solve.Response;
      stone: boolean[][];
    }
  | {
      ty: "no-solution";
      nodes?: number;
    }
  | {
      ty: "suspended";
      nodes?: number;
    }
  | {
      ty: "error";
      message: string;
    }
);

export type ClickHandEvent = {
  ty: "click-hand";
  color: Color | "pieceBox";
  kind: Kind | undefined;
};

export type ClickBoardEvent = {
  ty: "click-board";
  pos: [number, number];
};

export type Event =
  | ClickHandEvent
  | ClickBoardEvent
  | {
      ty: "right-click-board";
      pos: [number, number];
    }
  | {
      ty: "set-position";
      position: Position;
    }
  | {
      ty: "set-solving";
      solving: Solving | undefined;
    }
  | {
      ty: "set-problems";
      problems: Array<Problem>;
    }
  | {
      ty: "set-solve-response";
      response: SolveResponse | undefined;
    }
  | {
      ty: "key-down";
      key: string;
    }
  | {
      ty: "set-solution-limit";
      n: number;
    }
  | {
      ty: "set-one-way-mate-mode";
      oneWayMateMode: boolean;
    }
  | {
      ty: "set-use-server";
      useServer: boolean;
    }
  | {
      ty: "set-mode";
      mode: SolverMode;
      position?: Position;
    }
  | {
      ty: "shift";
      dir: "up" | "down" | "left" | "right";
    };

export type Dispatcher = (event: Event) => void;
