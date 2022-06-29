import { Color, Hands, Piece } from ".";


export type Position = {
    pieces: (Piece | undefined)[][],
    hands: { [C in Color]: Hands },
}
