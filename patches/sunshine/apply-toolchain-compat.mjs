import fs from 'node:fs';import path from 'node:path';import crypto from 'node:crypto';import {execFileSync} from 'node:child_process';import {fileURLToPath} from 'node:url';import {revision} from './apply-capture-recovery.mjs';
const here=path.dirname(fileURLToPath(import.meta.url));const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
export function compat(source){
 if(execFileSync('git',['-C',source,'rev-parse','HEAD'],{encoding:'utf8'}).trim()!==revision)throw Error('WRONG_SOURCE_REVISION');
 const manifestFile=path.join(source,'cloudgime-toolchain-compat.json');
 const newFile='cmake/compile_definitions/cloudgime_windows_compat.cmake';const cmake=fs.readFileSync(path.join(here,'windows_toolchain_compat.cmake'),'utf8');
 if(fs.existsSync(manifestFile)){const m=JSON.parse(fs.readFileSync(manifestFile));for(const[n,h]of Object.entries(m.outputs)){if(sha(fs.readFileSync(path.join(source,n)))!==h)throw Error('COMPAT_SOURCE_CHANGED '+n);}if(m.outputs[newFile]!==sha(Buffer.from(cmake)))throw Error('COMPAT_VERSION_CHANGED');return m;}
 const output={};
 for(const name of ['src/platform/windows/input.cpp','cmake/compile_definitions/windows.cmake']){
  const canonical=execFileSync('git',['-C',source,'show',revision+':'+name],{encoding:'utf8'}).replaceAll('\r\n','\n');
  const current=fs.readFileSync(path.join(source,name),'utf8').replaceAll('\r\n','\n');if(current!==canonical)throw Error('UNEXPECTED_COMPAT_SOURCE '+name);
  if(name.endsWith('input.cpp')){const old='#ifdef __MINGW32__\nDECLARE_HANDLE(HSYNTHETICPOINTERDEVICE);';if(current.split(old).length!==2)throw Error('COMPAT_HUNK');output[name]=current.replace(old,'#if defined(__MINGW32__) && !defined(CG_WINDOWS_HAS_SYNTHETIC_POINTER_API)\nDECLARE_HANDLE(HSYNTHETICPOINTERDEVICE);');}
  else output[name]=current+'\ninclude("${CMAKE_CURRENT_LIST_DIR}/cloudgime_windows_compat.cmake")\n';
 }
 output[newFile]=cmake;const hashes={};for(const[n,v]of Object.entries(output)){fs.writeFileSync(path.join(source,n),v);hashes[n]=sha(Buffer.from(v));}const m={revision,patch:'windows-header-feature-detection-v1',outputs:hashes};fs.writeFileSync(manifestFile,JSON.stringify(m,null,2));return m;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){if(process.argv.length!==3)throw Error('Usage: apply-toolchain-compat.mjs <checkout>');console.log(JSON.stringify(compat(path.resolve(process.argv[2])),null,2));}
