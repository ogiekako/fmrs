import { SELECTED_COLOR } from "../constants";
import * as model from "../../model";

export default function Hands(props: {
  hands: model.Hands;
  selected: model.Kind | "" | undefined;
  onClick: (kind: model.Kind | undefined) => void;
  pieceBox?: boolean;
}) {
  let nothing = true;
  const pieces = [];
  for (const k of model.KINDS) {
    const n = props.hands[k];
    if (n) {
      nothing = false;
      pieces.push(
        <span
          className={props.pieceBox ? "text-secondary" : ""}
          key={k}
          onClick={(e) => {
            e.stopPropagation();
            props.onClick(k);
          }}
        >
          <Kind
            kind={k}
            selected={props.selected === k}
            pieceBox={props.pieceBox}
          />
          {n}
        </span>
      );
    }
  }
  const res = nothing ? (
    <span
      onClick={(e) => {
        e.stopPropagation();
        props.onClick(undefined);
      }}
    >
      <Kind
        kind={""}
        selected={props.selected === ""}
        pieceBox={props.pieceBox}
      />
    </span>
  ) : (
    <>{pieces}</>
  );
  return <div style={{ fontSize: "1.5em" }}>{res}</div>;
}

const MAPPING: { [k in model.Kind]: string } = {
  P: "歩",
  L: "香",
  N: "桂",
  S: "銀",
  G: "金",
  B: "角",
  R: "飛",
  K: "玉",
};

function Kind(props: {
  kind: model.Kind | "";
  selected: boolean;
  pieceBox?: boolean;
}) {
  let letter = props.kind == "" ? "なし" : MAPPING[props.kind];
  return (
    <span
      className={props.pieceBox ? "text-secondary" : ""}
      style={{ backgroundColor: props.selected ? SELECTED_COLOR : "white" }}
    >
      {letter}
    </span>
  );
}
