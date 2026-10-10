#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-only
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const patchDir=path.dirname(fileURLToPath(import.meta.url));
export const revision='86188d47a7463b0f73b35de18a628353adeaa20e';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
function replaceOnce(s,before,after,label){if(s.split(before).length!==2)throw Error('Source mismatch: '+label);return s.replace(before,after);}
export function transform(files){
 let h=files['display.h'],base=files['display_base.cpp'];
 h=replaceOnce(h,'#include "src/video.h"','#include "src/video.h"\n#include "capture_recovery_windows.h"','header include');
 h=replaceOnce(h,'    ~duplication_t();',`    std::uint64_t recovery_generation = 0;
    bool recover_access_lost(HRESULT reason);
    ~duplication_t();

  private:
    display_base_t *recovery_display = nullptr;
    cloudgime::capture::duplication_recovery recovery;`,'duplication private state');
 for(const name of ['display_ddup_ram_t','display_ddup_vram_t']){
  const start=h.indexOf('  class '+name+':'),end=h.indexOf('\n  };',start);
  if(start<0||end<start)throw Error('Missing display class '+name);
  let block=h.slice(start,end);block=replaceOnce(block,'    duplication_t dup;','    duplication_t dup;\n    std::uint64_t seen_recovery_generation = 0;\n    bool awaiting_fresh_frame = false;',name);
  h=h.slice(0,start)+block+h.slice(end);
 }
 base=replaceOnce(base,'    HRESULT status;\n\n    // Capture format', '    HRESULT status;\n    bool used_output5 = false;\n    recovery_display = display;\n\n    // Capture format','init declaration');
 base=replaceOnce(base,'      if (SUCCEEDED(status)) {\n        // Ask the display','      if (SUCCEEDED(status)) {\n        used_output5 = true;\n        // Ask the display','init backend');
 base=replaceOnce(base,'    display->display_refresh_rate_rounded = lround(display_refresh_rate_decimal);\n    return 0;',`    display->display_refresh_rate_rounded = lround(display_refresh_rate_decimal);
    recovery.initialize(display->adapter.get(), display->output.get(), dup.get(),
                        display->get_supported_capture_formats(), used_output5);
    return 0;`,'init recovery');
 base=replaceOnce(base,'      case DXGI_ERROR_ACCESS_LOST:\n      case DXGI_ERROR_ACCESS_DENIED:\n        return capture_e::reinit;',`        return capture_e::reinit;
      case DXGI_ERROR_ACCESS_LOST:
        if (*res_p) { (*res_p)->Release(); *res_p = nullptr; }
        // Restart pacing and request a fresh snapshot; never return cached pixels as a new frame.
        return recover_access_lost(status) ? capture_e::timeout : capture_e::reinit;
      case DXGI_ERROR_ACCESS_DENIED:
        return capture_e::reinit;`,'acquire ACCESS_LOST');
 base=replaceOnce(base,'      case DXGI_ERROR_ACCESS_LOST:\n        return capture_e::reinit;',`      case DXGI_ERROR_ACCESS_LOST:
        return recover_access_lost(status) ? capture_e::ok : capture_e::reinit;`,'release ACCESS_LOST');
 base=replaceOnce(base,'  duplication_t::~duplication_t() {','  #include "capture_recovery_integration.inc"\n\n  duplication_t::~duplication_t() {','recovery definition');
  base=replaceOnce(base,'  duplication_t::~duplication_t() {\n    release_frame();','  duplication_t::~duplication_t() {\n    recovery_display = nullptr;  // Destruction must never create a replacement duplicator.\n    release_frame();','destructor no recovery');
  const result={'display.h':h,'display_base.cpp':base};
 for(const name of ['display_ram.cpp','display_vram.cpp']){
  let s=files[name];
  const clear=name==='display_ram.cpp'?`      cursor.img_data.clear(); cursor.shape_info = {}; cursor.visible = false;`:`      last_frame_variant = std::monostate {};
      old_surface_delayed_destruction.reset();
      old_surface_timestamp = {};
      cursor_alpha.visible = false; cursor_xor.visible = false;`;
  const prefix=`    if (seen_recovery_generation != dup.recovery_generation) {
      seen_recovery_generation = dup.recovery_generation;
      awaiting_fresh_frame = true;
${clear}
    }

`;
  const snapshotStart=s.indexOf('  capture_e display_ddup_'+(name==='display_ram.cpp'?'ram':'vram')+'_t::snapshot(');
  const snapshotEnd=s.indexOf('  capture_e display_ddup_'+(name==='display_ram.cpp'?'ram':'vram')+'_t::release_snapshot()',snapshotStart);
  if(snapshotStart<0||snapshotEnd<snapshotStart)throw Error('Missing DDAPI snapshot '+name);
  let snapshot=s.slice(snapshotStart,snapshotEnd);
  snapshot=replaceOnce(snapshot,'    if (capture_status != capture_e::ok) {',prefix+'    if (capture_status != capture_e::ok) {','cache reset '+name);
  // A mouse-only event after re-duplication must never resurrect the pre-loss frame.
  const marker='    const bool update_flag = mouse_update_flag || frame_update_flag;';
  snapshot=replaceOnce(snapshot,marker,marker+`
    if (awaiting_fresh_frame) {
      if (!frame_update_flag) return capture_e::timeout;
      awaiting_fresh_frame = false;
    }`,'fresh-frame gate '+name);
  s=s.slice(0,snapshotStart)+snapshot+s.slice(snapshotEnd);
  result[name]=s;
 }
 return result;
}
export function apply(sourceRoot){
 const head=execFileSync('git',['-C',sourceRoot,'rev-parse','HEAD'],{encoding:'utf8'}).trim();
 if(head!==revision)throw Error('SUNSHINE_REVISION_MISMATCH');
 const lock=JSON.parse(fs.readFileSync(path.join(patchDir,'upstream-lock.json'),'utf8'));
 const dir=path.join(sourceRoot,'src/platform/windows');
 const manifestPath=path.join(sourceRoot,'cloudgime-capture-patch.json');
 if(fs.existsSync(manifestPath)){
  const manifest=JSON.parse(fs.readFileSync(manifestPath));
  if(manifest.revision!==revision)throw Error('PATCH_MANIFEST_REVISION_MISMATCH');
  for(const [name,expected]of Object.entries(manifest.outputs)){if(sha(fs.readFileSync(path.join(dir,name)))!==expected)throw Error('PATCHED_SOURCE_CHANGED '+name);}
  const desired=Object.fromEntries(['capture_recovery_policy.h','capture_recovery_windows.h','capture_recovery_integration.inc'].map(n=>[n,sha(fs.readFileSync(path.join(patchDir,n)))]));
  for(const[n,v]of Object.entries(desired))if(manifest.outputs[n]!==v)throw Error('PATCH_REVISION_CHANGED_REQUIRES_FRESH_SOURCE');
  return {...manifest,unchanged:true};
 }
 const files={};for(const [name,expected]of Object.entries(lock.files)){
  const b=fs.readFileSync(path.join(dir,name));
  const canonical=b.toString('utf8').replaceAll('\r\n','\n');
  if(sha(Buffer.from(canonical))!==expected)throw Error('UPSTREAM_SOURCE_MISMATCH '+name);
  const committed=execFileSync('git',['-C',sourceRoot,'show',revision+':src/platform/windows/'+name],{encoding:'utf8'}).replaceAll('\r\n','\n');
  if(canonical!==committed)throw Error('CHECKOUT_DIFFERS_FROM_LOCKED_COMMIT '+name);
  files[name]=canonical;
 }
 const output=transform(files);
 for(const n of ['capture_recovery_policy.h','capture_recovery_windows.h','capture_recovery_integration.inc'])output[n]=fs.readFileSync(path.join(patchDir,n),'utf8');
 // Transform/validate all files before writes. Interrupted work requires fresh source, not fuzzy patching.
 const hashes={};for(const [name,text]of Object.entries(output)){fs.writeFileSync(path.join(dir,name),text);hashes[name]=sha(Buffer.from(text));}
 const manifest={schemaVersion:1,revision,patch:'cloudgime-ddapi-capture-only-recovery-v1',outputs:hashes,acceptance:'SOURCE_PATCH_ONLY_NOT_RELEASE_ACCEPTED'};
 fs.writeFileSync(manifestPath,JSON.stringify(manifest,null,2));return manifest;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 if(process.argv.length!==3)throw Error('Usage: node apply-capture-recovery.mjs <pinned-Sunshine-checkout>');
 console.log(JSON.stringify(apply(path.resolve(process.argv[2])),null,2));
}
