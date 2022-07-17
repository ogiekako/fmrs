import * as model from '../../model';
import Board from './Board';
import Hands from './Hands';
import * as types from './types';

export function Position(props: {
    position: model.Position,
    selected: types.Selected | undefined,
    dispatch: (event: types.Event) => void
}) {
    let boardSelected = undefined;
    let whiteHandSelected = undefined;
    let blackHandSelected = undefined;
    if (props.selected) {
        if (props.selected.ty === 'board') {
            boardSelected = props.selected.pos;
        } else if (props.selected.color === 'white') {
            whiteHandSelected = props.selected.kind
        } else {
            blackHandSelected = props.selected.kind
        }
    }

    return <div>
        <Hands
            hands={props.position.hands['white']}
            selected={whiteHandSelected}
            onClick={kind => props.dispatch({ ty: 'click-hand', color: 'white', kind })} />
        <Board
            pieces={props.position.board}
            selected={boardSelected}
            onClick={pos => props.dispatch({ ty: 'click-board', pos })}
            onRightClick={pos => props.dispatch({ ty: 'right-click-board', pos })} />
        <Hands
            hands={props.position.hands['black']}
            selected={blackHandSelected}
            onClick={kind => props.dispatch({ ty: 'click-hand', color: 'black', kind })} />
    </div>
}