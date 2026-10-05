import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.env.CG_DELIVERY_WORKSPACE;
if(!root)throw Error('CG_DELIVERY_WORKSPACE_REQUIRED');
const load=p=>JSON.parse(fs.readFileSync(root+'/06 TEST/'+p,'utf8'));
const comparison=load('20261006-independent-sync/windowed-comparison.json');
const incident=load('20261006-independent-sync/nack-delay-analysis.json');

test('same-window comparison keeps diagnostic and incomplete trace limits',()=>{
 for(const run of comparison.comparison){
  assert.equal(run.rtc.sameEpoch,true);
  assert.equal(run.rtc.acceptanceEligible,false);
  assert.ok(run.rtc.seconds<80);
  assert.equal(run.trace.complete,false);
  assert.ok(run.trace.footer.contention>0);
  assert.ok(run.joinedWithinWindow>1700);
 }
 const [control,experiment]=comparison.comparison;
 assert.ok(control.frameTiming.packetToDecoderMs.median>90);
 assert.ok(experiment.frameTiming.packetToDecoderMs.median<5);
 assert.equal(control.rtc.freezes,0);
 assert.equal(experiment.rtc.freezes,2);
});
test('packet-completion and NACK evidence preserve measurement boundaries',()=>{
 assert.equal(incident.incidents.length,2);
 for(const i of incident.incidents){
  assert.ok(i.originalFrameSendSpanMs<10);
  assert.ok(i.receiverFrameSpanMs>=160);
  assert.equal(i.advanceToNackMs,0);
  assert.ok(i.nackToReceiveMs>=70&&i.nackToReceiveMs<=75);
  assert.ok(i.packetCompleteToDecoderMs<3);
  assert.ok(i.decodeMs<20);
  assert.ok(i.outputToDrawMs<4);
 }
});
test('selected incidents have complete observed packet sets and adjacent raw draws',()=>{
 const c=load('20261006-independent-sync/correlated-delivery.json');
 assert.equal(c.rtc.truncatedBytes,0);
 for(const i of c.incidents){
  assert.equal(i.rawDrawAdjacent,true);
  for(const f of [i.before,i.after]){
   assert.equal(f.boundary.valid,true);
   assert.equal(new Set(f.packetRows.map(p=>p.seq)).size,f.packetRows.length);
   assert.equal(f.boundary.originalUniquePackets,f.boundary.receivedUniquePackets);
   assert.ok(f.input<=f.output&&f.output<=f.ingress&&f.ingress<=f.draw);
  }
 }
});
test('independent audio-sync remains opt-in and does not remove RTP audio',()=>{
 const base=new URL('../../',import.meta.url);
 const cargo=fs.readFileSync(new URL('streamer/Cargo.toml',base),'utf8');
 const source=fs.readFileSync(new URL('streamer/src/transport/webrtc/audio.rs',base),'utf8');
 assert.match(cargo,/default = \[\]/);
 assert.match(source,/cfg!\(feature="cloudgime-qa-independent-audio-sync"\)/);
 assert.match(source,/else \{ "moonlight".to_string\(\) \}/);
 assert.match(source,/self.sender.send_samples\(vec!\[packet\], false\).await/);
});
