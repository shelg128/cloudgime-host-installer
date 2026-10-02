const test=require('node:test'),assert=require('node:assert/strict'),fs=require('fs'),path=require('path');const {patchRoot}=require('../tools/apply_lazy_pipeline_static.cjs');
const fixture=`import { readExplicitStreamRoute } from "./route_policy.js";
import { gatherPipeInfo } from "./pipeline/index.js";
function* start(){
 const pipesInfo = yield gatherPipeInfo();
 this.debugLog(\`Pipe inventory ready (\${pipesInfo.size})\`);
 const pipesInfo = yield gatherPipeInfo();
 this.debugLog(\`Pipeline capability snapshot ready (\${pipesInfo.size})\`);
 return readExplicitStreamRoute();
}`;
test('minimal deployed root patch preserves explicit route code',()=>{const p=patchRoot(fixture);assert.match(p,/readExplicitStreamRoute/);assert.doesNotMatch(p,/gatherPipeInfo/);assert.equal(patchRoot(p),p);});
test('unknown compiled contract refuses patch instead of replacing module',()=>assert.throws(()=>patchRoot('import { gatherPipeInfo } from "./pipeline/index.js";'),/CONTRACT/));
test('startup has no eager global inventory; each candidate probes lazily',()=>{const r=path.resolve(__dirname,'../web/stream');const main=fs.readFileSync(path.join(r,'index.ts'),'utf8');assert.doesNotMatch(main,/await gatherPipeInfo\(/);for(const p of ['audio/pipeline.ts','video/pipeline.ts']){const s=fs.readFileSync(path.join(r,p),'utf8');assert.doesNotMatch(s,/await gatherPipeInfo\(/);assert.match(s,/await getPipeInfo\(pipe\)/);assert.ok(s.indexOf('if (pipeline.input != type)')<s.indexOf('await getPipeInfo(pipe)'));}});
test('worker probe settles errors outside event throw and always terminates',()=>{const s=fs.readFileSync(path.resolve(__dirname,'../web/stream/pipeline/worker_pipe.ts'),'utf8').split('readonly implementationName')[0];assert.match(s,/worker\.terminate\(\)/);assert.match(s,/worker\.onmessageerror/);assert.match(s,/12000/);assert.ok(s.indexOf('worker.onmessage =')<s.indexOf('worker.postMessage(sendMessage)'));assert.doesNotMatch(s,/throw message\.log/);});
