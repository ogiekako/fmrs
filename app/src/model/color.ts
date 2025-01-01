export type Color = "black" | "white";

export function oppositeColor(color: Color): Color {
  return color === "black" ? "white" : "black";
}
