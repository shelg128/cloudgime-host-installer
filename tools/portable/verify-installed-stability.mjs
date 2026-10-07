import fs from'node:fs';import path from'node:path';import crypto from'node:crypto';import{fileURLToPath}from'node:url';
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
// Read-only operator regression fence, not machine authentication or authorization.
export function verifyInstalledBaseline(manifest,root){
 if(manifest?.schema!==1||manifest.kind!=='operator-installed-stability-baseline'||!Array.isArray(manifest.files)||!manifest.files.length||manifest.files.length>64)throw Error('BASELINE_SCHEMA');
 root=path.resolve(root);const seen=new Set();for(const f of manifest.files){if(typeof f.path!=='string'||!/^[a-z0-9_./-]+$/i.test(f.path)||f.path.startsWith('/')||f.path.split('/').some(p=>!p||p==='.'||p==='..')||seen.has(f.path.toLowerCase())||!/^[a-f0-9]{64}$/.test(f.sha256))throw Error('BASELINE_ENTRY');seen.add(f.path.toLowerCase());const actual=path.resolve(root,f.path);if(!actual.startsWith(root+path.sep))throw Error('BASELINE_ESCAPE');if(!fs.existsSync(actual)||sha(actual)!==f.sha256)throw Error('INSTALLED_BASELINE_CHANGED:'+f.path);}
 if(!seen.has('moonlight/server/display-prepare-helper.exe')||!seen.has('moonlight/server/display-prepare-helper.exe.encoder-contract.json'))throw Error('BASELINE_HELPER_REQUIRED');
 return {verified:true,fileCount:manifest.files.length,scope:'hash identity only, not health/rental authority'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){const[file,root]=process.argv.slice(2);if(!file||!root)throw Error('USAGE manifest.json installedRoot');console.log(JSON.stringify(verifyInstalledBaseline(JSON.parse(fs.readFileSync(file,'utf8')),root)));}
