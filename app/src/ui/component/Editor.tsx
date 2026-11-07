import { useEffect, useReducer, useState } from "react";
import { Badge, Button, Card } from "react-bootstrap";
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
  findOgiekakoProblemBySfen,
  OgiekakoProblem,
} from "../../ogiekako_problems";

export function Editor(props: {}) {
  const [state, dispatch] = useReducer(reduce, undefined, newState);
  const [showOgiekako, setShowOgiekako] = useState<boolean>(() =>
    isOgiekakoPageUrl()
  );

  const sfen = encodeSfen(state.position);
  const matchedWork = findOgiekakoProblemBySfen(sfen);

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
      <div className="mb-2 d-flex justify-content-between align-items-center flex-wrap gap-2">
        <Button
          variant="outline-primary"
          size="sm"
          onClick={() => setShowOgiekako(true)}
        >
          ogiekako 協力詰作品集（全37局）を見る →
        </Button>
      </div>

      {matchedWork && (
        <Card className="mb-3 border-primary">
          <Card.Body className="py-2 px-3" style={{ fontSize: "0.9em" }}>
            <div className="d-flex justify-content-between align-items-start flex-wrap gap-2">
              <div>
                <span className="fw-bold me-2">
                  {matchedWork.title
                    ? `『${matchedWork.title}』`
                    : matchedWork.id}
                </span>
                <span className="text-muted me-2">
                  （{matchedWork.author} / {matchedWork.publishedAt}）
                </span>
                <Badge bg="primary" className="me-1">
                  {matchedWork.rule} {matchedWork.moves.toLocaleString()}手
                </Badge>
                {matchedWork.defenderHand && (
                  <Badge bg="secondary" className="me-1">
                    受方持駒：{matchedWork.defenderHand}
                  </Badge>
                )}
                {matchedWork.awards.map((award) => (
                  <Badge
                    key={award}
                    bg="warning"
                    text="dark"
                    className="me-1"
                  >
                    {award}
                  </Badge>
                ))}
              </div>
              <Button
                variant="link"
                size="sm"
                className="p-0 text-decoration-none"
                onClick={() => setShowOgiekako(true)}
              >
                ← 作品集一覧へ
              </Button>
            </div>
            <div className="mt-1">
              <span className="text-muted">出典：</span>
              {matchedWork.source}
              {matchedWork.url && (
                <a
                  href={matchedWork.url}
                  target="_blank"
                  rel="noreferrer"
                  className="ms-2 text-decoration-none"
                >
                  [出典URL↗]
                </a>
              )}
            </div>
            {matchedWork.notes && (
              <div className="text-muted mt-1">{matchedWork.notes}</div>
            )}
          </Card.Body>
        </Card>
      )}

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
