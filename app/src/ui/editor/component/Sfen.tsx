import * as model from '../../../model';
import { decode } from '../../../model/sfen/decode';
import * as types from '../types';

export default function Sfen(props: {
    position: model.Position,
    dispatch: types.Dispatcher,
}) {
    const sfen = model.sfen(props.position);
    return <div>SFEN <input type="text" value={sfen} onChange={e => {
        if (e.target.value === sfen) {
            return;
        }
        props.dispatch({
            ty: 'set-position',
            position: decode(e.target.value),
        });
    }} style={{ width: 250 }} /></div>
}