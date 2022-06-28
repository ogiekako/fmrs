import { useEffect } from "react";

export function Solution(props: { jkf: string }) {
    const id = "solution";
    useEffect(() => {
        const blob = new Blob([props.jkf], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        KifuForJS.load(url, id);
    }, [props.jkf])
    return <div id={id}></div>
}
