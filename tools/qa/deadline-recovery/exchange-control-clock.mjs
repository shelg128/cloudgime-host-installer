import {spawn} from 'node:child_process';
import {performance} from 'node:perf_hooks';
import {randomBytes} from 'node:crypto';
import {assessClockExchange} from './control-clock.mjs';

// Starts only the caller-supplied read-only helper/SSH command. Caller pins host key,
// executable hash and process scope. Never supplies a generic remote-shell request.
export async function exchangeControlClock({command,args=[],sampleCount=6,spawnProcess=spawn}) {
 if(typeof command!=='string'||!command||!Array.isArray(args)||!args.every(x=>typeof x==='string')||!Number.isInteger(sampleCount)||sampleCount<3||sampleCount>8)throw Error('CLOCK_CONFIG');
 const id=randomBytes(16).toString('hex'),began=performance.now();
 const r={schema:1,id,startedAtMs:Date.now(),samples:[],responses:[],scope:'read-only control-evidence offset intervals; no media latency claim',batchPolicy:'one batch, conservative common send time; correlated samples'};
 const child=spawnProcess(command,args,{windowsHide:true,stdio:['pipe','pipe','pipe']});let buffer='',waiter=null,queue=[],outputBytes=0,stdoutEnded=false,stderr='';
 let finishClose;const closed=new Promise(resolve=>finishClose=resolve);let processClosed=false;
 const rejectWaiting=e=>{if(waiter){const w=waiter;waiter=null;clearTimeout(w.timer);w.reject(e);}};
 child.on('error',e=>{r.spawnError=e.code||e.name;rejectWaiting(Error('CLOCK_SPAWN'));});
 child.on('close',(code,signal)=>{r.exit=code;r.signal=signal;processClosed=true;rejectWaiting(Error('CLOCK_CLOSED_EARLY'));finishClose();});
 child.stdin.on('error',e=>{r.stdinError=e.code||e.name;rejectWaiting(Error('CLOCK_STDIN'));});
 child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');child.stderr.on('data',x=>{stderr=(stderr+x).slice(-2000);});
 child.stdout.on('data',part=>{outputBytes+=Buffer.byteLength(part);if(outputBytes>16384){r.outputLimit=true;rejectWaiting(Error('CLOCK_OUTPUT_LIMIT'));child.kill();return;}
  buffer+=part;let at;while((at=buffer.indexOf('\n'))!==-1){const line=buffer.slice(0,at).trim();buffer=buffer.slice(at+1);if(!line)continue;let message;try{message=JSON.parse(line);}catch{r.invalidResponse=line.slice(0,200);rejectWaiting(Error('CLOCK_JSON'));child.kill();return;}
   const e={message,atPcMs:Date.now(),atMonoMs:performance.now()};r.responses.push(message);
   if(waiter){const w=waiter;waiter=null;clearTimeout(w.timer);w.resolve(e);}else{queue.push(e);if(queue.length>10){r.outputLimit=true;child.kill();}}
  }});
 child.stdout.on('end',()=>{stdoutEnded=true;if(!queue.length)rejectWaiting(Error('CLOCK_EOF'));});
 const next=ms=>new Promise((resolve,reject)=>{if(queue.length)return resolve(queue.shift());if(stdoutEnded||processClosed)return reject(Error('CLOCK_EOF'));if(waiter)return reject(Error('CLOCK_CONCURRENT_READ'));const timer=setTimeout(()=>{waiter=null;reject(Error('CLOCK_REPLY_TIMEOUT'));},ms);waiter={resolve,reject,timer};});
 const deadline=setTimeout(()=>{r.hardDeadline=true;rejectWaiting(Error('CLOCK_LIFETIME'));child.stdin.end();child.kill();},30000);
 try{
  const ready=await next(15000),m=ready.message;if(m.type!=='ready'||m.schema!==1||m.role!=='clock-only'||m.maxSamples!==8||m.maxLifetimeMs!==20000||m.idleTimeoutMs!==3000)throw Error('CLOCK_READY_CONTRACT');r.startupMs=performance.now()-began;
  const localSendMs=Date.now(),sentMono=performance.now();
  child.stdin.end(Array.from({length:sampleCount},(_,seq)=>JSON.stringify({action:'sample',id,seq})).concat(JSON.stringify({action:'exit',id,seq:sampleCount})).join('\n')+'\n');
  for(let seq=0;seq<sampleCount;seq++){const e=await next(3000),s=e.message;if(s.type!=='sample'||s.id!==id||s.seq!==seq)throw Error('CLOCK_RESPONSE_SCOPE');r.samples.push({...s,localSendMs,localReceiveMs:e.atPcMs,localElapsedMs:e.atMonoMs-sentMono});}
  const end=(await next(3000)).message;if(end.type!=='closed'||end.id!==id||end.seq!==sampleCount)throw Error('CLOCK_CLOSE_SCOPE');r.remoteClosed=true;
  await Promise.race([closed,new Promise((_,reject)=>{const t=setTimeout(()=>reject(Error('CLOCK_EXIT_TIMEOUT')),3000);closed.then(()=>clearTimeout(t));})]);
  if(r.exit!==0||r.signal||r.stdinError||r.spawnError||r.outputLimit||r.hardDeadline||buffer.trim()||queue.length||r.responses.length!==sampleCount+2)throw Error('CLOCK_EXIT_OR_EXTRA_OUTPUT');
  r.assessment=assessClockExchange(r.samples);if(!r.assessment.eligible)throw Error('CLOCK_INTERVAL_REJECTED');
 }catch(e){r.error=e.message;child.stdin.end();if(!processClosed){child.kill();await Promise.race([closed,new Promise(resolve=>setTimeout(resolve,4000))]);}}
 finally{clearTimeout(deadline);rejectWaiting(Error('CLOCK_SESSION_FINISHED'));r.finishedAtMs=Date.now();r.elapsedMs=performance.now()-began;r.processExited=processClosed;r.stderr=stderr;r.success=!r.error&&processClosed&&r.exit===0&&r.remoteClosed===true&&r.assessment?.eligible===true;}
 return r;
}
export function requirePrebillingClock(result,{nowMs=Date.now(),maxAgeMs=30000}={}) {
 if(!Number.isSafeInteger(nowMs)||!Number.isFinite(maxAgeMs)||maxAgeMs<=0||maxAgeMs>30000||result?.success!==true||result.exit!==0||result.processExited!==true||result.remoteClosed!==true||result.error||!Number.isSafeInteger(result.finishedAtMs)||nowMs<result.finishedAtMs||nowMs-result.finishedAtMs>maxAgeMs)throw Error('PREBILLING_CLOCK_NOT_QUALIFIED');
 const clock=assessClockExchange(result.samples);if(!clock.eligible||!result.samples.every(s=>s.id===result.id)||result.samples.some((s,i)=>s.seq!==i))throw Error('PREBILLING_CLOCK_INVALID_EVIDENCE');
 return {eligible:true,checkedAtMs:nowMs,maxWidthMs:1500,widthMs:clock.best.widthMs,rentalMutations:0,scope:'tool prerequisite only; refresh mapping near capture after login and recheck downstream command timing'};
}
