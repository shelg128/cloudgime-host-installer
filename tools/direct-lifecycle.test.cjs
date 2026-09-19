'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');
const root = process.env.DIRECT_LIFECYCLE_SOURCE_ROOT || path.join(__dirname, '..');
const sources = Object.fromEntries(['web/stream.ts','web/stream/index.ts'].map(rel => [rel, ts.createSourceFile(rel, fs.readFileSync(path.join(root,rel),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TS)]));
function findNode(sf, predicate) { let found; function visit(n) { if(predicate(n)) { found=n; return; } if(!found)ts.forEachChild(n,visit); } visit(sf); assert.ok(found,'source method/expression must exist');return found; }
function harness(file, methods) {
 const sf=sources[file], timers=new Map(), events=[];let now=100000,nextTimer=1;
 class Video { constructor(){this.paused=false;this.readyState=4;this.videoWidth=1280;this.videoHeight=720;} async play(){} }
 const ctx=vm.createContext({document:{visibilityState:'visible'},Date:{now:()=>now},HTMLVideoElement:Video,HTMLCanvasElement:class{},console:{info(){},debug(){}},CustomEvent:class{constructor(type,options){this.type=type;this.detail=options.detail;}},WebSocket:{OPEN:1},isMobileAutoReconnectOptedIn:()=>false,window:{setTimeout(fn,ms){const id=nextTimer++;timers.set(id,{fn,ms});return id;},clearTimeout(id){timers.delete(id);},matchMedia(){return{matches:false}}},navigator:{maxTouchPoints:0}});
 const members=methods.map(name=>findNode(sf,n=>ts.isMethodDeclaration(n)&&n.name.getText(sf)===name).getText(sf));
 const js=ts.transpileModule(`class Subject { ${members.join('\n')} }; globalThis.Subject=Subject`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
 vm.runInContext(js,ctx);const subject=new ctx.Subject();return{subject,ctx,timers,events,Video,setNow:v=>{now=v;},now:()=>now};
}
test('startup budget waits for display preparation and is bounded for every viewer',()=>{
 const sf=sources['web/stream/index.ts']; const expr=findNode(sf,n=>ts.isBinaryExpression(n)&&n.left.getText(sf)==='this.startupGuardTimeoutMs').right.getText(sf);
 for(const [input,expected]of [[undefined,45000],[11000,45000],[14000,45000],[60000,60000],[1e9,90000],[NaN,45000],[Infinity,45000]]){
  assert.equal(vm.runInNewContext(expr,{startupGuardTimeoutMs:input,isTouchMobileLikeClient:()=>true}),expected);
 }
});
test('startup timer never reports false timeout while hidden and stops on completion/closed channel',()=>{
 const h=harness('web/stream/index.ts',['clearStartStreamGuardTimer','armStartStreamGuardTimer']);const s=h.subject;
 Object.assign(s,{ws:{readyState:1},startupGuardTimeoutMs:45000,connectionCompleted:false,logClientEvent(){},getLifecycleSnapshot:()=>'',eventTarget:{dispatchEvent:e=>h.events.push(e)}});
 s.armStartStreamGuardTimer();let t=[...h.timers.values()][0];assert.equal(t.ms,45000);
 h.ctx.document.visibilityState='hidden';h.timers.clear();t.fn();assert.equal(h.events.length,0);assert.equal(h.timers.size,1);
 h.ctx.document.visibilityState='visible';t=[...h.timers.values()][0];h.timers.clear();t.fn();assert.equal(h.events.length,1);assert.equal(h.events[0].detail.type,'startupTimeout');
 for(const state of ['completed','closed']){s.connectionCompleted=state==='completed';s.ws.readyState=state==='closed'?3:1;s.armStartStreamGuardTimer();t=[...h.timers.values()].at(-1);h.timers.clear();t.fn();assert.equal(h.events.length,1);}
});
test('resuming sends heartbeat on the existing authorized socket only',()=>{
 const h=harness('web/stream/index.ts',['resumeControlHeartbeat','sendWsHeartbeat']);const s=h.subject;const sent=[];s.ws={readyState:1,send:x=>sent.push(JSON.parse(x))};
 s.resumeControlHeartbeat();assert.equal(sent.length,1);assert.deepEqual(Object.keys(sent[0]),['Heartbeat']);
 s.ws.readyState=3;s.resumeControlHeartbeat();assert.equal(sent.length,1);
});
test('asynchronous watchdog ticks cannot overlap and always release the lock',async()=>{
 const h=harness('web/stream.ts',['tickVideoStallWatch']);let release,calls=0;h.subject.observeVideoStallWatch=()=>{calls++;return new Promise(r=>{release=r;})};
 const first=h.subject.tickVideoStallWatch();await h.subject.tickVideoStallWatch();assert.equal(calls,1);release();await first;assert.equal(h.subject.videoStallWatchBusy,false);
 h.subject.observeVideoStallWatch=()=>Promise.reject(new Error('stats unavailable'));await assert.rejects(h.subject.tickVideoStallWatch(),/stats unavailable/);assert.equal(h.subject.videoStallWatchBusy,false);
});
function observation(overrides={}){
 const h=harness('web/stream.ts',['observeVideoStallWatch']);const counters={idr:0,recover:0,reset:0,reconnect:0};const media=new h.Video();
 const stream={requestIdrFrame(){counters.idr++;},logClientEvent(){},getVideoRenderer:()=>({onUserInteraction(){}}),hasInboundVideoActivitySince:async()=>true,recoverVideoRendererInPlace:async()=>{counters.recover++;return false;}};
 Object.assign(h.subject,{stream,reconnecting:false,startupInputLocked:false,videoStallSuppressedUntil:0,videoStallExpectMotionUntil:0,videoStallLastProgressAt:70000,videoStallLastKickAt:0,videoStallLastRecoveryAt:0,videoStallLastRendererRecoveryAt:0,isDisplayTransitionActive:()=>false,getActiveStreamMediaElement:()=>media,getStreamMediaDimensions:()=>[1280,720],readVideoProgressSample:()=>({kind:'frames',metric:77}),didVideoProgressAdvance:()=>false,isMobileUiMode:()=>false,hasRecentVideoStallUserInteraction:()=>true,refreshVideoStallInboundActivityBaseline(){},resetVideoStallWatch(){counters.reset++;},reconnectStream(){counters.reconnect++;},setStreamRecoveryFreezeFrame(){},captureVisibleStreamFrame(){},setStreamRecoveryOverlayVisible(){},setConnectionRoute(){},translate:x=>x,suppressVideoStallRecovery(){}},overrides);
 return{...h,counters,stream};
}
test('visible renderer stall requests IDR but never destroys the Direct session',async()=>{
 const h=observation();await h.subject.observeVideoStallWatch();assert.equal(h.counters.recover,1);assert.equal(h.counters.reconnect,0);assert.ok(h.counters.idr>0);assert.equal(h.counters.reset,1);assert.equal(h.subject.stream,h.stream);
});
test('static idle scene, hidden tab and missing media do not reconnect',async()=>{
 for(const variant of ['idle','hidden','no-media']){const h=observation();if(variant==='idle'){h.stream.hasInboundVideoActivitySince=async()=>false;h.subject.hasRecentVideoStallUserInteraction=()=>false;}if(variant==='hidden')h.ctx.document.visibilityState='hidden';if(variant==='no-media')h.subject.getActiveStreamMediaElement=()=>null;await h.subject.observeVideoStallWatch();assert.equal(h.counters.reconnect,0);assert.equal(h.counters.recover,0);}
});
test('backgrounding or replacing the stream during asynchronous stats invalidates the observation',async()=>{
 for(const variant of ['hidden','replaced','suppressed']){const h=observation();h.subject.videoStallLastKickAt=h.now();h.stream.hasInboundVideoActivitySince=async()=>{if(variant==='hidden')h.ctx.document.visibilityState='hidden';if(variant==='replaced')h.subject.stream={};if(variant==='suppressed')h.subject.videoStallSuppressedUntil=h.now()+8000;return true;};await h.subject.observeVideoStallWatch();assert.equal(h.counters.recover,0);assert.equal(h.counters.reconnect,0);assert.equal(h.counters.idr,0);}
});
test('returning to a connected tab renews observation grace, not the claim or transport',()=>{
 const h=harness('web/stream.ts',['resumeExistingStreamAfterVisibilityChange','scheduleResumeRecovery','onVisibilityChange']);let heartbeat=0,grace=0;const s=h.subject;
 Object.assign(s,{stream:{resumeControlHeartbeat(){heartbeat++;}},suppressVideoStallRecovery:ms=>{grace=ms;},currentRouteState:'direct',reconnecting:false,isMobileResumeRecoveryEnabled:()=>true,isMobileResumeAutoReconnectEnabled:()=>true,isMobileUiMode:()=>false,pushTouchDebugLine(){},syncDesktopUtilityDockState(){},clearResumeRecoveryTimer(){h.timers.clear();},reconnectStream(){throw Error('unexpected reconnect')},touchDebugAutoSnapshotEnabled:false});
 for(const reason of ['focus','pageshow','online']){s.scheduleResumeRecovery(reason,600000);assert.equal(grace,8000);assert.equal(h.timers.size,0);}
 s.onVisibilityChange();assert.equal(heartbeat,4);assert.equal(h.timers.size,0);
 s.currentRouteState='failed';s.scheduleResumeRecovery('online',600000);assert.equal(h.timers.size,1);
 s.currentRouteState='direct';[...h.timers.values()][0].fn();
});
