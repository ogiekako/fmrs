import * as types from './types';
import * as model from '../../model';

export function cloneState(state: types.State): types.State {
    return {
        position: model.clonePosition(state.position),
        selected: cloneSelected(state.selected),
    }
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

