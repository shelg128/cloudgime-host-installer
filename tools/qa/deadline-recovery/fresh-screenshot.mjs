import fs from'node:fs';import path from'node:path';import crypto from'node:crypto';import{spawnSync}from'node:child_process';import{performance}from'node:perf_hooks';
// Single acquisition with stdout bytes, no remote file, no recapture retries.
// Caller controls/validates visible window before capture. This function never unlocks a device.
export function captureFreshPng({adb,serial,directory,name,ffmpeg,inspectPng,run=spawnSync,wall=Date.now,mono=()=>performance.now()}){
 if(typeof serial!=='string'||!serial||serial.startsWith('-')||/[\s\x00]/.test(serial)||!/^[-a-zA-Z0-9_]{1,80}$/.test(name)||typeof inspectPng!=='function')throw Error('CAPTURE_CONFIG');
 fs.mkdirSync(directory,{recursive:true});const record=path.join(directory,name+'-capture.json'),png=path.join(directory,name+'.png'),jpg=path.join(directory,name+'.jpg');for(const f of [record,png,jpg])if(fs.existsSync(f))throw Error('CAPTURE_EXISTS');
 const r={schema:1,method:'one adb exec-out screencap PNG, host decoder validation, no remote temporary file',startedAtPcMs:wall(),recaptureAttempts:0,verified:false};const start=mono();const save=()=>fs.writeFileSync(record,JSON.stringify(r,null,2));save();
 try{
  const p=run(adb,['-s',serial,'exec-out','screencap','-p'],{encoding:null,windowsHide:true,timeout:10000,maxBuffer:16*1024*1024});r.captureExit=p.status;r.captureFinishedAtPcMs=wall();r.acquisitionMs=mono()-start;if(p.status!==0||!Buffer.isBuffer(p.stdout))throw Error('CAPTURE_TRANSPORT_NO_REPLAY');
  r.image=inspectPng(p.stdout);r.sha256=crypto.createHash('sha256').update(p.stdout).digest('hex');fs.writeFileSync(png,p.stdout,{flag:'wx'});
  const q=run(ffmpeg,['-hide_banner','-loglevel','error','-xerror','-i',png,'-frames:v','1','-vf','scale=min(480\\,iw):-2','-q:v','3','-n',jpg],{encoding:'utf8',windowsHide:true,timeout:10000,maxBuffer:100000});r.decodeExit=q.status;if(q.status!==0||!fs.existsSync(jpg))throw Error('CAPTURE_DECODE');const image=fs.readFileSync(jpg);if(image.length<256||image[0]!==255||image[1]!==216||image.at(-2)!==255||image.at(-1)!==217)throw Error('JPEG_INVALID');
  r.jpegSha256=crypto.createHash('sha256').update(image).digest('hex');r.verified=true;r.finishedAtPcMs=wall();r.totalMs=mono()-start;if(Math.abs((r.finishedAtPcMs-r.startedAtPcMs)-r.totalMs)>20)throw Error('CAPTURE_CLOCK_STEP');save();return{file:jpg,png,evidence:r};
 }catch(e){r.error=e.message;r.verified=false;r.finishedAtPcMs=wall();r.totalMs=mono()-start;save();throw e;}
}
