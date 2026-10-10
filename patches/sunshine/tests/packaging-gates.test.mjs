import fs from 'node:fs';import path from 'node:path';import {test} from 'node:test';import assert from 'node:assert/strict';import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
test('portable bundle checks capture before copies and checks copied result',()=>{
 const s=fs.readFileSync(path.join(root,'tools/portable/build_portable_host_bundle.ps1'),'utf8');
 assert.ok(s.indexOf('& node $captureRuntimeVerifier $SunshineSourceRoot')<s.indexOf('function Copy-SunshineRuntime'));
 assert.match(s,/& node \$captureRuntimeVerifier \$LegacySunshineSourceRoot/);
 assert.match(s,/& node \$captureRuntimeVerifier \$DestinationRoot/);
 assert.match(s,/if \(\$LASTEXITCODE -ne 0\) \{ throw "Copied capture/);
});
test('offline installer cannot bypass capture gate through SkipBundleBuild',()=>{
 const s=fs.readFileSync(path.join(root,'tools/portable/build-host-offline-installer.ps1'),'utf8');
 const gate=s.indexOf('Invoke-Checked "node" @($captureVerifier, (Join-Path $BundleRoot "sunshine"))');
 assert.ok(gate>s.indexOf('if (-not $SkipBundleBuild)'));
 assert.ok(gate<s.indexOf('Invoke-Checked $preparePayloadCmd'));
 assert.match(s,/Join-Path \$BundleRoot "sunshine-legacy"/);
});
