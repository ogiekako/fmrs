import * as model from "../../model";
import Board from "./Board";
import Hands from "./Hands";
import * as types from "../types";
import { Shifter } from "./Shifter";

export default function Position(props: {
  position: model.Position;
  selected: types.Selected | undefined;
  dispatch: types.Dispatcher;
  disabled: boolean;
}) {
  let boardSelected = undefined;
  let whiteHandSelected = undefined;
  let blackHandSelected = undefined;
  if (props.selected) {
    if (props.selected.ty === "board") {
      boardSelected = props.selected.pos;
    } else if (props.selected.color === "white") {
      whiteHandSelected = props.selected.kind ?? ("" as const);
    } else {
      blackHandSelected = props.selected.kind ?? ("" as const);
    }
  }

  return (
    <div
      style={{ outline: "none" }}
      tabIndex={0}
      className={props.disabled ? "text-muted" : ""}
      onKeyDown={(e) => {
        e.preventDefault();
        props.dispatch({
          ty: "key-down",
          key: e.key,
        });
      }}
    >
      <Hands
        hands={props.position.hands["white"]}
        selected={whiteHandSelected}
        onClick={(kind) =>
          props.dispatch({ ty: "click-hand", color: "white", kind })
        }
      />
      <Shifter dispatch={props.dispatch}>
        <Board
          pieces={props.position.board}
          selected={boardSelected}
          onClick={(pos) => props.dispatch({ ty: "click-board", pos })}
          onRightClick={(pos) =>
            props.dispatch({ ty: "right-click-board", pos })
          }
        />
      </Shifter>
      <Hands
        hands={props.position.hands["black"]}
        selected={blackHandSelected}
        onClick={(kind) =>
          props.dispatch({ ty: "click-hand", color: "black", kind })
        }
      />
    </div>
  );
}
