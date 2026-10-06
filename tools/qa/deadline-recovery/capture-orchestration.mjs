import {performance} from 'node:perf_hooks';
import fs from'node:fs';import crypto from'node:crypto';import{spawnSync}from'node:child_process';
import{exchangeControlClock,requirePrebillingClock}from'./exchange-control-clock.mjs';
import{mapEvidenceLowerBound}from'./control-clock.mjs';
export async function samplePinnedClock({sshArgs,remoteHelper,expectedHelperHash,localHelper,save}){
 if(!Array.isArray(sshArgs)||!/^[a-f0-9]{64}$/.test(expectedHelperHash)||crypto.createHash('sha256').update(fs.readFileSync(localHelper)).digest('hex')!==expectedHelperHash||!/^C:\/ProgramData\/Cloudgime\/qa-scoped-pilot\/run-\d+\/ClockProbe.exe$/.test(remoteHelper))throw Error('PINNED_CLOCK_SCOPE');
 const result=await exchangeControlClock({command:'ssh',args:[...sshArgs,'cmd.exe /d /s /c "'+remoteHelper+'"']});result.mappingAnchor={wallMs:Date.now(),monoMs:performance.now()};await save(result);requirePrebillingClock(result);return result;
}
export function makeCaptureEvidence({clock,context,streamerSha256,backend,backendReadStartPcMs,reviewedAtPcMs,screenshotStartedPcMs,nowPcMs,nowMonoMs=performance.now(),fixtureState,requestNonce}){
 // Screenshot acquisition time remains the oldest evidence; a late review cannot make it fresh.
 if(!clock?.mappingAnchor||!Number.isFinite(nowMonoMs)||nowMonoMs<clock.mappingAnchor.monoMs||Math.abs((nowPcMs-clock.mappingAnchor.wallMs)-(nowMonoMs-clock.mappingAnchor.monoMs))>20)throw Error('LOCAL_CLOCK_CHANGED');
 if(!Number.isSafeInteger(screenshotStartedPcMs)||!Number.isSafeInteger(reviewedAtPcMs)||reviewedAtPcMs<screenshotStartedPcMs||reviewedAtPcMs>nowPcMs)throw Error('SCREENSHOT_REVIEW_TIME');
 if(!Number.isSafeInteger(backendReadStartPcMs)||backendReadStartPcMs>nowPcMs||nowPcMs-backendReadStartPcMs>3000||nowPcMs-screenshotStartedPcMs>5000)throw Error('EVIDENCE_EXPIRED_BEFORE_DISPATCH');
 const e={context,streamerSha256,backendObservedHostLowerMs:mapEvidenceLowerBound(clock.assessment,backendReadStartPcMs,nowPcMs),reviewedHostLowerMs:mapEvidenceLowerBound(clock.assessment,screenshotStartedPcMs,nowPcMs),backend,visualApproved:true,route:'direct',width:720,height:1570,fps:30,targetKbps:5000,fixtureState,requestNonce};
 return {envelope:e,evidenceTimes:{backendReadStartPcMs,screenshotStartedPcMs,reviewedAtPcMs,preparedAtPcMs:nowPcMs},clockInterval:clock.assessment.best};
}
export function invokePinnedGate({sshArgs,root,nonce,coreHash,entryHash,envelope,dryRun=false,caseDir}){
 if(!/^C:\\ProgramData\\Cloudgime\\qa-scoped-pilot\\run-\d+$/.test(root)||!/^[a-f0-9]{64}$/.test(coreHash)||!/^[a-f0-9]{64}$/.test(entryHash))throw Error('GATE_SCOPE');
 const file=dryRun?'CaptureGate.DryRun.ps1':'CaptureGate.Live.ps1';
 if(dryRun){if(!new RegExp('^'+root.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')+'\\\\dryrun-[a-f0-9]{32}$').test(caseDir))throw Error('DRY_SCOPE');}
 else if(!/^[a-f0-9]{32}$/.test(nonce))throw Error('GATE_NONCE');
 const body=Buffer.from(JSON.stringify(envelope));if(body.length>8192)throw Error('GATE_ENVELOPE_SIZE');
 const script=`$ErrorActionPreference='Stop';$entry='${root}\\${file}';if((Get-FileHash $entry).Hash.ToLowerInvariant()-cne'${entryHash}'){throw 'GATE_HASH'};& $entry ${dryRun?`-CaseDir '${caseDir}'`:`-Root '${root}' -Nonce '${nonce}'`} -EnvelopeBase64 '${body.toString('base64')}' -CoreSha256 '${coreHash}'`;
 const startedAtMs=Date.now();const p=spawnSync('ssh',[...sshArgs,'powershell.exe -NoProfile -NonInteractive -EncodedCommand '+Buffer.from(script,'utf16le').toString('base64')],{encoding:'utf8',windowsHide:true,timeout:15000,maxBuffer:100000});
 const output={startedAtMs,finishedAtMs:Date.now(),exit:p.status,error:p.error?.code,stderr:p.stderr.slice(0,2000),dryRun};if(p.status===0){try{output.result=JSON.parse(p.stdout.trim());}catch{output.error='INVALID_GATE_JSON';}}else output.error??='GATE_COMMAND_FAILED';
 return output; // caller reconciles an uncertain live mutation; never automatic resend.
}
