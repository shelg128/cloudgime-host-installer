import fs from 'node:fs';import path from 'node:path';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';import {fileURLToPath} from 'node:url';
export const contract='cloudgime-managed-display-ccd-v1';
export function hash(file){return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');}
export function sources(root){return Object.fromEntries(fs.readdirSync(root).filter(n=>n.endsWith('.cs')||n.endsWith('.csproj')).sort().map(n=>[n,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,n),'utf8').replaceAll('\r\n','\n')).digest('hex')]));}
export function verifyManifest(manifest,exe,sourceRoot){
 if(manifest?.schema!==1||manifest.contract!==contract||manifest.binarySha256!==hash(exe))throw Error('DISPLAY_BINARY_CONTRACT_MISMATCH');
 const expected=sources(sourceRoot);if(JSON.stringify(manifest.sources)!==JSON.stringify(expected))throw Error('DISPLAY_SOURCE_IDENTITY_MISMATCH');
 for(const name of ['DisplayConfigModeSetter.cs','DisplayModeCatalog.cs','DisplayModeStability.cs','DisplayStateBroker.cs','DisplayBrokerTask.cs'])if(!expected[name])throw Error('DISPLAY_REQUIRED_SOURCE_MISSING');
 return true;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const [action,exe,sourceRoot]=process.argv.slice(2);if(!['record','verify'].includes(action)||!exe||!sourceRoot)throw Error('USAGE: record|verify exe sourceRoot');
 const file=exe+'.display-contract.json';
 if(action==='record'){
  const r=spawnSync(exe,['--display-runtime-contract'],{encoding:'utf8',windowsHide:true,timeout:10000});
  if(r.status!==0)throw Error('DISPLAY_RUNTIME_CONTRACT_FAILED');const c=JSON.parse(r.stdout.trim().replace(/^\uFEFF/,''));
  if(c.schema!==1||c.contract!==contract||!c.persistentModeCatalog||!c.exactCcdMode||!c.boundedStabilityCheck||!c.authenticatedStateBroker)throw Error('DISPLAY_RUNTIME_CAPABILITY_MISSING');
  fs.writeFileSync(file,JSON.stringify({schema:1,contract,binarySha256:hash(exe),sources:sources(sourceRoot),liveAcceptance:'NOT_ASSERTED_BY_BUILD'},null,2));
 }
 verifyManifest(JSON.parse(fs.readFileSync(file,'utf8')),exe,sourceRoot);console.log('DISPLAY_BUILD_IDENTITY_OK');
}
