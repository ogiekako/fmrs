import { useEffect, useRef } from "react";

export function Solution(props: { jkf: string }) {
    const outer = useRef<HTMLDivElement>(null);
    const id = generateId(props.jkf);

    useEffect(() => {
        if (!outer.current) {
            return;
        }
        while (outer.current.firstChild) {
            outer.current.removeChild(outer.current.firstChild);
        }
        const inner = document.createElement('div');
        inner.setAttribute('id', id);
        outer.current.appendChild(inner);

        const blob = new Blob([props.jkf], { type: 'application/json' });
        const url = URL.createObjectURL(blob);

        // Prevent flushing by showing the element after it is fully loaded.
        inner.style.visibility = 'hidden';

        KifuForJS.load(url, id).then(() => {
            // Somehow setTimeout is needed to prevent flushing.
            setTimeout(() => inner.style.visibility = "", 0);
        })
    }, [props.jkf, outer, id])
    return <div ref={outer}></div>
}

function generateId(s: string): string {
    let n = 0;
    for (let i = 0; i < s.length; i++) {
        n = (n * 63 + (s.charCodeAt(i))) % (1 << 30);
    }
    return "i" + n;
}
