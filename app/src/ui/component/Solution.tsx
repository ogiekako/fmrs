import { useEffect, useRef } from "react";

export default function Solution(props: { kif: string }) {
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
        tweakKifForJs(url);
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
  }, [props.kif, outer, id]);
  return <div ref={outer}></div>;
}

function generateId(s: string): string {
  let n = 0;
  for (let i = 0; i < s.length; i++) {
    n = (n * 63 + s.charCodeAt(i)) % (1 << 30);
  }
  return "i" + n;
}

function tweakKifForJs(url: string) {
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
}
