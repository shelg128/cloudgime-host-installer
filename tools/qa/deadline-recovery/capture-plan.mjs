// Offline preflight planner only. Caller must obtain fresh authenticated snapshots;
// this function never contacts billing/Host, grants authority, or writes request.json.
export function planScopedCapture({expected,peer,ready,backend,display,collector,nowMs,requestNonce,durationMs=45000}) {
 const reasons=[];const same=(a,b,keys)=>!!a&&!!b&&keys.every(k=>a[k]===b[k]);
 const contextKeys=['native_session_id','claim_id','handoff_binding_id','rental_session_id','connection_nonce','generation','authority_expires_ms'];
 const recent=(x,maxAge)=>x&&Number.isSafeInteger(x.observedAtMs)&&Number.isSafeInteger(nowMs)&&nowMs>=x.observedAtMs&&nowMs-x.observedAtMs<=maxAge;
 if(!Number.isSafeInteger(nowMs)||nowMs<=0)reasons.push('CLOCK');
 if(!expected||contextKeys.some(k=>expected[k]===null||expected[k]===undefined)||!Number.isSafeInteger(expected?.generation)||expected.generation<=0||!/^[a-f0-9]{32}$/i.test(expected?.connection_nonce||''))reasons.push('EXPECTED_CONTEXT');
 if(!same(expected,peer?.context,contextKeys)||!same(expected,ready?.context,contextKeys))reasons.push('PEER_SCOPE');
 if(!Number.isSafeInteger(peer?.process)||peer.process<=0||peer.process!==ready?.process||!Number.isSafeInteger(ready?.ssrc)||ready.ssrc<=0||ready.ssrc>0xffffffff||!Number.isSafeInteger(ready?.epoch)||ready.epoch<=0)reasons.push('PROCESS_OR_STREAM');
 if(!recent(backend,3000)||backend?.source!=='authenticated-backend-read'||backend?.billingStatus!=='BILLING_RUNNING'||backend?.endedAt!==null||backend?.pcId!=='2'||backend?.sessionId!==expected?.rental_session_id||backend?.claimId!==expected?.claim_id||backend?.nativeRoot!==expected?.native_session_id||typeof backend?.billingCommitId!=='string'||!backend.billingCommitId)reasons.push('LIVE_BILLING_BINDING');
 if(!recent(display,5000)||display?.reviewed!==true||display?.fixture!=='synthetic'||display?.route!=='direct'||display?.width!==720||display?.height!==1570||display?.fps!==30||display?.targetKbps!==5000||display?.sessionId!==backend?.sessionId||display?.claimId!==backend?.claimId)reasons.push('DISPLAY_REVIEW');
 if(!recent(collector,1000)||collector?.process!==peer?.process||collector?.ssrc!==ready?.ssrc||collector?.epoch!==ready?.epoch||!Number.isFinite(collector?.packetsPerSecond)||collector.packetsPerSecond<=0||!Number.isSafeInteger(collector?.progressFrames)||collector.progressFrames<3||collector?.activeTrace!==false)reasons.push('FRESH_SDK_PROGRESS');
 if(!/^[a-f0-9]{32}$/i.test(requestNonce||'')||!Number.isSafeInteger(durationMs)||durationMs<1000||durationMs>60000||!Number.isSafeInteger(expected?.authority_expires_ms)||nowMs+durationMs+2000>=expected.authority_expires_ms)reasons.push('DURATION_OR_AUTHORITY');
 // Worst-case measured original/repair RTP writes;2events(packet+frame) with50%headroom plus250events/s RTCP/lifecycle.
 const projectedEvents=Math.ceil((collector?.packetsPerSecond*3+250)*durationMs/1000);if(!Number.isFinite(projectedEvents)||projectedEvents>=45000)reasons.push('COLLECTOR_QUOTA_MARGIN');
 return {eligible:reasons.length===0,reasons:[...new Set(reasons)],projectedEvents,request:reasons.length?null:{context:{...expected},request_nonce:requestNonce,expires_ms:nowMs+2000,duration_ms:durationMs},rentalMutations:0,assumptions:['Fresh evidence provenance is a caller responsibility; strings/booleans are not cryptographic validation.','Ready-file content alone is not a live progress or display certificate.','Event forecast is conservative but not a hard proof; trace footer remains authoritative.'],performanceAccepted:false};
}
