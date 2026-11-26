import { apiUrl, CancellationToken, FutsuuResponse } from ".";
import { ServerUnavailableError } from "./server_solver";

type FutsuuServerEvent =
  | {
      ty: "progress";
      nodes: number;
    }
  | {
      ty: "error";
      message: string;
    }
  | {
      ty: "no_solution";
      nodes: number;
    }
  | {
      ty: "suspended";
      nodes: number;
    }
  | {
      ty: "solved";
      response: {
        steps: number;
        redundant: boolean;
        nodes: number;
        kif: string;
        sfen: string;
      };
    };

export async function solveFutsuuServer(
  sfen: string,
  cancelToken: CancellationToken,
  onNodes: (nodes: number) => void,
  connectTimeoutMs: number
): Promise<FutsuuResponse | undefined> {
  const controller = new AbortController();
  const timer = setTimeout(
    () => controller.abort(),
    Math.max(0, connectTimeoutMs)
  );
  let response: globalThis.Response;
  try {
    response = await fetch(apiUrl("/solve_futsuu"), {
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
    if (response.status === 400) {
      throw new Error(
        (await response.text()) || "サーバーでの解図に失敗しました。"
      );
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
  let lastYield = Date.now();
  const unsubscribeCancel = cancelToken.onCancel(() => {
    reader.cancel().catch(() => undefined);
  });
  try {
    for (;;) {
      let value: Uint8Array | undefined;
      let done: boolean;
      try {
        ({ value, done } = await reader.read());
      } catch {
        if (cancelToken.isCanceled()) {
          return undefined;
        }
        throw new ServerUnavailableError("サーバーとの通信が中断されました。");
      }
      if (cancelToken.isCanceled()) {
        return undefined;
      }
      if (done) {
        if (line) {
          const event = JSON.parse(line) as FutsuuServerEvent;
          const res = handleServerEvent(event, onNodes);
          return res ?? undefined;
        }
        return undefined;
      }

      const s = utf8Decoder.decode(value!, { stream: true });
      for (let i = 0; i < s.length; i++) {
        if (s[i] === "\n") {
          if (!line) {
            continue;
          }
          const event = JSON.parse(line) as FutsuuServerEvent;
          const res = handleServerEvent(event, onNodes);
          if (res !== null) {
            return res;
          }
          line = "";
          const now = Date.now();
          if (now - lastYield >= 50) {
            await new Promise((resolve) => setTimeout(resolve, 0));
            lastYield = Date.now();
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
  } finally {
    unsubscribeCancel();
  }
}

function handleServerEvent(
  event: FutsuuServerEvent,
  onNodes: (nodes: number) => void
): FutsuuResponse | null {
  switch (event.ty) {
    case "progress":
      onNodes(event.nodes);
      return null;
    case "error":
      throw new Error(event.message);
    case "no_solution":
      return { ty: "no-mate", nodes: event.nodes };
    case "suspended":
      return { ty: "suspended", nodes: event.nodes };
    case "solved":
      return {
        ty: "mate",
        steps: event.response.steps,
        redundant: event.response.redundant,
        nodes: event.response.nodes,
        kif: event.response.kif,
        sfen: event.response.sfen,
      };
  }
}
