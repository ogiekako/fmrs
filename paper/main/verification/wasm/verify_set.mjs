import fs from 'node:fs';
import * as glue from './index_bg.js';
const bytes=fs.readFileSync(new URL('./index_bg.wasm',import.meta.url));
const {instance}=await WebAssembly.instantiate(bytes,{'./index_bg.js':glue});
glue.__wbg_set_wasm(instance.exports);
const lines=fs.readFileSync('../sources/40piece_solutions.txt','utf8').trim().split('\n');
const out=fs.openSync('../all_2618_verification.jsonl','w');
const start=performance.now();let i=0;
for(const line of lines){
  const sfen=decodeURIComponent(line.trim().replace('https://ogiekako.github.io/fmrs/','')).replaceAll('_',' ');
  const solver=new glue.Solver(sfen,2,glue.Algorithm.Standard);
  while(!solver.solutions_found()&&!solver.no_solution())solver.advance();
  const moves=solver.solutions_sfen(); const r={index:i,sfen,solutions:solver.solutions_count(),plies:moves.trim().split(/\s+/).length,moves,redundant:solver.redundant()};
  fs.writeSync(out,JSON.stringify(r)+'\n');solver.free();
  if(r.solutions!==1||r.plies!==113||r.redundant)throw new Error(JSON.stringify(r));
  if(++i%100===0)console.error('verified',i,'elapsed_ms',Math.round(performance.now()-start));
}
fs.closeSync(out);console.log(JSON.stringify({count:i,all_unique:true,all_113:true,elapsed_ms:performance.now()-start},null,2));
