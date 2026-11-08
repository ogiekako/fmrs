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
        maxWidth: "100%",
      }}
    >
      {whiteHandText !== null && (
        <div
          className="text-dark mb-1"
          title={`持駒：${whiteHandText}`}
          style={{
            fontSize: props.compact ? "10px" : "11px",
            lineHeight: 1.25,
            textAlign: "left",
            overflowWrap: "anywhere",
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
        className="text-dark mt-1"
        title={`持駒：${blackHandText}`}
        style={{
          fontSize: props.compact ? "10px" : "11px",
          lineHeight: 1.25,
          textAlign: "left",
          overflowWrap: "anywhere",
        }}
      >
        <span className="text-muted">持駒：</span>
        {blackHandText}
      </div>
    </div>
  );
}

const SELECTED_COUNT = OGIEKAKO_PROBLEMS.filter((p) => p.selected).length;

function hasTextSelection(): boolean {
  return (window.getSelection()?.toString().length ?? 0) > 0;
}

interface ThemePreset {
  id: string;
  label: string;
  dotColor: string;
  dotBorder: string;
  css: string;
}

const THEME_PRESETS: ThemePreset[] = [
  {
    id: "github",
    label: "1. GitHub Primer風 (淡色タグ＋控えめボタン)",
    dotColor: "#ddf4ff",
    dotBorder: "#0969da",
    css: `
      :root {
        --bs-link-color: #0969da;
        --bs-link-hover-color: #0550ae;
      }
      /* フィルターボタン: 非選択は淡グレー枠、選択中はダーク */
      .ogk-filter-bar .btn-primary {
        background-color: #24292f !important;
        border-color: #24292f !important;
        color: #ffffff !important;
      }
      .ogk-filter-bar .btn-outline-primary {
        background-color: #f6f8fa !important;
        border-color: #d0d7de !important;
        color: #24292f !important;
      }
      .ogk-filter-bar .btn-outline-primary:hover {
        background-color: #eaeef2 !important;
        border-color: #afb8c1 !important;
      }
      /* 手数バッジ: GitHub トピックタグ風 (淡青背景 + 青文字) */
      .badge.bg-primary {
        background-color: #ddf4ff !important;
        color: #0969da !important;
        border: 1px solid rgba(84, 174, 255, 0.4) !important;
        font-weight: 600 !important;
      }
      /* 受方持駒バッジ: 淡グレー */
      .badge.bg-secondary {
        background-color: #f6f8fa !important;
        color: #57606a !important;
        border: 1px solid #d0d7de !important;
        font-weight: 500 !important;
      }
      /* 受賞バッジ: ソフトアンバー */
      .badge.bg-warning {
        background-color: #fff8c5 !important;
        color: #9a6700 !important;
        border: 1px solid rgba(212, 167, 44, 0.4) !important;
        font-weight: 600 !important;
      }
      /* カード内の「解図済みで開く」ボタン: 控えめなソフトブルー枠ボタン */
      .ogk-actions .btn-primary {
        background-color: #f6f8fa !important;
        border-color: #d0d7de !important;
        color: #0969da !important;
        font-weight: 600;
      }
      .ogk-actions .btn-primary:hover {
        background-color: #ddf4ff !important;
        border-color: #54aeff !important;
        color: #0969da !important;
      }
      .ogk-actions .btn-outline-secondary {
        background-color: #ffffff !important;
        border-color: #d0d7de !important;
        color: #57606a !important;
      }
      .ogk-actions .btn-outline-secondary:hover {
        background-color: #f6f8fa !important;
        color: #24292f !important;
      }
    `,
  },
  {
    id: "shadcn",
    label: "2. Shadcn / Zenn風 (モノトーン＋青リンクのみ)",
    dotColor: "#f1f5f9",
    dotBorder: "#0f172a",
    css: `
      :root {
        --bs-link-color: #2563eb;
        --bs-link-hover-color: #1d4ed8;
      }
      .ogk-filter-bar .btn-primary {
        background-color: #0f172a !important;
        border-color: #0f172a !important;
        color: #f8fafc !important;
      }
      .ogk-filter-bar .btn-outline-primary {
        background-color: #ffffff !important;
        border-color: #e2e8f0 !important;
        color: #475569 !important;
      }
      .ogk-filter-bar .btn-outline-primary:hover {
        background-color: #f8fafc !important;
        color: #0f172a !important;
      }
      /* 手数バッジ: スレートピル */
      .badge.bg-primary {
        background-color: #f1f5f9 !important;
        color: #0f172a !important;
        border: 1px solid #cbd5e1 !important;
        font-weight: 600 !important;
      }
      .badge.bg-secondary {
        background-color: #f8fafc !important;
        color: #475569 !important;
        border: 1px solid #e2e8f0 !important;
        font-weight: 500 !important;
      }
      .badge.bg-warning {
        background-color: #fef3c7 !important;
        color: #92400e !important;
        border: 1px solid #fde68a !important;
        font-weight: 600 !important;
      }
      .ogk-actions .btn-primary {
        background-color: #ffffff !important;
        border-color: #cbd5e1 !important;
        color: #0f172a !important;
        font-weight: 600;
      }
      .ogk-actions .btn-primary:hover {
        background-color: #f1f5f9 !important;
        border-color: #94a3b8 !important;
      }
      .ogk-actions .btn-outline-secondary {
        border-color: #e2e8f0 !important;
        color: #64748b !important;
      }
    `,
  },
  {
    id: "lichess",
    label: "3. Lichess / 将棋盤調和風 (亜麻色手数＋藍色ボタン)",
    dotColor: "#f5ebe0",
    dotBorder: "#2b4c6f",
    css: `
      :root {
        --bs-link-color: #2b4c6f;
        --bs-link-hover-color: #1a365d;
      }
      .ogk-filter-bar .btn-primary {
        background-color: #2b4c6f !important;
        border-color: #2b4c6f !important;
        color: #ffffff !important;
      }
      .ogk-filter-bar .btn-outline-primary {
        background-color: #ffffff !important;
        border-color: #cbd5e1 !important;
        color: #334e68 !important;
      }
      .ogk-filter-bar .btn-outline-primary:hover {
        background-color: #f0f4f8 !important;
      }
      /* 手数バッジ: 将棋盤と調和する亜麻色ウォームピル */
      .badge.bg-primary {
        background-color: #f5ebe0 !important;
        color: #5c3d2e !important;
        border: 1px solid #dfccb8 !important;
        font-weight: 600 !important;
      }
      .badge.bg-secondary {
        background-color: #f4f1ea !important;
        color: #57534e !important;
        border: 1px solid #d6d3cd !important;
        font-weight: 500 !important;
      }
      .badge.bg-warning {
        background-color: #fdf6e2 !important;
        color: #85581a !important;
        border: 1px solid #e6cf9c !important;
        font-weight: 600 !important;
      }
      .ogk-actions .btn-primary {
        background-color: #f0f4f8 !important;
        border-color: #bcccdc !important;
        color: #2b4c6f !important;
        font-weight: 600;
      }
      .ogk-actions .btn-primary:hover {
        background-color: #d9e2ec !important;
        border-color: #9fb3c8 !important;
      }
    `,
  },
  {
    id: "m3-tonal",
    label: "4. Google M3 Tonal風 (淡パステル色分け・ベタ塗りなし)",
    dotColor: "#e8f0fe",
    dotBorder: "#1967d2",
    css: `
      :root {
        --bs-link-color: #1967d2;
        --bs-link-hover-color: #174ea6;
      }
      .ogk-filter-bar .btn-primary {
        background-color: #d3e3fd !important;
        border-color: #a8c7fa !important;
        color: #0b57d0 !important;
        font-weight: 600;
      }
      .ogk-filter-bar .btn-outline-primary {
        background-color: #ffffff !important;
        border-color: #dadce0 !important;
        color: #3c4043 !important;
      }
      .ogk-filter-bar .btn-outline-primary:hover {
        background-color: #f1f3f4 !important;
      }
      /* 手数バッジ: ティール系パステルでボタンと色分け */
      .badge.bg-primary {
        background-color: #e0f2fe !important;
        color: #0369a1 !important;
        border: 1px solid #bae6fd !important;
        font-weight: 600 !important;
      }
      .badge.bg-secondary {
        background-color: #f1f5f9 !important;
        color: #475569 !important;
        border: 1px solid #cbd5e1 !important;
        font-weight: 500 !important;
      }
      .badge.bg-warning {
        background-color: #fef3c7 !important;
        color: #b45309 !important;
        border: 1px solid #fde68a !important;
        font-weight: 600 !important;
      }
      .ogk-actions .btn-primary {
        background-color: #e8f0fe !important;
        border-color: #d2e3fc !important;
        color: #1967d2 !important;
        font-weight: 600;
      }
      .ogk-actions .btn-primary:hover {
        background-color: #d2e3fc !important;
        border-color: #aecbfa !important;
      }
    `,
  },
  {
    id: "outline",
    label: "5. クリーンアウトライン (背景白＋細枠のみ)",
    dotColor: "#ffffff",
    dotBorder: "#1e88e5",
    css: `
      :root {
        --bs-link-color: #1976d2;
      }
      .ogk-filter-bar .btn-primary {
        background-color: #e3f2fd !important;
        border-color: #1976d2 !important;
        color: #1565c0 !important;
        font-weight: 600;
      }
      .ogk-filter-bar .btn-outline-primary {
        background-color: #ffffff !important;
        border-color: #dee2e6 !important;
        color: #495057 !important;
      }
      .badge.bg-primary {
        background-color: #ffffff !important;
        color: #334155 !important;
        border: 1px solid #cbd5e1 !important;
        font-weight: 600 !important;
      }
      .badge.bg-secondary {
        background-color: #ffffff !important;
        color: #64748b !important;
        border: 1px solid #e2e8f0 !important;
        font-weight: 500 !important;
      }
      .badge.bg-warning {
        background-color: #fffbeb !important;
        color: #b45309 !important;
        border: 1px solid #fcd34d !important;
        font-weight: 600 !important;
      }
      .ogk-actions .btn-primary {
        background-color: #ffffff !important;
        border-color: #90caf9 !important;
        color: #1976d2 !important;
        font-weight: 600;
      }
      .ogk-actions .btn-primary:hover {
        background-color: #e3f2fd !important;
        border-color: #1976d2 !important;
      }
    `,
  },
];

function buildDynamicThemeCss(themeId: string): string {
  const preset = THEME_PRESETS.find((t) => t.id === themeId);
  if (preset) return preset.css;
  return THEME_PRESETS[0].css;
}

export default function OgiekakoCollection(props: {
  onSelectProblem: (problem: OgiekakoProblem, preSolved: boolean) => void;
  onBackToSolver: () => void;
}) {
  const [filter, setFilter] = useState<FilterCategory>("selected");
  const [sortKey, setSortKey] = useState<SortKey>("date_desc");
  const [viewMode, setViewMode] = useState<ViewMode>("cards");
  const [query, setQuery] = useState("");
  const [previewTheme, setPreviewTheme] = useState<string | null>(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get("color")) return params.get("color")!;
    if (params.get("preview")) return "github";
    return null;
  });

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
      <style>{buildDynamicThemeCss(previewTheme ?? "github")}</style>
      {previewTheme && (
        <Card className="mb-3 border-secondary bg-light">
            <Card.Body className="py-2 px-3">
              <div className="d-flex flex-wrap align-items-center gap-2">
                <span className="fw-bold" style={{ fontSize: "0.85em" }}>
                  🎨 カラーテーマ比較プレビュー:
                </span>
                {THEME_PRESETS.map((t) => (
                  <Button
                    key={t.id}
                    size="sm"
                    variant={
                      previewTheme === t.id ? "dark" : "outline-secondary"
                    }
                    onClick={() => {
                      setPreviewTheme(t.id);
                      const url = new URL(window.location.href);
                      url.searchParams.set("color", t.id);
                      window.history.replaceState({}, "", url.toString());
                    }}
                  >
                    <span
                      style={{
                        display: "inline-block",
                        width: 10,
                        height: 10,
                        borderRadius: "50%",
                        backgroundColor: t.dotColor,
                        border: `1.5px solid ${t.dotBorder}`,
                        marginRight: 5,
                      }}
                    />
                    {t.label}
                  </Button>
                ))}
              </div>
            </Card.Body>
          </Card>
      )}
      <div className="d-flex justify-content-between align-items-center flex-wrap gap-2 mb-3">
        <div>
          <h1 className="mb-1">ogiekako 協力詰作品集</h1>
          <div className="text-muted" style={{ fontSize: "0.9em" }}>
            純粋な協力詰（全作一意解検証済み）。作品をクリックすると解図済み状態（手順再生可能）でソルバーを開きます。
          </div>
        </div>
        <Button variant="outline-secondary" onClick={props.onBackToSolver}>
          ← ソルバーに戻る
        </Button>
      </div>

      <Card className="mb-3">
        <Card.Body className="py-2 px-2 px-sm-3">
          <div className="d-flex flex-wrap gap-2 align-items-center justify-content-between">
            <div className="d-flex flex-wrap gap-1 ogk-filter-bar">
              <Button
                size="sm"
                variant={filter === "selected" ? "primary" : "outline-primary"}
                onClick={() => setFilter("selected")}
              >
                セレクション ({SELECTED_COUNT})
              </Button>
              <Button
                size="sm"
                variant={filter === "all" ? "primary" : "outline-primary"}
                onClick={() => setFilter("all")}
              >
                すべて ({OGIEKAKO_PROBLEMS.length})
              </Button>
              <Button
                size="sm"
                variant={filter === "award" ? "primary" : "outline-primary"}
                onClick={() => setFilter("award")}
              >
                Fairy TOPix 入賞
              </Button>
              <Button
                size="sm"
                variant={filter === "short" ? "primary" : "outline-primary"}
                onClick={() => setFilter("short")}
              >
                短・中編 (〜99手)
              </Button>
              <Button
                size="sm"
                variant={filter === "long" ? "primary" : "outline-primary"}
                onClick={() => setFilter("long")}
              >
                長編 (100〜999手)
              </Button>
              <Button
                size="sm"
                variant={filter === "ultralong" ? "primary" : "outline-primary"}
                onClick={() => setFilter("ultralong")}
              >
                超長編 (1000手〜)
              </Button>
              <Button
                size="sm"
                variant={filter === "smoke" ? "primary" : "outline-primary"}
                onClick={() => setFilter("smoke")}
              >
                煙詰
              </Button>
              <Button
                size="sm"
                variant={
                  filter === "hand_limit" ? "primary" : "outline-primary"
                }
                onClick={() => setFilter("hand_limit")}
              >
                受方持駒制限あり
              </Button>
            </div>

            <div className="d-flex gap-2 align-items-center flex-wrap mw-100">
              <InputGroup
                size="sm"
                style={{ width: "210px", maxWidth: "100%" }}
              >
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
                style={{ width: "170px", maxWidth: "100%" }}
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as SortKey)}
              >
                <option value="date_desc">発表年月（新しい順）</option>
                <option value="date_asc">発表年月（古い順）</option>
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
              <Col key={p.id} style={{ minWidth: 0 }}>
                <Card
                  className="h-100 shadow-sm overflow-hidden"
                  style={{ cursor: "pointer" }}
                  onClick={() => {
                    if (hasTextSelection()) return;
                    props.onSelectProblem(p, true);
                  }}
                >
                  <Card.Body className="p-2 p-sm-3">
                    <div className="d-flex gap-2 gap-sm-3 align-items-start">
                      <div className="flex-shrink-0">
                        <BoardThumbnail
                          sfen={p.sfen}
                          showWhiteHand={Boolean(p.defenderHand)}
                        />
                      </div>
                      <div
                        className="flex-grow-1 min-w-0 d-flex flex-column justify-content-between h-100"
                        style={{
                          minWidth: 0,
                          overflowWrap: "anywhere",
                          wordBreak: "break-word",
                        }}
                      >
                        <div style={{ minWidth: 0 }}>
                          <div className="d-flex justify-content-between align-items-start gap-1 mb-1">
                            <div style={{ minWidth: 0 }}>
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

                          <div className="mb-2 d-flex flex-wrap gap-1">
                            <Badge
                              bg="primary"
                              className="text-wrap text-start"
                              style={{ maxWidth: "100%", lineHeight: 1.35 }}
                            >
                              {p.rule} {p.moves.toLocaleString()}手
                            </Badge>
                            {p.defenderHand && (
                              <Badge
                                bg="secondary"
                                className="text-wrap text-start"
                                style={{ maxWidth: "100%", lineHeight: 1.35 }}
                              >
                                受方持駒：{p.defenderHand}
                              </Badge>
                            )}
                            {p.awards.map((award) => (
                              <Badge
                                key={award}
                                bg="warning"
                                text="dark"
                                className="text-wrap text-start"
                                style={{ maxWidth: "100%", lineHeight: 1.35 }}
                              >
                                {award}
                              </Badge>
                            ))}
                          </div>

                          <div
                            className="mb-1"
                            style={{ fontSize: "0.88em" }}
                          >
                            {p.url ? (
                              <a
                                href={p.url}
                                target="_blank"
                                rel="noreferrer"
                                draggable={false}
                                className="text-decoration-none text-reset"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  if (hasTextSelection()) {
                                    e.preventDefault();
                                  }
                                }}
                              >
                                <span className="fw-semibold">{p.source}</span>
                                <span
                                  className="ms-2 text-nowrap"
                                  style={{ color: "var(--bs-link-color)" }}
                                >
                                  [出典↗]
                                </span>
                              </a>
                            ) : (
                              <span className="fw-semibold">{p.source}</span>
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
                          className="mt-2 pt-1 d-flex flex-wrap gap-1 ogk-actions"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <Button
                            size="sm"
                            variant="primary"
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
                    onClick={() => {
                      if (hasTextSelection()) return;
                      props.onSelectProblem(p, true);
                    }}
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
                        {p.url ? (
                          <a
                            href={p.url}
                            target="_blank"
                            rel="noreferrer"
                            draggable={false}
                            className="text-decoration-none text-reset"
                            onClick={(e) => {
                              e.stopPropagation();
                              if (hasTextSelection()) {
                                e.preventDefault();
                              }
                            }}
                          >
                            <span className="fw-semibold">{p.source}</span>
                            <span
                              className="ms-2 text-nowrap"
                              style={{ color: "var(--bs-link-color)" }}
                            >
                              [出典↗]
                            </span>
                          </a>
                        ) : (
                          <span className="fw-semibold">{p.source}</span>
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
                      className="text-center text-nowrap ogk-actions"
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
