import React, { useMemo, useState } from "react";
import {
  Badge,
  Button,
  ButtonGroup,
  Card,
  Col,
  Form,
  InputGroup,
  Row,
  Table,
} from "react-bootstrap";
import * as model from "../../model";
import { OGIEKAKO_PROBLEMS, OgiekakoProblem } from "../../ogiekako_problems";

type FilterCategory =
  | "selected"
  | "all"
  | "award"
  | "short"
  | "long"
  | "ultralong"
  | "smoke"
  | "hand_limit";

type SortKey = "date_asc" | "date_desc" | "moves_asc" | "moves_desc";
type ViewMode = "cards" | "table";

const PIECE_LABELS: { [K in model.Kind]: [string, string] } = {
  P: ["歩", "と"],
  L: ["香", "杏"],
  N: ["桂", "圭"],
  S: ["銀", "全"],
  G: ["金", ""],
  B: ["角", "馬"],
  R: ["飛", "龍"],
  K: ["玉", ""],
};

const HAND_ORDER: model.Kind[] = ["R", "B", "G", "S", "N", "L", "P"];

function formatHands(hands: model.Hands): string {
  const parts: string[] = [];
  for (const k of HAND_ORDER) {
    const n = hands[k];
    if (n > 0) {
      parts.push(n > 1 ? `${PIECE_LABELS[k][0]}${n}` : PIECE_LABELS[k][0]);
    }
  }
  return parts.length > 0 ? parts.join(" ") : "なし";
}

function BoardThumbnail(props: {
  sfen: string;
  showWhiteHand?: boolean;
  compact?: boolean;
}) {
  const position = useMemo(() => model.decodeSfen(props.sfen), [props.sfen]);
  const cellW = props.compact ? 14 : 17;
  const cellH = props.compact ? 15 : 19;
  const fontSize = props.compact ? "9.5px" : "11.5px";

  const rows = [];
  for (let row = 0; row < 9; row++) {
    const cells = [];
    for (let col = 8; col >= 0; col--) {
      const p = position.board[row][col];
      let content: React.ReactNode = null;
      if (p) {
        const color = p === "O" ? "black" : p.color;
        const letter =
          p === "O" ? "⬤" : PIECE_LABELS[p.kind][p.promoted ? 1 : 0];
        content = (
          <div
            style={{
              transform: color === "black" ? "rotate(0)" : "rotate(180deg)",
              textAlign: "center",
              lineHeight: 1,
              userSelect: "none",
            }}
          >
            {letter}
          </div>
        );
      }
      cells.push(
        <td
          key={col}
          style={{
            width: cellW,
            height: cellH,
            padding: 0,
            border: "1px solid #888",
            backgroundColor: "#fffdf7",
            fontSize,
            verticalAlign: "middle",
          }}
        >
          {content}
        </td>
      );
    }
    rows.push(<tr key={row}>{cells}</tr>);
  }

  const whiteHandText = props.showWhiteHand
    ? formatHands(position.hands.white)
    : null;
  const blackHandText = formatHands(position.hands.black);

  return (
    <div
      style={{
        display: "inline-block",
        width: cellW * 9 + 2,
      }}
    >
      {whiteHandText !== null && (
        <div
          className="text-dark mb-1 text-truncate"
          title={`持駒：${whiteHandText}`}
          style={{
            fontSize: props.compact ? "10px" : "11px",
            lineHeight: 1.2,
            textAlign: "left",
          }}
        >
          <span className="text-muted">持駒：</span>
          {whiteHandText}
        </div>
      )}
      <table
        style={{
          borderCollapse: "collapse",
          border: "1.5px solid #222",
          margin: "0 auto",
        }}
      >
        <tbody>{rows}</tbody>
      </table>
      <div
        className="text-dark mt-1 text-truncate"
        title={`持駒：${blackHandText}`}
        style={{
          fontSize: props.compact ? "10px" : "11px",
          lineHeight: 1.2,
          textAlign: "left",
        }}
      >
        <span className="text-muted">持駒：</span>
        {blackHandText}
      </div>
    </div>
  );
}

const SELECTED_COUNT = OGIEKAKO_PROBLEMS.filter((p) => p.selected).length;

export default function OgiekakoCollection(props: {
  onSelectProblem: (problem: OgiekakoProblem, preSolved: boolean) => void;
  onBackToSolver: () => void;
}) {
  const [filter, setFilter] = useState<FilterCategory>("selected");
  const [sortKey, setSortKey] = useState<SortKey>("date_asc");
  const [viewMode, setViewMode] = useState<ViewMode>("cards");
  const [query, setQuery] = useState("");

  const filteredProblems = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = OGIEKAKO_PROBLEMS.filter((p) => {
      if (filter === "selected" && !p.selected) return false;
      if (filter === "award" && p.awards.length === 0) return false;
      if (filter === "short" && p.moves >= 100) return false;
      if (filter === "long" && (p.moves < 100 || p.moves >= 1000)) return false;
      if (filter === "ultralong" && p.moves < 1000) return false;
      if (filter === "smoke" && !(p.notes?.includes("煙") ?? false))
        return false;
      if (filter === "hand_limit" && !p.defenderHand) return false;

      if (!q) return true;
      const haystack = [
        p.id,
        p.title ?? "",
        p.source,
        p.publishedAt,
        String(p.moves),
        p.rule,
        p.defenderHand ?? "",
        p.awards.join(" "),
        p.notes ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });

    const indexed = list.map((p, idx) => ({ p, idx }));
    indexed.sort((a, b) => {
      switch (sortKey) {
        case "date_asc":
          return (
            a.p.publishedAt.localeCompare(b.p.publishedAt) || a.idx - b.idx
          );
        case "date_desc":
          return (
            b.p.publishedAt.localeCompare(a.p.publishedAt) || b.idx - a.idx
          );
        case "moves_asc":
          return a.p.moves - b.p.moves || a.idx - b.idx;
        case "moves_desc":
          return b.p.moves - a.p.moves || a.idx - b.idx;
      }
    });
    return indexed.map((x) => x.p);
  }, [filter, sortKey, query]);

  return (
    <div className="my-3">
      <div className="d-flex justify-content-between align-items-center flex-wrap gap-2 mb-3">
        <div>
          <h2 className="h4 mb-1">ogiekako 協力詰作品集</h2>
          <div className="text-muted" style={{ fontSize: "0.9em" }}>
            純粋な協力詰（全作一意解検証済み）。作品をクリックすると解図済み状態（手順再生可能）でソルバーを開きます。
          </div>
        </div>
        <Button variant="outline-secondary" onClick={props.onBackToSolver}>
          ← ソルバーに戻る
        </Button>
      </div>

      <Card className="mb-3">
        <Card.Body className="py-2 px-3">
          <div className="d-flex flex-wrap gap-2 align-items-center justify-content-between">
            <ButtonGroup size="sm" className="flex-wrap">
              <Button
                variant={filter === "selected" ? "primary" : "outline-primary"}
                onClick={() => setFilter("selected")}
              >
                セレクション ({SELECTED_COUNT})
              </Button>
              <Button
                variant={filter === "all" ? "primary" : "outline-primary"}
                onClick={() => setFilter("all")}
              >
                すべて ({OGIEKAKO_PROBLEMS.length})
              </Button>
              <Button
                variant={filter === "award" ? "primary" : "outline-primary"}
                onClick={() => setFilter("award")}
              >
                Fairy TOPix 入賞
              </Button>
              <Button
                variant={filter === "short" ? "primary" : "outline-primary"}
                onClick={() => setFilter("short")}
              >
                短・中編 (〜99手)
              </Button>
              <Button
                variant={filter === "long" ? "primary" : "outline-primary"}
                onClick={() => setFilter("long")}
              >
                長編 (100〜999手)
              </Button>
              <Button
                variant={filter === "ultralong" ? "primary" : "outline-primary"}
                onClick={() => setFilter("ultralong")}
              >
                超長編 (1000手〜)
              </Button>
              <Button
                variant={filter === "smoke" ? "primary" : "outline-primary"}
                onClick={() => setFilter("smoke")}
              >
                煙詰
              </Button>
              <Button
                variant={
                  filter === "hand_limit" ? "primary" : "outline-primary"
                }
                onClick={() => setFilter("hand_limit")}
              >
                受方持駒制限あり
              </Button>
            </ButtonGroup>

            <div className="d-flex gap-2 align-items-center flex-wrap">
              <InputGroup size="sm" style={{ width: "210px" }}>
                <Form.Control
                  type="text"
                  placeholder="作品名・出典・解説で検索..."
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                {query && (
                  <Button
                    variant="outline-secondary"
                    onClick={() => setQuery("")}
                  >
                    ×
                  </Button>
                )}
              </InputGroup>

              <Form.Select
                size="sm"
                style={{ width: "170px" }}
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as SortKey)}
              >
                <option value="date_asc">発表年月（古い順）</option>
                <option value="date_desc">発表年月（新しい順）</option>
                <option value="moves_asc">手数（短い順）</option>
                <option value="moves_desc">手数（長い順）</option>
              </Form.Select>

              <ButtonGroup size="sm">
                <Button
                  variant={
                    viewMode === "cards" ? "secondary" : "outline-secondary"
                  }
                  onClick={() => setViewMode("cards")}
                >
                  カード
                </Button>
                <Button
                  variant={
                    viewMode === "table" ? "secondary" : "outline-secondary"
                  }
                  onClick={() => setViewMode("table")}
                >
                  一覧表
                </Button>
              </ButtonGroup>
            </div>
          </div>
        </Card.Body>
      </Card>

      {viewMode === "cards" ? (
        <Row className="row-cols-1 row-cols-lg-2 g-3">
          {filteredProblems.map((p) => {
            const origIndex = OGIEKAKO_PROBLEMS.indexOf(p) + 1;
            return (
              <Col key={p.id}>
                <Card
                  className="h-100 shadow-sm"
                  style={{ cursor: "pointer" }}
                  onClick={() => props.onSelectProblem(p, true)}
                >
                  <Card.Body className="p-3">
                    <div className="d-flex gap-3 align-items-start">
                      <div className="flex-shrink-0">
                        <BoardThumbnail
                          sfen={p.sfen}
                          showWhiteHand={Boolean(p.defenderHand)}
                        />
                      </div>
                      <div className="flex-grow-1 min-w-0 d-flex flex-column justify-content-between h-100">
                        <div>
                          <div className="d-flex justify-content-between align-items-start gap-1 mb-1">
                            <div>
                              <span className="text-muted me-2">
                                #{origIndex}
                              </span>
                              {p.title ? (
                                <span className="fw-bold fs-6 me-2">
                                  『{p.title}』
                                </span>
                              ) : null}
                              <span
                                className="text-muted"
                                style={{ fontSize: "0.88em" }}
                              >
                                （{p.publishedAt}）
                              </span>
                            </div>
                          </div>

                          <div className="mb-2">
                            <Badge bg="primary" className="me-1">
                              {p.rule} {p.moves.toLocaleString()}手
                            </Badge>
                            {p.defenderHand && (
                              <Badge bg="secondary" className="me-1">
                                受方持駒：{p.defenderHand}
                              </Badge>
                            )}
                            {p.awards.map((award) => (
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

                          <div
                            className="mb-1"
                            style={{ fontSize: "0.88em" }}
                          >
                            <span className="fw-semibold">{p.source}</span>
                            {p.url && (
                              <a
                                href={p.url}
                                target="_blank"
                                rel="noreferrer"
                                className="ms-2 text-decoration-none"
                                onClick={(e) => e.stopPropagation()}
                              >
                                [出典↗]
                              </a>
                            )}
                          </div>

                          {p.notes && (
                            <div
                              className="text-muted mb-2"
                              style={{ fontSize: "0.86em" }}
                            >
                              {p.notes}
                            </div>
                          )}
                        </div>

                        <div
                          className="mt-2 pt-1"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <Button
                            size="sm"
                            variant="primary"
                            className="me-2"
                            onClick={() => props.onSelectProblem(p, true)}
                          >
                            解図済みで開く
                          </Button>
                          <Button
                            size="sm"
                            variant="outline-secondary"
                            onClick={() => props.onSelectProblem(p, false)}
                          >
                            初形のみ
                          </Button>
                        </div>
                      </div>
                    </div>
                  </Card.Body>
                </Card>
              </Col>
            );
          })}
        </Row>
      ) : (
        <div className="table-responsive">
          <Table
            hover
            bordered
            className="align-middle"
            style={{ fontSize: "0.92em" }}
          >
            <thead className="table-light">
              <tr>
                <th style={{ width: "45px" }} className="text-center">
                  #
                </th>
                <th style={{ width: "140px" }} className="text-center">
                  盤面
                </th>
                <th style={{ width: "90px" }}>発表年月</th>
                <th style={{ width: "130px" }}>作品名</th>
                <th style={{ width: "95px" }} className="text-end">
                  手数
                </th>
                <th style={{ width: "135px" }}>ルール</th>
                <th>出典・備考・受賞</th>
                <th style={{ width: "195px" }} className="text-center">
                  操作
                </th>
              </tr>
            </thead>
            <tbody>
              {filteredProblems.map((p) => {
                const origIndex = OGIEKAKO_PROBLEMS.indexOf(p) + 1;
                return (
                  <tr
                    key={p.id}
                    style={{ cursor: "pointer" }}
                    onClick={() => props.onSelectProblem(p, true)}
                  >
                    <td className="text-center text-muted">{origIndex}</td>
                    <td className="text-center py-2">
                      <BoardThumbnail
                        sfen={p.sfen}
                        showWhiteHand={Boolean(p.defenderHand)}
                        compact
                      />
                    </td>
                    <td className="text-nowrap">{p.publishedAt}</td>
                    <td>
                      {p.title ? (
                        <div className="fw-bold">『{p.title}』</div>
                      ) : (
                        <span className="text-muted">—</span>
                      )}
                    </td>
                    <td className="text-end fw-bold text-nowrap">
                      {p.moves.toLocaleString()}手
                    </td>
                    <td>
                      <div>{p.rule}</div>
                      {p.defenderHand && (
                        <Badge bg="secondary" className="fw-normal mt-1">
                          受方持駒：{p.defenderHand}
                        </Badge>
                      )}
                    </td>
                    <td>
                      <div className="mb-1">
                        <span className="fw-semibold">{p.source}</span>
                        {p.url && (
                          <a
                            href={p.url}
                            target="_blank"
                            rel="noreferrer"
                            className="ms-2 text-decoration-none"
                            onClick={(e) => e.stopPropagation()}
                          >
                            [出典↗]
                          </a>
                        )}
                      </div>
                      {p.awards.length > 0 && (
                        <div className="mb-1">
                          {p.awards.map((award) => (
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
                      )}
                      {p.notes && (
                        <div
                          className="text-muted"
                          style={{ fontSize: "0.9em" }}
                        >
                          {p.notes}
                        </div>
                      )}
                    </td>
                    <td
                      className="text-center text-nowrap"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Button
                        size="sm"
                        variant="primary"
                        className="me-1"
                        onClick={() => props.onSelectProblem(p, true)}
                      >
                        解図済みで開く
                      </Button>
                      <Button
                        size="sm"
                        variant="outline-secondary"
                        onClick={() => props.onSelectProblem(p, false)}
                      >
                        初形のみ
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        </div>
      )}
    </div>
  );
}
