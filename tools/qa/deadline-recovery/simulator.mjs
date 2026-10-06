import {DeadlineRecovery} from './controller.mjs';
class Heap {
 constructor(){this.a=[];this.order=0;}
 push(t,fn){if(!Number.isFinite(t)||t<0)throw Error('EVENT_TIME');const e={t,fn,o:this.order++},a=this.a;a.push(e);let i=a.length-1;while(i){const p=(i-1)>>1;if(a[p].t<e.t||a[p].t===e.t&&a[p].o<e.o)break;a[i]=a[p];i=p;}a[i]=e;}
 pop(){const a=this.a,root=a[0],last=a.pop();if(a.length){let i=0;while(i*2+1<a.length){let c=i*2+1;if(c+1<a.length&&(a[c+1].t<a[c].t||a[c+1].t===a[c].t&&a[c+1].o<a[c].o))c++;if(last.t<a[c].t||last.t===a[c].t&&last.o<a[c].o)break;a[i]=a[c];i=c;}a[i]=last;}return root;}
}
function random(seed,id,kind,salt=0){let x=(seed^Math.imul(id+1,0x9e3779b1)^Math.imul(kind+salt+1,0x85ebca6b))>>>0;x^=x>>>16;x=Math.imul(x,0x7feb352d);x^=x>>>15;x=Math.imul(x,0x846ca68b);x^=x>>>16;return (x>>>0)/4294967296;}
export function serializeEnd(start,bytes,s){let t=start,left=bytes;for(let i=0;i<10000;i++){let cap=s.capacityMbps,until=Infinity;if(s.capacityPeriodMs){const cycle=t%s.capacityPeriodMs,low=cycle<s.capacityDipMs;cap=low?s.lowCapacityMbps:s.capacityMbps;until=t+(low?s.capacityDipMs-cycle:s.capacityPeriodMs-cycle);}const rate=cap*125;if(!(rate>0))throw Error('CAPACITY');const use=Math.min(left,(until-t)*rate);t+=use/rate;left-=use;if(left<1e-6)return t;if(until===Infinity)throw Error('SERIALIZE');t=Math.max(t,until+1e-8);}throw Error('SERVICE_LIMIT');}
const stat=a=>{a=[...a].sort((a,b)=>a-b);return {n:a.length,mean:a.length?a.reduce((x,y)=>x+y,0)/a.length:null,p50:a[Math.floor((a.length-1)*.5)]??null,p95:a[Math.ceil(a.length*.95)-1]??null,max:a.at(-1)??null};};
export function simulate({workload,scenario,seed,mode,metadata='unknown-conservative'}){
 const h=new Heap(),ctl=new DeadlineRecovery({mode});ctl.reset('offline-session');
 const packets=workload.packets.map(p=>({...p,deadlineMs:workload.frames[p.frame].startMs+110,reference:metadata==='synthetic-known'?workload.frames[p.frame].reference:'unknown'}));
 const n=packets.length,rx=Array(n).fill(Infinity),ack=Array(n).fill(false),attempt=Array(n).fill(0),tries=Array(n).fill(0),gapAsked=new Set(),nackPending=new Set();
 let high=-1,now=0,events=0,reverseFree=0,feedbackBytes=0,feedbackPackets=0,queue=[],queueHead=0,busy=false,currentQueueBytes=0,maxQueueBytes=0,lastRtt=scenario.oneWayMs*2,cancelledNack=0,maxPending=0;
 const counters={original:{packets:0,bytes:0,dropped:0},nack:{packets:0,bytes:0,dropped:0},proactive:{packets:0,bytes:0,dropped:0}},queueWaits=[],originalSubmissions=[],decisions=[];
 function send(packet,kind){const c=counters[kind];c.packets++;c.bytes+=packet.bytes;if(kind==='original')originalSubmissions.push({id:packet.id,time:now});queue.push({packet,kind,submitted:now,attempt:++attempt[packet.id]});currentQueueBytes+=packet.bytes;maxQueueBytes=Math.max(maxQueueBytes,currentQueueBytes);if(!busy)service();}
 function service(){if(queueHead>=queue.length){busy=false;return;}busy=true;const tx=queue[queueHead++];if(queueHead>2048){queue=queue.slice(queueHead);queueHead=0;}currentQueueBytes-=tx.packet.bytes;const start=now,end=serializeEnd(start,tx.packet.bytes,scenario);if(tx.kind==='original')queueWaits.push(start-tx.submitted);
  h.push(end,()=>{const shift=(seed%997)*1.7,burst=scenario.burstPeriodMs&&(end+shift)%scenario.burstPeriodMs<scenario.burstDurationMs;const u=random(seed,tx.packet.id,tx.attempt,1);const lost=burst||u<scenario.loss;if(lost)counters[tx.kind].dropped++;
   else{const stall=random(seed,tx.packet.id,tx.attempt,5)<scenario.stallProbability?(scenario.stallMs||0):0;const jitter=random(seed,tx.packet.id,tx.attempt,9)*(scenario.jitterMs||0);h.push(end+scenario.oneWayMs+jitter+stall,()=>receive(tx));}
   service();});
 }
 function feedback(fn){const end=serializeEnd(Math.max(now,reverseFree),78,scenario);reverseFree=end;feedbackBytes+=78;feedbackPackets++;h.push(end+scenario.oneWayMs,fn);}
 function requestMissing(id){if(rx[id]!==Infinity||nackPending.has(id)||tries[id]>=3||now>packets[id].sendMs+1000)return;nackPending.add(id);tries[id]++;feedback(()=>{nackPending.delete(id);const packet=packets[id];ctl.feedback({nowMs:now,rttMs:lastRtt,gap:true});const decision=ctl.nack({packet,nowMs:now,scope:'offline-session'});if(!ack[id]&&decision.send&&now<=packet.sendMs+1000)send(packet,'nack');else if(!decision.send){cancelledNack++;if(decisions.length<20)decisions.push({id,at:now,reason:decision.reason});}
    h.push(now+scenario.oneWayMs*2+10,()=>requestMissing(id));});
 }
 function receive(tx){const id=tx.packet.id;if(rx[id]===Infinity){rx[id]=now;feedback(()=>{ack[id]=true;const rtt=now-tx.submitted;lastRtt=rtt;ctl.feedback({nowMs:now,rttMs:rtt,gap:false});});
   if(id>high){for(let m=high+1;m<id;m++)if(!gapAsked.has(m)){gapAsked.add(m);h.push(now+(scenario.reorderGraceMs??8),()=>requestMissing(m));}high=id;}
  }
 }
 for(const packet of packets)h.push(packet.sendMs,()=>{send(packet,'original');const ticket=ctl.original({packet,nowMs:now,scope:'offline-session'});maxPending=Math.max(maxPending,ctl.pending?1:0);if(ticket){if(mode==='fixed-share-25')send(packet,'proactive');else h.push(ticket.dueMs,()=>{const d=ctl.dispatch(ticket,{nowMs:now,scope:'offline-session',alreadyReceived:ack[packet.id]});if(d)send(d.packet,'proactive');});}});
 const maxTime=packets.at(-1).sendMs+10000;
 while(h.a.length){const e=h.pop();now=e.t;if(now>maxTime)throw Error('DRAIN_TIMEOUT');e.fn();if(++events>2000000)throw Error('EVENT_LIMIT');}
 const ready=[],missed=[],late=[],available=[],ownLate=[];
 for(const f of workload.frames){const complete=Math.max(...f.packetIds.map(i=>rx[i]));const parent=f.parent===null?0:ready[f.parent];const output=Number.isFinite(complete)&&Number.isFinite(parent)?Math.max(complete,parent)+14:Infinity;ready.push(output);const deadline=f.startMs+110;available.push(Number.isFinite(output));missed.push(output>deadline);ownLate.push(complete+14>deadline);if(Number.isFinite(output))late.push(Math.max(0,output-deadline));}
 const submissionInvariant=originalSubmissions.length===packets.length&&originalSubmissions.every(x=>x.time===packets[x.id].sendMs);
 const allBytes=Object.values(counters).reduce((s,c)=>s+c.bytes,0),duration=packets.at(-1).sendMs-packets[0].sendMs;
 return {scenario:scenario.name,seed,mode,metadata,frameCount:workload.frames.length,packetCount:n,missedFrames:missed.filter(Boolean).length,deadlineMissPercent:100*missed.filter(Boolean).length/missed.length,ownPayloadLatePercent:100*ownLate.filter(Boolean).length/ownLate.length,unavailableFrames:available.filter(x=>!x).length,lateOutputMs:stat(late),originalQueueMs:stat(queueWaits),counters,feedbackBytes,feedbackPackets,totalWireBytes:allBytes,totalBidirectionalWireBytes:allBytes+feedbackBytes,totalWireMbps:allBytes*8/duration/1000,proactivePercent:100*counters.proactive.bytes/counters.original.bytes,maxQueueBytes,maxPending,cancelledNack,decisions,originalSubmissionInvariant:submissionInvariant,events,physicalFreezePercent:null,productionAccepted:false};
}
