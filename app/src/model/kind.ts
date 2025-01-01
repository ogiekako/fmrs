export const KINDS: Kind[] = ["P", "L", "N", "S", "G", "B", "R", "K"];
export type Kind = "P" | "L" | "N" | "S" | "G" | "B" | "R" | "K";

export function kindCanPromote(kind: Kind): boolean {
  return kind !== "G" && kind !== "K";
}
