/* @ts-self-types="./index.d.ts" */

import * as wasm from "./index_bg.wasm";
import { __wbg_set_wasm } from "./index_bg.js";
__wbg_set_wasm(wasm);
wasm.__wbindgen_start();
export {
    Algorithm, BackwardSearch, Solver, greet
} from "./index_bg.js";
