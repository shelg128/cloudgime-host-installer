// Offline consumer of SDK boundary JSONL. Never upgrades observations to NIC/photon timing.
export function inspectSdkTrace(text) {
 if(typeof text!=='string'||Buffer.byteLength(text)>16*1024*1024)throw Error('TRACE_SIZE');
 const raw=text.trimEnd().split(/\r?\n/);if(raw.length<2||raw.length>50002)throw Error('TRACE_LENGTH');
 let parsed;try{parsed=raw.map(JSON.parse);}catch{throw Error('TRACE_JSON');}
 const header=parsed[0],footer=parsed.at(-1),events=parsed.slice(1,-1);const reasons=[];
 if(header.type!=='header'||header.schema!==1||header.payload_retained!==false||header.clock!=='peer-local Instant microseconds'||!Number.isSafeInteger(header.peer)||header.peer<1||!Number.isSafeInteger(header.process)||header.process<1||!/^[a-fA-F0-9]{32}$/.test(header.nonce||''))throw Error('TRACE_HEADER');
 if(footer.type!=='footer')throw Error('TRACE_FOOTER_MISSING');
 for(const n of ['accepted','written','dropped_queue','omitted','outstanding_calls','cancelled_calls','elapsed_us'])if(!Number.isSafeInteger(footer[n])||footer[n]<0)throw Error('TRACE_FOOTER_COUNTER');
 if(footer.observation_complete!==true||footer.io_error!==false||footer.quota!==false||footer.dropped_queue!==0||footer.omitted!==0||footer.outstanding_calls!==0||footer.cancelled_calls!==0||footer.accepted!==events.length||footer.written!==events.length)reasons.push('INCOMPLETE_OBSERVATION');
 const u=(v,bits)=>Number.isSafeInteger(v)&&v>=0&&v<2**bits;const ids=new Set();
 for(const e of events){if(!Number.isSafeInteger(e.id)||e.id<0||ids.has(e.id)||!Number.isSafeInteger(e.started_us)||!Number.isSafeInteger(e.completed_us)||e.started_us<0||e.completed_us<e.started_us||e.completed_us>footer.elapsed_us||!e.data||typeof e.data!=='object')throw Error('TRACE_EVENT');ids.add(e.id);
  const d=e.data;const required={Bind:{epoch:32,ssrc:32,pt:8,clock_rate:32},Unbind:{ssrc:32},Rtp:{epoch:32,ssrc:32,pt:8,seq:16,rtp:32,payload_bytes:32,returned_bytes:32},Sr:{ssrc:32,ntp_seconds:32,ntp_fraction:32,compact_ntp:32,rtp:32,returned_bytes:32},Rr:{sender:32,ssrc:32,fraction_lost:8,total_lost_raw:32,highest_sequence:32,jitter:32,lsr:32,dlsr:32},Nack:{sender:32,ssrc:32,pid:16,blp:16},Remb:{sender:32,ssrc:32},Pli:{sender:32,ssrc:32},ReadError:{}}[d.kind];
  if(!required)throw Error('TRACE_KIND');for(const [k,bits]of Object.entries(required))if(!u(d[k],bits))throw Error('TRACE_FIELD_'+k);
  if(['Rtp','Sr'].includes(d.kind)&&typeof d.ok!=='boolean')throw Error('TRACE_RESULT');
  if(d.kind==='Rtp'&&typeof d.marker!=='boolean')throw Error('TRACE_MARKER');
  if(d.kind==='Remb'&&(!Number.isFinite(d.bitrate)||d.bitrate<0))throw Error('TRACE_REMB');
  if(d.kind==='ReadError')reasons.push('SDK_READ_ERROR');
  if(d.kind==='Sr'&&(((d.ntp_seconds&65535)*65536+(d.ntp_fraction>>>16))>>>0)!==d.compact_ntp)throw Error('TRACE_NTP_PRECISION');
 }
 if(ids.size&&Math.max(...ids)!==ids.size-1)reasons.push('EVENT_ID_GAP');
 const sorted=[...events].sort((a,b)=>a.completed_us-b.completed_us||a.id-b.id),bindings=new Map(),active=new Map(),packets=new Map(),rrs=[],srs=[],nacks=[],remb=[];
 for(const e of sorted){const d=e.data;
  if(d.kind==='Bind'){if(active.has(d.ssrc))reasons.push('SSRC_REBOUND_WITHOUT_UNBIND');if(bindings.has(d.epoch))reasons.push('EPOCH_REUSED');bindings.set(d.epoch,d);active.set(d.ssrc,d.epoch);}
  if(d.kind==='Unbind')active.delete(d.ssrc);
  if(d.kind==='Rtp'){const b=bindings.get(d.epoch);if(!b||active.get(d.ssrc)!==d.epoch||b.ssrc!==d.ssrc||b.pt!==d.pt)reasons.push('RTP_BINDING_MISMATCH');if(!d.ok||!d.returned_bytes)reasons.push('RTP_WRITE_NOT_SUCCESSFUL');const key=[d.epoch,d.ssrc,d.seq,d.rtp].join(':');if(!packets.has(key))packets.set(key,[]);packets.get(key).push(e);}
  if(d.kind==='Sr')srs.push(e);if(d.kind==='Rr')rrs.push(e);if(d.kind==='Nack')nacks.push(e);if(d.kind==='Remb')remb.push(e);
 }
 const matched=[],unmatched=[];
 for(const rr of rrs){const r=rr.data;if(r.lsr===0){unmatched.push({id:rr.id,reason:'NO_LSR'});continue;}
  const candidates=srs.filter(s=>s.data.ssrc===r.ssrc&&s.data.compact_ntp===r.lsr&&s.data.ok&&s.data.returned_bytes>0&&s.completed_us<=rr.completed_us&&rr.completed_us-s.started_us<=10000000);
  if(candidates.length!==1){unmatched.push({id:rr.id,reason:candidates.length?'AMBIGUOUS_SR':'NO_LOCAL_SR'});continue;}
  const sr=candidates[0],delay=r.dlsr*1000/65536,lower=(rr.completed_us-sr.completed_us)/1000-delay,upper=(rr.completed_us-sr.started_us)/1000-delay;
  if(!Number.isFinite(lower)||lower<0||upper<lower||upper>3000){unmatched.push({id:rr.id,reason:'INVALID_SDK_CALL_RTT'});continue;}
  matched.push({ssrc:r.ssrc,receiverSsrc:r.sender,compactNtp:r.lsr,srEvent:sr.id,rrEvent:rr.id,srStartUs:sr.started_us,srCompleteUs:sr.completed_us,rrReadCompleteUs:rr.completed_us,dlsrMs:delay,sdkCallRttLowerMs:lower,sdkCallRttUpperMs:upper,boundaryWidthMs:upper-lower,networkRttProven:false});
 }
 return {observationComplete:!reasons.length,reasons:[...new Set(reasons)],peer:header.peer,process:header.process,events:events.length,counts:events.reduce((a,e)=>(a[e.data.kind]=(a[e.data.kind]||0)+1,a),{}),uniqueObservedRtpPackets:packets.size,repeatWriteObservations:[...packets.values()].reduce((s,p)=>s+Math.max(0,p.length-1),0),matchedSrRr:matched,unmatchedRr:unmatched,rtcpNackBlocks:nacks.length,rembReports:remb.length,rentalBound:false,positivePacketAcksAvailable:false,receiverDeadlineAvailable:false,productionRecoveryEnabled:false,physicalQualityAccepted:false,limitations:['Counts cover only the bounded SDK observation window; not entire rental.','SDK call entry/return and post-decrypt read completion are not NIC transmit/receive times.','Repeat RTP writes cannot alone distinguish NACK retry, proactive copy, or caller duplication.','RTP packet payload and AU dependency mapping are not retained by this interceptor.','RTCP may be observed through per-sender readers; duplicates must be reconciled before claiming unique wire packets.','Expiry or peer close does not certify low observer overhead or physical streaming quality.']};
}
