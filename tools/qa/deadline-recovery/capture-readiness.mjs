// A qualification decision, not runtime authorization. Do not average away a slow
// acquisition or restamp screenshot age at review time to obtain a passing result.
export function assessCaptureReadiness({dryRuns,screenshots,reviewBudgetMs=1000,safetyMs=250,maxVisualAgeMs=5000,maxBackendAgeMs=3000}){
 const reasons=[];
 if(!Number.isFinite(reviewBudgetMs)||reviewBudgetMs<1000||!Number.isFinite(safetyMs)||safetyMs<250||maxVisualAgeMs!==5000||maxBackendAgeMs!==3000)reasons.push('LIMITS_CHANGED');
 if(!Array.isArray(dryRuns)||dryRuns.length<3||!Array.isArray(screenshots)||screenshots.length<2)return {eligible:false,reasons:[...reasons,'INSUFFICIENT_PATH_EVIDENCE'],rentalMutations:0};
 for(const r of dryRuns){if(r.exit!==0||r.dryRun!==true||r.requestWritten!==false||r.requestExists!==false||r.eligible!==true||!Number.isFinite(r.backendAgeUpperMs)||r.backendAgeUpperMs<0||r.backendAgeUpperMs>maxBackendAgeMs||!Number.isFinite(r.reviewAgeUpperMs)||r.reviewAgeUpperMs<0||!Number.isFinite(r.clockWidthMs)||r.clockWidthMs<0||r.clockWidthMs>1500)reasons.push('DRYRUN_INCOMPLETE_OR_FAILED');}
 for(const s of screenshots){if(s.verified!==true||!Number.isFinite(s.totalMs)||s.totalMs<0||!Number.isSafeInteger(s.startedAtPcMs)||!Number.isSafeInteger(s.finishedAtPcMs)||s.finishedAtPcMs<s.startedAtPcMs||Math.abs((s.finishedAtPcMs-s.startedAtPcMs)-s.totalMs)>20)reasons.push('SCREENSHOT_EVIDENCE_INVALID');}
 const maxScreenshotMs=Math.max(...screenshots.map(x=>x.totalMs));const maxAfterScreenshotGateAgeMs=Math.max(...dryRuns.map(x=>x.reviewAgeUpperMs));
 const visualBudgetNeededMs=maxScreenshotMs+maxAfterScreenshotGateAgeMs+reviewBudgetMs+safetyMs;
 if(!Number.isFinite(visualBudgetNeededMs)||visualBudgetNeededMs>maxVisualAgeMs)reasons.push('VISUAL_PATH_EXCEEDS_FRESHNESS_BUDGET');
 return {eligible:reasons.length===0,reasons:[...new Set(reasons)],maxScreenshotMs,maxAfterScreenshotGateAgeMs,reviewBudgetMs,safetyMs,visualBudgetNeededMs,maxVisualAgeMs,maxBackendAgeMs,rentalMutations:0,limits:'Separate idle samples estimate feasibility, not a guarantee under streaming load; actual age must be rechecked at dispatch.',streamQualityAccepted:false};
}
export function requireCaptureReadiness(result){if(result?.eligible!==true||!Array.isArray(result.reasons)||result.reasons.length||result.maxVisualAgeMs!==5000||result.maxBackendAgeMs!==3000||!Number.isFinite(result.visualBudgetNeededMs)||result.visualBudgetNeededMs>5000||result.rentalMutations!==0)throw Error('LIVE_CAPTURE_PATH_NOT_QUALIFIED');return true;}
