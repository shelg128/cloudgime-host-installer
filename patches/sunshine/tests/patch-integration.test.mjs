import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import crypto from 'node:crypto';import path from 'node:path';import {transform} from '../apply-capture-recovery.mjs';
const src=process.env.CG_SUNSHINE_TEST_SOURCE;
if(!src)throw Error('Set CG_SUNSHINE_TEST_SOURCE to upstream src/platform/windows directory');
const dir=new URL('..',import.meta.url);const lock=JSON.parse(fs.readFileSync(new URL('upstream-lock.json',dir)));
const files={};for(const[n,hash]of Object.entries(lock.files)){const b=fs.readFileSync(path.join(src,n));assert.equal(crypto.createHash('sha256').update(b).digest('hex'),hash);files[n]=b.toString('utf8');}
const patch=transform(files);
test('capture acquisition recovers only ACCESS_LOST; other errors retain reinit',()=>{
 const s=patch['display_base.cpp'];assert.match(s,/case WAIT_ABANDONED:\s*return capture_e::reinit;/);
 assert.match(s,/case DXGI_ERROR_ACCESS_LOST:\s*if \(\*res_p\)/);
 assert.match(s,/recover_access_lost\(status\) \? capture_e::timeout : capture_e::reinit/);
 assert.match(s,/case DXGI_ERROR_ACCESS_DENIED:\s*return capture_e::reinit;/);
 assert.match(s,/recover_access_lost\(status\) \? capture_e::ok : capture_e::reinit/);
});
test('RAM and VRAM require fresh real frame after capture recovery',()=>{
 for(const f of ['display_ram.cpp','display_vram.cpp']){
  assert.match(patch[f],/awaiting_fresh_frame = true;/);
  assert.match(patch[f],/if \(!frame_update_flag\) return capture_e::timeout;/);
  assert.match(patch[f],/Capture size changed/);assert.match(patch[f],/Capture format changed/);
 }
 assert.match(patch['display_vram.cpp'],/last_frame_variant = std::monostate/);
 assert.match(patch['display_vram.cpp'],/old_surface_delayed_destruction.reset\(\)/);
 assert.match(patch['display_ram.cpp'],/cursor.img_data.clear\(\)/);
});
test('normal display invalidation remains a full reinit',()=>assert.match(patch['display_base.cpp'],/if \(!factory->IsCurrent\(\)\) \{\s*return platf::capture_e::reinit;/));
test('the API selected at initial capture is retained',()=>{assert.match(patch['display_base.cpp'],/used_output5 = true/);assert.match(patch['display_base.cpp'],/get_supported_capture_formats\(\), used_output5/);});
test('altered source is rejected instead of fuzzy patched',()=>{assert.throws(()=>transform({...files,'display.h':files['display.h'].replace('    ~duplication_t();','    ~different_class();')}),/Source mismatch/);});
test('capture recovery implementation never switches desktop or owns encoder',()=>{
 const s=fs.readFileSync(new URL('capture_recovery_windows.h',dir),'utf8');assert.doesNotMatch(s,/\b(SetThreadDesktop|SwitchDesktop|syncThreadDesktop)\s*\(/);
 assert.match(s,/OpenInputDesktop/);assert.match(s,/GetDeviceRemovedReason/);assert.match(s,/initial_==read_identity\(adapter,output,nd\)/);
 const i=fs.readFileSync(new URL('capture_recovery_integration.inc',dir),'utf8');assert.doesNotMatch(i.replace(/\/\/[^\n]*/g,''),/\b(init|sleep_for)\s*\(/);
 assert.match(i,/\+\+recovery_generation/);
});
