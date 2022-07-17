import { cloneState } from '../clone';
import * as types from '../types';
import * as position from './position';
import * as model from '../../../model';

export function newState(): types.State {
    const initialPosition = position.create();
    return {
        position: initialPosition,
        selected: undefined,
        solving: false,
        problems: [
            [initialPosition, "empty"],
            [model.decodeSfen("rBK4S1/L1G2+SL1B/4L+P1N1/1nngpsp2/1grl1g1P1/1kP1n1P2/3P1P3/4+p4/P1s1P4 b P6p 1"), "jugemu_32189"]
        ],
    }
}

export function reduce(original: types.State, event: types.Event): types.State {
    if (original.solving && event.ty !== 'set-solving') {
        return original;
    }
    let mutableState;
    switch (event.ty) {
        case 'click-board':
        case 'click-hand':
            return handleClick(original, event)
        case 'right-click-board':
            return handleRightClick(original, event.pos);
        case 'set-position':
            mutableState = cloneState(original);
            mutableState.position = event.position;
            mutableState.selected = undefined;
            return mutableState;
        case 'set-solving':
            mutableState = cloneState(original);
            mutableState.solving = event.solving;
            return mutableState
        case 'set-problems':
            mutableState = cloneState(original);
            mutableState.problems = event.problems;
            return mutableState
    }
}

function handleClick(original: types.State, event: types.ClickBoardEvent | types.ClickHandEvent): types.State {
    const mutableState = cloneState(original);
    if (!original.selected) {
        if (event.ty === 'click-hand') {
            if (event.kind === undefined) {
                return mutableState;
            }
            mutableState.selected = {
                ty: 'hand',
                color: event.color,
                kind: event.kind
            };
            return mutableState;
        }
        if (original.position.board[event.pos[0]][event.pos[1]]) {
            mutableState.selected = {
                ty: 'board',
                pos: event.pos,
            };
        }
        return mutableState;
    }

    mutableState.selected = undefined;

    if (event.ty === 'click-hand') {
        if (original.selected.ty === 'hand') {
            mutableState.position.hands[original.selected.color][original.selected.kind]--;
            mutableState.position.hands[event.color][original.selected.kind]++;
            return mutableState;
        }
        const p = mutableState.position.board[original.selected.pos[0]][original.selected.pos[1]];
        if (p && p.kind !== 'K') {
            mutableState.position.hands[event.color][p.kind]++;
            mutableState.position.board[original.selected.pos[0]][original.selected.pos[1]] = undefined;
        }
        return mutableState;
    }

    const target = original.position.board[event.pos[0]][event.pos[1]];
    if (!target) {
        if (original.selected.ty === 'hand') {
            mutableState.position.hands[original.selected.color][original.selected.kind]--;
            mutableState.position.board[event.pos[0]][event.pos[1]] = {
                color: 'black',
                kind: original.selected.kind,
                promoted: false
            };
            return mutableState;
        }
        mutableState.position.board[event.pos[0]][event.pos[1]] = original.position.board[original.selected.pos[0]][original.selected.pos[1]];
        mutableState.position.board[original.selected.pos[0]][original.selected.pos[1]] = undefined;
        return mutableState;
    }
    if (target.kind === 'K') {
        return mutableState;
    }
    if (original.selected.ty === 'hand') {
        mutableState.position.hands[original.selected.color][target.kind]++;
        mutableState.position.hands[original.selected.color][original.selected.kind]--;
        mutableState.position.board[event.pos[0]][event.pos[1]] = {
            color: 'black',
            kind: original.selected.kind,
            promoted: false
        };
        return mutableState;
    }
    const from = original.position.board[original.selected.pos[0]][original.selected.pos[1]];
    if (!from) {
        return mutableState;
    }
    mutableState.position.hands[from.color][target.kind]++;
    mutableState.position.board[event.pos[0]][event.pos[1]] = from;
    mutableState.position.board[original.selected.pos[0]][original.selected.pos[1]] = undefined;
    return mutableState;
}

function handleRightClick(original: types.State, pos: [number, number]): types.State {
    const mutableState = cloneState(original);
    const mutablePiece = mutableState.position.board[pos[0]][pos[1]];
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
