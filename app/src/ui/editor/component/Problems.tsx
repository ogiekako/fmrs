import { Button, CloseButton, Dropdown } from 'react-bootstrap';
import DropdownItem from 'react-bootstrap/esm/DropdownItem';
import DropdownMenu from 'react-bootstrap/esm/DropdownMenu';
import * as model from '../../../model';
import * as types from '../types';

export default function Problems(props: {
    position: model.Position,
    problems: Array<types.Problem>,
    dispatch: types.Dispatcher,
    disabled: boolean
}) {
    return <DropdownMenu show>
        <Dropdown.Header>Saved positions <Button variant="secondary" onClick={
            () => {
                const inputName = window.prompt('Name the position');
                if (inputName === null) {
                    return;
                }
                const name = inputName || model.encodeSfen(props.position);
                const mutableProblems = props.problems.slice();
                mutableProblems.push([props.position, name]);
                props.dispatch({
                    ty: 'set-problems',
                    problems: mutableProblems,
                })
            }
        }>+</Button></Dropdown.Header>
        {
            props.problems.map(([position, name], i) => <DropdownItem key={i} onClick={
                () => props.dispatch({
                    ty: 'set-position',
                    position,
                })
            }>
                <div className="d-flex justify-content-between">
                    <span className={props.disabled ? "text-muted" : ""}>{name}</span>
                    <CloseButton onClick={e => {
                        e.stopPropagation();
                        const problems = [...props.problems.slice(0, i), ...props.problems.slice(i + 1)];
                        props.dispatch({ ty: 'set-problems', problems });
                    }} />
                </div>
            </DropdownItem>)
        }
    </DropdownMenu >
}
