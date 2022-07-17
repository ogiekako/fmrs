import { Color, Kind, Position } from "../../model"

export type State = {
    position: Position,
    selected: Selected | undefined,
}

export type Selected = {
    ty: 'hand'
    color: Color
    kind: Kind
} | {
    ty: 'board'
    pos: [number, number] // zero-origin
}

export type Event = {
    ty: 'click-hand'
    color: Color,
    kind: Kind | undefined
} | {
    ty: 'click-board'
    pos: [number, number]
} | {
    ty: 'right-click-board',
    pos: [number, number]
}
