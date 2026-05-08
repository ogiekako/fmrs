declare const FMRS_BASE_PATH: string;

function basePath(): string {
  if (typeof FMRS_BASE_PATH === "string" && FMRS_BASE_PATH) {
    return FMRS_BASE_PATH;
  }
  return "/";
}

export function sfenFromUrl(): string | null {
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
