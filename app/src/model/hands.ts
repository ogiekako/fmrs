import { Kind } from ".";

export type Hands = {
  [K in Kind]: number;
};

export function emptyHands(): Hands {
  return {
    P: 0,
    L: 0,
    N: 0,
    S: 0,
    G: 0,
    B: 0,
    R: 0,
    K: 0,
  };
}

export function fullHands(): Hands {
  return {
    P: 18,
    L: 4,
    N: 4,
    S: 4,
    G: 4,
    B: 2,
    R: 2,
    K: 0,
  };
}
