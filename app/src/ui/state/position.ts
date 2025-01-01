import * as model from "../../model";

export function create(): model.Position {
  const pieces = model.emptyBoard();
  pieces[0][4] = {
    color: "white",
    kind: "K",
    promoted: false,
  };
  pieces[8][4] = {
    color: "black",
    kind: "K",
    promoted: false,
  };
  return {
    board: pieces,
    hands: {
      black: model.emptyHands(),
      white: model.fullHands(),
    },
  };
}
