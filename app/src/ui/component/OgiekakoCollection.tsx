import React, { useMemo, useState } from "react";
import { Badge, Button, ButtonGroup, Card, Form, InputGroup, Table } from "react-bootstrap";
import { OGIEKAKO_PROBLEMS, OgiekakoProblem } from "../../ogiekako_problems";

type FilterCategory =
  | "all"
  | "award"
  | "short"
  | "long"
  | "ultralong"
  | "smoke"
  | "hand_limit";

type SortKey = "date_asc" | "date_desc" | "moves_asc" | "moves_desc";

export default function OgiekakoCollection(props: {
  onSelectProblem: (problem: OgiekakoProblem, preSolved: boolean) => void;
  onBackToSolver: () => void;
}) {
  const [filter, setFilter] = useState<FilterCategory>("all");
  const [sortKey, setSortKey] = useState<SortKey>("date_asc");
  const [query, setQuery] = useState("");

  const filteredProblems = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = OGIEKAKO_PROBLEMS.filter((p) => {
      if (filter === "award" && p.awards.length === 0) return false;
      if (filter === "short" && p.moves >= 100) return false;
      if (filter === "long" && (p.moves < 100 || p.moves >= 1000)) return false;
      if (filter === "ultralong" && p.moves < 1000) return false;
      if (filter === "smoke" && !(p.notes?.includes("煙") ?? false)) return false;
      if (filter === "hand_limit" && !p.defenderHand) return false;

      if (!q) return true;
      const haystack = [
        p.id,
        p.title ?? "",
        p.author,
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
          <h2 className="h4 mb-1">
            ogiekako（荻江香木 / 荻絵香木）協力詰作品集
          </h2>
          <div className="text-muted" style={{ fontSize: "0.9em" }}>
            純粋な協力詰・ばか詰 全{OGIEKAKO_PROBLEMS.length}局（2005年〜2026年・全作一意解検証済み）。作品をクリックすると解図済み状態（手順再生可能）でソルバーを開きます。
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
              <InputGroup size="sm" style={{ width: "220px" }}>
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
                style={{ width: "175px" }}
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as SortKey)}
              >
                <option value="date_asc">発表年月（古い順）</option>
                <option value="date_desc">発表年月（新しい順）</option>
                <option value="moves_asc">手数（短い順）</option>
                <option value="moves_desc">手数（長い順）</option>
              </Form.Select>
            </div>
          </div>
        </Card.Body>
      </Card>

      <div className="table-responsive">
        <Table hover bordered className="align-middle" style={{ fontSize: "0.92em" }}>
          <thead className="table-light">
            <tr>
              <th style={{ width: "45px" }} className="text-center">#</th>
              <th style={{ width: "90px" }}>発表年月</th>
              <th style={{ width: "130px" }}>作品名 / 名義</th>
              <th style={{ width: "95px" }} className="text-end">手数</th>
              <th style={{ width: "135px" }}>ルール</th>
              <th>出典・備考・受賞</th>
              <th style={{ width: "195px" }} className="text-center">操作</th>
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
                  <td className="text-nowrap">{p.publishedAt}</td>
                  <td>
                    {p.title ? (
                      <div className="fw-bold">『{p.title}』</div>
                    ) : null}
                    <div className="text-muted" style={{ fontSize: "0.88em" }}>
                      {p.author}
                    </div>
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
                      <div className="text-muted" style={{ fontSize: "0.9em" }}>
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
    </div>
  );
}
