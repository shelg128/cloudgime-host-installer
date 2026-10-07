// SPDX-License-Identifier: GPL-3.0-only
import fs from 'node:fs';import path from 'node:path';import crypto from 'node:crypto';import {fileURLToPath} from 'node:url';
import {revision} from './apply-capture-recovery.mjs';
const dir=path.dirname(fileURLToPath(import.meta.url));const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
export function patchFingerprint(){const names=['upstream-lock.json','apply-capture-recovery.mjs','capture_recovery_policy.h','capture_recovery_windows.h','capture_recovery_integration.inc','apply-toolchain-compat.mjs','windows_toolchain_compat.cmake'];return sha(Buffer.from(names.map(n=>n+':'+sha(fs.readFileSync(path.join(dir,n)))).join('\n')));}
export function verifyRuntime(root,{requireAcceptance=true}={}){
 const m=JSON.parse(fs.readFileSync(path.join(root,'cloudgime-capture-runtime.json'),'utf8'));
 if(m.schemaVersion!==1||m.upstreamRevision!==revision||m.patch!=='cloudgime-ddapi-capture-only-recovery-v1'||m.patchFingerprint!==patchFingerprint())throw Error('CAPTURE_RUNTIME_SOURCE_MISMATCH');
 if(!/^[a-f0-9]{64}$/.test(m.binarySHA256||'')||sha(fs.readFileSync(path.join(root,'sunshine.exe')))!==m.binarySHA256)throw Error('CAPTURE_RUNTIME_BINARY_MISMATCH');
 if(requireAcceptance&&(m.acceptance?.status!=='PASS'||m.acceptance?.directMedia!==true||m.acceptance?.relayMedia!==true||m.acceptance?.recoveryObserved!==true||m.acceptance?.fullResetPreserved!==true||m.acceptance?.pacingAccepted!==true||m.acceptance?.regressionAccepted!==true||m.acceptance?.binarySHA256!==m.binarySHA256))throw Error('CAPTURE_RUNTIME_LIVE_ACCEPTANCE_REQUIRED');
 return {ok:true,binarySHA256:m.binarySHA256,revision:m.upstreamRevision,patch:m.patch,acceptance:m.acceptance?.status||'NOT_RUN'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){if(process.argv.length!==3)throw Error('Usage: node verify-runtime.mjs <runtime-root>');console.log(JSON.stringify(verifyRuntime(path.resolve(process.argv[2])),null,2));}
