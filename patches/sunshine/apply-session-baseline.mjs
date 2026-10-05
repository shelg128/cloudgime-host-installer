import fs from'node:fs';import path from'node:path';import crypto from'node:crypto';import{execFileSync}from'node:child_process';import{fileURLToPath}from'node:url';
export const policyHeader=`#pragma once
#include <cstdint>
#include <string_view>
namespace cloudgime::baseline {
inline int parse_level(std::string_view value) {
  if(value=="0")return 0;
  if(value=="31")return 31;
  if(value=="32")return 32;
  if(value=="40")return 40;
  if(value=="41")return 41;
  if(value=="42")return 42;
  return -1;
}
inline bool accepts(int level,int width,int height,int fps,int bitrate,int format,int range,int chroma) {
  if(level==0)return true;
  if(format!=0||range!=0||chroma!=0||width<=0||height<=0||fps<=0||bitrate<=0)return false;
  std::int64_t max_frame=0,max_second=0,max_bitrate=0;
  switch(level){case 31:max_frame=3600;max_second=108000;max_bitrate=14000;break;
  case 32:max_frame=5120;max_second=216000;max_bitrate=20000;break;
  case 40:max_frame=8192;max_second=245760;max_bitrate=20000;break;
  case 41:max_frame=8192;max_second=245760;max_bitrate=50000;break;
  case 42:max_frame=8704;max_second=522240;max_bitrate=50000;break;default:return false;}
  const auto w=(static_cast<std::int64_t>(width)+15)/16,h=(static_cast<std::int64_t>(height)+15)/16;
  if(w>max_frame||h>max_frame)return false;
  return w*h<=max_frame&&w*h*fps<=max_second&&w*w<=8*max_frame&&h*h<=8*max_frame&&bitrate<=max_bitrate;
}
}
`;
export function transform(files){const out={...files};function edit(n,a,b){if(out[n].split(a).length!==2)throw Error('CONTRACT '+n+' '+a.slice(0,40));out[n]=out[n].replace(a,b);}
edit('src/video.h','    int enableIntraRefresh;  // 0 - disabled, 1 - enabled','    int enableIntraRefresh;  // 0 - disabled, 1 - enabled\n\n    // Per RTSP session, zero preserves legacy profile. Never node-global mutation.\n    int cgH264BaselineLevel = 0;');
edit('src/rtsp.cpp','#include "video.h"','#include "video.h"\n#include "cloudgime_baseline_policy.h"');
edit('src/rtsp.cpp','    // Tell the client about our supported features','    // Versioned profile request extension; unsupported encoder paths fail closed.\n    ss << "a=x-cg-video.h264BaselineSession:1" << std::endl;\n\n    // Tell the client about our supported features');
edit('src/rtsp.cpp','    args.try_emplace("x-ss-video[0].intraRefresh"sv, "0"sv);','    args.try_emplace("x-ss-video[0].intraRefresh"sv, "0"sv);\n    args.try_emplace("x-cg-video.h264BaselineLevel"sv, "0"sv);');
edit('src/rtsp.cpp','      config.monitor.enableIntraRefresh = util::from_view(args.at("x-ss-video[0].intraRefresh"sv));','      config.monitor.enableIntraRefresh = util::from_view(args.at("x-ss-video[0].intraRefresh"sv));\n      config.monitor.cgH264BaselineLevel = cloudgime::baseline::parse_level(args.at("x-cg-video.h264BaselineLevel"sv));\n      if (!cloudgime::baseline::accepts(config.monitor.cgH264BaselineLevel, config.monitor.width, config.monitor.height, config.monitor.framerate, config.monitor.bitrate, config.monitor.videoFormat, config.monitor.dynamicRange, config.monitor.chromaSamplingType)) {\n        respond(sock, session, &option, 400, "H264 BASELINE LEVEL MISMATCH", req->sequenceNumber, {});\n        return;\n      }');
edit('src/video.cpp','    if (dynamic_cast<platf::avcodec_encode_device_t *>(encode_device.get())) {','    if (config.cgH264BaselineLevel != 0 && !dynamic_cast<platf::nvenc_encode_device_t *>(encode_device.get())) {\n      BOOST_LOG(error) << "Session Baseline unsupported on selected encoder; no automatic codec fallback";\n      return nullptr;\n    }\n    if (dynamic_cast<platf::avcodec_encode_device_t *>(encode_device.get())) {');
edit('src/nvenc/nvenc_base.cpp','          if (config.h264_baseline && (buffer_is_yuv444() || buffer_is_10bit())) {','          const bool baseline_profile = config.h264_baseline || client_config.cgH264BaselineLevel != 0;\n          if (baseline_profile && (buffer_is_yuv444() || buffer_is_10bit())) {');
edit('src/nvenc/nvenc_base.cpp','enc_config.profileGUID = config.h264_baseline ?','enc_config.profileGUID = baseline_profile ?');
edit('src/nvenc/nvenc_base.cpp','if (config.h264_baseline || config.h264_cavlc ||','if (baseline_profile || config.h264_cavlc ||');
edit('src/nvenc/nvenc_base.cpp','          if (config.h264_baseline) {','          if (baseline_profile) {\n            if (client_config.cgH264BaselineLevel != 0) {\n              format_config.level = client_config.cgH264BaselineLevel;\n              BOOST_LOG(info) << "NvEnc: session Baseline level=" << client_config.cgH264BaselineLevel;\n            }');
out['src/cloudgime_baseline_policy.h']=policyHeader;return out;}
export function apply(root){
 const revision='86188d47a7463b0f73b35de18a628353adeaa20e';
 if(execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('REVISION_MISMATCH');
 const names=['src/video.h','src/rtsp.cpp','src/video.cpp','src/nvenc/nvenc_base.cpp'];
 const read=n=>fs.readFileSync(path.join(root,n),'utf8').replaceAll('\r\n','\n');
 const sha=s=>crypto.createHash('sha256').update(s).digest('hex');
 const files=Object.fromEntries(names.map(n=>[n,read(n)]));
 const prior=JSON.parse(fs.readFileSync(path.join(root,'cloudgime-baseline-profile.json'),'utf8'));
 if(prior.revision!==revision)throw Error('BASELINE_PATCH_REVISION');
 const manifestPath=path.join(root,'cloudgime-session-baseline.json');
 if(fs.existsSync(manifestPath)){
  const m=JSON.parse(fs.readFileSync(manifestPath));
  if(m.revision!==revision||m.outputs['src/cloudgime_baseline_policy.h']!==sha(policyHeader))throw Error('PATCH_VERSION_MISMATCH');
  for(const [n,h]of Object.entries(m.outputs))if(sha(read(n))!==h)throw Error('PATCHED_SOURCE_CHANGED '+n);
  return {...m,unchanged:true};
 }
 for(const n of names){
  const expected=n==='src/nvenc/nvenc_base.cpp'?prior.files.find(x=>x.path===n)?.after:sha(execFileSync('git',['-C',root,'show','HEAD:'+n],{encoding:'utf8',maxBuffer:2e6}).replaceAll('\r\n','\n'));
  if(!expected||sha(files[n])!==expected)throw Error('SOURCE_PROVENANCE_MISMATCH '+n);
 }
 const out=transform(files);
 const manifest={feature:'session_baseline_v1',revision,defaultLevel:0,inputs:Object.fromEntries(names.map(n=>[n,sha(files[n])])),outputs:Object.fromEntries(Object.entries(out).map(([n,s])=>[n,sha(s)])),releaseAccepted:false};
 for(const [n,s]of Object.entries(out))fs.writeFileSync(path.join(root,n),s);
 fs.writeFileSync(manifestPath,JSON.stringify(manifest,null,2));
 return manifest;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify(apply(process.argv[2])));
