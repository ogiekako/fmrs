// メインスレッドから wasm を使うための入口。
//
// wasm_api を静的に import すると、webpack (asyncWebAssembly) は main.js 全体を
// wasm (圧縮後でも約 500KB) のダウンロードとコンパイルが終わるまで止める。
// wasm を使わない作品集ページまでそれを待つことになり、モバイル回線では表示が
// 数秒遅れていた。そこで wasm は動的 import で読み、使う所だけがそれを待つ。
// (Worker 側は各自で動的 import しているので、ここは経由しない。)

type WasmApi = typeof import("./wasm_api");

let loaded: WasmApi | undefined;
let loading: Promise<WasmApi> | undefined;

export function loadWasm(): Promise<WasmApi> {
  if (!loading) {
    loading = import("./wasm_api").then((m) => (loaded = m));
    // 失敗したら次の呼び出しで読み直せるようにする。
    loading.catch(() => {
      loading = undefined;
    });
  }
  return loading;
}

// 読み込み済みなら wasm を返す。未読み込みなら undefined。
export function wasmIfLoaded(): WasmApi | undefined {
  return loaded;
}

// 読み込み済みであることが前提の箇所で使う。ソルバー画面は wasm の読み込みを
// 待ってから描画するので、そこから辿られる処理はこれで同期的に呼べる。
export function wasm(): WasmApi {
  if (!loaded) {
    throw new Error("wasm がまだ読み込まれていません");
  }
  return loaded;
}
