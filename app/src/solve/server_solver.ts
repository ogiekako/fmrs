import { CancellationToken, Response } from ".";

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
  onStep: (step: number) => void
): Promise<Response | undefined> {
  let response: globalThis.Response;
  try {
    response = await fetch(`/solve?solutions_upto=${solutionLimit + 1}`, {
      method: "POST",
      body: sfen,
    });
  } catch {
    throw new Error("サーバーに接続できませんでした。");
  }

  if (!response.ok) {
    throw new Error((await response.text()) || "サーバーでの解図に失敗しました。");
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("サーバー応答を読み取れませんでした。");
  }

  const utf8Decoder = new TextDecoder("utf-8");
  let line = "";
  let nextYieldStep = nextAwait(0);
  for (;;) {
    const { value, done } = await reader.read();
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
