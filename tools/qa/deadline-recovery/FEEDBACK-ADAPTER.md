# Real RTCP / Baseline metadata adapter — offline shadow milestone

## Decision

The `rtcp-feedback.mjs`, `h264-metadata.mjs`, and `recovery-shadow.mjs` modules now provide an **offline, non-sending adapter** driven by real RTCP structures rather than fictional per-packet positive ACKs. No production code imports them, no installed binary changed, and no rental was started. The earlier simulator and locked252-run evaluation are unchanged.

**Not a performance fix or SDK integration PASS.** Local UDP loopback and historical packet parsing pass; production Host receipt/deadline evidence remains absent. No packet is sent, skipped, dropped, or reprioritized by the shadow controller. Its only output is a diagnostic proposal or a refusal reason.

## Actual evidence

Two historical receiver RTC logs, untruncated complete protobuf prefixes:

| Dataset | RTCP compounds | RR | REMB | SR | NACK |
|---|---:|---:|---:|---:|---:|
| shared-sync correctedtrace |725|493|502|202|30|
| independent-sync experiment |713|494|490|202|17|

These logs include prebuffer/premeasurement data; counts are not attributed to only the original quality window. VideoRRmedianinterval206/208ms (about6frames at30FPS), not per-frame ACKs. Extended highest sequence is **not proof that every lower packet arrived**. Recovered packets can reduce cumulative loss; signed loss is parsed without assuming monotonically increasing loss.

REMBvideo values were nearly flat: shared5,667,872bps, independent5,430,816–5,525,568bps. The adapter exposes receiver-reported bitrate, not guaranteed available capacity. Constant values alone do not prove application clamping, estimator behavior, or congestion absence.

All1,034receiver-outgoing compound observations are rejected as Host input (523+511). This is intentional: a receiver-send timestamp cannot be substituted for Host-receipt time. Matching an LSR to an SR observed **at the receiver** does not provide a Host RTT. Historical parsing succeeded while Host RTT/deadline validity remained unavailable. No time offsets or feedback arrivals were fabricated.

### True Baseline bitstream

The preserved capture from `20261005-session-baseline-live3/session-1791214900887/host-fixture.cgr` has180access-unit records, SHA256 `3047baaa17d9ba676226218812f2a688074e48ed37ccba32e38092861f9b5647`.
Concatenatedpayload exactly equals `20261005-session-baseline/session-baseline.h264`, SHA256 `500c770243cfa9c73cc03e966b744df66815f776278d59ea3e03a80a2a78516e`.

SPSprofile66,constraints192,level42:1IDR+179referenceAUs by NALheaders;0drop-certifiedframes. All180framesdecode through installedFFmpeg with emptyerrorlog. **This is not a new physical Android decoder replay or quality acceptance**. NALreferencebits are not an exact reference graph or proof of syntax correctness; boundary verification comes from the captured AU container. Even a nonreferenceheader is never labelled safe-to-drop by this adapter.

## Module contracts

### `parseRtcp`

Input must be bounded, already decrypted and authenticated RTCP at the integration boundary—not SRTCPciphertext. Pure parser validates compound sizes/version/padding, SR/RRreportblockbounds, NACKFCI, REMBtargetcount/exponent/mantissa, and PLIlength. Unknown packettypes remain descriptive. Parsing the full compound completes before adapterstate changes. Maximum input64KiB. HMACenvelope from loopback is **not** a proposed production protocol.

### `SenderFeedbackAdapter`

Pinned `{scope,videoSsrc,receiverSsrc}` identifies one stream epoch. Accept only authenticated `host-received` records with sender-monotonic receipt time. Store bounded128localSRsendrecords and compute:

`rtt_ms = host_rr_receipt_mono_ms - host_sr_sent_mono_ms - dlsr * 1000 / 65536`

Require matchingLSR,correctSSRC,freshsameepochSR,nonnegativeboundedRTT; noRTT/2onewayclaim. Missing/staleSR gives noRTT. Receiptclockrewind and senderSRclockrewind reject. RRextendedsequencerewind invalidatesallstate and requires construction of a new scoped adapter. A repeatedhighestsequence report does not refresh the RTT/loss snapshot. NegativeNACKlists never become positiveACKs.

### `RecoveryShadow`

Keeps<=4096packetmetadataentries by default, <=1000ms age; collisionacross16bitwrap cannot map a stale NACK. RRhighestsequence beyond the known lastsentextendedsequence is rejected, provided alloriginals are captured by the integration contract. Sourceidle discards optionalcredit. Revocation cannot be reversed by reusing the object.

Records `KEEP_EXISTING_NACK` for mappedrequests and `UNMAPPED_KEEP_EXISTING_NACK` otherwise; never stops the real responder. It does **not** feed every RR into the old simulator's per-ACK EWMA. A potential copy proposal requires:
- explicit validated Host-monotonic deadline mapping with<=5msdeclareduncertainty;
- freshHostRR/REMB,priorobservedRTTfloor and actualsenderwirebudget;
- reportedloss,referencepriority,enoughdeadline/headroom and optionalbytecredit;
- uses wholeobservedRTT+decodeallowance as a conservative heuristic, not measuredonewaybound;
- no priorproposal forthatpacket, bounded25%credit/1328carry/2Mbps tokenrate.

These are input assertions in a research module, not cryptographic proof or calibrated QoS. External observations must themselves be validated; attacker-controlled client reports or caller-supplied booleans do not form a secure production policy. REMBis an estimate/limit, not a capacity oracle. No deadline mapping exists in historical evidence, so actual-evidence proposals remain off.

## Loopback integration validation

The script binds two ephemeralUDPports only on127.0.0.1, sends3syntheticSRs and returnscompoundRR/NACK/REMB. Records real senderperformance.now timestamps, receiverreportsDLSR; subtractsDLSRwithoutreceiverwallclock. TestenvelopeHMACkeyexistsinmemoryonly and iszeroed; one intentionallytamperedmessageisrejected. All3validresponses parsed, NACK65535mappedacrosssurroundingsequencewrap, positiveACKcount0. Missingdeadline correctly yields`NONE/NO_VALIDATED_DEADLINE`. Bothsocketsclosed. Cachedmediaentries are metadatafixtures; no encodedmedia sent. This does **not** exercise productionWebRTCinterceptors/SRTCP or proveinternetRTTquality.

## Production integration prerequisites — not executed

1. Hook decrypted/authenticatedRTCP at `TrackLocalSender::create_track` after SDKinterceptor processing, preserving the existing responder and returning promptly. Never block its readloop with synchronousdisk/networkwork.
2. Record actual outgoingSRcompactNTP+Hostmonotonicsendtime. Current reviewedapplicationcallbackdoesnotexposeallSRsendtiming; use a verifiedSDKinterceptoradapter, not receiverlogs or PCwallclockguessing.
3. Bind actual outgoingsequence/SSRC/epoch at `SequencedTrackLocalStaticRTP` write boundary. Earlierpacketization usessequence0 and SSRC0 placeholders; logging those is invalid. Captureboundedmetadata only, not payloadsecrets.
4. InspectAUmetadata before packetization (`send_decode_unit`) and propagatevalidatedAUidentitythroughallfragments. `important=IDR` alone does not mean otherframesarenonreference.
5. Validate receiverdeadline/uncertainty or keepdeadline-drivenactions off. SR-basedA/Vtime mapping alone is not a displaydeadline.
6. End-to-endbitrateaudit: `video.rs` reviewedcallbackignoresREMB, **but** client`NativeDirectWebRtcRuntime.kt:updateStreamSettings` sends bitrate/adaptiveflags through a separatechannel. Do not claim allprojectadaptationisabsent. Establish howupdatedbudgetreachesactualencoderandoriginal+repairtraffic before changingit.
7. Any liveinstrumentation must have defaultoffbuildflag, boundednonblockingqueue/dropcounters, expiry, scopecleanup and tests. No newRTP/RTCPtraffic,relaychange,frame-droporcopybehavior until observationcontract is proven.

## Reproduction and validation

Workspaceevidence: `06 TEST/20261006-recovery-feedback-adapter`. `scripts/verify.mjs` runsloopback,historicalreplay,thenallprototype+adaptertests with both evidenceenvironmentvariables enabled.64tests at thischeckpoint,0fail/skip. Old26tests remain, includingfrozencontrollerhashes and negativeevaluationoutcomes.

Tests use fabricatedRTCPunitfixtures only for protocol/safety; physicalhistoricalcounts and Baselinehashes are separately verified. No claim tests=streamingimprovement. Negativefindings—noHostreceiptclock, no positiveACKs, allreferenceBaseline—are retained and must not be patched over to force a live experiment.
