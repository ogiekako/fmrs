import * as model from "../../model";
import { decodeSfen } from "../../model/sfen/decode";
import * as types from "../types";

export default function Sfen(props: {
  position: model.Position;
  dispatch: types.Dispatcher;
  disabled: boolean;
}) {
  const sfen = model.encodeSfen(props.position);
  return (
    <div>
      SFEN{" "}
      <input
        className={props.disabled ? "text-muted" : ""}
        type="text"
        readOnly={props.disabled}
        value={sfen}
        onChange={(e) => {
          if (e.target.value === sfen) {
            return;
          }
          props.dispatch({
            ty: "set-position",
            position: decodeSfen(e.target.value),
          });
        }}
        style={{ width: 250 }}
      />
    </div>
  );
}
