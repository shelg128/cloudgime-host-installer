# Live scoped pilot: capture blocked, daemon environment root cause corrected without another rental

## Outcome

One real PC-02→physical Android Direct rental was run with temporary scoped Host/streamer/Sunshine and the previously validated temporary APK. Native prelogin rendered, billing began, and the official post-billing app launch preserved the same rental, claim, native root and billing commit. Mode720×1570@30,target5000kbps was verified.

**No capture was collected.** The runner stopped with `SCOPED_CONTEXT_NOT_READY` before screenshot approval/request because the trace root contained no peer directories. Do not describe this as successful stall tracing, a recovered frame, improved latency, or a sub-1% result. There was no retry rental.

Session: `7f4af20f-817a-4abd-812d-b1571718b3c1`.
Native root: `android-native-83643f7cc22647428c572fba711b1b4e`.
Billingcommit: `commit_55af25ca7cfc4b6796ce207eb65c7191`.
Ended and BILLING_SETTLED;cleanupCOMPLETED/claimreleased. Temporary binaries, APK and environment restored. PC01 quarantine untouched.

## Proven integration mistake

The actual web-server ancestry is:

`CloudgimeHost-Host service → cloudgime-runtime-agent run-daemon → web-server.exe`

It is **not** directly owned by the Sunshine `CloudgimeRuntime-Host` service. The correct service was used for environment staging, but service stop only sent `StopBundle`; the old daemon survived. After service restart, `ensure_daemon_running` reused that daemon. The replacement web-server therefore inherited its old process environment, not the new service environment.

Process evidence shows daemonPID1988 and identical creationtime from before rehearsal and pilot, while the servicePIDandwebPIDchanged. Source`run_service_main`stop path and`ensure_daemon_running`confirm this behavior. Empty diagnostic root supports activation failure before a validchildcontext, not a media-routing failure. Environmentreadbackafterrollback cannot by itself prove the failedrun's environment; ancestry evidence and the controlled corrective rehearsal provide the causal chain.

## Correction tested without rental

`scoped-pilot-deploy.ps1` is a **PC02-specific operator QA deployment script**, not a production installer. It keeps hash locks, idle-only gate, retained backups, service environment backup, fixedtargets and watchdog rollback. Corrected sequence:

1. Verify the daemonPIDpoints to the exact bundle runtime-agent executable and `run-daemon`command.
2. Stop `CloudgimeHost-Host` and wait for stopped.
3. Invoke that agent's existing `shutdown-daemon` command, verify the retained daemonhandle exits.
4. Stop Sunshine runtime, replace only hash-matching testartifacts, stage processserviceenvironment, start services.
5. On rollback, repeat the same daemon shutdown before restoring environment and baselinebinaries. Never leave a daemon carrying QA environment after registryrestore.

`src/qa_trace_startup.rs`, compiled only with the scopedfeature, writes a minimalcreate-newreceipt at serverstartup ifexplicitoperatorenvironmentisactive. It records PID,time,featureenabled,directorywritable; no bearer,claim,rentalorcredentials. The file is **startup readiness only**, not authority or capturepermission. Absent/invalidenvironment leaves it inactive; no publicendpointadded.

Corrective maintenance rehearsal on PC02:
- Newdaemon9444 parentedweb14956.
- `parent-activation-14956.json` confirmedenabled+writableroot fromwebPID14956.
- Startupready and allrollbackcompleted.
- Finaldaemon4868 afterrollback;serviceenvironmentexactlyrestored,zeroQAvariables.
- No rental in this correction. The corrected configuration has **not** been retested for livecapture.

The first HTTP-only startuprehearsal hadpassed but missed the retained-daemonenvironmentproblem. Future prebilling gates must check actualactivationreceipt+newdaemon+binaryhash, not HTTP200alone. `startup-activation.mjs` encodes those checks; evidenceprovenance still belongs to the trusted operator supervisor.

## Fresh packet-rate progress

Previous `ready.json` was a one-time marker, insufficient to budget capture. The scopedworker now writes `progress.json` beforeactivation at~500msintervals containingcontext/process/SSRC/epoch,timestamp,packets_sent,packets_per_second,progress_frames. Atomicpacketcountcountsallsuccessfulvideowritesincludingretransmissions;forwardRTPtimestampcheckstillcontrolsframereadiness. Duringactivecapture,progressfileupdatesstop to avoidextraI/O.

Filewrites may overlap a reader; consumers must bound retries/reject malformed or oldJSON. Do not use cachedreadytimestampasfreshsample. Beforecapture, parseandcheckexactcontextandtruehosttime. Snapshotwritesoperateonworker,notpacketcallback;oneatomiccounteraddedperobservedwrite. ScopedRustsuite82passed includingfreshprogressandsnapshot-pausecase. Overheadbenchmarksfrompriorversiondo **not** automaticallyqualifythisnewcounter/snapshotbehaviorunderliveGPUload.

## Limits preserved in pilot runner

ExplicitDirectonly,noRelayfallback. Same-rentalpostbillingnativeone-uselaunch;tokensnotwrittenintoresults. Mode/fixturevalidationbeforeactivation,completecontextcheckandbudget-based10–25secondcapture. The pilotneverreachedapprovalorrequest;candidatecaptureplanningandcollectioncodebeyondthatpointremainunexercisedlive.

The runner was corrected to subtract truePC-measuredevidenceage fromHostsnapshotclock rather thanrestampingbackend/displayreadbacksasfresh. It rejects slowhostsnapshot>1s and stalepreconditions. Those strictboundsmayrequire tighterorchestration; do not silentlyrelax or fabricatefreshness. A futurevisualapprovalmust recordreviewtime and retain actualscreenshot time. Mediaqualityclaimscannotcome fromthisshortdiagnosticwindow.

## Build/test evidence

- Streamerscopedprogress: `e5c6cd83cc5f0ee9859169dd4286402be56aa45e4d5a7d4f1e1787a57b6d7d1f`.
- Newwebstartupreceiptbuild: `de5245d93c0d5c8e6973ad77d6d56a4808bce5858cd3b84a9fb66260df5263d6`.
- ScopedRust82passed;3explicitqualificationtestsignoredbydefault (priorworkevidenceretained,notrerunhere).
- StartupreceiptRust2passed.
- NodeQA111passed,0fail/skip includingfailedpilotandcorrectivemaintenancerehearsalevidence.
- Buildwrapper120stimeoutsreconciledusingchildexit0journals,finishedmarkers,binaryhashes. No claimcleancommitbyte-identicalbuild.

## Final state and next gate

Livepilotandbothmaintenance rehearsalsrestored. Web10524F1C...,streamerAF431737...,SunshineC9FEAFA...,config80958A2B...,APKf8abfb...baseline. PC02ready/maintenance,noowner/claim/media/QArollbacktasks. Keeperconnectedandfresh;HPunlocked,screenON,lobbyholdscreenactive. Registryenvironmentbackup/restorereadbackmatchesexactly.

Next liveattempt, ifauthorized: mandatorymaintenanceactivationreceiptpreflightbeforebilling, correcteddaemon-awaredeployscript, freshprogresscapturebudget, then onecontrolledcaptureonsamerental. Do notrerentbecauseanactivationtoolfailedagain. No change to deadline-recovery policy was deployed and no improvement in freeze has been established by this milestone.

Evidence: `06 TEST/20261006-scoped-pilot-preflight` (failedpilot/rehearsal/daemonidentity/progressbuild/111QAresults/finalaudit), `20261006-scoped-pilot-run/session-1791300692548`, and `20261006-scoped-pilot-activation-v2` (correctedstartup/rollback/envreadback).
