import { Button, Spinner } from "react-bootstrap";
import * as model from "../../model";
import * as types from "../types";
import * as solve from "../../solve";
import SolveResponse from "./SolveResponse";

const USE_WASM = true;

export default function SolveButton(props: {
  position: model.Position;
  solving: types.Solving | undefined;
  solveResponse: types.SolveResponse | undefined;
  dispatch: types.Dispatcher;
}) {
  const solutionLimit = 10;
  const buttonText = props.solving ? "Cancel" : "Solve";
  const buttonVariant = props.solving ? "danger" : "primary";
  return (
    <div>
      <div className="d-flex" style={{ gap: "5px" }}>
        <Button
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
                solutionLimit,
                cancelToken,
                onStep
              );
              const millis = new Date().getTime() - start.getTime();
              if (response) {
                props.dispatch({
                  ty: "set-solve-response",
                  response: {
                    ty: "solved",
                    response,
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
      {props.solveResponse ? (
        <SolveResponse
          solveResponse={props.solveResponse}
          solutionLimit={solutionLimit}
        />
      ) : (
        <></>
      )}
    </div>
  );
}
