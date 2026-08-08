import { apiUrl, CancellationToken, Response } from ".";

/**
 * サーバーが使えない (つながらない・遅すぎる・途中で切れた) ことを表すエラー。
 * 呼び出し側はこれを受けたら wasm にフォールバックする。
 */
export class ServerUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ServerUnavailableError";
  }
}

type ServerEvent =
  | {
      ty: "progress";
      step: number;
    }
  | {
      ty: "error";
      message: string;
    }
  | {
      ty: "no_solution";
    }
  | {
      ty: "solved";
      response: {
        redundant: boolean;
        solutions: number;
        kif: string;
        sfen: string;
        from_white: boolean;
      };
    };

export async function solveServer(
  sfen: string,
  solutionLimit: number,
  cancelToken: CancellationToken,
  onStep: (step: number) => void,
  connectTimeoutMs: number
): Promise<Response | undefined> {
  // ヘッダが返るまでの時間だけを制限する。解図自体は何分かかってもよい。
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), Math.max(0, connectTimeoutMs));
  let response: globalThis.Response;
  try {
    response = await fetch(apiUrl(`/solve?solutions_upto=${solutionLimit + 1}`), {
      method: "POST",
      body: sfen,
      signal: controller.signal,
    });
  } catch {
    throw new ServerUnavailableError(
      controller.signal.aborted
        ? `サーバーが ${connectTimeoutMs}ms 以内に応答しませんでした。`
        : "サーバーに接続できませんでした。"
    );
  } finally {
    clearTimeout(timer);
  }

  if (!response.ok) {
    // 400 は局面が不正などサーバーが下した判断なのでそのまま表示する。
    // それ以外 (404/405/5xx など) はサーバーが機能していないとみなす。
    if (response.status === 400) {
      throw new Error((await response.text()) || "サーバーでの解図に失敗しました。");
    }
    throw new ServerUnavailableError(
      `サーバーが解図に失敗しました (HTTP ${response.status})。`
    );
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new ServerUnavailableError("サーバー応答を読み取れませんでした。");
  }

  const utf8Decoder = new TextDecoder("utf-8");
  let line = "";
  let nextYieldStep = nextAwait(0);
  for (;;) {
    let value: Uint8Array | undefined;
    let done: boolean;
    try {
      ({ value, done } = await reader.read());
    } catch {
      // ストリーム途中で回線が切れた場合もフォールバックさせる。
      throw new ServerUnavailableError("サーバーとの通信が中断されました。");
    }
    if (done) {
      if (line) {
        const event = JSON.parse(line) as ServerEvent;
        const res = handleServerEvent(event, onStep);
        return res === null ? undefined : res;
      }
      return undefined;
    }

    const s = utf8Decoder.decode(value!, { stream: true });
    for (let i = 0; i < s.length; i++) {
      if (s[i] === "\n") {
        if (!line) {
          continue;
        }
        const event = JSON.parse(line) as ServerEvent;
        const res = handleServerEvent(event, onStep);
        if (res !== null) {
          return res;
        }
        line = "";
        if (event.ty === "progress" && event.step >= nextYieldStep) {
          await yieldToBrowser();
          nextYieldStep = nextAwait(event.step);
        }
        if (cancelToken.isCanceled()) {
          reader.cancel().catch(() => undefined);
          return undefined;
        }
        continue;
      }
      line += s[i];
    }
  }
}

function handleServerEvent(
  event: ServerEvent,
  onStep: (step: number) => void
): Response | undefined | null {
  switch (event.ty) {
    case "progress":
      onStep(event.step);
      return null;
    case "error":
      throw new Error(event.message);
    case "no_solution":
      return undefined;
    case "solved":
      return {
        redundant: event.response.redundant,
        solutions: event.response.solutions,
        kif: event.response.kif,
        sfen: event.response.sfen,
        fromWhite: event.response.from_white,
      };
  }
}

function nextAwait(step: number) {
  if (step < 100) {
    return step + 1;
  }
  if (step < 1000) {
    return step + 10;
  }
  return step + 100;
}

async function yieldToBrowser() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}
