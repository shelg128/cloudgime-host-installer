'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');

const root = path.resolve(__dirname, '../..');
const bridgeDir = path.join(root, 'vendor/moonlight-common-rust/src/stream/c');
const bridge = fs.readFileSync(path.join(bridgeDir, 'video.rs'), 'utf8');
const cRoot = path.join(root, 'vendor/moonlight-common-rust/moonlight-common-sys/moonlight-common-c/src');
const header = fs.readFileSync(path.join(cRoot, 'Limelight.h'), 'utf8');
const producer = fs.readFileSync(path.join(cRoot, 'RtpVideoQueue.c'), 'utf8');
const packetizer = fs.readFileSync(path.join(root, 'streamer/src/transport/webrtc/video.rs'), 'utf8');

// These tests pin the units on both sides of the actual, shared push/pull FFI
// bridge. They do not replace a full Host crate build or a live media test.
test('upstream C producer and header specify presentationTimeUs as microseconds', () => {
  assert.match(header, /Presentation time in microseconds[\s\S]*?uint64_t presentationTimeUs/);
  assert.match(header, /Original RTP timestamp in 90kHz units[\s\S]*?uint32_t rtpTimestamp/);
  assert.match(producer, /presentationTimeUs = \(\(uint64_t\)packet->timestamp \* 1000\) \/ PTS_DIVISOR/);
});

test('push and pull decode paths use the corrected common conversion exactly once', () => {
  assert.match(bridge, /#\[path = "presentation_time.rs"\]\s+mod presentation_time;/);
  assert.match(bridge, /timestamp:\s*presentation_time::from_microseconds\(raw.presentationTimeUs\)/);
  assert.doesNotMatch(bridge, /presentationTimeUs[^\r\n]*(?:90_000|90000|1_000_000_000)/);
  assert.match(bridge, /fn submit_decode_unit[\s\S]*?convert_decode_unit\(decode_unit, &mut buffers\)/);
  assert.match(bridge, /fn handle_next_video_frame[\s\S]*?convert_decode_unit\(decode_unit, &mut self.buffers\)/);
  assert.match(packetizer, /unit.timestamp.as_nanos\(\) \* 90000 \/ 1_000_000_000/);
});

test('real production timing helper passes standalone Rust regression tests', () => {
  const toolchain = fs.readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8')
    .match(/channel\s*=\s*"([^"]+)"/)[1];
  const lookup = cp.spawnSync('rustup', ['which', '--toolchain', toolchain, 'rustc'],
    {cwd: root, encoding: 'utf8', timeout: 15000, windowsHide: true});
  assert.equal(lookup.status, 0, lookup.stderr || lookup.error?.message);
  const rustc = lookup.stdout.trim();
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'cloudgime-presentation-time-test-'));
  // The helper has pub(super) visibility. A wrapper models its actual module
  // placement; the source under test is the production file, not a copy.
  const wrapper = path.join(temp, 'test.rs');
  const source = path.join(bridgeDir, 'presentation_time.rs').replaceAll('\\', '/');
  fs.writeFileSync(wrapper, `#[path = ${JSON.stringify(source)}]\nmod presentation_time;\n`);
  const exe = path.join(temp, process.platform === 'win32' ? 'test.exe' : 'test');
  const compile = cp.spawnSync(rustc, ['--edition=2024', '--test', wrapper, '-o', exe],
    {cwd: root, encoding: 'utf8', timeout: 60000, windowsHide: true});
  assert.equal(compile.status, 0, compile.stderr || compile.error?.message);
  const run = cp.spawnSync(exe, ['--nocapture'],
    {cwd: root, encoding: 'utf8', timeout: 15000, windowsHide: true});
  console.log(run.stdout.trim());
  assert.equal(run.status, 0, run.stderr || run.error?.message);
  assert.match(run.stdout, /10 passed; 0 failed/);
  console.log('Test artifact directory: ' + temp);
});
