#!/usr/bin/env node
/**
 * apply-cg-build-fixes.mjs — applies the reproducible build fixes needed to
 * compile the capture-recovery Sunshine on MSYS2 UCRT64 GCC 16 + LizardByte
 * build-deps FFmpeg bundle. Idempotent: safe to run on an already-patched tree.
 *
 * Fixes applied:
 *   1. libdisplaydevice json.h/json.cpp — PlainStringSet converter overload
 *      (GCC 16 treats std::set<std::string, std::less<>> as a distinct type).
 *   2. src/rswrapper.h — publish DATA_SHARDS_MAX and the full _reed_solomon
 *      layout (upstream nanors/rs.h used to leak these in).
 *   3. FFmpeg bundle NVENC headers — pin NVENC API 12.0 from the repo's
 *      nv-codec-headers submodule instead of the bundle's 13.1.
 *   4. cmake/dependencies/common.cmake — list the static codec backends
 *      (dav1d, opus, swresample, vpl, strmiids) required by libavcodec.a.
 */
import { readFileSync, writeFileSync, existsSync, mkdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..', '..'); // patches/sunshine -> repo root

const srcRoot = process.argv[2];
if (!srcRoot) {
  console.error('usage: node apply-cg-build-fixes.mjs <sunshine-source-root>');
  process.exit(2);
}
const src = join(srcRoot);
if (!existsSync(join(src, 'CMakeLists.txt'))) {
  console.error(`not a Sunshine source root: ${src}`);
  process.exit(3);
}

const log = (m) => console.log(m);
const read = (p) => readFileSync(p, 'utf8');
const write = (p, s) => writeFileSync(p, s, 'utf8');

let changed = 0;

// ---- 1. PlainStringSet converter -------------------------------------------
{
  const h = join(src, 'third-party/libdisplaydevice/src/common/include/display_device/json.h');
  const c = join(src, 'third-party/libdisplaydevice/src/common/json.cpp');
  if (existsSync(h) && existsSync(c)) {
    let ht = read(h);
    let ct = read(c);
    let dirty = false;

    // Remove any earlier misplaced declaration.
    const misplaced = ht.match(/\/\/ Toolchains that treat[\s\S]*?DD_JSON_DECLARE_CONVERTER\(PlainStringSet\)\s*\n?/);
    if (misplaced) { ht = ht.replace(misplaced[0], ''); dirty = true; }

    if (!ht.includes('using PlainStringSet = std::set<std::string>;')) {
      const anchor = 'namespace display_device {';
      if (ht.includes(anchor)) {
        ht = ht.replace(anchor, anchor + `\n\n  // Toolchains that treat std::set<std::string> as distinct from StringSet\n  // (std::less<> comparator) need their own converter.\n  using PlainStringSet = std::set<std::string>;\n  DD_JSON_DECLARE_CONVERTER(PlainStringSet)`);
        dirty = true;
      }
    }

    if (!ct.includes('PlainStringSet')) {
      const anchor = 'DD_JSON_DEFINE_CONVERTER(StringSet)';
      if (ct.includes(anchor)) {
        const impl = `DD_JSON_DEFINE_CONVERTER(StringSet)

// Default-comparator variant: build the canonical StringSet and reuse its converter.
std::string toJson(const PlainStringSet &obj, const std::optional<unsigned int> &indent, bool *success) {
  const StringSet canonical {obj.begin(), obj.end()};
  return toJson(canonical, indent, success);
}

bool fromJson(const std::string &string, PlainStringSet &obj, std::string *error_message) {
  StringSet canonical;
  const bool ok {fromJson(string, canonical, error_message)};
  if (ok) {
    obj = PlainStringSet {canonical.begin(), canonical.end()};
  }
  return ok;
}
`;
        ct = ct.replace(anchor, impl);
        dirty = true;
      }
    }

    if (dirty) { write(h, ht); write(c, ct); changed++; log('json.h/json.cpp: PlainStringSet converter OK'); }
    else log('json.h/json.cpp: already patched');
  } else {
    log('WARN: libdisplaydevice json files missing, skipping fix 1');
  }
}

// ---- 2. rswrapper.h ---------------------------------------------------------
{
  const f = join(src, 'src/rswrapper.h');
  if (existsSync(f)) {
    let t = read(f);
    let dirty = false;
    if (!t.includes('DATA_SHARDS_MAX')) {
      const anchor = 'typedef struct _reed_solomon reed_solomon;';
      if (t.includes(anchor)) {
        t = t.replace(anchor, `// Shard layout, mirrors nanors rs.h. stream.cpp writes rs->p directly to apply
// the NVIDIA-compatible parity matrix, so the full definition is required.
typedef struct _reed_solomon {
    int ds;
    int ps;
    int ts;
    uint8_t p[];
} reed_solomon;

// Reed-Solomon shard ceiling, mirrors nanors rs.h DATA_SHARDS_MAX.
// The wrapper replaced rs.h, so this constant must be published here.
#define DATA_SHARDS_MAX 255`);
        dirty = true;
      }
    }
    if (dirty) { write(f, t); changed++; log('rswrapper.h: DATA_SHARDS_MAX + struct OK'); }
    else log('rswrapper.h: already patched');
  } else log('WARN: rswrapper.h missing, skipping fix 2');
}

// ---- 3. NVENC header pin (API 12.0 from repo submodule) ---------------------
{
  const sub = join(src, 'third-party/nv-codec-headers/include/ffnvcodec');
  const bundle = process.env.CG_FFMPEG_INCLUDE; // optional override
  const bundleDir = bundle ? join(bundle, 'ffnvcodec') : null;
  if (existsSync(join(sub, 'nvEncodeAPI.h'))) {
    // The FFmpeg bundle ships NVENC API 13.1 which removes fields Sunshine 86188d4
    // still uses. Overwrite the bundle copies with the repo's pinned 12.0 headers.
    const names = ['nvEncodeAPI.h', 'dynlink_cuda.h', 'dynlink_cuviddec.h', 'dynlink_loader.h', 'dynlink_nvcuvid.h'];
    let applied = 0;
    for (const n of names) {
      const s = join(sub, n);
      if (!existsSync(s)) continue;
      const target = bundleDir ? join(bundleDir, n) : null;
      if (target && existsSync(target)) {
        copyFileSync(s, target);
        applied++;
      }
    }
    if (applied) { changed++; log(`nvenc headers pinned to API 12.0 (${applied} files)`); }
    else log('nvenc headers: no bundle override target set (set CG_FFMPEG_INCLUDE)');
  } else log('WARN: nv-codec-headers submodule missing, skipping fix 3');
}

// ---- 4. FFmpeg static backends in common.cmake ------------------------------
{
  const f = join(src, 'cmake/dependencies/common.cmake');
  if (existsSync(f)) {
    let t = read(f);
    if (!t.includes('libdav1d.a')) {
      const anchor = '        "${FFMPEG_PREPARED_BINARIES}/lib/libx265.a"';
      if (t.includes(anchor)) {
        const add = `${anchor}\n        "${FFMPEG_PREPARED_BINARIES}/lib/libdav1d.a"\n        "${FFMPEG_PREPARED_BINARIES}/lib/libopus.a"\n        "${FFMPEG_PREPARED_BINARIES}/lib/libswresample.a"\n        "${FFMPEG_PREPARED_BINARIES}/lib/libvpl.a"`;
        t = t.replace(anchor, add);
        // Append strmiids to the closing of the FFMPEG_LIBRARIES list.
        t = t.replace(/\$\{FFMPEG_PLATFORM_LIBRARIES\}\)/, '${FFMPEG_PLATFORM_LIBRARIES}\n        strmiids)');
        write(f, t);
        changed++;
        log('common.cmake: static backends (dav1d/opus/swresample/vpl/strmiids) OK');
      } else log('WARN: common.cmake anchor not found, skipping fix 4');
    } else log('common.cmake: already patched');
  } else log('WARN: common.cmake missing, skipping fix 4');
}

log(`done: ${changed} fix group(s) applied to ${src}`);
// Idempotent by design: a tree that already carries every fix is a valid result.
process.exit(0);