# Default-off Host SDK boundary metadata interceptor

## Status

Implemented and compiled against pinned `webrtc/interceptor/rtcp/rtp 0.17.1`. Installed Host/APK unchanged; no rental or production deployment. No recovery action, RTP mutation, pacing/buffer change, codec change, new feedback, or route fallback is introduced.

This milestone moves observation into the actual Host SDK pipeline instead of relying on simulated ACKs or receiver-side clocks. It does **not** complete authenticated rental scoping, access-unit dependency propagation, receiver display-deadline mapping, overhead qualification, or <1% physical freeze acceptance.

## Activation and scope

Two independent opt-ins:

1. Build `streamer` with `--features cloudgime-recovery-metadata`. Cargo defaults remain `[]`.
2. Before the first peer is created, an operator must set all three process-local variables:
   - `CG_QA_RECOVERY_TRACE_DIR`: absolute **existing** output directory.
   - `CG_QA_RECOVERY_TRACE_NONCE`: exactly32 hexadecimal characters.
   - `CG_QA_RECOVERY_TRACE_EXPIRES_MS`: absolute UTC milliseconds, future and no more than60seconds away at validation.

No directory is created by this feature. A new file is created with `create_new` and nonce/process/peer filename; existing files are not overwritten. One environment-armed collector per process and one peer per builder; peer recreation cannot multiply the observation budget. Invalid/missing/expired configuration silently leaves the observer disabled. Initialization/file failure does not fail streaming.

**Trusted local operator QA only.** The directory inherits existing OS permissions; the module does not install ACLs or authenticate a rental. Canonicalizing the path and requiring a nonce are not a security boundary against a malicious local account or junction replacement. Do not enable on production/customer sessions without external access-controlled staging and an authenticated session/claim contract.

The timer begins during peer/API setup, not when a later fixture starts. There is no hot-arm API. This intentionally restricts the current feature; a normal login/preflight may consume the entire short window. A scoped activation path after native readiness is still required before a useful live CloudGime diagnostic. It must not extend the window implicitly or enable unrestricted background logging.

## SDK integration point

`streamer/src/transport/webrtc/mod.rs` conditionally adds `ObserverBuilder` **before** `register_default_interceptors`.

Pinned SDK chain binding wraps in registration order, making the first observer innermost:
- local RTP writes have actual bound SSRC/PT and assigned sequence by the time they reach the observer;
- default NACK responder resend writes pass through the same inner writer;
- default SenderReport-generated writes pass through the inner RTCP writer;
- RTCP read completes after underlying SRTCP decryption, and is observed before the default NACK/report reader wrappers process it.

Only video RTP streams are observed. SR/RR/NACK/REMB/PLI metadata may include audio/control SSRCs; offline analysis must filter the intended stream. No SDP, credentials, addresses, packet payload, RTCP raw blob, codec bitstream, or key material is stored. IDs and timing themselves are operational metadata and still require access controls.

The observer forwards the **same references, attributes, errors and return values**, calls the underlying writer/reader once, and never invokes a resend. NACK/PLI behavior belongs to existing interceptors and application callbacks. Closing/unbinding affects observation only, not the forwarded data path. Stream rebind gets a new epoch; stale wrappers stop observing while still forwarding normally.

## Timing and data semantics

All event times use one peer-local `Instant` origin, in integer microseconds:
- RTP and SR: call entry and return of the awaited inner writer.
- RR/NACK/REMB/PLI: time the inner decrypted reader completes.
- Bind/unbind: observation of SDK stream lifecycle.

**These are not NIC egress/ingress timestamps.** Writer completion can include transport work and scheduling. SDK send success does not prove remote delivery. Repeat writes alone do not identify whether the cause was a NACK, proactive protection, or duplicate caller submission.

Full NTP is stored as `ntp_seconds`, `ntp_fraction`, `compact_ntp` (32bit fields), preventing loss of precision in JavaScript JSON consumers. Receiver report `total_lost_raw` preserves the SDK's u32 field; consumers must interpret the signed24bit wire semantics if using it as a loss delta. Do not assume it is monotonic or a positive packet ACK.

When a single successful local SR matches an RR's LSR, `sdk-boundary-trace.mjs` computes the **SDK-call interval estimate**:

```
lower = rr_read_complete - sr_write_complete - dlsr
upper = rr_read_complete - sr_write_start    - dlsr
```

Negative/ambiguous/out-of-range intervals are rejected. This brackets only uncertainty between the observed SR call endpoints; it is **not a proven bound on true network RTT**, nor a one-way or receiver presentation deadline. Even a complete trace leaves `rentalBound=false`, `receiverDeadlineAvailable=false`, `productionRecoveryEnabled=false`.

## Bounded observation and failure behavior

- `sync_channel(512)` plus `try_send`; producers never wait for disk or free queue capacity. No media-path payload copy or serialization.
- Fixed numeric metadata events, max50,000 attempted records, max16MiB file reservation, <=60s activation. Per-RTCP scan bounded to32packets,31RR blocks,64NACK blocks,32REMB targets; truncated metadata increments `omitted`.
- Separate writer thread serializes/flushed JSONL, with20ms receive timeout. File setup/header writes happen at initialization, not per packet.
- Full queue increments `dropped_queue` without withholding media. Errors, omissions, quota exhaustion and cancelled/outstanding write calls invalidate completeness.
- In-flight write cancellation is tracked by an RAII guard. Expiry/unbind during a write counts as a gap; close with outstanding calls has an incomplete footer.
- Footer explicitly counts accepted/written/queue drops/omissions/cancelled/outstanding calls and I/O/quota errors. Missing/malformed footer is invalid.

`observation_complete=true` means the observed bounded SDK window had no detected collector loss; **not** all-session completeness, zero streaming loss, frame decode success, or low observer overhead. Disk/OS blocking in the worker cannot be forcibly cancelled; a stuck or terminated process may never write its footer and is rejected offline. No join/wait is inserted into stream close. CPU/atomic/cache effects remain possible even though queue submission is nonblocking; performance overhead is not yet measured on PC-02.

## Validation actually performed

Rust feature tests: **70passed**, zero failed/ignored in final run. Normal default tests: **46passed**. Offline JavaScript prototype/adapter/trace tests: **74passed**, zero failures/skips with evidence enabled.

1. Packet payload/header/attributes preserved, exact return values and sentinel errors preserved under normal/full/closed collector.
2. Pinned SDK's real NACK responder produces the requested retransmission; observer sees both writes with same sequence and payload, while NACK handling is unchanged.
3. Pinned SDK's real SenderReport interceptor produces an SR seen at the inner writer.
4. Two actual `PeerConnection`s, UDP4 loopback-only IP filter, no STUN/TURN servers, ICE/DTLS/SRTP established.20synthetic RTP packets received with nonzero negotiated SSRC/PT matching recorded final IDs and identical payload. Synthetic payload is **not decodable game video**.
5. Both peers read RTCP, producing nonzero matching LSR/DLSR. Four retained local runs before the latest checkpoint show SDK-call intervals roughly0.47–1.01ms. These are localhost integration timings, not CloudGime internet latency.
6. Actual JSONL writer and footer replayed offline. Tests intentionally generate incomplete traces for cancellation, outstanding calls and truncation; they remain stored separately.
7. Separate test processes validate absent/expired/invalid-nonce/missing-directory settings produce zero files. Valid settings produce exactly one file with footer; second activation is denied.
8. Normal and feature binaries built and hashed. Normal binary has no `CG_QA_RECOVERY_TRACE_NONCE` string; feature binary does. Source cfg/default assertions remain the principal default-off guarantee, not string scanning alone.

### Preserved failures / limits

- First real PeerConnection test failed because the test process had not installed the same Rustls AWS-LC provider selected by application `main`. No production crypto configuration changed; test init fixed, failed log retained.
- First encrypted loopback passed packet/recording checks but RR had `LSR=0` because the remote test receiver wasn't draining RTCP. That snapshot is retained and cannot yield RTT. The test now drains RTCP and requires a genuine matching SR/RR pair.
- Final build wrapper reported a120s timeout. Child journal subsequently recorded all three stages exit0, finished timestamp and binary hashes; artifacts independently rechecked. Do not equate wrapper status alone with success/failure.
- Release binaries were built before adding the final **test-only** environment activation module and documentation. Runtime observer source was unchanged; no byte-identical clean-commit build claim is made.
- There is no authenticated rental mapping or AU-reference label in this tap; RTP timestamp joins need a separate bounded validated propagation mechanism.

## Evidence / reproduction

Workspace root: `06 TEST/20261006-host-metadata-interceptor`.

- `build.mjs` records each Rust job's own exit and log; `final-build.json` hashes normal/feature artifacts.
- `sdk-loopback-*.json`, `sdk-trace-*.jsonl`: actual encrypted local peer evidence, plus intentionally incomplete safety traces.
- `verify.mjs`: checks activation in separate processes, validates trace SR/RR intervals and artifact hashes.
- `qa-tests.mjs`: reruns all prior locked offline prototype/adaptor tests plus the SDK trace consumer.

No deployment, no rental, no production recovery promotion. Next required step is a **scoped, readiness-triggered metadata activation contract and verified session/frame mapping**, followed by overhead-controlled measurement. Do not start another rental merely to generate a trace that expires before the fixture is ready.
