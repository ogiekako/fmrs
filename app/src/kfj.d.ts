interface IOptions {
  kifu?: string;
  src?: string;
}

declare module KifuForJS {
  function loadString(kifu: string, idOrOptions?: string): Promise<unknown>;
  function load(option: IOptions, idOrOptions?: string): Promise<unknown>;
  function load(url: string, idOrOptions?: string): Promise<unknown>;
}
