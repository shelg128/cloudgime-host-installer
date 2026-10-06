export function requireReadyVisualTimingScreen(assessment){if(assessment?.eligible!==true||assessment.timingScreenOnly!==true||assessment.reviewAllowanceSimulated!==true||assessment.realReviewerQualified!==false||assessment.safetyMs!==250||assessment.maxVisualAgeMs!==5000||assessment.maxBackendAgeMs!==3000||!Array.isArray(assessment.reasons)||assessment.reasons.length||!Array.isArray(assessment.samples)||assessment.samples.length<3||assessment.samples.some(x=>!Number.isFinite(x.visualMarginMs)||x.visualMarginMs<250||!Number.isFinite(x.backendAgeMs)||x.backendAgeMs>3000))throw Error('READY_VISUAL_TIMING_NOT_QUALIFIED');return {timingOnly:true,actualFreshVisualApprovalStillRequired:true};}
export function assessReadyVisualPath(runs,{safetyMs=250,maxVisualAgeMs=5000,maxBackendAgeMs=3000}={}){
 const reasons=[];
 if(safetyMs!==250||maxVisualAgeMs!==5000||maxBackendAgeMs!==3000)reasons.push('LIMITS_CHANGED');
 if(!Array.isArray(runs)||runs.length<3||runs.length>20)return {eligible:false,reasons:[...reasons,'SAMPLE_COUNT'],newRentalAllowed:false};
 const samples=runs.map(r=>{
  const i=r.image?.evidence,g=r.output,o=g?.result,v=o?.gate;
  if(i?.verified!==true||i?.exit!==0||i.noRecapture!==true||i.meta?.captureCount!==1||typeof i.nonce!=='string'||i.meta.nonce!==i.nonce||!Number.isSafeInteger(i.startedAtPcMs)||!Number.isSafeInteger(i.finishedAtPcMs)||i.finishedAtPcMs<i.startedAtPcMs||!Number.isFinite(i.totalMs)||i.totalMs<0||Math.abs(i.finishedAtPcMs-i.startedAtPcMs-i.totalMs)>20)reasons.push('IMAGE_EVIDENCE');
  if(g?.success!==true||g.exit!==0||g.processExited!==true||g.uploadExit!==0||g.dryRun!==true||o?.eligible!==true||o.requestWritten!==false||o.requestExists!==false||!Number.isFinite(v?.reviewAgeUpperMs)||!Number.isFinite(v?.backendAgeUpperMs)||v.reviewAgeUpperMs<0||v.backendAgeUpperMs<0)reasons.push('GATE_EVIDENCE');
  const times=r.prepared?.evidenceTimes;
  if(!times||times.screenshotStartedPcMs!==i?.startedAtPcMs||times.reviewedAtPcMs< i?.finishedAtPcMs||times.preparedAtPcMs<times.reviewedAtPcMs||times.preparedAtPcMs-times.reviewedAtPcMs>3000||times.reviewedAtPcMs-i?.finishedAtPcMs<1000)reasons.push('REVIEW_TIMING');
  if(!Number.isFinite(r.clockWidthMs)||r.clockWidthMs<0||r.clockWidthMs>1500)reasons.push('CLOCK');
  if(v?.reviewAgeUpperMs+safetyMs>maxVisualAgeMs||v?.backendAgeUpperMs>maxBackendAgeMs)reasons.push('AGE_MARGIN');
  return {imageMs:i?.totalMs,reviewAgeMs:v?.reviewAgeUpperMs,visualMarginMs:maxVisualAgeMs-(v?.reviewAgeUpperMs??Infinity),backendAgeMs:v?.backendAgeUpperMs};
 });
 return {eligible:reasons.length===0,reasons:[...new Set(reasons)],samples,safetyMs,maxVisualAgeMs,maxBackendAgeMs,timingScreenOnly:true,reviewAllowanceSimulated:true,realReviewerQualified:false,newRentalAllowed:false,streamQualityAccepted:false,limits:'Idle actual phone/Host dry-run with synthetic authority and 1s review allowance. Revalidate all fresh state and actual visual approval in any separately authorized pilot; this is not a live-capture authorization.'};
}
