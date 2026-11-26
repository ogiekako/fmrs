import { useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { newState, reduce } from "../state/state";
import Info from "./Info";
import Position from "./Position";
import Problems from "./Problems";
import Sfen from "./Sfen";
import SolveButton from "./SolveButton";
import OgiekakoCollection from "./OgiekakoCollection";
import * as solve from "../../solve";
import {
  decodeSfen,
  encodeSfen,
  sfenFromUrl,
  sfenToPath,
  isOldFormatUrl,
  isNamedPositionUrl,
  helpmatePagePath,
  futsuuPagePath,
  ogiekakoPagePath,
  isOgiekakoPageUrl,
  isRootFutsuuUrl,
  solverModeFromUrl,
  isSolvedFromUrl,
} from "../../model";
import { positionStone } from "../../model/position";
import {
  buildPrecomputedSolveResponse,
  findOgiekakoProblemBySfen,
  getPrecomputedSolveResponseBySfen,
  OgiekakoProblem,
} from "../../ogiekako_problems";

export function Editor(props: {}) {
  const [state, dispatch] = useReducer(reduce, undefined, newState);
  const [showOgiekako, setShowOgiekako] = useState<boolean>(() =>
    isOgiekakoPageUrl()
  );
  const ogiekakoScrollY = useRef<number>(0);
  const scrollToSolutionOnLoad = useRef<boolean>(
    !isOgiekakoPageUrl() && isSolvedFromUrl()
  );

  const sfen = encodeSfen(state.position);
  const isSolvedOgiekako =
    state.mode === "helpmate" &&
    state.solveResponse?.ty === "solved" &&
    state.solveResponse.response.solutions > 0 &&
    !!findOgiekakoProblemBySfen(sfen);

  useLayoutEffect(() => {
    if (showOgiekako) {
      window.scrollTo(0, ogiekakoScrollY.current);
    } else if (!scrollToSolutionOnLoad.current) {
      window.scrollTo(0, 0);
    }
  }, [showOgiekako]);

  useEffect(() => {
    document.title = showOgiekako
      ? "ogiekako 協力詰作品集"
      : state.mode === "futsuu"
        ? "Tsume Shogi Solver"
        : "Shogi Helpmate Solver";
  }, [showOgiekako, state.mode]);

  useEffect(() => {
    if (!showOgiekako) {
      solve.warmWorkerForMode(state.mode);
    }
  }, [showOgiekako, state.mode]);

  useEffect(() => {
    const url = new URL(window.location.href);
    if (showOgiekako) {
      const target = ogiekakoPagePath();
      if (
        url.pathname.replace(/\/$/, "") !== target.replace(/\/$/, "") ||
        url.search
      ) {
        window.history.pushState({}, "", window.location.origin + target);
      }
      return;
    }

    const targetPath = sfenToPath(sfen, state.mode);
    const targetSearch = isSolvedOgiekako ? "?solved=1" : "";
    if (url.pathname !== targetPath || url.search !== targetSearch) {
      const newHref =
        window.location.origin + targetPath + targetSearch + window.location.hash;
      if (
        isOldFormatUrl() ||
        isNamedPositionUrl() ||
        isRootFutsuuUrl() ||
        url.pathname === targetPath
      ) {
        window.history.replaceState({}, "", newHref);
      } else {
        window.history.pushState({}, "", newHref);
      }
    }
  }, [showOgiekako, state.mode, sfen, isSolvedOgiekako]);

  useEffect(() => {
    if ("scrollRestoration" in window.history) {
      window.history.scrollRestoration = "manual";
    }
    const onPopState = () => {
      if (isOgiekakoPageUrl()) {
        setShowOgiekako(true);
        return;
      }
      const nextMode = solverModeFromUrl();
      const solved = nextMode === "helpmate" && isSolvedFromUrl();
      scrollToSolutionOnLoad.current = solved;
      setShowOgiekako(false);
      const urlSfen = sfenFromUrl();
      const pos = urlSfen ? decodeSfen(urlSfen) : undefined;
      dispatch({
        ty: "set-mode",
        mode: nextMode,
        position: pos,
      });
      if (solved && pos) {
        const response = getPrecomputedSolveResponseBySfen(encodeSfen(pos));
        dispatch({
          ty: "set-solve-response",
          response: response
            ? {
                ty: "solved",
                response,
                stone: positionStone(pos),
                millis: 0,
              }
            : undefined,
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
      ty: "set-mode",
      mode: "helpmate",
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
    ogiekakoScrollY.current = window.scrollY;
    scrollToSolutionOnLoad.current = preSolved;
    setShowOgiekako(false);
  };

  if (showOgiekako) {
    return (
      <OgiekakoCollection
        onSelectProblem={handleSelectOgiekakoProblem}
        onBackToSolver={() => {
          ogiekakoScrollY.current = window.scrollY;
          setShowOgiekako(false);
        }}
      />
    );
  }

  return (
    <div>
      <div className="d-flex justify-content-between align-items-baseline flex-wrap gap-2 my-2">
        <h1 className="mb-0">
          {state.mode === "futsuu"
            ? "Tsume Shogi Solver"
            : "Shogi Helpmate Solver"}
        </h1>
        <div className="d-flex gap-3 ms-auto" style={{ fontSize: "0.95rem" }}>
          {state.mode === "futsuu" ? (
            <a
              href={helpmatePagePath()}
              className="text-secondary text-decoration-none"
              onClick={(e) => {
                e.preventDefault();
                if (state.solving) return;
                dispatch({ ty: "set-mode", mode: "helpmate" });
              }}
            >
              ← 協力詰ソルバー
            </a>
          ) : (
            <>
              <a
                href={futsuuPagePath()}
                className="text-secondary text-decoration-none"
                onClick={(e) => {
                  e.preventDefault();
                  if (state.solving) return;
                  dispatch({ ty: "set-mode", mode: "futsuu" });
                }}
              >
                普通詰将棋ソルバー →
              </a>
              <a
                href={ogiekakoPagePath()}
                className="text-secondary text-decoration-none"
                onClick={(e) => {
                  e.preventDefault();
                  if (state.solving) return;
                  setShowOgiekako(true);
                }}
              >
                ogiekako 協力詰作品集 →
              </a>
            </>
          )}
        </div>
      </div>
      <div className="d-flex flex-wrap">
        <div className="d-flex">
          <div className="ps-4 ps-sm-0">
            <Position
              position={state.position}
              selected={state.selected}
              dispatch={dispatch}
              disabled={!!state.solving}
            />
          </div>
          <Info mode={state.mode} />
        </div>
        <div className="p-3 d-none d-md-block position-relative">
          <div style={{ position: "absolute", top: "1rem", left: "1rem" }}>
            <Problems
              mode={state.mode}
              position={state.position}
              problems={state.problems}
              dispatch={dispatch}
              disabled={!!state.solving}
            />
          </div>
        </div>
      </div>
      <Sfen
        position={state.position}
        dispatch={dispatch}
        disabled={!!state.solving}
      />
      <SolveButton
        mode={state.mode}
        position={state.position}
        solving={state.solving}
        solveResponse={state.solveResponse}
        solutionLimit={state.solutionLimit}
        oneWayMateMode={state.oneWayMateMode}
        useServer={state.useServer}
        dispatch={dispatch}
        onSolutionLoaded={(container) => {
          if (!scrollToSolutionOnLoad.current) {
            return;
          }
          scrollToSolutionOnLoad.current = false;
          const centerBoard = () => {
            const boardEl =
              (container.querySelector(
                ".kifuforjs-board"
              ) as HTMLElement | null) ?? container;
            const rect = boardEl.getBoundingClientRect();
            const targetY =
              window.scrollY +
              rect.top +
              rect.height / 2 -
              window.innerHeight / 2;
            window.scrollTo(0, Math.max(0, Math.round(targetY)));
          };
          centerBoard();
          requestAnimationFrame(centerBoard);
        }}
      />
      <div className="py-3 d-md-none">
        <Problems
          mode={state.mode}
          position={state.position}
          problems={state.problems}
          dispatch={dispatch}
          disabled={!!state.solving}
        />
      </div>
    </div>
  );
}
