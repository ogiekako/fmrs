import * as types from './types';

export function cloneState(state: types.State): types.State {
    return {
        position: clonePosition(state.position),
        selected: cloneSelected(state.selected),
    }
}

function clonePosition(position: types.Position): types.Position {
    return {
        pieces: clonePieces(position.pieces),
        hands: {
            'black': cloneHands(position.hands['black']),
            'white': cloneHands(position.hands['white']),
        }
    }
}

function clonePieces(pieces: (types.Piece | undefined)[][]): (types.Piece | undefined)[][] {
    const pieces2 = [];
    for (const col of pieces) {
        const col2 = []
        for (const piece of col) {
            col2.push(piece ? clonePiece(piece) : undefined);
        }
        pieces2.push(col2);
    }
    return pieces2;
}

function clonePiece(piece: types.Piece): types.Piece {
    return {
        color: piece.color,
        kind: piece.kind,
        promoted: piece.promoted
    };
}

function cloneHands(hands: types.Hands): types.Hands {
    return Object.assign({}, hands)
}

function cloneSelected(selected: types.Selected | undefined): types.Selected | undefined {
    if (!selected) {
        return undefined;
    }
    return selected.ty === 'board' ? {
        ty: 'board',
        pos: [selected.pos[0], selected.pos[1]]
    } : {
        ty: 'hand',
        color: selected.color,
        kind: selected.kind
    };
}

