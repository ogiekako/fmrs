// 解答再生盤 (kifu-for-js) を CDN から読み込む。
//
// 以前は HTML に同期 <script> で書いていたため、別ドメイン (jsdelivr) への
// 接続とダウンロード (約 180KB) が終わるまで main.js が実行されず、再生盤を
// 使わない作品集ページの表示まで遅れていた。必要になった時点 (と、初回描画後の
// 先読み) で読み込む。

const KIFU_FOR_JS_URL =
  "https://cdn.jsdelivr.net/npm/kifu-for-js@4/bundle/kifu-for-js.min.js";

let loading: Promise<void> | undefined;

export function loadKifuForJS(): Promise<void> {
  if (!loading) {
    loading = new Promise<void>((resolve, reject) => {
      const script = document.createElement("script");
      script.src = KIFU_FOR_JS_URL;
      script.charset = "utf-8";
      script.onload = () => resolve();
      script.onerror = () => {
        script.remove();
        loading = undefined;
        reject(new Error("kifu-for-js の読み込みに失敗しました"));
      };
      document.head.appendChild(script);
    });
  }
  return loading;
}
