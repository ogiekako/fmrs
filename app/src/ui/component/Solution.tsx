import { useEffect, useRef } from "react";
import { Button } from "react-bootstrap";
import * as types from "../types";
import * as model from "../../model";

export default function Solution(props: {
  kif: string;
  stone: boolean[][];
  fromWhite: boolean;
  dispatch: types.Dispatcher;
}) {
  const outer = useRef<HTMLDivElement>(null);
  const id = generateId(props.kif);

  let validurl = undefined as string | undefined;

  useEffect(() => {
    const update = setTimeout(() => {
      if (!outer.current) {
        return;
      }
      while (outer.current.firstChild) {
        outer.current.removeChild(outer.current.firstChild);
      }
      const inner = document.createElement("div");
      inner.setAttribute("id", id);
      outer.current.appendChild(inner);

      if (validurl) URL.revokeObjectURL(validurl);
      validurl = URL.createObjectURL(
        new Blob([props.kif], { type: "text/plain" })
      );
      const url = validurl;

      // Prevent flushing by showing the element after it is fully loaded.
      inner.style.visibility = "hidden";

      KifuForJS.loadString(props.kif, id).then(() => {
        tweakKifForJs(url, props.stone, props.fromWhite);
        inner.style.visibility = "";
      });
    }, 200);

    return () => {
      clearTimeout(update);
      if (validurl) {
        URL.revokeObjectURL(validurl);
        validurl = undefined;
      }
    };
  }, [props.kif, props.stone, outer, id]);
  return (
    <div className="position-relative mb-2">
      <div ref={outer}></div>
      <div
        className="position-absolute top-0"
        style={{ maxWidth: "570px", minWidth: "570px" }}
      >
        <Button
          className="btn-secondary position-absolute end-0"
          onClick={(e) => {
            e.preventDefault();
            const position = extractPosition(props.fromWhite, props.stone);
            if (position) {
              props.dispatch({
                ty: "set-position",
                position: position!,
              });
            }
          }}
        >
          図面に反映
        </Button>
      </div>
    </div>
  );
}

function generateId(s: string): string {
  let n = 0;
  for (let i = 0; i < s.length; i++) {
    n = (n * 63 + s.charCodeAt(i)) % (1 << 30);
  }
  return "i" + n;
}

function tweakKifForJs(url: string, stone: boolean[][], fromWhite: boolean) {
  // Remove preset info
  const info = document.getElementsByClassName(
    "kifuforjs-info"
  )?.[0] as HTMLElement;
  info.setAttribute("style", "visibility: hidden;");
  // Remove comment section
  const comment = document.getElementsByClassName(
    "kifuforjs-comment"
  )?.[0] as HTMLElement;
  comment.setAttribute("style", "display: none;");

  // Intersept the download button to open a kif file.
  const dl = document.getElementsByClassName(
    "kifuforjs-dl"
  )?.[0] as HTMLButtonElement;
  if (dl && !dl.getElementsByClassName("interseptor").length) {
    dl.disabled = false;
    dl.style.position = "relative";
    const interseptor = document.createElement("span");
    dl.insertBefore(interseptor, dl.firstChild);

    interseptor.className = "interseptor";
    interseptor.style.top = "0";
    interseptor.style.left = "0";
    interseptor.style.minWidth = "100%";
    interseptor.style.position = "absolute";
    interseptor.style.minHeight = "100%";
    interseptor.style.zIndex = "1";
    interseptor.style.backgroundColor = "transparent";
    // div.style.border = "1px solid red"; // debug

    interseptor.addEventListener("click", (e) => {
      e.stopPropagation();
      const a = document.createElement("a");
      document.body.appendChild(a);
      a.style.visibility = "hidden";
      a.href = url!;
      a.download = "solution.kif";
      a.dispatchEvent(new MouseEvent("click"));
    });
  }

  const cells = document.getElementsByClassName("kifuforjs-cell");
  for (let i = 0; i < cells.length; i++) {
    const cell = cells[i] as HTMLElement;
    const x = fromWhite ? i % 9 : 8 - (i % 9);
    const y = fromWhite ? 8 - Math.floor(i / 9) : Math.floor(i / 9);
    if (stone[y][x]) {
      const div = document.createElement("div");
      cell.style.position = "relative";
      cell.appendChild(div);
      const [w, h] = [cell.scrollWidth, cell.scrollHeight];
      const r = Math.round(w * 0.85);
      div.style.position = "absolute";
      div.style.left = Math.round((w - r) / 2) + "px";
      div.style.top = Math.round((h - r) / 2) + "px";
      div.style.width = r + "px";
      div.style.height = r + "px";
      div.style.borderRadius = "50%";
      div.style.backgroundColor = "black";
    }
  }
  if (fromWhite) {
    (
      document.getElementsByClassName(
        "kifuforjs-control-tools"
      )?.[0] as HTMLElement
    ).click();
  }
}

function extractPosition(
  fromWhite: boolean,
  stone: boolean[][]
): model.Position | undefined {
  const handBodies = document.getElementsByClassName("kifuforjs-hand-body");
  if (handBodies.length !== 2) {
    return undefined;
  }
  const whiteHands = extractHands(handBodies[0]);
  const blackHands = extractHands(handBodies[1]);

  const cells = document.getElementsByClassName("kifuforjs-cell");
  if (cells.length != 81) {
    return undefined;
  }
  const board = model.emptyBoard();
  for (let i = 0; i < 81; i++) {
    const x = Math.floor(i / 9);
    const y = 8 - (i % 9);
    if (stone[x][y]) {
      board[x][y] = "O";
      continue;
    }

    const aria = cells[i].querySelector("[aria-label]")?.ariaLabel;
    if (!aria) continue;

    let color: model.Color;
    if (aria.includes("先手")) {
      color = fromWhite ? "white" : "black";
    } else if (aria.includes("後手")) {
      color = fromWhite ? "black" : "white";
    } else {
      continue;
    }
    const promoted =
      aria.includes("成") ||
      aria.includes("と") ||
      aria.includes("馬") ||
      aria.includes("龍");
    const kind = (
      {
        歩: "P",
        と: "P",
        香: "L",
        桂: "N",
        銀: "S",
        金: "G",
        角: "B",
        馬: "B",
        飛: "R",
        龍: "R",
        玉: "K",
        王: "K",
      } as const
    )[aria[aria.length - 1]];
    if (kind) {
      board[x][y] = { color, kind, promoted };
    }
  }
  return {
    board,
    hands: {
      white: whiteHands,
      black: blackHands,
    },
  };
}

function extractHands(handBody: Element): model.Hands {
  const res = model.emptyHands();
  for (const e of handBody.getElementsByClassName("kifuforjs-pieceinhand")) {
    if (!e.childNodes.length) {
      continue;
    }
    const aria = e.ariaLabel;
    if (!aria) continue;
    let count = 1;
    if (aria.endsWith("枚")) {
      const m = aria.match(/(\d+)枚$/);
      if (m) {
        count = parseInt(m[1]);
      }
    }
    const kind = (
      {
        歩: "P",
        香: "L",
        桂: "N",
        銀: "S",
        金: "G",
        角: "B",
        飛: "R",
      } as const
    )[aria[0]];
    if (!kind) continue;
    res[kind] = count;
  }
  return res;
}
