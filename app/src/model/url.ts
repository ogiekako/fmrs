declare const FMRS_BASE_PATH: string;

export type SolverMode = "helpmate" | "futsuu";

function basePath(): string {
  if (typeof FMRS_BASE_PATH === "string" && FMRS_BASE_PATH) {
    return FMRS_BASE_PATH;
  }
  return "/";
}

// 作品の恒久リンク。/fmrs/<名前> で該当局面を開く。
// 読み込み後、URL は通常の SFEN 形式に置き換わる。
const NAMED_POSITIONS: Record<string, string> = {
  noroshi:
    "8+P/6K1p/4S3+P/3+Pp+PP+pn/2BpPP1p1/3Bk+p+p1P/1RRs+lgLNG/2SPNlNPG/G+pL4S1 b - 1",
};

function namedPosition(): string | null {
  const base = basePath();
  const path = window.location.pathname;
  if (!path.startsWith(base)) {
    return null;
  }
  const name = path.slice(base.length).replace(/\/$/, "").toLowerCase();
  return NAMED_POSITIONS[name] ?? null;
}

export function helpmatePagePath(): string {
  return basePath();
}

export function futsuuPagePath(): string {
  return basePath() + "futsuu";
}

export function ogiekakoPagePath(): string {
  return basePath() + "ogiekako";
}

export function isOgiekakoPageUrl(): boolean {
  const base = basePath();
  const path = window.location.pathname;
  if (!path.startsWith(base)) {
    return false;
  }
  const name = path.slice(base.length).replace(/\/$/, "").toLowerCase();
  return name === "ogiekako";
}

export function isFutsuuPageUrl(): boolean {
  const base = basePath();
  const path = window.location.pathname;
  if (!path.startsWith(base)) {
    return false;
  }
  const rest = path.slice(base.length);
  return (
    rest === "futsuu" ||
    rest === "futsuu/" ||
    rest.startsWith("futsuu/")
  );
}

export function isRootFutsuuUrl(): boolean {
  const base = basePath();
  const path = window.location.pathname;
  if (!path.startsWith(base)) {
    return false;
  }
  const rest = path.slice(base.length);
  return rest === "futsuu" || rest === "futsuu/";
}

export function solverModeFromUrl(): SolverMode {
  return isFutsuuPageUrl() ? "futsuu" : "helpmate";
}

export function isNamedPositionUrl(): boolean {
  return namedPosition() !== null;
}

export function sfenFromUrl(): string | null {
  if (isOgiekakoPageUrl()) {
    return null;
  }
  const base = basePath();
  const path = window.location.pathname;
  if (isFutsuuPageUrl()) {
    const prefix = base + "futsuu/";
    if (path.startsWith(prefix) && path.length > prefix.length) {
      const rest = path.slice(prefix.length);
      try {
        return decodeURIComponent(rest).replace(/_/g, " ");
      } catch {
        return rest.replace(/_/g, " ");
      }
    }
    return new URL(window.location.href).searchParams.get("sfen");
  }
  const named = namedPosition();
  if (named) {
    return named;
  }
  if (path.startsWith(base) && path.length > base.length) {
    const rest = path.slice(base.length);
    try {
      return decodeURIComponent(rest).replace(/_/g, " ");
    } catch {
      return rest.replace(/_/g, " ");
    }
  }
  return new URL(window.location.href).searchParams.get("sfen");
}

export function sfenToPath(
  sfen: string,
  mode: SolverMode = "helpmate"
): string {
  const encoded = sfen.replace(/ /g, "_");
  if (mode === "futsuu") {
    return basePath() + "futsuu/" + encoded;
  }
  return basePath() + encoded;
}

export function isOldFormatUrl(): boolean {
  const base = basePath();
  const path = window.location.pathname;
  if (
    path === base ||
    path === base.replace(/\/$/, "") ||
    isRootFutsuuUrl()
  ) {
    return new URL(window.location.href).searchParams.has("sfen");
  }
  return false;
}

export function isSolvedFromUrl(): boolean {
  const url = new URL(window.location.href);
  const param = url.searchParams.get("solved");
  return param === "1" || param === "true" || url.hash === "#solved";
}
