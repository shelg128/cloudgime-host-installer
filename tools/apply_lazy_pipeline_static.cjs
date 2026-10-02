const fs=require('fs'),path=require('path'),crypto=require('crypto');
const names=['stream/index.js','stream/pipeline/index.js','stream/pipeline/worker_pipe.js','stream/video/pipeline.js','stream/audio/pipeline.js'];
function patchRoot(s){
 if(s.includes('CG_LAZY_PIPELINE_STARTUP_V1'))return s;
 const before=s; s=s.replace('import { gatherPipeInfo, getPipe }','import { getPipe }').replace(/^import \{ gatherPipeInfo \} from [^\n]+\n/m,'');
 for(const label of ['Pipe inventory ready','Pipeline capability snapshot ready']){
  const escaped=label.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
  const re=new RegExp('^[ \\t]*const pipesInfo = yield gatherPipeInfo\\(\\);\\r?\\n[ \\t]*this\\.debugLog\\(`'+escaped+' \\(\\$\\{pipesInfo\\.size\\}\\)`\\);\\r?\\n','m');
  if(!re.test(s))throw Error('ROOT_CONTRACT_'+label);s=s.replace(re,'');
 }
 if(s===before||/gatherPipeInfo/.test(s))throw Error('ROOT_NOT_PATCHED');
 return '// CG_LAZY_PIPELINE_STARTUP_V1\n'+s;
}
function prepare({baseline,compiled,output}){
 fs.mkdirSync(output,{recursive:true});const manifest=[];
 for(const n of names){const original=fs.readFileSync(path.join(baseline,n),'utf8');let after;
  if(n==='stream/index.js')after=patchRoot(original);
  else{after=fs.readFileSync(path.join(compiled,n),'utf8');const tag=original.match(/\.js\?v=([^"']+)/)?.[1];if(tag)after=after.replace(/((?:\.{1,2}\/)[^"']+\.js)(["'])/g,'$1?v='+tag+'$2');}
  fs.mkdirSync(path.dirname(path.join(output,n)),{recursive:true});fs.writeFileSync(path.join(output,n),after);
  manifest.push({name:n,before:crypto.createHash('sha256').update(original).digest('hex'),after:crypto.createHash('sha256').update(after).digest('hex')});
 }
 fs.writeFileSync(path.join(output,'manifest.json'),JSON.stringify({schema:1,purpose:'Only lazy capability discovery; preserve deployed route/mobile/build behavior',files:manifest},null,2));return manifest;
}
module.exports={patchRoot,prepare,names};
if(require.main===module){const [baseline,compiled,output]=process.argv.slice(2);if(!baseline||!compiled||!output)throw Error('BASELINE_COMPILED_OUTPUT_REQUIRED');console.log(JSON.stringify(prepare({baseline,compiled,output})));}
