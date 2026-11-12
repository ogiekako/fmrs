import React from "react";
import ReactDOM from "react-dom/client";
import "bootstrap/dist/css/bootstrap.min.css";
import "./index.css";
import App from "./App";
import reportWebVitals from "./reportWebVitals";
import { loadWasm } from "./wasm_main";
import { loadKifuForJS } from "./kifu_for_js";
import { isOgiekakoPageUrl } from "./model";

const root = ReactDOM.createRoot(
  document.getElementById("root") as HTMLElement,
);

function render() {
  root.render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

// どちらも描画とは並行して読み始める。作品集ページから作品を開くまでに
// 読み終わっていれば、遷移時に待たされない。
const wasmLoaded = loadWasm();
loadKifuForJS().catch((e) => console.error(e));

if (isOgiekakoPageUrl()) {
  // 作品集ページは wasm を使わないので、読み込みを待たずに描画する。
  // ソルバー画面へ移るときに wasm を待つ (Editor 参照)。
  render();
} else {
  // ソルバー画面は初期状態の計算 (SFEN の手番判定や解答済み作品の棋譜生成) に
  // wasm を同期的に使うので、読み込みを待ってから描画する。
  wasmLoaded.then(render, (e) => {
    console.error(e);
    root.render(
      <div className="container my-3">
        読み込みに失敗しました。ページを再読み込みしてください。
      </div>,
    );
  });
}

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
