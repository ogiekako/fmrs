import { Color, Kind, Position } from "../../model"
import * as solve from "../../solve"

export type State = {
    position: Position,
    selected: Selected | undefined,
    solving: Solving | undefined,
    problems: Array<Problem>,
}

export type Problem = [Position, /* name */ string];

export type Selected = {
    ty: 'hand'
    color: Color
    kind: Kind
} | {
    ty: 'board'
    pos: [number, number] // zero-origin
}

export type Solving = {
    cancelToken: solve.CancellationToken
    step: number,
}

export type ClickHandEvent = {
    ty: 'click-hand',
    color: Color,
    kind: Kind | undefined
}

export type ClickBoardEvent = {
    ty: 'click-board',
    pos: [number, number],
}

export type Event = ClickHandEvent | ClickBoardEvent | {
    ty: 'right-click-board',
    pos: [number, number]
} | {
    ty: 'set-position',
    position: Position,
} | {
    ty: 'set-solving',
    solving: Solving | undefined,
} | {
    ty: 'set-problems',
    problems: Array<Problem>,
}

export type Dispatcher = (event: Event) => void
