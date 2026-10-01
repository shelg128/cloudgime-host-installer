# Passive NACK dispatch QA — Host responder not the measured bottleneck

## Offline receiver-side evidence (existing physical RTC log)
New strict RTCP parser follows WebRTC blob encoding (all varint lengths, then payloads) and RFC4585 PID/BLP expansion. It reads only complete top-level events from the previously recorded encrypted-media session; no new packet payload capture.

Selected H264 stream SSRC1415736854 has sequence0..55424, with no rollover/sequence reuse in this evidence. 1308 outgoing NACK messages request3518distinct packets; all arrive later in the log. Phone-clock measurements: loss-hole→firstNACK median0ms/p951ms; firstNACK→receipt median59ms/p95180ms/max1364ms; latestNACK→receipt median58ms/p9584ms/max118ms. Slow samples use repeated NACKs (up to13). This is not evidence the receiver waits hundreds of milliseconds before its first request.

Host NIC correlation shows39slow packets (>300ms) in capture; all had multiple Host sends before first receipt. Clock-aligned timing is relative only and cannot identify exact path hop, accepted physical copy, or Host receipt time. No assumption that all delay is network residence.

## Passive Host observer
Feature `cloudgime-nack-dispatch-qa` is absent from default features. Registered BEFORE default interceptors, it observes RTCP after decryption but before default responder, and RTP writes below responder so retransmissions are visible. The actual default responder still runs unchanged. A real default-interceptor integration test verifies this order and exactly one delegated write for each call.

Stores sequence/timestamp/Instant metadata only: four video tracks maximum,8192slots per track,10sNACK-attribution age,600slogging period,1800rows maximum per track, one-eighth sequence sample. No media payload retention, endpoint/credential log, new retry, spawn per packet, network route, codec, playout delay or bandwidth change. Audio passthrough; unbind/replace/closeinvalidate the oldprobe; diagnostic lock neverheldacrossawait. Requested sequence collision/expiry/timestamp reuse do not alias anotherpacket.

Rows `CG_NQ` fit within existing Host log truncation: e=nNACK/e=wcompletedwrite/e=eerror; sSSRC,qsequence,tRTPtimestamp,nNACKcount,aµsfromlatestobservedNACKtowriterentry,wµsdownstreamwrite.

## Physical result
Run `capture-resize-1790873498993`, session `62d4e622-e61b-421c-9535-aeb60e164d5e`, original Android APK, NVENC720x1570@30, publicDirectP2P, noNICcapture:
-498sampledNACKs and498retransmissionwrites, zeroerror/truncatedrows.
-Host dispatch median0.297ms,p950.792ms,max5.247ms.
-Downstreamwrite median0.079ms,p950.222ms,max0.774ms.
-No sampled dispatch/write over10ms. Thus this sample does not justify “speed up Host NACK responder” or more aggressive retry. Unsampled/pastconditions are not proven by this observer.
-Quality stillFAIL:96.700672s,2872frames29.6999fps,median3335kbps,19freezes/4.382s=4.5315% >3%gate. No long decoded-progress stall in measured windows.
-RecoverytrafficbudgetPASS. Observerwasremovedonrollback; baseline media binariesrestored. No automaticRelay or billing change.

Tests:6JS RTCPparser tests;63WebRTCQA tests for deployedbinary, then65after two additional lifecycle/isolation tests;58defaultWebRTC tests and defaultreleasebuildpass. ObserverremainsQAonly/notpromoted.

## Conclusion/limits
Evidence narrows remaining freeze to end-to-end delivery/recovery after prompt request and prompt Host dispatch, not a measured multihundred-millisecond Host queue. It does not identify WiFi/ISP/NAT/OS cause, prove the forward/reverse path independently, or authorize changing routersettings/route.

Next justified experiments are controlled network/path or bounded original-copy timing comparisons with exactly sameNVENC/resolution and byte/packetceilings, not speculative decoder/retransmitrewrites. Direct/Relay choice must remain explicit; no silentrelay. Quality not accepted.

Evidence: workspace`06 TEST/20261001-nack-timeline`, especially`host-dispatch-result.json`,`nack-receive-summary.json`,`nack-vs-host-sends.json`,`slow-recovery-attempts.json`.
