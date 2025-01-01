declare module KifuForJS {
  function loadString(kifu: string, idOrOptions?: string): Promise<unknown>;
  function load(file: string, idOrOptions?: string): Promise<unknown>;
}
