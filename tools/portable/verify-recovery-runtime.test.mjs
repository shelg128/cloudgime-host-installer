import{test}from'node:test';import assert from'node:assert/strict';import{contract,validate}from'./verify-recovery-runtime.mjs';
const h={schema:1,contract,component:'host',ticketRouteBinding:true,ipcGrant:true},s={schema:1,contract,component:'streamer',scoped:true,negotiatedVideo:true,bandwidthBudget:true,qaUnscoped:false};
test('matching scoped pair accepted',()=>assert.equal(validate(h,s),true));
test('old or mismatched pair refused',()=>{assert.throws(()=>validate({},s));assert.throws(()=>validate(h,{...s,contract:'old'}));assert.throws(()=>validate(s,h));});
test('QA unscoped and partially built guards refused',()=>{assert.throws(()=>validate(h,{...s,qaUnscoped:true}));for(const k of['scoped','negotiatedVideo','bandwidthBudget'])assert.throws(()=>validate(h,{...s,[k]:false}));assert.throws(()=>validate({...h,ticketRouteBinding:false},s));});
