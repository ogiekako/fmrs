import fs from 'node:fs';
import * as glue from './index_bg.js';
const bytes=fs.readFileSync(new URL('./index_bg.wasm',import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, { './index_bg.js': glue });
glue.__wbg_set_wasm(instance.exports);
const sfen = process.argv[2] || '8+P/6K1p/4S3+P/3+Pp+PP+pn/2BpPP1p1/3Bk+p+p1P/1RRs+lgLNG/2SPNlNPG/G+pL4S1 b - 1';
const solver=new glue.Solver(sfen, 2, glue.Algorithm.Standard);
const start=performance.now(); let step=0;
while(!solver.solutions_found() && !solver.no_solution()) { step=solver.advance(); if(step%10===1)console.error('depth',step,'ms',Math.round(performance.now()-start)); }
const out={sfen,solutions:solver.solutions_count(),moves:solver.solutions_sfen(),kif:solver.solutions_kif(),redundant:solver.redundant(),time_ms:performance.now()-start};
console.log(JSON.stringify(out,null,2));
solver.free();
