export async function* solveServer(sfen: string) {
  const utf8Decoder = new TextDecoder("utf-8");
  const response = await fetch("http://localhost:1234/solve", {
    method: "POST",
    body: sfen,
  });
  const reader = response.body!.getReader();

  let line = "";
  for (;;) {
    let { value, done } = await reader.read();
    if (done) {
      if (line) {
        yield line;
      }
      return;
    }
    const s = utf8Decoder.decode(value!);
    for (let i = 0; i < s.length; i++) {
      if (s[i] === "\n") {
        yield line;
        line = "";
        continue;
      }
      line += s[i];
    }
  }
}
