# Explicit original-write accounting V3 — local validation only

## Defect addressed
V1 called every no-CopyScope write a retransmission. V2 corrected that to bounded sequence/timestamp matching with an Unknown category, but still recognized original writes only through permission to copy. Those are different concepts: a valid original write may proceed while optional-copy permission is unavailable (e.g. ICE negotiation or no recovery grant). Under V2 it could be charged as Unknown and unnecessarily suppress optional recovery.

V3 sets a bounded task-local `(sequence,timestamp)` marker at the actual SequencedTrackLocalStaticRTP original send, independent of CopyScope. The innermost accounting interceptor reads this marker; default NACK responder's spawned retransmit task cannot inherit it. Known repeats match the bounded8192entry ring; unknown writes remain distinct and conservatively consume optional-copy headroom. Diagnostics use `CG_EG3` to avoid mixing evidence versions.

This marker carries no peer, socket, grant, credential, or copy authority. Existing signed DirectRecoveryGrant, exact CopyScope, transport/SSRC/PT/seq match, expiry, loss policy and rate/share caps remain mandatory to emit a copy. A real-default-responder/UDP integration test sends20 originals without CopyScope despite ample accounting credit and verifies **zero copies**. Explicit sender sequence mutation and encryption behavior are unchanged.

## Verification
-80 egress V3 WebRTC tests pass,84 with optional pacing,69 default.
-Three JS source-contract tests pass.
-Identity matching, nested scope restoration, cancellation scope cleanup and task-spawn noninheritance tested.
-RealWebRTC peer handshake/protected-copy behavior and real NACK responder with no-CopyScope originals verified.
-Default-off; only compiled under existing `cloudgime-total-egress-qa`. No vendor budget change in this patch.

## Physical status
Candidate prepared with originalAPK, fixedNVENC/H264720x1570@30, Direct route,5Mbps request and frozen strict sub1/latency/FPS gates. At preflight four network samples succeeded but ADB transport closed before preparation/deploy/rental. One reconnect attempt timed out. **No deployment or rental occurred. V3 has no physical quality pass.**

Previous readiness failure was reviewed offline: startup had real decoded progress, followed by later runtime-stop/reconnect in Host log. It does not prove decoder failure; missing failure-side Android snapshot was an evidence gap. New runner preserves that snapshot without widening readiness or changing automatic route behavior.

The prior25%-copy two-pass3% candidatea41da8f stays unchanged in its archive. Strict<1%/ideally<0.5% target remains open, not replaced by the3%result.
