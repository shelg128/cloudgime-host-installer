# Frame-tail copy allocation — evidence-led QA, physical validation pending

## Why this experiment differs
Previous accounting/debt and delayed-copy candidates did not meet the sub1% target. Return to the bounded25% share concept that passed the earlier3% gate twice. Do not combine rejected egress/debt or delay logic.

Archived packet evidence (one selected H264 SSRC/PT/UDP direction, unique sequence+timestamp, no reuse) shows end-marked packets had264NACKs/1815packets (14.545%), versus1381/20411nonmarkers (6.766%). A NACK may reflect reordering or loss; this correlation alone does not prove the cause of a freeze.

## Candidate implementation
`cloudgime-frame-tail-copy-qa` is off by default and implies existing scoped25%share recovery. Protected RTP header marker bit selects whether the copy must preserve1328bytecredit for an upcoming end packet. Body copies may spend only credit above this reserve; end copies spend usual available credit. Bookkeeping credit ceiling grows from one to two max datagrams (2656bytes) to permit body+tail allocation; no free startup/idle credit is added. Cumulative extra/original bytes still<=25%at every prefix. Existing signed byte/packet token-bucket ceilings and burst maxima are unchanged.

No queued copies, timer, worker, original delay, packet/header rewrite, re-encryption, retransmission change, codec/bitrate/resolution/FPS/route change or decoder buffer increase. Copies remain exact ciphertext sent immediately after successful eligible originals. Exact socket/peer/SSRC/PT/seq, context lifetime, grant expiry, loss hysteresis, revoke/rebind and backpressure checks remain mandatory. The feature refuses combination with total-egress or delayed-copy experiments. Atomic CG_TAIL counters describe successfully emitted copies by packet class.

## Local verification
72tail-feature Rust tests pass;70prior25%feature and70default tests pass;7JSmodel/contract tests pass;releasebuild succeeds. Includes realUDP exactciphertext/original ordering, no-context refusal, prefix share bound, hardbyte/pps cap, expiry, revocation, bodyreserve, realWebRTCpeer and all prior guard tests.

Policy-only replay of22226unique archived packets: normal25%policy copies712of1815endpackets and4928bodypackets (24.389%bytes); tail-priority copies1815endpackets and4061bodypackets (23.976%). Replay assumes always-active recovery and omits rate tokens/path feedback, so it is **not a latency/quality prediction or live proof**. Actual run must satisfy frozen sub1%/ideally0.5%, latency,FPS,media load, fixedNVENC720x1570, DirectUDP and billing/cleanup gates.

## Physical status
ADB5555was initially available but becameoffline while preparing the built candidate. One reconnect timed out. No deployment, rental or APK change occurred in this continuation. Candidate prepared at workspace `06 TEST/20261002-frame-copy-selection/`. Previous a41da8f remains untouched. Target not achieved or claimed.
