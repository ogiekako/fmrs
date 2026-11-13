// メインスレッドから wasm を使うための入口。
//
// wasm_api を静的に import すると、webpack (asyncWebAssembly) は main.js 全体を
// wasm (圧縮後でも約 500KB) のダウンロードとコンパイルが終わるまで止め、
// モバイル回線では初期表示が数秒遅れる。そこで画面は wasm を待たずに描画し、
// wasm は動的 import で裏読みする。
//
// メインスレッドで wasm を使うのは解答表示の付加情報 (既知作品の KIF 生成と
// 一本道判定) だけで、これらは useWasm() で読み込み後に表示する。局面の手番判定は
// TS で持っている (model/check.ts)。解図・自動逆算は Worker が各自で wasm を読む。
import { useSyncExternalStore } from "react";

export type WasmApi = typeof import("./wasm_api");

let loaded: WasmApi | undefined;
let loading: Promise<WasmApi> | undefined;
const listeners = new Set<() => void>();

export function loadWasm(): Promise<WasmApi> {
  if (!loading) {
    loading = import("./wasm_api").then((m) => {
      loaded = m;
      listeners.forEach((l) => l());
      return m;
    });
    // 失敗したら次の呼び出しで読み直せるようにする。
    loading.catch((e) => {
      console.error(e);
      loading = undefined;
    });
  }
  return loading;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  loadWasm().catch(() => {});
  return () => {
    listeners.delete(listener);
  };
}

// 読み込み済みなら wasm、未読み込みなら undefined を返す。読み込みが終わると
// 呼び出したコンポーネントが再描画される。
export function useWasm(): WasmApi | undefined {
  return useSyncExternalStore(subscribe, () => loaded);
}
