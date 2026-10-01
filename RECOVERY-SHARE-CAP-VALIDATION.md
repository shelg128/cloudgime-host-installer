# Recovery original-byte share cap — two scoped physical passes

## Implemented behavior
Opt-in candidate feature `cloudgime-recovery-share-qa` changes exactly one variable: proactive protected-video copies may spend at most 25% of successfully sent eligible original IP+UDP bytes. It is an additional bound, not a replacement for the signed grant's 2Mbps/1200pps ceiling, token-bucket burst, loss hysteresis, monotonic expiry, selected Direct UDP pair and exact binding checks.

Credit starts at zero, accrues from eligible originals only, and is capped to one maximum eligible datagram (1328 bytes including IPv6+UDP). No idle/time-based share refill. Suspend, inactive recovery and backpressure discard share credit. A failed copy spends credit without recording successful extra bytes; it cannot increase the allowed share. Retransmissions, RTCP, audio and child tasks still have no copy authority.

Original packets are not delayed, queued, dropped or changed by this share cap. No new packet/timer/task per sample, SRTP reencryption, route change, jitter delay, codec switch or bitrate adjustment. NACK retransmissions continue through the unmodified default responder and are NOT included in this proactive-copy cap. Total congestion control and paced-copy scheduling remain separate work, not claims of this change.

`CopyBudget::new` preserves prior default behavior. `new_with_share` provides the stricter conjunction, selected by the opt-in feature in `Recovery::new`. Global release defaults remain unchanged pending wider regression coverage. Candidate tests exercise actual local encrypted peer, socket byte equality, authority/revocation, stale binding, pressure and mixed packet sizes.

## Why this experiment
Prior physical evidence established prompt NACK generation and Host dispatch (p95 0.792ms); rewriting the responder or increasing retries was unjustified. Proactive recovery previously consumed around1.60Mbps in the latest complete diagnostic run on top of3.34Mbps video, and brief freezes were4.53%. Share-cap tests keep the same phone, Wi-Fi, Direct route, original APK, NVENC H264720x1570@30, requested5Mbps and original acceptance thresholds. They do not prove a particular Wi-Fi/ISP hop is faulty or comparable to GFN's path.

## Validation
-66 WebRTC tests pass with QA share enabled and66 with normal defaults; real UDP and real encrypted peer included.
-4 JS evidence-assessment tests pass, refusing torn logs, epoch-overrun and any excessive sample.
-QA release build and normal release build pass.
-Offline replay of22226unique originals predicts only a traffic bound (~24.39% or869kbps), not packet-loss or quality improvements.

Two consecutive physical trials, unchanged candidate (share25%, default0/0playout, no diagnostic APK/NIC capture):

| Metric | First complete trial | Confirmation |
|---|---:|---:|
| Evidence suffix |1790877136502|1790877783538|
| Counter seconds |96.657656|96.795153|
| Decoded frames |2872|2879|
| Decoded FPS |29.7131|29.7432|
| Actual video median kbps |3384|3286|
| Freeze count |12|10|
| Freeze seconds |2.560|2.538|
| Freeze percent |2.6485%|2.6220%|
| Long no-decoded-progress sample |0s|0s|
| Avg proactive extra kbps |660.47|694.61|
| Max interval proactive extra kbps |915.46|890.45|
| Recorded cumulative copy share |22.6277%|23.7230%|
| Original-byte share cap |PASS|PASS|
| Signed byte/packet grant caps |PASS|PASS|
| Existing video >3Mbps / FPS / freeze<3% gates |PASS|PASS|
| Cleanup / preference restore / rollback |PASS|PASS|

Timing windows and scopes differ from earlier sessions, and these are sequential network trials, not randomized A/B proof. Both whole windows pass without slicing, changing thresholds, reducing resolution, replacing Wi-Fi or selecting Relay. Short freezes are reduced but not eliminated (12/10 per window). No promise of all-game, all-NAT, all-resolution or long-soak acceptance.

First trial initially had a pre-billing capture-preparation SSH timeout; rollback/health verified and no rental created. The two complete tests each preserve their own exact claim/rental/nativeRoot/billingcommit through stop and cleanup, and there was no route fallback. No accepted audio/reconnect/fullscreen test was repeated.

## Deployment status
Candidate was installed temporarily only for these tests, then the pre-test media web-server/streamer/Sunshine baseline was restored by the runner. The previously accepted PC-02 helper91ea2f1 and runtime-agent1ca8009 pilots remain unchanged. This is a validated candidate for this target condition, not an unattended production deployment or a claim the entire product is finished.

Evidence: workspace `06 TEST/20261002-recovery-share/`, `06 TEST/20261002-recovery-share-confirm/`, and corresponding capture-resize folders. Archive includes exact candidate pair, required existing capture runtime and rollback information; using only a streamer with an incompatible Host grant contract is not supported.
