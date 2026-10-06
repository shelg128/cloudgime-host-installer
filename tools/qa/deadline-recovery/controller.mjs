// Offline prototype only. No WebRTC, sockets, production imports, or hidden loss oracle.
export class DeadlineRecovery {
 constructor({mode='deadline-aware',share=.25,carryBytes=1328,rateBytesPerMs=250,burstBytes=1328}={}) {
  if(!['nack-only','fixed-share-25','deadline-aware'].includes(mode))throw Error('MODE');
  this.mode=mode;this.share=share;this.carry=carryBytes;this.credit=0;this.rate=rateBytesPerMs;this.burst=burstBytes;this.tokens=0;this.lastTokenMs=0;this.scope=null;this.pending=null;this.lastFeedback=-Infinity;this.rtt=null;this.minRtt=null;this.lossRisk=0;this.lastNow=-Infinity;
 }
 reset(scope=null){this.scope=scope;this.credit=0;this.tokens=0;this.pending=null;this.lastFeedback=-Infinity;this.rtt=null;this.minRtt=null;this.lossRisk=0;this.lastNow=-Infinity;this.lastTokenMs=0;}
 check(now){if(!Number.isFinite(now)||now<this.lastNow)throw Error('CLOCK_ROLLBACK');this.lastNow=now;}
 feedback({nowMs,rttMs,gap=false}){this.check(nowMs);if(!Number.isFinite(rttMs)||rttMs<=0)throw Error('RTT');this.lastFeedback=nowMs;this.minRtt=this.minRtt===null?rttMs:Math.min(this.minRtt,rttMs);this.rtt=this.rtt===null?rttMs:.85*this.rtt+.15*rttMs;this.lossRisk=.98*this.lossRisk+.02*(gap?1:0);}
 congested(){return this.rtt!==null&&this.minRtt!==null&&this.rtt-this.minRtt>10;}
 original({packet,nowMs,scope,eligible=true}){
  this.check(nowMs);if(scope!==this.scope){this.reset(scope);this.check(nowMs);}
  if(!eligible){this.credit=0;this.pending=null;return null;}
  if(!Number.isInteger(packet.bytes)||packet.bytes<=0)throw Error('SIZE');
  this.credit=Math.min(this.carry,this.credit+packet.bytes*this.share);
  if(this.mode==='nack-only')return null;
  if(this.mode==='fixed-share-25')return this.spend(packet,nowMs)?{packet,dueMs:nowMs,reason:'fixed-share'}:null;
  if(nowMs-this.lastFeedback>500||this.rtt===null||this.lossRisk<.004||this.congested()) {if(this.congested()){this.credit=0;this.pending=null;}return null;}
  const estimate=this.rtt/2+6; // model assumption, not a production one-way measurement
  const slack=packet.deadlineMs-nowMs-14;
  if(slack<=estimate+5||slack>estimate+90)return null;
  const priority=packet.reference==='key'?4:packet.reference==='ref'?3:packet.reference==='unknown'?2:1;
  if(priority===1)return null;
  if(!this.pending||this.pending.packet.deadlineMs<nowMs||priority>this.pending.priority){this.pending={packet,scope,priority,dueMs:nowMs+6};return this.pending;}
  return null;
 }
 spend(packet,nowMs){this.tokens=Math.min(this.burst,this.tokens+Math.max(0,nowMs-this.lastTokenMs)*this.rate);this.lastTokenMs=nowMs;if(this.credit+1e-8<packet.bytes||this.tokens+1e-8<packet.bytes)return false;this.credit-=packet.bytes;this.tokens-=packet.bytes;return true;}
 dispatch(ticket,{nowMs,scope,alreadyReceived=false}){this.check(nowMs);if(this.mode!=='deadline-aware'||ticket!==this.pending)return null;this.pending=null;
  if(scope!==this.scope||scope!==ticket.scope||nowMs<ticket.dueMs||alreadyReceived||nowMs<this.lastFeedback||nowMs-this.lastFeedback>500||this.congested()||this.rtt===null||nowMs+this.rtt/2+19>=ticket.packet.deadlineMs)return null;
  return this.spend(ticket.packet,nowMs)?{packet:ticket.packet,reason:'deadline-dependency-copy'}:null;
 }
 nack({packet,nowMs,scope}){this.check(nowMs);if(scope!==this.scope)return {send:false,reason:'wrong-scope'};
  // Unknown/critical reference recovery remains on existing path even if its own display deadline passed.
  if(this.mode==='deadline-aware'&&packet.reference==='leaf'&&this.rtt!==null&&nowMs+this.rtt/2+14>=packet.deadlineMs)return {send:false,reason:'expired-certified-leaf'};
  return {send:true,reason:packet.reference==='leaf'?'useful-leaf':'preserve-reference-recovery'};
 }
}
