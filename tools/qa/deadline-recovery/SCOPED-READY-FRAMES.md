# Scoped deferred capture and internal frame mapping — default-off QA

## Result and scope

The observer can now wait for SDK video-send progress and a matching one-shot operator request before opening its <=60second recording window. The parent Host derives diagnostic context from an **already authorized native stream ticket** before spawning the child. Internal SDK Attributes carry frame ID/fragment index/count/reference-header class into the innermost RTP writer. No new byte is added to the RTP payload/header on the network, and no recovery decision is changed.

This remains an **undeployed source-level QA feature**. Tests prove contracts and local encrypted SDK behavior; no new rental, production rollout, internet-overhead measurement, billing attestation or <1% freeze claim. Parent context and SDK-send progress are not proof the receiver is rendering the fixture. Local files and process environment rely on a trusted operator/OS boundary, not cryptographic remote attestation.

## Feature wiring

- Root web-server and streamer each expose `cloudgime-scoped-recovery-metadata`, absent from defaults.
- Streamer scoped feature includes the previous `cloudgime-recovery-metadata` feature and `common/qa-trace-scope`.
- Root feature enables only the shared scoped contract and the parent hook. Both matching artifacts would be needed for deployment; **neither is deployed here**.
- Normal builds retain original packet types and APIs. Scoped builds wrap video/audio samples internally; audio carries frame_id0 and **no frame attributes**, with payload and audio RTP clock unchanged.
- Legacy unscoped observer remains available only when building its old feature without scoped mode. Scoped mode never falls back to the old eager environment gate.

### Authorized parent context

`src/api/qa_trace_parent.rs` is called at the native stream child-spawn command after `authorize_android_native_stream_ticket_for_stream` and scoped connection registration. Context contains:

`native_session_id, claim_id, handoff_binding_id, rental_session_id, connection_nonce, generation, authority_expires_ms`.

It includes no stream ticket, launch token, private key or certificate. The context expiry is capped by the ticket's authority/expiry. It never fills a missing rental ID from an operator request or local rental file: missing rental binding disables the feature. Some initial prelogin tickets may not yet contain a rental ID; this feature **does not resolve that lifecycle limitation** and must not be loosened to force a capture.

The parent clears inherited child diagnostic-context variables and old eager-activation variables before considering activation. With build feature and `CG_QA_ENABLE_SCOPED_TRACE=1`, it requires an absolute existing `CG_QA_SCOPED_TRACE_ROOT` and creates a unique directory named by a fresh32hex connection nonce. It sets only process-local `CG_QA_TRACE_PARENT_CONTEXT` and `CG_QA_SCOPED_TRACE_DIR` for that child. Without a ticket or explicit operator opt-in, no context is set.

The child trusts the environment as parent-origin because it is spawned under retained native ownership. A local attacker who can launch a process with arbitrary environment or edit diagnostic files is **outside this QA trust model**. ACL provisioning, directory junction race resistance, rate-limiting repeated parent connection directories, and authenticated operator tooling are not certified. No new public API endpoint was added.

## Deferred activation state machine

A dedicated worker checks bounded local JSON files, never in a packet callback. Max file read4096bytes;100ms polling; at most15minutes waiting. Child checks the prepared directory basename equals the connection nonce and writes `peer.json` create-new.

Activation requires:
1. Context not expired and exact claim/rental match in `C:\Users\CloudGimeRenter\AppData\Roaming\SentinelLauncher\state\session.json`, PC02 only for this pilot.
2. Exactly one video binding, at least3successful marked RTP sends with **forward RTP timestamp progression**; lastprogress<=1second. Repeated or backward retransmissions do not create readiness. Rebind/additional video binding closes this capture rather than merging epochs.
3. Operator `request.json` contains the same entire context, fresh32hex `request_nonce`, expiry within30seconds, duration1–60000ms and capture end before authority expiry.
4. Scope/readiness rechecked immediately before publishing the collector.

Only then `capture-<request_nonce>.jsonl` is opened create-new, a scoped header and current Bind event are written, and the collector is published once with `OnceLock`. The <=60s duration begins now, not at peer creation. `ready.json` and `armed.json` are coordination artifacts; they are not proof of receiver rendering/billing.

Before activation, absent/mismatched/expired requests produce no capture. After activation, rental-state mismatch, peerclose/unbind/rebind, `stop.json`, authority expiry or collectorexpiry closes the observer. It cannot rearm in the same instance. Local mismatch is noticed on the worker's next poll, nominally100ms but OS scheduling/file I/O can delay it; no instantaneous security-revocation claim. Existing parent native lifecycle ownership stops/revokes the actual child independently.

State file comparison validates IDs, not the semantic billing phase or current rendering. State-file freshness is not independently attested; an old matching local record alone cannot prove current rental validity. This remains a controlled QA facility.

## Frame mapping

`recovery_frame.rs` assigns monotonic in-process frame IDs after packetization and labels each fragment with index,count and conservative H264 header classification:
-0unknown;
-1key/IDRheader;
-2referenceheader;
-3nonreferenceheader only.

No class is a drop certificate or full dependency graph. Unsupported codecs use unknown; audio has no mapping. H264 parsing continues to use the existing production NAL reader/payloader; optional classification reads the same NAL headers. Data-partitioned/unsupported/mixed reference headers stay unknown.

`SequencedTrackLocalStaticRTP` still assigns sequence exactly as before, then calls the same SDK write path with **internal Attributes**. The SDK fills actual bound SSRC/PT. Observer records `FramePacket` beside the ordinary `Rtp` event; earlier packetizer sequence0/SSRC0 placeholders are never treated as wire identity.

The default NACK responder can resend without carrying original Attributes. Offline mapping joins a resend by existing `(epoch,ssrc,sequence,rtp_timestamp)` metadata from the original write. A missing original mapping remains incomplete; it is not inferred. Colliding mappings, changed counts/reference/timestamps, missing fragments, bad markers, unsuccessful writes, unmatchedRTP and collectorloss are rejected by `scoped-frame-trace.mjs`.

A capture can begin mid-frame. Such a frame is not silently filled or discarded to make acceptance pass: incomplete mapping is reported. No IDR is forced and no media is delayed for the diagnostic boundary. In-process frame ID and SDK Attributes use64bit Windows assumptions; no32bit build certification. Runtime-enabled scoped builds perform extra metadata allocation/copy even if recording is not armed; overhead is **not measured or claimed zero**.

## Tests and real local evidence

Final observed tests at this checkpoint:
-Scoped streamer81passed.
-Legacy observer70passed (before latest test-only RR wait update; production logic unchanged).
-Normal streamer46passed.
-Common107passed.
-Parent filter `qa_trace_parent`:3passed; **not all web-server tests**.
-Node prototype/adapter/trace82passed,0skips, including historical frozen evidence.

The scoped integration uses actual PeerConnection ICE/DTLS/SRTP on loopback only, existing H264packetizer and production sequencedwriter:
-3warmup packets precede activation and do not appear in capture.
-Matching operatorrequest activates capture after progress.
-4synthetic frames→20original packets received byte-for-byte equal to packetizer output; sameRTPtimestamp/marker and finalSSRC/sequence map back to4complete frame IDs.
-One manual RTCPNACK makes the real SDK responder issue a second write for the requested packet, mapped to the same originalframe.
-Receiver does not need to deliver an alreadyreceivedduplicate to the application; **duplicate delivery/rendering is not claimed**. Payload identity for resend is separately checked by existing SDKmockresponder tests; encrypted loopback confirms matching resend metadata.
-SR/RR with nonzero matchingLSR is awaited with a6seconddeadline, rather than fixed2.3second sleep. This changes only test synchronization, not production RTCPtimers.
-Actual JSONLfooter completeness, scope equality against `peer.json`/`request.json`, frame fragment coverage and resend observations are replayed offline.

Peer/rental files in these tests are synthetic fixtures, not a live authenticated rental. Four synthetic H264-like framepayloads exercise packetization/transport, not video decode or visual quality. No physicalphone MediaCodec/AValignment was retested.

## Preserved failures

Initial builds exposed sharedRTPwriter audio type mismatch and a testmodulepath error; fixed, logs retained. First payload-size assertion was off by one for lastfragment; replaced by stronger byte-exactpacketizercomparison rather than changingwireformat. A resend test wrongly waited for a duplicate to be delivered by thereceiver; sendertrace already showedNACK+resend, so it now validates precisely that and does not claim duplicatedisplay. Two fixed-sleeploopbacktests lackedvalidLSR at2.3s; event-basedboundedwaiting now requiresactualLSR. A JSON-string contextcomparison falsely rejected keyordering; changed to structural equality without ignoring anyfield.

Some outer command wrappers timedout at120s while childjournals recorded completedexit0jobs; finalcounts verified from per-joblogs/finishedmarkers. All failedtrace/buildlogs retained. No failed result is counted as a passingstreamqualitytrial.

## Remaining live gates

This code solves delayedactivation and internalframecorrelation in localtests. Before any PC02deployment:
- verify the real authorized ticket has a complete rentalbinding at childcreation; if not, implement a trusted scoped context update rather than read missingauthority from localfiles;
- stage an access-controlled operatorroot and use true fixture/render readiness before issuing request;
- verify runtime overhead and tracecompleteness on the target build, with no recovery intervention;
- preserve billing/session/routing policies and exact cleanup;
- receiverdeadline and clockuncertainty still require separate validation. Matchingframes/RTCPalone does not establish a playoutdeadline.

No releasebinary/installer was produced for this change; Rusttestexecutables compiled all modified paths. Installed Host/APK unchanged. Do not use a successful scoped test as approval to enable deadline-based dropping, speculativecopies, alteredNACKtimers or Relayfallback.

Evidence root: workspace `06 TEST/20261006-scoped-ready-frame-trace`; `verification.json`, `*-latest.json`, `qa-tests.json`, `scoped-sdk-loopback-*.json`, `scoped-peer-*`, retained failedlogs. Reproduction scripts refuse or preserve prior artifacts as documented in the previous observer milestone.
