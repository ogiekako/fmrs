import { useRef, useState } from "react";
import { Button, CloseButton, Dropdown, Form } from "react-bootstrap";
import DropdownItem from "react-bootstrap/esm/DropdownItem";
import DropdownMenu from "react-bootstrap/esm/DropdownMenu";
import * as model from "../../model";
import * as types from "../types";
import { PRESET_PROBLEMS } from "../../problem";

export default function Problems(props: {
  position: model.Position;
  problems: Array<types.Problem>;
  dispatch: types.Dispatcher;
  disabled: boolean;
}) {
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  function startAdding() {
    setName(model.encodeSfen(props.position));
    setAdding(true);
    setTimeout(() => inputRef.current?.select(), 0);
  }

  function confirmAdd() {
    const trimmed = name.trim() || model.encodeSfen(props.position);
    props.dispatch({
      ty: "set-problems",
      problems: [...props.problems, [props.position, trimmed]],
    });
    setAdding(false);
  }

  function cancelAdd() {
    setAdding(false);
  }

  return (
    <DropdownMenu show>
      <Dropdown.Header>
        Saved positions{" "}
        {!adding && (
          <Button variant="secondary" onClick={startAdding}>
            +
          </Button>
        )}{" "}
        <Button
          variant="outline-secondary"
          size="sm"
          title="Reset to defaults"
          onClick={() => {
            if (!window.confirm("Reset saved positions to defaults?")) return;
            props.dispatch({
              ty: "set-problems",
              problems: PRESET_PROBLEMS.map(([sfen, name]) => [
                model.decodeSfen(sfen),
                name,
              ]),
            });
          }}
        >
          ↺
        </Button>
      </Dropdown.Header>
      {adding && (
        <div className="px-3 py-1 d-flex gap-1">
          <Form.Control
            ref={inputRef}
            size="sm"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") confirmAdd();
              if (e.key === "Escape") cancelAdd();
            }}
          />
          <Button size="sm" variant="primary" onClick={confirmAdd}>
            Save
          </Button>
          <Button size="sm" variant="outline-secondary" onClick={cancelAdd}>
            ✕
          </Button>
        </div>
      )}
      {props.problems.map(([position, name], i) => (
        <DropdownItem
          key={i}
          onClick={() =>
            props.dispatch({
              ty: "set-position",
              position,
            })
          }
        >
          <div className="d-flex justify-content-between">
            <span className={props.disabled ? "text-muted" : ""}>{name}</span>
            <CloseButton
              onClick={(e) => {
                e.stopPropagation();
                const problems = [
                  ...props.problems.slice(0, i),
                  ...props.problems.slice(i + 1),
                ];
                props.dispatch({ ty: "set-problems", problems });
              }}
            />
          </div>
        </DropdownItem>
      ))}
    </DropdownMenu>
  );
}
