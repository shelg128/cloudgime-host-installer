# Independent audio sync-group experiment — QA only, default off

Feature `cloudgime-qa-independent-audio-sync` changes only the audio track's stream ID from `moonlight` to `moonlight-audio-qa-independent`. The video track retains `moonlight`. Audio RTP, codec, volume, packet duration, bitrate, route and decoder are unchanged. This is a discriminator for receiver A/V synchronization delay, **not an accepted product behavior**. Do not enable by default or silently trade audio/video alignment for a faster video metric.

## Physical comparison

PC02 → physical RMX5367, Direct public srflxUDP, per-session H264 Baseline,720x1570@30,target5000kbps; same trace APK743c…f26 and Sunshine candidate. Three separately cleaned rentals were used: initial trace (decoder sink rotation bug), corrected shared-sync control, independent-audio-sync experiment. No automatic Relay or router change.

Using complete packet-bounded frame joins **inside each measurement window**:
- Shared-sync control1744frames: last received packet→decoder median95.238ms,p95100.507ms; decoder12.758ms; ingress→EGLdraw1.740ms.
- Independent-sync1751frames: last received packet→decoder median1.311ms,p954.076ms; decoder13.308ms; ingress→EGLdraw1.897ms.
- Median wait difference93.927ms. This strongly implicates the shared sync group in the additional pre-decode wait for this setup; one sequential comparison does not eliminate time/network/observer confounds.
- Audio RTP and audio playout events continued. Sender-report slopes showed no gross clock-rate rewind in the recorded window; neither event implies verified sound or content A/V alignment. Physical lip-sync, waveform+flash timing, audio drift and long-run behavior **NOT TESTED**.

## Remaining two stalls

Independent-sync raw adjacent draw gaps215.140ms and207.768ms:
- Complete observed frames11/17packets, originalHostsendspan1.012/6.414ms.
- Receiverframepacketspan184/163ms; decoderentered2.479/2.143ms after final receipt, decoded15.980/14.445ms, output→draw3.329/2.528ms.
- Missingseq20901/16496 requested withNACK at the same **millisecond** first higher sequence was observed; NACK→receipt72/73ms. Originalframefirstreceipt→NACK112/90ms. This does not support simply shortening the NACK timer.
- Host/receiver clock offsets are independent; no one-way latency or unique NACK-to-ciphertext mapping claimed. Loss location kernel/radio/network remains unproven.

## Acceptance limits

Counter spans68.518scontrol and68.556sexperiment;0 and2freezes respectively. Diagnostic ratios0% and0.616% are **not release passes**: unchanged80/90squality gates unmet, traceoverhead and A/Vsync unknown. Runner PASS meant diagnostic stage completion only. Fulltraceincomplete(contention27/32); missingcallbacksnotlostmedia. Onlycompleteframejoinswithrawdrawadjacencyused.

Headercapture64bytes contains an all-zero3-byteRTPextension for some packets but cuts others; SDPmapping/receiverinterpretation not fully proven from prefix alone. Main-branchWebRTCsource is a reference, not exactSDKsource. PinnedSDKbinarytiming was inspected but no memorypatch applied.

## Reproduction and cleanup

Evidence:`06 TEST/20261005-session-delivery-trace*`, `06 TEST/20261006-independent-sync`, andassociatedlivefolders. Correlation scripts, JSONs and manifests retained. ArtifactFFCE2557…191FB4F8 is experimental and was removed fromPC02.

Finalaudit18:25:30UTC:three rentalsENDED/BILLING_SETTLED;cleanupCOMPLETED/claimreleased;PC02MAINTENANCE/Ready;PC01unchanged. Hostbinary/config/APKoriginalhashesrestored,media/tasks/runnersempty,PacketMonitornotrunning/filterNone,QAtracefilesarchivedwithhashreadback. No experiment remains active.

Next: validate A/Vcontentalignment before considering independent-sync as a mode; preferably correct the actual audio clock/sync delay while preserving alignment. Separately investigate packetdelivery burst/recovery on the now-isolated stalls. Do not use randomFEC/MTU/pacing or smallerbuffer without a testable hypothesis.
