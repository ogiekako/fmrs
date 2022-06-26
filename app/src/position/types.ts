export type Color = 'Black' | 'White'
export type Kind = 'P' | 'L' | 'N' | 'S' | 'G' | 'B' | 'R' | 'K'

export type Piece = {
    color: Color,
    kind: Kind,
    promoted: boolean,
}

export type Hand = {
    [K in Kind]: number;
}

export type Board = {
    pieces: (Piece | undefined)[][],
    blackHand: Hand,
    whiteHand: Hand,
}

export type State = {
    board: Board,
    selected: [number, number], // zero-origin
}
