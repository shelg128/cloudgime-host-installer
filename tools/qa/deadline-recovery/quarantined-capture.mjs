import crypto from'node:crypto';
import{mapEvidenceLowerBound}from'./control-clock.mjs';
import{performance}from'node:perf_hooks';
export const QUARANTINE_PROTOCOL='quarantined-metadata-v1';
const SHA=/^[a-f0-9]{64}$/,NONCE=/^[a-f0-9]{32}$/;
export function makeQuarantinedCaptureEvidence({clock,capture,context,streamerSha256,backend,backendReadStartPcMs,nowPcMs=Date.now(),nowMonoMs=performance.now(),fixtureState,requestNonce}){
 const e=capture?.evidence;if(!e||e.verified!==true||e.exit!==0||e.processExited!==true||e.nonce!==e.meta?.nonce||!NONCE.test(e.nonce)||!SHA.test(e.meta?.sha256)||!Number.isSafeInteger(e.startedAtPcMs)||!Number.isSafeInteger(e.finishedAtPcMs)||e.finishedAtPcMs<e.startedAtPcMs||nowPcMs<e.finishedAtPcMs||nowPcMs-e.startedAtPcMs>5000)throw Error('QUARANTINE_IMAGE_NOT_FRESH');
 if(!Number.isSafeInteger(nowPcMs)||!Number.isFinite(nowMonoMs)||!clock?.mappingAnchor||!Number.isFinite(clock.mappingAnchor.monoMs)||nowMonoMs<clock.mappingAnchor.monoMs||Math.abs((nowPcMs-clock.mappingAnchor.wallMs)-(nowMonoMs-clock.mappingAnchor.monoMs))>20)throw Error('LOCAL_CLOCK_CHANGED');
 if(!Number.isSafeInteger(backendReadStartPcMs)||backendReadStartPcMs>nowPcMs||nowPcMs-backendReadStartPcMs>3000)throw Error('BACKEND_NOT_FRESH');
 if(!NONCE.test(requestNonce)||!SHA.test(streamerSha256)||!context||!Number.isSafeInteger(context.generation)||context.generation<1||!NONCE.test(context.connection_nonce)||!Number.isSafeInteger(context.authority_expires_ms)||context.authority_expires_ms<=0||['native_session_id','rental_session_id','claim_id','handoff_binding_id'].some(k=>typeof context[k]!=='string'||!context[k]))throw Error('ENVELOPE_IDENTITY');
 return {protocol:QUARANTINE_PROTOCOL,context,streamerSha256,backend,backendObservedHostLowerMs:mapEvidenceLowerBound(clock.assessment,backendReadStartPcMs,nowPcMs),capturedHostLowerMs:mapEvidenceLowerBound(clock.assessment,e.startedAtPcMs,nowPcMs),image:{captureNonce:e.nonce,jpegSha256:e.meta.sha256,verified:true},visualApproved:false,analysisDisposition:'quarantine-pending-review',route:'direct',width:720,height:1570,fps:30,targetKbps:5000,fixtureState,requestNonce};
}
export function sealQuarantinedCollection({envelope,traceBytes,preImageBytes,postImageBytes,postEvidence,result}){
 if(envelope?.protocol!==QUARANTINE_PROTOCOL||envelope.visualApproved!==false||envelope.analysisDisposition!=='quarantine-pending-review'||result?.captureCollected!==true||!Buffer.isBuffer(traceBytes)||!Buffer.isBuffer(preImageBytes)||!Buffer.isBuffer(postImageBytes))throw Error('COLLECTION_NOT_QUARANTINED');
 if(!NONCE.test(envelope.requestNonce)||!NONCE.test(envelope.image?.captureNonce)||!SHA.test(envelope.image?.jpegSha256)||envelope.image.verified!==true||postEvidence?.exit!==0||postEvidence?.processExited!==true||!NONCE.test(postEvidence?.nonce)||postEvidence.nonce===envelope.image.captureNonce)throw Error('COLLECTION_IMAGE_IDENTITY');
 const hash=b=>crypto.createHash('sha256').update(b).digest('hex');if(hash(preImageBytes)!==envelope.image.jpegSha256||postEvidence?.verified!==true||hash(postImageBytes)!==postEvidence.meta?.sha256||postEvidence.nonce!==postEvidence.meta?.nonce)throw Error('IMAGE_HASH');
 return {schema:1,protocol:QUARANTINE_PROTOCOL,disposition:'quarantine-pending-review',requestNonce:envelope.requestNonce,context:{...envelope.context},traceSha256:hash(traceBytes),preImageSha256:hash(preImageBytes),postImageSha256:hash(postImageBytes),preImageNonce:envelope.image.captureNonce,postImageNonce:postEvidence.nonce,qualityAccepted:false};
}
export function assessQuarantineReview(seal,review,{traceBytes,preImageBytes,postImageBytes,scopeVerified,traceComplete,continuityVerified}){
 const reasons=[];const hash=b=>Buffer.isBuffer(b)?crypto.createHash('sha256').update(b).digest('hex'):null;
 if(seal?.protocol!==QUARANTINE_PROTOCOL||seal?.disposition!=='quarantine-pending-review')reasons.push('SEAL');
 if(!NONCE.test(seal?.requestNonce||'')||!SHA.test(seal?.preImageSha256||'')||!SHA.test(seal?.postImageSha256||'')||!NONCE.test(seal?.preImageNonce||'')||!NONCE.test(seal?.postImageNonce||'')||seal.preImageNonce===seal.postImageNonce)reasons.push('SEALED_IDENTITY');
 if(!SHA.test(seal?.traceSha256||'')||seal.traceSha256!==hash(traceBytes)||seal.preImageSha256!==hash(preImageBytes)||seal.postImageSha256!==hash(postImageBytes))reasons.push('BUNDLE_HASH');
 if(review?.reviewed!==true||review?.requestNonce!==seal?.requestNonce||review?.traceSha256!==seal?.traceSha256||review?.preImageSha256!==seal?.preImageSha256||review?.postImageSha256!==seal?.postImageSha256||review?.bothImagesShowExpectedFixture!==true||typeof review?.reviewer!=='string'||!review.reviewer||!Number.isSafeInteger(review?.reviewedAtPcMs))reasons.push('REVIEW_REQUIRED');
 if(scopeVerified!==true)reasons.push('SCOPE_NOT_VERIFIED');if(traceComplete!==true)reasons.push('TRACE_INCOMPLETE');if(continuityVerified!==true)reasons.push('CONTINUITY_NOT_VERIFIED');
 return {eligibleForDiagnosticAnalysis:reasons.length===0,reasons,disposition:reasons.length?'quarantine-not-accepted':'reviewed-diagnostic-only',physicalQualityAccepted:false,limits:'Pre/post visual review and native/Host continuity are evidence checks, not uninterrupted display proof or a performance improvement.'};
}
