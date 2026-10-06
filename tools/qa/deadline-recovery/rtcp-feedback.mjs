// Offline QA parser for already-decapsulated RTCP. Never parses SRTCP ciphertext.
export function parseRtcp(buffer) {
 if(!Buffer.isBuffer(buffer)||buffer.length<4||buffer.length>65536)throw Error('RTCP_SIZE');
 const out=[];let pos=0;
 while(pos<buffer.length){if(pos+4>buffer.length)throw Error('RTCP_HEADER');const v=buffer[pos]>>6,rc=buffer[pos]&31,pt=buffer[pos+1],size=(buffer.readUInt16BE(pos+2)+1)*4;let end=pos+size;
  if(v!==2||size<4||end>buffer.length)throw Error('RTCP_LENGTH');
  if(buffer[pos]&32){if(end!==buffer.length)throw Error('RTCP_PADDING_POSITION');const n=buffer[end-1];if(!n||n>size-4)throw Error('RTCP_PADDING');end-=n;}
  const len=end-pos,read=i=>buffer.readUInt32BE(pos+i);let row={pt,fmt:rc,wireBytes:size};
  if(pt===200||pt===201){const base=pt===200?28:8;if(len<base+24*rc)throw Error('RTCP_REPORT_LENGTH');row.kind=pt===200?'sr':'rr';row.senderSsrc=read(4);row.reports=[];
   if(pt===200){row.ntpSeconds=read(8);row.ntpFraction=read(12);row.compactNtp=((row.ntpSeconds&65535)*65536+(row.ntpFraction>>>16))>>>0;row.rtpTimestamp=read(16);row.packetCount=read(20);row.octetCount=read(24);}
   for(let i=0;i<rc;i++){const p=pos+base+i*24;let lost=buffer.readUIntBE(p+5,3);if(lost&0x800000)lost-=0x1000000;row.reports.push({ssrc:buffer.readUInt32BE(p),fractionLost:buffer[p+4]/256,cumulativeLost:lost,extendedHighestSequence:buffer.readUInt32BE(p+8),jitterTicks:buffer.readUInt32BE(p+12),lsr:buffer.readUInt32BE(p+16),dlsr:buffer.readUInt32BE(p+20)});}
   row.profileExtensionBytes=len-base-rc*24;
  }else if(pt===205&&rc===1){if(len<16||(len-12)%4)throw Error('RTCP_NACK_LENGTH');row.kind='nack';row.senderSsrc=read(4);row.mediaSsrc=read(8);const seqs=new Set();for(let p=pos+12;p<end;p+=4){const pid=buffer.readUInt16BE(p),blp=buffer.readUInt16BE(p+2);seqs.add(pid);for(let bit=0;bit<16;bit++)if(blp&(1<<bit))seqs.add((pid+bit+1)&65535);}row.sequences=[...seqs];
  }else if(pt===206&&rc===15&&len>=16&&buffer.toString('ascii',pos+12,pos+16)==='REMB'){if(len<20)throw Error('RTCP_REMB_LENGTH');const count=buffer[pos+16];if(!count||len!==20+4*count)throw Error('RTCP_REMB_SSRC');const exponent=buffer[pos+17]>>2,mantissa=((buffer[pos+17]&3)<<16)|buffer.readUInt16BE(pos+18);row.kind='remb';row.senderSsrc=read(4);row.mediaSsrc=read(8);row.bitrateBps=(BigInt(mantissa)<<BigInt(exponent)).toString();row.ssrcs=Array.from({length:count},(_,i)=>read(20+4*i));
  }else if(pt===206&&rc===1){if(len!==12)throw Error('RTCP_PLI_LENGTH');row.kind='pli';row.senderSsrc=read(4);row.mediaSsrc=read(8);
  }else row.kind='other';out.push(row);pos+=size;
 }
 return out;
}

// Temporal and scope boundary: callers must supply authenticated decrypted feedback
// stamped at Host receipt and real local SR-send evidence, never phone-send timestamps.
export class SenderFeedbackAdapter {
 constructor({scope,videoSsrc,receiverSsrc,clockRate=90000,maxAgeMs=2000,maxSrAgeMs=10000}) {
  if(typeof scope!=='string'||!scope||!Number.isInteger(videoSsrc)||videoSsrc<=0||videoSsrc>0xffffffff||!Number.isInteger(receiverSsrc)||receiverSsrc<0||receiverSsrc>0xffffffff||clockRate<=0)throw Error('SCOPE_CONFIG');
  Object.assign(this,{scope,videoSsrc,receiverSsrc,clockRate,maxAgeMs,maxSrAgeMs});this.sr=new Map();this.lastTime=-Infinity;this.lastExt=null;this.report=null;this.remb=null;this.generation=0;this.blocked=false;this.lastSrMs=-Infinity;
 }
 invalidate(){this.sr.clear();this.report=null;this.remb=null;this.lastExt=null;this.lastTime=-Infinity;this.generation++;this.blocked=true;this.lastSrMs=-Infinity;}
 sentReport({scope,ssrc,compactNtp,sentMonoMs}){if(this.blocked)throw Error('REBIND_REQUIRED');if(scope!==this.scope||ssrc!==this.videoSsrc||!Number.isInteger(compactNtp)||compactNtp<=0||compactNtp>0xffffffff||!Number.isFinite(sentMonoMs)||sentMonoMs<0)throw Error('SR_EVIDENCE');if(sentMonoMs<this.lastSrMs)throw Error('SR_CLOCK_REWIND');if(this.sr.has(compactNtp))throw Error('SR_ID_REUSED');for(const [key,value] of this.sr)if(sentMonoMs-value>this.maxSrAgeMs)this.sr.delete(key);if(this.sr.size>=128)this.sr.delete(this.sr.keys().next().value);this.sr.set(compactNtp,sentMonoMs);this.lastSrMs=sentMonoMs;}
 ingest(bytes,env){const packets=parseRtcp(bytes);const base={packetKinds:packets.map(x=>x.kind),rttSamples:[],negativeAcknowledgements:[],positivePacketAcknowledgements:[],rembUpdates:[],rejections:[],generation:this.generation};
  if(this.blocked)return {...base,accepted:false,reason:'REBIND_REQUIRED'};
  if(env.observation!=='host-received')return {...base,accepted:false,reason:'RECEIVER_LOG_OBSERVATION_ONLY'};
  if(env.authenticated!==true||env.scope!==this.scope)return {...base,accepted:false,reason:'UNTRUSTED_OR_WRONG_SCOPE'};
  if(!Number.isFinite(env.receivedMonoMs)||!Number.isFinite(env.nowMonoMs)||env.receivedMonoMs<0||env.nowMonoMs<env.receivedMonoMs||env.nowMonoMs-env.receivedMonoMs>this.maxAgeMs)return {...base,accepted:false,reason:'INVALID_RECEIPT_TIME'};
  if(env.receivedMonoMs<this.lastTime)return {...base,accepted:false,reason:'RECEIPT_CLOCK_REWIND'};this.lastTime=env.receivedMonoMs;
  for(const packet of packets){if(packet.senderSsrc!==this.receiverSsrc){base.rejections.push('RECEIVER_SSRC_MISMATCH');continue;}
   if(packet.kind==='rr'){for(const report of packet.reports){if(report.ssrc!==this.videoSsrc){base.rejections.push('REPORT_MEDIA_MISMATCH');continue;}
     if(this.lastExt!==null&&report.extendedHighestSequence<this.lastExt){this.invalidate();base.rejections.push('REPORT_SEQUENCE_RESET_REQUIRES_REBIND');base.accepted=false;base.reason='REPORT_SEQUENCE_RESET_REQUIRES_REBIND';return base;}
     const progressed=this.lastExt===null||report.extendedHighestSequence>this.lastExt;this.lastExt=report.extendedHighestSequence;
     if(report.lsr===0){base.rejections.push('NO_VALID_LOCAL_SR_RTT');continue;}
     const srTime=this.sr.get(report.lsr),elapsed=env.receivedMonoMs-srTime,delay=report.dlsr*1000/65536,rtt=elapsed-delay;
     if(srTime===undefined||elapsed<0||elapsed>this.maxSrAgeMs||rtt<0||rtt>3000){base.rejections.push('NO_VALID_LOCAL_SR_RTT');continue;}
     const sample={rttMs:rtt,fractionLost:report.fractionLost,cumulativeLost:report.cumulativeLost,jitterMs:report.jitterTicks*1000/this.clockRate,extendedHighestSequence:report.extendedHighestSequence,receivedMonoMs:env.receivedMonoMs,progressed};
     if(progressed){this.report=sample;base.rttSamples.push(sample);} // RR highest sequence is NOT a cumulative packet ACK.
    }
   }else if(packet.kind==='nack'&&packet.mediaSsrc===this.videoSsrc){base.negativeAcknowledgements.push(...packet.sequences);
   }else if(packet.kind==='remb'&&packet.ssrcs.length===1&&packet.ssrcs[0]===this.videoSsrc){const bps=BigInt(packet.bitrateBps);if(bps>0n&&bps<=1000000000n){this.remb={bitrateBps:Number(bps),receivedMonoMs:env.receivedMonoMs};base.rembUpdates.push(this.remb);}else base.rejections.push('REMB_RANGE');
   }else base.rejections.push('UNSUPPORTED_OR_MEDIA_MISMATCH');
  }
  base.accepted=base.rttSamples.length+base.negativeAcknowledgements.length+base.rembUpdates.length>0;return base;
 }
 snapshot(nowMonoMs){if(!Number.isFinite(nowMonoMs)||nowMonoMs<this.lastTime)throw Error('SNAPSHOT_CLOCK');return {scope:this.scope,generation:this.generation,report:this.report&&nowMonoMs-this.report.receivedMonoMs<=this.maxAgeMs?{...this.report}:null,remb:this.remb&&nowMonoMs-this.remb.receivedMonoMs<=this.maxAgeMs?{...this.remb}:null,packetDeliveryKnown:false};}
}
