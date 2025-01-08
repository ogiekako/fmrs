import * as model from "../../model";
import Board from "./Board";
import Hands from "./Hands";
import * as types from "../types";
import { Shifter } from "./Shifter";
import { positionPieceBox } from "../../model/position";
import { FloatingLabel } from "react-bootstrap";

export default function Position(props: {
  position: model.Position;
  selected: types.Selected;
  dispatch: types.Dispatcher;
  disabled: boolean;
}) {
  let boardSelected = undefined;
  let whiteHandSelected = undefined;
  let blackHandSelected = undefined;
  let pieceBoxSelected = undefined;
  if (props.selected.shown) {
    if (props.selected.ty === "board") {
      boardSelected = props.selected.pos;
    } else if (props.selected.color === "white") {
      whiteHandSelected = props.selected.kind ?? ("" as const);
    } else if (props.selected.color === "black") {
      blackHandSelected = props.selected.kind ?? ("" as const);
    } else {
      pieceBoxSelected = props.selected.kind ?? ("" as const);
    }
  }
  const pieceBox = positionPieceBox(props.position);

  return (
    <div
      style={{ outline: "none" }}
      tabIndex={0}
      className={props.disabled ? "text-muted" : ""}
      onKeyDown={(e) => {
        e.preventDefault();
        if (e.key === ".") {
          document.getElementById("solve-button")?.click();
          return;
        }
        props.dispatch({
          ty: "key-down",
          key: e.key,
        });
      }}
    >
      <div style={{ position: "relative" }}>
        <label
          className="text-secondary"
          style={{
            marginRight: "0.25em",
            display: "flex",
            top: "50%",
            transform: "translateY(-50%)",
            position: "absolute",
            right: "100%",
            fontSize: "0.75em",
            width: "2em",
          }}
        >
          駒箱
        </label>
        <Hands
          pieceBox={true}
          hands={pieceBox}
          selected={pieceBoxSelected}
          onClick={(kind) =>
            props.dispatch({ ty: "click-hand", color: "pieceBox", kind })
          }
        />
      </div>
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
