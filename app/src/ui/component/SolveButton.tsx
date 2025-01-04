import { Button, Form, InputGroup, Spinner } from "react-bootstrap";
import * as model from "../../model";
import * as types from "../types";
import * as solve from "../../solve";
import SolveResponse from "./SolveResponse";
import { positionStone } from "../../model/position";

export default function SolveButton(props: {
  position: model.Position;
  solving: types.Solving | undefined;
  solveResponse: types.SolveResponse | undefined;
  solutionLimit: number;
  dispatch: types.Dispatcher;
}) {
  const buttonText = props.solving ? "Cancel" : "Solve";
  const buttonVariant = props.solving ? "danger" : "primary";

  const solveButtonWithProgress = (
    <div className="d-flex py-2" style={{ gap: "5px" }}>
      <Button
        id="solve-button"
        variant={buttonVariant}
        onClick={async (event) => {
          event.currentTarget.blur();
          if (props.solving) {
            props.solving.cancelToken.cancel();
            return;
          }
          const cancelToken = new solve.CancellationToken();

          props.dispatch({
            ty: "set-solving",
            solving: { cancelToken, step: 0 },
          });
          props.dispatch({ ty: "set-solve-response", response: undefined });

          const onStep = (step: number) => {
            props.dispatch({
              ty: "set-solving",
              solving: { cancelToken, step },
            });
          };
          const start = new Date();
          try {
            const response = await solve.solve(
              props.position,
              props.solutionLimit,
              cancelToken,
              onStep
            );
            const stone = positionStone(props.position);
            const millis = new Date().getTime() - start.getTime();
            if (response) {
              props.dispatch({
                ty: "set-solve-response",
                response: {
                  ty: "solved",
                  response,
                  stone,
                  millis,
                },
              });
            } else if (!cancelToken.isCanceled()) {
              props.dispatch({
                ty: "set-solve-response",
                response: { ty: "no-solution", millis },
              });
            }
          } catch (e: any) {
            const millis = new Date().getTime() - start.getTime();

            console.error(e);
            props.dispatch({
              ty: "set-solve-response",
              response: {
                ty: "error",
                message: (e as Error).message,
                millis,
              },
            });
          } finally {
            props.dispatch({ ty: "set-solving", solving: undefined });
          }
        }}
      >
        {buttonText}
      </Button>
      {props.solving ? (
        <>
          <span style={{ fontSize: "0.8em" }}>
            Step
            <br />
            {props.solving.step}
          </span>
          <Spinner animation="border" role="status">
            <span className="visually-hidden">Solving...</span>
          </Spinner>
        </>
      ) : (
        <></>
      )}
    </div>
  );

  return (
    <div>
      {solveButtonWithProgress}
      <InputGroup className="mb-3" style={{ width: "200px" }}>
        <InputGroup.Text>最大検出解数</InputGroup.Text>
        <Form.Control
          type="number"
          value={props.solutionLimit === 0 ? "" : props.solutionLimit}
          onChange={(event) => {
            const n = parseInt(event.target.value);
            if (n >= 0) {
              props.dispatch({ ty: "set-solution-limit", n });
            } else {
              props.dispatch({ ty: "set-solution-limit", n: 0 });
            }
          }}
          disabled={!!props.solving}
        />
      </InputGroup>
      {props.solveResponse ? (
        <SolveResponse
          solveResponse={props.solveResponse}
          solutionLimit={props.solutionLimit}
        />
      ) : (
        <></>
      )}
    </div>
  );
}
