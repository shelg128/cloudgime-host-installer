import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
const root = process.env.CG_BASELINE_EVIDENCE;
if (!root) throw Error('CG_BASELINE_EVIDENCE_REQUIRED');
const load = name => JSON.parse(fs.readFileSync(root + '/' + name));
const sha = data => crypto.createHash('sha256').update(data).digest('hex');

test('profile-only discriminator changes one byte and no payload or timing bytes', () => {
  const baseline = fs.readFileSync(root + '/base-control.cgr');
  const main = fs.readFileSync(root + '/base-main-constrained.cgr');
  assert.equal(baseline.length, main.length);
  const differences = [];
  for (let i = 0; i < baseline.length; i++) if (baseline[i] !== main[i]) differences.push(i);
  assert.deepEqual(differences, [731]);
  assert.equal(baseline[731], 66);
  assert.equal(main[731], 77);
  assert.equal(sha(baseline), '41220f0f6230513cde274e7a32b5420f614a0c466c1ede90a149fe6300c97c14');
  assert.equal(sha(main), '8c3e3d01a193ba036e3d295f11f0407e343a146da83d89f04a6fb34316d2c894');
});

test('real NVENC capture is Baseline and complete, not a relabeled High stream', () => {
  const metadata = load('nvenc-bitstream-verified.json');
  assert.equal(metadata.profileIdc, 66);
  assert.equal(metadata.entropyCodingMode, 0);
  assert.equal(metadata.frames, 180);
  assert.equal(metadata.headerExit, 0);
  assert.equal(metadata.sha256, sha(fs.readFileSync(root + '/nvenc-baseline.cgr')));
});

test('replay preserves codec output completeness and injected gap evidence', () => {
  const candidate = load('nvenc-baseline.json');
  const control = load('control-after.json');
  for (const run of [...candidate.runs, ...control.runs]) {
    assert.equal(run.inputs.length, 180);
    assert.ok(run.inputs.every(i => i.status === 'OK'));
    assert.equal(new Set(run.inputs.map(i => i.ptsUs)).size, 180);
    assert.equal(new Set(run.codecOutputs.map(i => i.ptsUs)).size, run.codecOutputs.length);
    assert.ok(!run.error);
    if (run.gapMs === 300) {
      const a = run.inputs.find(i => i.index === 29);
      const b = run.inputs.find(i => i.index === 30);
      assert.ok((b.atNs - a.atNs) / 1e6 >= 300);
      const output = run.codecOutputs.find(o => o.ptsUs === a.ptsUs);
      assert.ok(output);
      assert.equal(output.atNs < b.atNs, candidate.runs.includes(run));
    }
  }
  assert.ok(candidate.runs.every(r => r.codecOutputs.length === 180));
  assert.equal(candidate.runs.reduce((s,r) => s + r.outputs.length, 0), 719,
    'Do not hide the one callback missing in the initial Baseline replay');
  assert.ok(control.runs.every(r => r.codecOutputs.length === 179));
});

test('live quality remains rejected despite fast decoder', () => {
  const evidence = load('final-analysis.json');
  assert.equal(evidence.live.valid, true);
  assert.equal(evidence.live.qualityAccepted, false);
  assert.ok(evidence.live.freezePercent > 1);
  assert.equal(evidence.live.raw.decode.median, 10);
  assert.equal(evidence.live.raw.jitterBuffer.median, 88);
});
