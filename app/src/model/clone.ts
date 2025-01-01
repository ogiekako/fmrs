import { Board, Hands, Piece, Position } from ".";

export function clonePosition(position: Position): Position {
  return {
    board: cloneBoard(position.board),
    hands: {
      black: cloneHands(position.hands["black"]),
      white: cloneHands(position.hands["white"]),
    },
  };
}

function cloneBoard(board: Board): Board {
  return board.map((col) =>
    col.map((piece) => (piece ? clonePiece(piece) : undefined)),
  );
}

function clonePiece(piece: Piece): Piece {
  return {
    color: piece.color,
    kind: piece.kind,
    promoted: piece.promoted,
  };
}

function cloneHands(hands: Hands): Hands {
  return Object.assign({}, hands);
}
