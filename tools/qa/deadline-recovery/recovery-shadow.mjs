import {SenderFeedbackAdapter, parseRtcp} from './rtcp-feedback.mjs';
// OFFLINE SHADOW ONLY. Stores bounded send history and proposals; cannot send/drop packets.
export class RecoveryShadow {
 constructor({scope,videoSsrc,receiverSsrc,nowMs=0,maxPackets=4096,cacheMs=1000}){
  if(!Number.isFinite(nowMs)||nowMs<0||!Number.isInteger(maxPackets)||maxPackets<1||maxPackets>16384||!Number.isFinite(cacheMs)||cacheMs<=0||cacheMs>10000)throw Error('SHADOW_CONFIG');
  Object.assign(this,{scope,videoSsrc,maxPackets,cacheMs});this.feedback=new SenderFeedbackAdapter({scope,videoSsrc,receiverSsrc});this.packets=new Map();this.blockedSeq=new Map();this.lastTime=nowMs;this.lastOriginalTime=-Infinity;this.lastOrdinal=null;this.credit=0;this.tokens=0;this.tokenTime=nowMs;this.originalBytes=0;this.proposedBytes=0;this.active=true;
 }
 time(now){if(!Number.isFinite(now)||now<this.lastTime)throw Error('SHADOW_CLOCK');this.lastTime=now;for(const [seq,p]of this.packets)if(now-p.sentMonoMs>this.cacheMs)this.packets.delete(seq);for(const[seq,until]of this.blockedSeq)if(now>until)this.blockedSeq.delete(seq);}
 remember(p){if(!this.active)throw Error('REVOKED');this.time(p.sentMonoMs);if(p.scope!==this.scope||p.ssrc!==this.videoSsrc||!Number.isInteger(p.sequence)||p.sequence<0||p.sequence>65535||!Number.isSafeInteger(p.extendedSequence)||p.extendedSequence<0||(p.extendedSequence%65536)!==p.sequence||!Number.isInteger(p.rtpTimestamp)||p.rtpTimestamp<0||p.rtpTimestamp>0xffffffff||!Number.isInteger(p.bytes)||p.bytes<1||p.bytes>1328||!['key','ref','unknown','nonreference-header'].includes(p.reference))throw Error('PACKET_SCOPE');
  if(p.sentMonoMs-this.lastOriginalTime>this.cacheMs){this.credit=0;this.tokens=0;}this.lastOriginalTime=p.sentMonoMs;
  if(this.lastOrdinal!==null&&p.extendedSequence<=this.lastOrdinal)throw Error('ORIGINAL_SEQUENCE_NOT_INCREASING');this.lastOrdinal=p.extendedSequence;
  if(this.packets.has(p.sequence)){this.packets.delete(p.sequence);this.blockedSeq.set(p.sequence,p.sentMonoMs+this.cacheMs);return {stored:false,reason:'AMBIGUOUS_SEQUENCE_EPOCH'};}
  if(this.blockedSeq.has(p.sequence))return {stored:false,reason:'AMBIGUOUS_SEQUENCE_EPOCH'};
  if(this.packets.size>=this.maxPackets)this.packets.delete(this.packets.keys().next().value);
  const trustedDeadline=p.deadline?.clock==='host-monotonic'&&p.deadline?.source==='validated-receiver-mapping'&&Number.isFinite(p.deadline.atMs)&&Number.isFinite(p.deadline.uncertaintyMs)&&p.deadline.uncertaintyMs>=0&&p.deadline.uncertaintyMs<=5;
  this.packets.set(p.sequence,{...p,deadline:trustedDeadline?{...p.deadline}:null});this.originalBytes+=p.bytes;this.credit=Math.min(1328,this.credit+p.bytes*.25);return {stored:true,deadlineTrusted:trustedDeadline};
 }
 receive(bytes,env){if(!this.active)throw Error('REVOKED');if(env.observation==='host-received'&&env.authenticated===true&&env.scope===this.scope){const parsed=parseRtcp(bytes);if(parsed.some(p=>p.kind==='rr'&&p.senderSsrc===this.feedback.receiverSsrc&&p.reports.some(r=>r.ssrc===this.videoSsrc&&(this.lastOrdinal===null||r.extendedHighestSequence>this.lastOrdinal))))return {accepted:false,reason:'REPORT_ACKNOWLEDGES_UNSENT_SEQUENCE',positivePacketAcknowledgements:[],negativeAcknowledgements:[],matched:[],shadowOnly:true};}const result=this.feedback.ingest(bytes,env);if(env.observation==='host-received'&&env.authenticated===true&&env.scope===this.scope&&!result.reason)this.time(env.nowMonoMs);const matched=result.negativeAcknowledgements.map(sequence=>{const p=this.packets.get(sequence);return p?{sequence,extendedSequence:p.extendedSequence,rtpTimestamp:p.rtpTimestamp,reference:p.reference,deadlineTrusted:!!p.deadline,action:'KEEP_EXISTING_NACK',dropAllowed:false}:{sequence,action:'UNMAPPED_KEEP_EXISTING_NACK',dropAllowed:false};});if(result.reason==='REPORT_SEQUENCE_RESET_REQUIRES_REBIND')this.revoke();return {...result,matched,shadowOnly:true};}
 propose({sequence,nowMs,feedbackFreshMs=500,observedRttFloorMs,totalVideoWireRateBps}){
  this.time(nowMs);const deny=reason=>({action:'NONE',reason,shadowOnly:true});if(!this.active)return deny('REVOKED');const p=this.packets.get(sequence),state=this.feedback.snapshot(nowMs);
  if(!Number.isFinite(feedbackFreshMs)||feedbackFreshMs<=0||feedbackFreshMs>500)return deny('INVALID_FRESHNESS_LIMIT');
  if(!p)return deny('MISSING_OR_EXPIRED_PACKET');if(!p.deadline)return deny('NO_VALIDATED_DEADLINE');
  if(!state.report||nowMs-state.report.receivedMonoMs>feedbackFreshMs)return deny('NO_FRESH_HOST_RR');
  if(!state.remb||nowMs-state.remb.receivedMonoMs>feedbackFreshMs)return deny('NO_FRESH_VIDEO_REMB');
  if(!Number.isFinite(observedRttFloorMs)||observedRttFloorMs<=0||!Number.isFinite(totalVideoWireRateBps)||totalVideoWireRateBps<0)return deny('MISSING_SENDER_OBSERVATIONS');
  if(state.report.rttMs-observedRttFloorMs>10||totalVideoWireRateBps>=state.remb.bitrateBps*.9){this.credit=0;return deny('NO_CONGESTION_HEADROOM');}
  if(state.report.fractionLost===0)return deny('NO_RR_LOSS_SIGNAL');
  // Conservatively uses whole observed RTT, not asserted RTT/2 one-way truth.
  if(nowMs+state.report.rttMs+14+p.deadline.uncertaintyMs>=p.deadline.atMs)return deny('INSUFFICIENT_DEADLINE_MARGIN');
  if(!['key','ref'].includes(p.reference))return deny('NO_REFERENCE_PRIORITY');
  if(p.proposed)return deny('ALREADY_PROPOSED');this.tokens=Math.min(1328,this.tokens+(nowMs-this.tokenTime)*250);this.tokenTime=nowMs;
  if(this.credit<p.bytes||this.tokens<p.bytes)return deny('NO_OPTIONAL_BYTE_BUDGET');this.credit-=p.bytes;this.tokens-=p.bytes;this.proposedBytes+=p.bytes;p.proposed=true;
  return {action:'PROPOSE_COPY_ONLY',sequence,bytes:p.bytes,reason:'REFERENCE_DEADLINE_WITH_HEADROOM',positiveAckAvailable:false,shadowOnly:true};
 }
 revoke(){this.active=false;this.packets.clear();this.blockedSeq.clear();this.feedback.invalidate();this.credit=0;this.tokens=0;}
}
