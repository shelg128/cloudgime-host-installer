// Offset interval only, never a media one-way latency claim. Four timestamps:
// local send L0, Host receive H1, Host send H2, local receive L3.
export function assessClockExchange(samples,{maxWidthMs=1500,maxDriftPpm=100,maxUseAgeMs=180000}={}){
 const intervals=[];const reasons=[];
 if(!Number.isFinite(maxWidthMs)||maxWidthMs<=0||maxWidthMs>1500||!Number.isFinite(maxDriftPpm)||maxDriftPpm<0||maxDriftPpm>1000||!Number.isFinite(maxUseAgeMs)||maxUseAgeMs<=0||maxUseAgeMs>180000)return{eligible:false,reasons:['INVALID_LIMITS']};
 if(!Array.isArray(samples)||samples.length<3||samples.length>8)return{eligible:false,reasons:['SAMPLE_COUNT']};
 let priorSend=-Infinity,priorReceive=-Infinity;
 for(const s of samples){const {localSendMs:l0,localReceiveMs:l3,localElapsedMs:elapsed,receivedUtcMs:h1,sentUtcMs:h2,receivedMonoMs:m1,sentMonoMs:m2}=s;
  if(![l0,l3,h1,h2].every(Number.isSafeInteger)||!Number.isFinite(elapsed)||!Number.isFinite(m1)||!Number.isFinite(m2)||Math.min(l0,l3,h1,h2,m1,m2)<0||l3<l0||elapsed<0||l0<priorSend||l3<priorReceive||h2<h1||m2<m1){reasons.push('INVALID_TIMESTAMPS');continue;}
  if(Math.abs((l3-l0)-elapsed)>20||Math.abs((h2-h1)-(m2-m1))>20){reasons.push('CLOCK_STEP');continue;}
  priorSend=l0;priorReceive=l3;const lower=h2-l3-2,upper=h1-l0+2;if(lower>upper){reasons.push('IMPOSSIBLE_INTERVAL');continue;}
  intervals.push({lowerOffsetMs:lower,upperOffsetMs:upper,widthMs:upper-lower,localReceiveMs:l3,exchangeMs:elapsed,remoteWorkMs:m2-m1});
 }
 if(reasons.length||intervals.length!==samples.length)return{eligible:false,reasons:[...new Set(reasons)]};
 // Individual message with narrowest interval; never claim an absolute point offset.
 const best=intervals.reduce((a,b)=>a.widthMs<=b.widthMs?a:b);
 if(best.widthMs>maxWidthMs)reasons.push('UNCERTAINTY_TOO_WIDE');
 const lower=Math.max(...intervals.map(i=>i.lowerOffsetMs)),upper=Math.min(...intervals.map(i=>i.upperOffsetMs)); if(lower>upper)reasons.push('INCONSISTENT_OFFSETS');
 return{eligible:!reasons.length,reasons,best,intervals,maxDriftPpm,maxUseAgeMs,scope:'control-evidence clock-offset bounds, not photon or one-way network timing'};
}
export function mapEvidenceLowerBound(clock,observedPcMs,nowPcMs){if(!clock?.eligible||!Number.isFinite(clock?.best?.lowerOffsetMs)||!Number.isFinite(clock?.best?.upperOffsetMs)||clock.best.lowerOffsetMs>clock.best.upperOffsetMs||!Number.isFinite(clock?.best?.localReceiveMs)||!Number.isFinite(clock.maxDriftPpm)||clock.maxDriftPpm<0||!Number.isFinite(clock.maxUseAgeMs)||clock.maxUseAgeMs<=0||![observedPcMs,nowPcMs].every(Number.isSafeInteger)||nowPcMs<observedPcMs)throw Error('INVALID_EVIDENCE_TIME');const age=Math.max(Math.abs(nowPcMs-clock.best.localReceiveMs),Math.abs(observedPcMs-clock.best.localReceiveMs));if(age>clock.maxUseAgeMs)throw Error('CLOCK_SAMPLE_EXPIRED');const drift=age*clock.maxDriftPpm/1e6;return observedPcMs+clock.best.lowerOffsetMs-drift;}
