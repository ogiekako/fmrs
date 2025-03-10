import { Color, Kind, Position } from "../model";
import * as solve from "../solve";

export type State = {
  position: Position;
  selected: Selected;
  solving: Solving | undefined;
  problems: Array<Problem>;
  solveResponse: SolveResponse | undefined;
  solutionLimit: number;
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
};

export type SolveResponse = { millis: number } & (
  | {
      ty: "solved";
      response: solve.Response;
      stone: boolean[][];
    }
  | {
      ty: "no-solution";
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
      ty: "shift";
      dir: "up" | "down" | "left" | "right";
    };

export type Dispatcher = (event: Event) => void;
