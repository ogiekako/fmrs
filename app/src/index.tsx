import React from "react";
import ReactDOM from "react-dom/client";
import "bootstrap/dist/css/bootstrap.min.css";
import "./index.css";
import App from "./App";
import reportWebVitals from "./reportWebVitals";
import { loadWasm } from "./wasm_main";
import { loadKifuForJS } from "./kifu_for_js";

const root = ReactDOM.createRoot(
  document.getElementById("root") as HTMLElement,
);
// 画面は wasm と解答再生盤 (kifu-for-js) を待たずに描画する。どちらも解答を
// 表示するときに要るので、描画と並行して先読みしておく。
root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
loadWasm().catch(() => {});
loadKifuForJS().catch((e) => console.error(e));

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
