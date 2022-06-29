import * as model from '../model';
import { cloneState } from './clone';
import * as types from './types';

export function newState(): types.State {
    const pieces = model.emptyBoard();
    pieces[0][4] = {
        color: 'white',
        kind: 'K',
        promoted: false,
    };
    pieces[8][4] = {
        color: 'black',
        kind: 'K',
        promoted: false,
    };
    return {
        position: {
            pieces,
            hands: {
                'black': model.emptyHands(),
                'white': model.fullHands(),
            },
        },
        selected: undefined,
    }
}

export function updatedState(original: types.State, event: types.ClickEvent): types.State {
    const mutableState = cloneState(original);
    if (!original.selected) {
        if (event.ty === 'hand') {
            if (event.kind === undefined) {
                return mutableState;
            }
            mutableState.selected = {
                ty: event.ty,
                color: event.color,
                kind: event.kind
            };
            return mutableState;
        }
        if (original.position.pieces[event.pos[0]][event.pos[1]]) {
            mutableState.selected = event;
        }
        return mutableState;
    }

    mutableState.selected = undefined;

    if (event.ty === 'hand') {
        if (original.selected.ty === 'hand') {
            mutableState.position.hands[original.selected.color][original.selected.kind]--;
            mutableState.position.hands[event.color][original.selected.kind]++;
            return mutableState;
        }
        const p = mutableState.position.pieces[original.selected.pos[0]][original.selected.pos[1]];
        if (p && p.kind !== 'K') {
            mutableState.position.hands[event.color][p.kind]++;
            mutableState.position.pieces[original.selected.pos[0]][original.selected.pos[1]] = undefined;
        }
        return mutableState;
    }

    const target = original.position.pieces[event.pos[0]][event.pos[1]];
    if (!target) {
        if (original.selected.ty === 'hand') {
            mutableState.position.hands[original.selected.color][original.selected.kind]--;
            mutableState.position.pieces[event.pos[0]][event.pos[1]] = {
                color: 'black',
                kind: original.selected.kind,
                promoted: false
            };
            return mutableState;
        }
        mutableState.position.pieces[event.pos[0]][event.pos[1]] = original.position.pieces[original.selected.pos[0]][original.selected.pos[1]];
        mutableState.position.pieces[original.selected.pos[0]][original.selected.pos[1]] = undefined;
        return mutableState;
    }
    if (target.kind === 'K') {
        return mutableState;
    }
    if (original.selected.ty === 'hand') {
        mutableState.position.hands[original.selected.color][target.kind]++;
        mutableState.position.hands[original.selected.color][original.selected.kind]--;
        mutableState.position.pieces[event.pos[0]][event.pos[1]] = {
            color: 'black',
            kind: original.selected.kind,
            promoted: false
        };
        return mutableState;
    }
    const from = original.position.pieces[original.selected.pos[0]][original.selected.pos[1]];
    if (!from) {
        return mutableState;
    }
    mutableState.position.hands[from.color][target.kind]++;
    mutableState.position.pieces[event.pos[0]][event.pos[1]] = from;
    mutableState.position.pieces[original.selected.pos[0]][original.selected.pos[1]] = undefined;
    return mutableState;
}

export function updateStateOnRightClick(original: types.State, pos: [number, number]): types.State {
    const mutableState = cloneState(original);
    const mutablePiece = mutableState.position.pieces[pos[0]][pos[1]];
    if (!mutablePiece) {
        return mutableState;
    }
    if (mutablePiece.kind === 'K') {
        return mutableState;
    }
    if (mutablePiece.kind === 'G') {
        mutablePiece.color = mutablePiece.color === 'black' ? 'white' : 'black';
        return mutableState;
    }
    if (!mutablePiece.promoted) {
        mutablePiece.promoted = true;
        return mutableState;
    }
    mutablePiece.color = mutablePiece.color === 'black' ? 'white' : 'black';
    mutablePiece.promoted = false;
    return mutableState;
}
