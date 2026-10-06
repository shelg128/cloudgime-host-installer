import {spawn,spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {randomBytes,createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';

// One fixed-purpose gate process and one nonce-bound inbox, not a remote command loop.
// Caller pins SSH identity and verifies helper hashes in private staging before use.
export async function prepareReadyGate({sshArgs,root,caseDir,nonce,coreHash,entryHash,bridgeHash,dryRun=true,spawnProcess=spawn,upload=spawnSync}) {
 if(!Array.isArray(sshArgs)||!sshArgs.every(s=>typeof s==='string')||sshArgs.at(-1)!=='admin@10.144.10.10'||typeof dryRun!=='boolean'||!/^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$/.test(root)||![coreHash,entryHash,bridgeHash].every(h=>/^[a-f0-9]{64}$/.test(h)))throw Error('READY_GATE_CONFIG');
 if(dryRun){if(typeof caseDir!=='string'||!caseDir.startsWith(root+'\\dryrun-')||!/^[a-f0-9]{32}$/.test(caseDir.slice((root+'\\dryrun-').length)))throw Error('DRY_SCOPE');}
 else if(!/^[a-f0-9]{32}$/.test(nonce))throw Error('LIVE_NONCE');
 const exchangeNonce=randomBytes(16).toString('hex');
 const script=`$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$entry='${root}\\CaptureGate.Ready.ps1';if((Get-FileHash $entry).Hash.ToLowerInvariant()-cne'${bridgeHash}'){throw 'BRIDGE_HASH'};& $entry -Root '${root}' ${dryRun?`-DryRun -CaseDir '${caseDir}'`:`-Nonce '${nonce}'`} -ExchangeNonce '${exchangeNonce}' -CoreSha256 '${coreHash}' -EntrySha256 '${entryHash}'`;
 const began=performance.now();
 const child=spawnProcess('ssh',[...sshArgs,'powershell.exe -NoProfile -NonInteractive -EncodedCommand '+Buffer.from(script,'utf16le').toString('base64')],{windowsHide:true,stdio:['pipe','pipe','pipe']});
 const r={exchangeNonce,dryRun,at:new Date().toISOString(),messages:[],submitted:false};
 let buffer='',bytes=0,waiter=null,queue=[],ended=false,closedByCaller=false,closeResolve;
 const closed=new Promise(res=>closeResolve=res);
 const fail=e=>{r.error??=e.message;if(waiter){const w=waiter;waiter=null;clearTimeout(w.t);w.reject(e);}};
 child.on('error',e=>fail(e));child.stdin.on('error',e=>fail(e));
 child.stderr.on('data',b=>r.stderr=((r.stderr||'')+b.toString()).slice(-2000));child.stdout.setEncoding('utf8');
 child.on('close',(code,signal)=>{r.exit=code;r.signal=signal;ended=true;if(waiter)fail(Error('READY_GATE_EARLY_EXIT'));closeResolve();});
 child.stdout.on('data',p=>{
  bytes+=Buffer.byteLength(p);if(bytes>100000){fail(Error('GATE_OUTPUT_LIMIT'));child.kill();return;}
  buffer+=p;let i;
  while((i=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,i).trim();buffer=buffer.slice(i+1);if(!line)continue;
   let m;try{m=JSON.parse(line);}catch{fail(Error('GATE_JSON'));child.kill();return;}
   r.messages.push(m);if(r.messages.length>2){fail(Error('GATE_EXTRA_OUTPUT'));child.kill();return;}
   if(waiter){const w=waiter;waiter=null;clearTimeout(w.t);w.resolve(m);}else queue.push(m);
  }
 });
 const next=ms=>new Promise((resolve,reject)=>{if(r.error)return reject(Error(r.error));if(queue.length)return resolve(queue.shift());if(ended)return reject(Error('GATE_CLOSED'));if(waiter)return reject(Error('CONCURRENT_GATE_READ'));const t=setTimeout(()=>{waiter=null;reject(Error('GATE_REPLY_TIMEOUT'));},ms);waiter={resolve,reject,t};});
 const timer=setTimeout(()=>{fail(Error('READY_GATE_LIFETIME'));child.kill();},30000);
 async function stop(){closedByCaller=true;child.stdin.end();if(!ended)child.kill();await Promise.race([closed,new Promise(res=>{const t=setTimeout(res,2000);closed.then(()=>clearTimeout(t));})]);clearTimeout(timer);r.processExited=ended;return r;}
 try{
  const m=await next(15000);
  if(m.type!=='gate-ready'||m.schema!==1||m.exchangeNonce!==exchangeNonce||m.dryRun!==dryRun||m.idleTimeoutMs!==10000||m.lifetimeMs!==15000||m.delivery!=='hashed-private-inbox')throw Error('GATE_READY_SCOPE');
  r.startupMs=performance.now()-began;r.readyAtPcMs=Date.now();r.readyMonoMs=performance.now();
 }catch(e){fail(e);await stop();throw Error(r.error);}
 return {state:r,close:stop,submit:async envelope=>{
  if(r.submitted||closedByCaller)throw Error('GATE_ALREADY_USED');
  r.submitted=true;r.submittedAtPcMs=Date.now();const sent=performance.now();
  try{
   if(ended||r.error||sent-r.readyMonoMs>9000)throw Error('GATE_NOT_READY_OR_EXPIRED');
   if(Math.abs((r.submittedAtPcMs-r.readyAtPcMs)-(sent-r.readyMonoMs))>20)throw Error('GATE_CLOCK_STEP');
   const body=Buffer.from(JSON.stringify(envelope));if(body.length>8192)throw Error('ENVELOPE_LIMIT');
   const tmp=path.join(os.tmpdir(),'cg-gate-envelope-'+exchangeNonce+'.json');
   fs.writeFileSync(tmp,JSON.stringify({exchangeNonce,bytes:body.length,sha256:createHash('sha256').update(body).digest('hex'),payloadBase64:body.toString('base64')}),{flag:'wx'});
   const remote=sshArgs.at(-1);const up=upload('scp',[...sshArgs.slice(0,-1),tmp,remote+':'+root.replaceAll('\\','/')+'/gate-inbox-'+exchangeNonce+'.json'],{encoding:'utf8',windowsHide:true,timeout:8000,maxBuffer:10000});
   r.uploadExit=up.status;r.uploadError=up.error?.code;child.stdin.end();
   if(up.status!==0)throw Error('GATE_ENVELOPE_UPLOAD');
   const m=await next(12000);r.result=m;r.responseAtPcMs=Date.now();
   await Promise.race([closed,new Promise((_,reject)=>{const t=setTimeout(()=>reject(Error('GATE_EXIT_TIMEOUT')),2000);closed.then(()=>clearTimeout(t));})]);
   if(r.exit!==0||r.signal||r.error||r.messages.length!==2||buffer.trim()||queue.length)throw Error('GATE_RESULT_REJECTED');
   if(dryRun){if(m.dryRun!==true||m.requestWritten!==false||m.requestExists!==false||typeof m.eligible!=='boolean')throw Error('DRY_RESULT_CONTRACT');}
   else if(m.requested!==true||m.dryRun!==false)throw Error('LIVE_RESULT_CONTRACT');
   r.success=true;
  }catch(e){fail(e);}finally{r.afterSubmitMs=performance.now()-sent;await stop();}
  return r;
 }};
}
