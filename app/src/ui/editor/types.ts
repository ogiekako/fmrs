import { Color, Kind, Position } from "../../model"

export type State = {
    position: Position,
    selected: Selected | undefined,
    solving: boolean,
}

export type Selected = {
    ty: 'hand'
    color: Color
    kind: Kind
} | {
    ty: 'board'
    pos: [number, number] // zero-origin
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
    solving: boolean,
}

export type Dispatcher = (event: Event) => void
