declare const FMRS_BASE_PATH: string;

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

export function isNamedPositionUrl(): boolean {
  return namedPosition() !== null;
}

export function sfenFromUrl(): string | null {
  const named = namedPosition();
  if (named) {
    return named;
  }
  const base = basePath();
  const path = window.location.pathname;
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

export function sfenToPath(sfen: string): string {
  return basePath() + sfen.replace(/ /g, "_");
}

export function isOldFormatUrl(): boolean {
  const base = basePath();
  const path = window.location.pathname;
  if (path === base || path === base.replace(/\/$/, "")) {
    return new URL(window.location.href).searchParams.has("sfen");
  }
  return false;
}
