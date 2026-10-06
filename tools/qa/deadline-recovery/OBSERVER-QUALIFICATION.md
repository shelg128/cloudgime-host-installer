# Observer qualification: authority, historical video, overhead, and physical decoder

## Decision

**Local diagnostic qualification substantially advanced; no streaming-quality acceptance or deployment.** This work tests the previously built scoped observer instead of adding another speculative recovery algorithm. It establishes a usable existing post-billing launch path, verifies historical Baseline video through encrypted SDK transport, measures producer overhead on both the operator PC and idle PC-02, and replays that same bitstream on the physical Android phone.

Remaining limits are explicit: no observation during a live billed stream in this work, no GPU-saturated overhead measurement, no new input-to-photon/A/V measurement, and two missing initial SDK texture callbacks in the first physical replay. No new rental, wallet/billing mutation, installed APK replacement, or production Host binary replacement occurred. Feature remains default-off.

## 1. Existing authority path — no new authorization protocol

Source audit:
- Backend `streamHandoff.ts` prelogin issuance intentionally has no rental/prepare ID before billing.
- Backend `routes/floor.ts` app handoff validates `BILLING_RUNNING`, active rental, PC/user/claim/native-root and Host activation before requesting a scoped native sibling launch with complete rental/prepare binding.
- Host `issue_android_native_sibling_launch_token` preserves native root, claim and authority deadline. Existing prelogin bearer is not silently upgraded.

New tests call real `App` issuance, token exchange and stream authorization methods, then the real parent context mapper. They demonstrate:
- prelogin context remains unavailable;
- a trusted backend-supplied post-billing sibling gets a complete diagnostic context without changing the root or extending authority;
- wrong user/claim/host/app and terminal root are denied.

These tests **assume** the backend-authenticated caller supplying the complete binding. They do not attest a real billing transaction or replace the tunnel credential boundary. Existing backend race tests separately execute with a mocked database and nonproduction DB URL:20passed, no actual rental.

The diagnostic runner should use this existing post-billing app handoff on the **same rental**, not invent a late rental ID in an operator file. A new child/peer may be created on that rental; that is not the same peer or a same-epoch performance crossover. Do not claim source integration tests demonstrate real billing never changes under arbitrary deployment.

## 2. Historical Baseline transport and image integrity

Input: preserved PC-02 `host-fixture.cgr` from `20261005-session-baseline-live3/session-1791214900887`, SHA256 `3047baaa17d9ba676226218812f2a688074e48ed37ccba32e38092861f9b5647`. H264 payload SHA256 `500c770243cfa9c73cc03e966b744df66815f776278d59ea3e03a80a2a78516e`.

Explicit test `historical_baseline_transport`:
- three warmup packets excluded before the scoped operator request;
- all180real captured access units packetized using the existing H264 payloader and production sequenced RTP writer;
- actual localhost ICE/DTLS/SRTP PeerConnections receive2,053original RTP packets;
- every received payload/timestamp/marker matches packetizer output;
- all180frames have complete fragment mappings,1key/179reference-header classes;
- a NACK for a recent cached packet yields one observed resend write, mapped to the original frame;
- test-only receiver binary export is depacketized with strict sequence/FU-A/STAP-A bounds and decoded using installed FFmpeg;
- reconstructed Annex-B bytes have the **same SHA256**, and all180decoded frame MD5s match the original source.

Receiver payload export exists only in the explicit test harness for the user-authorized synthetic fixture. The production metadata tap still exports no payload. FFmpeg image equality is not physical phone/display equality and does not validate internet jitter or loss handling. Original per-frame capture PTS are retained for phone replay; local transport test uses an artificial paced schedule.

## 3. Observer producer-cost screen

Predeclared in evidence `PROTOCOL.md`:9runs,3rounds each plain/attributes/recording, alternating order;4,000packets per run at2,000pps,2seconds. Same packet data and counting inner writer, producer timings include internal Attributes creation and observer submission. Worker serialization runs concurrently. This is a producer-call screen, **not total process CPU, memory bandwidth, GPU, codec, network or loaded-Host overhead**.

| Location | Median paired p95 increase over Attributes-only | Recording p99 by round | Metadata loss |
|---|---:|---|---|
| Operator PC |13.7µs|41.5 /39.6 /41.4µs|0 in all3runs|
| PC-02 idle |23.7µs|77.9 /83.7 /82.1µs|0 in all3runs|

Both pass the declared local screen:recording p99<1ms,median paired p95increase<250µs,unchanged packet/returnvalue and complete footer. In operator-PC attributes-only run3,max call was10.7208ms despite low p99. Scheduling outlier remains in raw evidence; do not claim every call is bounded by p99.

PC-02 benchmark ran as `observer-cost-test.exe` from a fresh admin/SYSTEM-only staging directory under `C:\ProgramData\Cloudgime\qa-observer-qualification`. Existing media processes must be absent, Hostready and baselinehashes match before launch. Exact testbinaryhash was checked; child timed out at35s if necessary (not triggered), exited normally, and full testlog says1passed/0failed. PowerShell Process.ExitCode returned null; completion was reconciled from process exit, generated report, testlog and independent readback. **Do not fabricate exit0 for that remote wrapper.** No service restart or productionfile replacement.

A second45second scoped worker test at400pps sent18,000packets/900frames and wrote36,001events,6,914,053bytes,zerooverflow/omission/cancellation/quota. Producer p99=50.5µs;maximum1.6776ms. Counting sink retains testpacket copies, so memory usage is not representative of production. This test measures sustained local collector reliability, not physical freeze.

## 4. Physical RMX5367 decoder replay

No APK installation and no rental. Existing probeJAR+same pinned SDK library (debugstripped, hashverified) and sameBaselineCGR copied to unique `/data/local/tmp` directory. SDK`AndroidVideoDecoder`,codec`c2.unisoc.avc.decoder`,shellUID/EGLoffscreen,defaultparameters,4runs of180frames (0/300msinputgap ×2repetitions). Lobby foreground retained keep-awake. Assets retired by rename after probe exited.

| Run | Input | MediaCodec output | SDK callback | Callback median |
|---|---:|---:|---:|---:|
| First, no gap |180|180|178|12.909ms|
| First,300ms gap |180|180|180|12.963ms|
| Second,no gap |180|180|180|12.960ms|
| Second,300ms gap |180|180|180|12.931ms|

Totals720input/720codecoutput/**718SDKcallbacks**. The two missing callbacks are inputindices1and2 in the first run. Codec outputs exist; first callbackresidence84.77ms, with early codec output clustering. Pinned SDKbytecode contains a branch releasing an outputwithoutdisplay if `renderedTextureMetadata` is still occupied. This is a plausible explanation, **not a proven causal diagnosis of these two events**, since that branch was not instrumented in this run.

At300ms pause boundary, precedingframe callbacks completed about320–321ms before next input, consistent with the previously proven absence of next-inputwait for valid Baseline. Still no fullcallbackPASS and no visualpresentation/AValignmentclaim. First-frame anomalies are not removed from results or “fixed” by enlarging buffers.

## 5. Capture planning and operational limits

`capture-plan.mjs` is a pure offline planner requiring matched expected/peer/ready context, fresh authenticated-backend snapshot with billingrunning, exact PC02rental/claim/nativeRoot/billingcommit, reviewed Directsyntheticfixture720×1570@30,target5000kbps, and fresh SDKprogress/correctprocess/SSRC/epoch. It validates authoritydeadline and forecasts packet+framemetadata+RTCP events with headroom. It emits a proposed request or reasons; never contacts endpoints, charges, or grants authority.

Example: a45secondtrace at400pps can fit the raw36,001eventcount but the planner's conservative forecast includes50%headroom and RTCP allowance and may require a shorter window. Do not silently increase50,000eventlimit. Actualfooter still determines completeness. Ready-filecontent alone is not fresh progress evidence; a future operatorcollector must obtain that securely. Caller-supplied booleans/provenance labels are **not cryptographic proof**.

Default-off release pair compiled, **not installed**:
- streamer scoped+sessionBaseline SHA256 `1795c7a6ac6b52c808c65fd8cbeb72d4294bc09fdec618f15b44c27a388e7cf0`.
- web-server scoped SHA256 `ecd1617f72c922d53bb3f5a04b0bd9b99bd47733a0824f58292ad89d2beb5363`.

These are sourceworktree candidates; media features inherited from earlier branches are not automatically acceptedproduction. No clean-byteidenticalfinalcommitclaim; subsequent changes were test-only/planner/docs. Installer parity, realcaptureunderloadedPC02androllbackpaircompatibilityremainliveprerequisites.

## Preserved failures and cleanup incident

- New test path variable shadowing failedcompile, fixed testonly.
- Initial historical NACKcase asked for an early packet after>2,000writes. A subsequent test still closed peer before asynchronousresendbecauseRRreadinesswasalreadytrue. Corrected to recentpacket and explicit observedresendcompletion. Productioncache/NACKtimerunchanged. Earlierfailurelogsretained;cacheexpiryexplanationalonewasnotproven.
- PhysicalmissingSDKcallbacksretained as above.
- A read-only remote audit used line-by-line PowerShell enumeration of largeJSONL files and became CPU-heavy. Its SSHreceiptremainedstarted. ExactPID/starttime/parent/decodeddispatcherpath+hashwereverified,then **onlythatdiagnosticprocess** was terminated. Testbenchmarkhadalreadyfinished;Hostbinary/serviceuntouched. Receipt remainsinterrupted,not forgedcomplete. Fastreadbackverified benchmarkPASS,no benchmarkprocess,Hostreadyandbaselinehashes.
- Outer120s commandtimeouts occurred duringcompletedlongbuilds. Per-jobchildjournals/finishedmarkers/hashreadbacksprovebuildcompletion; wrapperexitnottrustedalone.

## Test inventory and interpretation

At checkpoint:scopedstreamer81normaltestsplus3explicitignoredqualificationtests (baseline,cost,sustained) executed separately;parentcontextfilter5tests;19nativeticketsecurity;17nativeprocessscope;46normalstreamer;20backendhandoffmocktests;103NodeQA tests (updated count in final manifest). Counts overlap across suites/previousmilestones and are **not additive physicalqualityevidence**.

No claim of <1%, no tested newrecoverypolicy, no productionpromotion. The next permissible live action is a bounded diagnostic pilot using officialpostbillinghandoff on the same rental after exactpairedartifact/ACL/readiness/rollbackpreflight. If those gates remain unimplemented, report them as blockers rather than improvisingcredentials or opening repeatedrentals. Receiverdeadline/clockmappingandliveobserverconfoundstill require measurement.

Evidence: workspace `06 TEST/20261006-observer-qualification`, including protocol,Appauthorityfixture,backendtests,encryptedBaselineRTP/MD5,PC/localcostreports,sustainedtrace,phoneoriginalresultsandmissingcallbackdiagnosis,buildhashes,diagnosticcleanupreceipts,andfinalaudit.
