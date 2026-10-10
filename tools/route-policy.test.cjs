const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const ts = require('typescript');
const source = fs.readFileSync(path.join(__dirname, '../web/stream/route_policy.ts'), 'utf8');
// Compile the actual production module, not a duplicated policy implementation.
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
}).outputText;
const policy = import('data:text/javascript;base64,' + Buffer.from(compiled).toString('base64'));
const servers = [{ urls: ['stun:example:3478', 'turn:example:3478', 'turns:example:5349'], username: 'fixture', credential: 'test-only' }];
const direct = 'candidate:1 1 udp 1 203.0.113.10 5000 typ srflx';
const relay = 'candidate:2 1 udp 1 203.0.113.20 6000 typ relay';

test('only explicit routes parse; auto and missing route are rejected', async () => {
  const p = await policy;
  for (const value of [null, undefined, '', 'auto', 'stable', 'unknown']) assert.equal(p.parseExplicitStreamRoute(value), null);
  assert.equal(p.parseExplicitStreamRoute('DIRECT'), 'direct');
  assert.equal(p.readExplicitStreamRoute('?autoRoute=relay'), 'relay');
});
test('Direct configuration excludes all TURN URLs, including mixed lists', async () => {
  const result = (await policy).routeIceConfiguration(servers, 'direct');
  assert.equal(result.iceTransportPolicy, 'all');
  assert.deepEqual(result.iceServers[0].urls, ['stun:example:3478']);
});
test('Relay configuration is TURN-only and retains TURN credentials', async () => {
  const result = (await policy).routeIceConfiguration(servers, 'relay');
  assert.equal(result.iceTransportPolicy, 'relay');
  assert.deepEqual(result.iceServers[0].urls, ['turn:example:3478', 'turns:example:5349']);
  assert.equal(result.iceServers[0].credential, 'test-only');
});
test('Direct rejects relay trickle candidates and malformed candidate types', async () => {
  const p = await policy;
  assert.equal(p.candidateAllowed('direct', direct), true);
  assert.equal(p.candidateAllowed('direct', relay), false);
  assert.equal(p.candidateAllowed('direct', 'bad'), false);
  assert.equal(p.candidateAllowed(null, direct), false);
});
test('Direct strips relay candidates from SDP while preserving other SDP fields', async () => {
  const p = await policy;
  assert.equal(p.filterRouteSdp('direct', `v=0\r\na=${direct}\r\na=${relay}\r\n`), `v=0\r\na=${direct}\r\n`);
});
test('Relay permits remote direct peer while local ICE policy forces TURN', async () => {
  const p = await policy;
  assert.equal(p.candidateAllowed('relay', direct), true);
  assert.equal(p.routeIceConfiguration(servers, 'relay').iceTransportPolicy, 'relay');
});
test('Direct Relay Direct Relay alternation does not mutate the server configuration', async () => {
  const p = await policy;
  const before = JSON.stringify(servers);
  for (const route of ['direct', 'relay', 'direct', 'relay']) {
    const config = p.routeIceConfiguration(servers, route);
    assert.equal(config.iceTransportPolicy, route === 'relay' ? 'relay' : 'all');
    assert.equal(JSON.stringify(servers), before);
  }
});
