import * as types from "../types";
import Solution from "./Solution";

export default function SolveResponse(props: {
  solveResponse: types.SolveResponse;
  solutionLimit: number;
}) {
  const message = getMessage(props.solveResponse, props.solutionLimit);

  const text = message ? (
    <div>
      {message} ({(props.solveResponse.millis / 1000).toFixed(1)}s)
    </div>
  ) : (
    <div></div>
  );

  return props.solveResponse.ty === "solved" ? (
    <div>
      {text}
      <Solution kif={props.solveResponse.response.kif} />
    </div>
  ) : (
    text
  );
}

function getMessage(r: types.SolveResponse, limit: number) {
  switch (r.ty) {
    case "error":
      return `Internal error: ${r.message}`;
    case "no-solution":
      return "No solution";
    case "solved":
      const count = r.response.solutions;
      if (!count) {
        return "";
      }
      if (count > limit) {
        return `More than ${limit} solutions found`;
      } else if (count > 1) {
        return `${count} solutions found`;
      } else {
        return `${count} solution found`;
      }
  }
}
