import { useEffect, useReducer, useState } from "react";
import { newState, reduce } from "../state/state";
import Info from "./Info";
import Position from "./Position";
import Problems from "./Problems";
import Sfen from "./Sfen";
import SolveButton from "./SolveButton";
import OgiekakoCollection from "./OgiekakoCollection";
import {
  decodeSfen,
  encodeSfen,
  sfenFromUrl,
  sfenToPath,
  isOldFormatUrl,
  isNamedPositionUrl,
  ogiekakoPagePath,
  isOgiekakoPageUrl,
} from "../../model";
import { positionStone } from "../../model/position";
import {
  buildPrecomputedSolveResponse,
  OgiekakoProblem,
} from "../../ogiekako_problems";

export function Editor(props: {}) {
  const [state, dispatch] = useReducer(reduce, undefined, newState);
  const [showOgiekako, setShowOgiekako] = useState<boolean>(() =>
    isOgiekakoPageUrl()
  );

  const sfen = encodeSfen(state.position);

  useEffect(() => {
    document.title = showOgiekako
      ? "ogiekako 協力詰作品集"
      : "Shogi Helpmate Solver";
  }, [showOgiekako]);

  useEffect(() => {
    const url = new URL(window.location.href);
    if (showOgiekako) {
      const target = ogiekakoPagePath();
      if (url.pathname.replace(/\/$/, "") !== target.replace(/\/$/, "")) {
        window.history.pushState({}, "", window.location.origin + target);
      }
      return;
    }

    const targetPath = sfenToPath(sfen);
    if (url.pathname !== targetPath || url.search) {
      const newHref = window.location.origin + targetPath + window.location.hash;
      if (isOldFormatUrl() || isNamedPositionUrl()) {
        window.history.replaceState({}, "", newHref);
      } else {
        window.history.pushState({}, "", newHref);
      }
    }
  }, [showOgiekako, sfen]);

  useEffect(() => {
    const onPopState = () => {
      if (isOgiekakoPageUrl()) {
        setShowOgiekako(true);
        return;
      }
      setShowOgiekako(false);
      const urlSfen = sfenFromUrl();
      if (urlSfen) {
        dispatch({
          ty: "set-position",
          position: decodeSfen(urlSfen),
        });
      }
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const handleSelectOgiekakoProblem = (
    problem: OgiekakoProblem,
    preSolved: boolean
  ) => {
    const pos = decodeSfen(problem.sfen);
    dispatch({
      ty: "set-position",
      position: pos,
    });
    if (preSolved) {
      const response = buildPrecomputedSolveResponse(problem);
      if (response) {
        dispatch({
          ty: "set-solve-response",
          response: {
            ty: "solved",
            response,
            stone: positionStone(pos),
            millis: 0,
          },
        });
      }
    } else {
      dispatch({
        ty: "set-solve-response",
        response: undefined,
      });
    }
    setShowOgiekako(false);
    window.scrollTo({ top: 0, behavior: "smooth" });
  };

  if (showOgiekako) {
    return (
      <OgiekakoCollection
        onSelectProblem={handleSelectOgiekakoProblem}
        onBackToSolver={() => setShowOgiekako(false)}
      />
    );
  }

  return (
    <div>
      <h1>Shogi Helpmate Solver</h1>
      <div className="d-flex">
        <div>
          <Position
            position={state.position}
            selected={state.selected}
            dispatch={dispatch}
            disabled={!!state.solving}
          />
        </div>
        <Info />
        <div className="p-3">
          <Problems
            position={state.position}
            problems={state.problems}
            dispatch={dispatch}
            disabled={!!state.solving}
          />
        </div>
      </div>
      <Sfen
        position={state.position}
        dispatch={dispatch}
        disabled={!!state.solving}
      />
      <SolveButton
        position={state.position}
        solving={state.solving}
        solveResponse={state.solveResponse}
        solutionLimit={state.solutionLimit}
        oneWayMateMode={state.oneWayMateMode}
        dispatch={dispatch}
      />
    </div>
  );
}
