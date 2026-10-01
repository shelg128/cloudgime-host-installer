# Bounded playout-delay QA — experiment not accepted

Feature `cloudgime-jitter-window-qa` is opt-in and absent from default features. It changes only video RTP playout-delay from0/0 to0/10 units (10ms units, hence0–100ms), keeping minimum zero. Audio and unknown track labels remain0/0. No change to codec, sequence, SSRC, timestamps, route, bandwidth caps or billing.

## Physical result
Prior normal RTP-clock994c817candidate run `capture-resize-1790861682715` completed104.836s,3145decodedframes=29.999fps, median actual3314kbps, fixed720x1570NVENC, no decoded-progress stall. Freeze4.281s=4.0835%, above unchanged3%gate: overallFAIL. This is the first full-window physical evidence that the earlier52–64second permanent stall did not recur, not proof all streaming quality is fixed.

QA run `capture-resize-1790863014593` completed108.659s,2955frames=27.195fps, freeze29.85s=27.47%, actualmedian2780kbps(below3Mbpsgate), lost2265. Encoder was software/libx264 and geometry660x1440, unlike priorNVENC720x1570. Heavy packetloss and changedencoder/geometry confound directA/B causality. **Not accepted; no claim the playout change alone caused all regression.** No decoded-progress long stall appeared in this run either.

Both tests maintained originalAndroidAPK, explicitDirectP2P, unchangedclaim/nativeRoot/billing within each test, and boundedrecoverytraffic. Neither used automaticRelay. Both completedautomaticcleanup and restored baseline media binaries/preferences. Runtime-agentpilot1ca8009remains separate.

58targetedWebRTCtests pass in QA mode including newaudio/defaultisolationtest. Normal defaultmode tests/build must remain recorded separately; no QAfeature is enabled in normal installer/release. Preserve the candidate/evidence but do not promote or silently enable this jitter window.

Next: stabilize/record selectedencoder and matchedgeometry before comparing any jitter changes; correlate receivepacket/framearrival timing with remaining brief freezes. Do not lower3%freeze gate, increasebudgets, repeatpreviouslypassedreconnect/fullscreen/audio, or maskfailure by increasinglatencywithoutmeasurement.
