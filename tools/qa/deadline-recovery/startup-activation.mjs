// Read-only prebilling gate. Evidence must be acquired by the trusted deployment
// supervisor; JSON fields themselves do not authenticate a remote process.
export function assessStartupActivation({nowMs,observedAtMs,process,receipt,artifact,priorDaemon,slot}) {
 const reasons=[];
 const positive=n=>Number.isSafeInteger(n)&&n>0;
 if(!positive(nowMs)||!positive(observedAtMs)||observedAtMs>nowMs||nowMs-observedAtMs>5000)reasons.push('STALE_OBSERVATION');
 if(!positive(process?.pid)||!positive(process?.parentPid)||!positive(process?.startedAtMs)||process.startedAtMs>nowMs)reasons.push('PROCESS_IDENTITY');
 if(!/^[a-f0-9]{64}$/i.test(artifact?.expectedSha256||'')||artifact?.actualSha256?.toLowerCase()!==artifact?.expectedSha256?.toLowerCase())reasons.push('WEB_HASH');
 if(receipt?.schema!==1||receipt?.process!==process?.pid||receipt?.scoped_feature!==true||receipt?.operator_enabled!==true||receipt?.root_writable!==true||!positive(receipt?.created_at_ms)||receipt.created_at_ms<process?.startedAtMs||receipt.created_at_ms>observedAtMs)reasons.push('STARTUP_RECEIPT');
 if(!positive(process?.daemonPid)||process?.parentPid!==process?.daemonPid||!positive(process?.daemonStartedAtMs)||process?.daemonStartedAtMs>process?.startedAtMs)reasons.push('DAEMON_BINDING');
 if(!positive(priorDaemon?.pid)||!positive(priorDaemon?.startedAtMs)||process?.daemonStartedAtMs<=priorDaemon?.startedAtMs)reasons.push('DAEMON_NOT_RECREATED');
 if(slot?.pcId!=='2'||slot?.status!=='MAINTENANCE'||slot?.owner||slot?.claim||slot?.pending)reasons.push('PC_NOT_IDLE');
 return {eligible:reasons.length===0,reasons,scope:'activation only; no authenticated rental or capture readiness implied',rentalMutations:0,streamQualityAccepted:false};
}
