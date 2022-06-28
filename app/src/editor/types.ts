export type Color = 'black' | 'white'
export type Kind = 'P' | 'L' | 'N' | 'S' | 'G' | 'B' | 'R' | 'K'

export type Piece = {
    color: Color,
    kind: Kind,
    promoted: boolean,
}

export type Hands = {
    [K in Kind]: number;
}

export type Position = {
    pieces: (Piece | undefined)[][],
    hands: { [C in Color]: Hands },
}

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

export type RightClickEvent = {
    ty: 'hand'
    color: Color,
    kind: Kind | undefined
} | {
    ty: 'board'
    pos: [number, number]
}