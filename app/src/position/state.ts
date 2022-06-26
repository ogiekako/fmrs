import * as types from './types';

export function newState(): types.State {
    const pieces: (types.Piece | undefined)[][] = new Array(9).fill(null).map(() => new Array(9));
    const whiteHand: types.Hand = {
        'P': 18,
        'L': 4,
        'N': 4,
        'S': 4,
        'G': 4,
        'B': 2,
        'R': 2,
        'K': 2,
    };
    return {
        board: {
            pieces,
            blackHand: emptyHand(),
            whiteHand,
        },
        selected: [4, 4],
    }
}

export function emptyHand(): types.Hand {
    return {
        'P': 0,
        'L': 0,
        'N': 0,
        'S': 0,
        'G': 0,
        'B': 0,
        'R': 0,
        'K': 0,
    }
}
