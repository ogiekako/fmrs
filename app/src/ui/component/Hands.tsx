import { SELECTED_COLOR } from "../constants";
import * as model from "../../model";

export default function Hands(props: {
  hands: model.Hands;
  selected: model.Kind | "" | undefined;
  onClick: (kind: model.Kind | undefined) => void;
}) {
  let nothing = true;
  const pieces = [];
  for (const k of model.KINDS) {
    const n = props.hands[k];
    if (n) {
      nothing = false;
      pieces.push(
        <span
          key={k}
          onClick={(e) => {
            e.stopPropagation();
            props.onClick(k);
          }}
        >
          <Kind kind={k} selected={props.selected === k} />
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
      <Kind kind={""} selected={props.selected === ""} />
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

function Kind(props: { kind: model.Kind | ""; selected: boolean }) {
  let letter = props.kind == "" ? "なし" : MAPPING[props.kind];
  return (
    <span
      style={{ backgroundColor: props.selected ? SELECTED_COLOR : "white" }}
    >
      {letter}
    </span>
  );
}
