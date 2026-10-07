// Prevent unaccepted capture-recovery binaries from entering releases via copied
// or stripped manifests. This guard does not qualify an unrelated runtime.
import fs from'node:fs';import path from'node:path';import crypto from'node:crypto';import{fileURLToPath}from'node:url';import{verifyRuntime}from'../../patches/sunshine/verify-runtime.mjs';
export function verifyCaptureCandidate(root){
 const exe=path.join(root,'sunshine.exe'),manifest=path.join(root,'cloudgime-capture-runtime.json');
 if(!fs.existsSync(exe))throw Error('CAPTURE_RUNTIME_MISSING');const size=fs.statSync(exe).size;if(size<1||size>256*1024*1024)throw Error('CAPTURE_RUNTIME_SIZE');
 const bytes=fs.readFileSync(exe),hash=crypto.createHash('sha256').update(bytes).digest('hex');
 const known=['846ef6cf6e9a8ea15719c5a45585434481f5feff3e1c2766ffefd74715f61ef9','f1adc5659c2395058c8f455386b9351eee711f35eadb10a425ba0c45dcbcad0b'];
 const marked=known.includes(hash)||bytes.includes(Buffer.from('Cloudgime capture-only recovered; encoder retained'))||bytes.includes(Buffer.from('Cloudgime capture-only recovered; encoder retained','utf16le'));
 if(!fs.existsSync(manifest)&&marked)throw Error('CAPTURE_CANDIDATE_MANIFEST_REQUIRED');
 if(fs.existsSync(manifest))return{...verifyRuntime(root),classified:'capture-recovery',releaseGateApplied:true};
 return{classified:'no-capture-marker',releaseGateApplied:false,binarySHA256:hash,scope:'Other existing release checks still required; this is not quality approval.'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){if(process.argv.length!==3)throw Error('Usage: verify-capture-candidate.mjs runtime-root');console.log(JSON.stringify(verifyCaptureCandidate(path.resolve(process.argv[2]))));}
