export type Color = "black" | "white";

export function colorOpposite(color: Color): Color {
  return color === "black" ? "white" : "black";
}
