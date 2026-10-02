# Combined video egress accounting and byte-burst pacing — QA, not accepted

## Scope and unchanged safety policy
`cloudgime-total-egress-qa` enables an innermost interceptor to observe negotiated H264 writes, including default NACK-responder writes, at the actual protected UDP send boundary. It is an accounting signal only and never grants duplication authority. Existing exact peer/SSRC/PT/sequence CopyScope, signed grant, Direct UDP requirement, loss policy, expiry, 25% original-byte copy cap, 2Mbps/1200pps caps and backpressure all remain mandatory.

Confirmed retransmissions consume credit before optional proactive copies. Original and mandatory retransmission sends are NEVER suppressed, delayed or capped by the ledger. Consequently this is **not a hard whole-connection bandwidth limiter**, and total recovery can exceed25% when mandatory retransmissions alone exceed it. Audio, control, DTLS, STUN and TURN are not part of the video ledger; they remain unchanged. No routing fallback, codec/resolution/FPS change or larger playout buffer.

Ledger holds one endpoint per bound track (maximum2 H264 tracks), bounded byte credit (one eligible IPv6 datagram), clears positive credit after idle/endpoint change, and revokes on unbind/close. It does not have a growing queue or background worker. Copy send fails closed if accounting scope is missing. Combined delayed-copy QA is compile-time prohibited because that combination was not validated.

## Corrected classification
The initially tested V1 treated any write lacking live original CopyScope as retry. That is **ambiguous**: a legitimate original can lack usable scope (e.g. unsettled transport). Its r counter must not be reported as proven NACK traffic. V2 retains a bounded8192-entry sequence+RTP-timestamp ring populated only by scoped originals. Matching repeated writes are Retry; unmatched writes are Unknown, not Retry. Unknown bytes consume optional-copy headroom conservatively but cannot establish retry volume. `CG_EG2` logs o/r/u/c/d/x. A strict evidence parser rejects legacy logs, unknown bytes and epoch changes. V1 evidence is retained with an explicit correction, not overwritten.

## Original-frame pacing QA
`cloudgime-egress-pacing-qa` adds a bounded frame schedule: first4800 estimated wire-byte group immediate, subsequent absolute targets use24Mbps burst service with a6ms frame cap and25% recovery allowance. Absolute deadlines consume time already spent writing. No accumulated next-frame debt or trailing frame sleep. Retransmissions are not queued by this scheduler. Its allowance is an estimate; it does NOT directly pace every actual egress byte. Quality acceptance still requires no latency regression.

## Physical results (all on same phone/Wi-Fi, original APK, Direct, NVENC720x1570@30)
1. V1 egress candidate (`capture-resize-1790895418771`): valid96.708603s window,29.9973FPS,1.421s freeze=1.46936%. Gameplay latency median136ms versus preregistered105ms maximum. FAIL sub1% and latency; never promoted. V1 ledger attribution marked ambiguous.
2. V1 pacing (`...1790896420609`): fixture stopped after641ms with IOException during state-file access. High-load never started. INVALID, not a media performance result. Fixture now uses sharing-friendly read and bounded IO-only retry, still fails closed on changed identity or malformed JSON; tests verify this.
3. V1 pacing retry (`...1790897164142`): full attempted load but stats source/counters changed; ADB offline intermittently. INVALID. Very high V1 r counter is NOT evidence of a retransmission storm because unknown writes were conflated. Original screen timeout1800000 and original APK hash later independently verified; rollback and cleanup completed.
4. V2 egress (`...1790899035999`): native first frame seen but ACTIVE_MEDIA_NOT_READY; Host log shows runtime-stop and reconnect/startup activity. No valid high-load measurement or V2 egress summary. Runtime rollback succeeded; cleanup completed slightly after runner deadline, then PC-02 held maintenance via official API. Original runner error preserved with final reconciliation.

Target freeze<1%, preferably<0.5%, with no latency/FPS/geometry regression is still OPEN. None of these candidates is accepted or installed. Earliera41da8f 25%-copy candidate remains archived as the prior two-pass3% solution, not a sub1% solution.

## Local verification
Final tests:77 egress,81 pacing,69 default WebRTC tests PASS, including actual default responder + UDP sink observation, real peer protected-copy behavior, unknown/write classification, source identity, revocation, original/retry byte preservation and credit budgeting. Seven JS accounting/containment tests and PowerShell bounded fixture IO checks PASS. Default behavior unchanged; flags remain off.

## Evidence
Workspace `06 TEST/20261002-total-egress`, `20261002-egress-pacing`, `20261002-total-egress-v2`. The frozen latency/FPS/load envelope remains the prior `20261002-subpercent/baseline-envelope.json`. Do not silently widen it or declare the old3% result a new target pass.
