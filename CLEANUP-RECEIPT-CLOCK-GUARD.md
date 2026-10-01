# Cleanup receipt observation clock guard

## Reproduced physical failure
Test rental `a58bee01-e920-4a69-b2da-2db381eea119` (PC-02, 2026-10-01) stopped after wireless ADB became offline before Android native launch. Billing settled; Sentinel cleanup receipt was accepted, but Host cleanup stayed pending.

The durable Host receipt observedAt was `2026-10-01T10:14:36.786Z`, whereas the backend command requestedAt was `2026-10-01T10:14:37.056Z`: 270ms earlier due to inter-machine clock skew. Backend correctly returned409 `CLEANUP_OBSERVATION_TIME_INVALID`. Host retried the same durable early receipt forever.

Authorized operator recovery preserved the original encrypted receipt under admin/SYSTEM ACLs, confirmed the rejection, and archived only that invalid receipt. No timestamp/proof/challenge was edited and no receipt was manufactured. The running official Host worker re-inspected the exact native session and generated a new receipt at10:22:30.424Z. Both authentic receipts completed cleanup/claim release at10:22:32.963Z. This was automatic receipt completion with operator-assisted invalid-outbox recovery, not an unattended pass.

## Source change
Before constructing or persisting a new receipt, compare the actual serialized millisecond Host observation time with command requestedAt. If behind, return `HOST_RECEIPT_CLOCK_BEHIND_COMMAND`, persist nothing, and let the existing5-second loop revoke/re-inspect again next poll. Never clamp observedAt to server time, advance the clock, loosen backend validation, change authentication, or mutate an existing durable receipt. Existing malformed/early receipts still need explicit investigated recovery; this patch prevents creation of new too-early receipts.

Six clock tests cover actual270ms skew, valid next poll, equality, millisecond truncation, malformed command time and backward clock movement. Before:2/6passed,4failed. After:all6pass, together with5existing authenticated revoke/OS-child tests =11passed. Normal release web-server build passed. Candidate not deployed.

## Separate runner issue
The physical Android harness returned immediately from its exception path after requesting session stop. It did not wait for receipts before the outer runner began rollback. A new bounded read-only final cleanup waiter verifies the same rental is ENDED/settled and claim cleared, even if ADB is offline. It never sends stop/cancel, changes billing or releases claims itself. Five tests passed. This is test tooling, not a production lifecycle change.

## Validation limits
RTP-clock candidate994c817 remains physically unvalidated: the latest attempt never launched Android media because ADB became offline after a successful preflight. This receipt clock patch does not fix ADB or prove streaming quality. PC-02 returned to maintenance and accepted Device Match baseline; PC-01 untouched.

Evidence: workspace `06 TEST/20261001-clock-postreset`, `receipt-before.log`, `receipt-after.log`, `receipt-reobservation.json`, `latest-cleanup-audit.json`, `receipt-host-build.json`.
