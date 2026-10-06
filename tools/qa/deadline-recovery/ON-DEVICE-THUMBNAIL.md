# On-device fresh thumbnail and already-ready capture gate

## Outcome

The previous measured visual path exceeded its5second freshness budget. This implementation reduces image acquisition/transfer by preparing a shellUID Androidhelper first, capturing once on a nonce-boundcommand, resizing/compressing on the phone, and sending a small JPEG in binary-safe Base64. The Host gate is also started and initialized **before** taking the image; one authenticated-by-SSH/private-directory envelope is then delivered through a nonce/hash inbox.

Three lobby and three local-video dryruns passed the unchanged visual age5seconds,backendage3seconds and250msmargin. The exact v4prebillinghelper passed a further actual dryrun. **No new rental or liveHostcapture was run.** One-second review delays in these tests are explicit simulatedallowances, not proof that a human/assistant can review within that time. Actualfreshvisualapproval remains mandatory and is now bound to the capture nonce and JPEGhash.

This is an improvement in the QAmeasurementtool, **not cloudgaminglatency, freeze%, input-to-photon or A/Vsync**. No production app/encoder/decoder/routing changes were made.

## Android helper

`FreshThumbnail.java` runs via existing Android`app_process`,notanAPK. The JAR is hashedlocallyandremotely. It declaresready first;clientsets `startedAtPcMs` immediatelybefore sending`CAPTURE <nonce>`. ConsequentlyJava/startup/hashverification occurs before the image exists and doesnotconsumeimageage. Capturetakesplaceonlyafterthatcommand.

- Singlecaptureofnormaldisplaythrough`/system/bin/screencap`,usingexistingOSpermissions. NoPIN/keyguardchange,secure-layeroverride,inputcontrol,microphone,networkconnectionorbackgroundservice.
- Readsbounds32MiB,width/height<=8192,onlyexactAndroid16rawlayout`width,height,format,dataspace,RGBA8888pixels`. Rejectsotherformats/strides;doesnotguessorpickafallbackimage.
- Four-colorrawbitmapselftestverifiedpixelchannelinterpretationonthephysicalphonebeforecapture. It isnotacalibratedcolorimetrytest;capturedlobby/localvideoimageswerealsoactuallyviewed.
- Scaleswidthdown toatmost480,aspectratiopreserved,JPEGquality80,<=1MiB. Streamresolutionisunchanged. Reducedthumbnaildoesnotcertifyfinecodecquality/readabilityofeveryoverlay.
- Nonce32hex,idleinputtimeout10seconds,independentprocesslifetime20seconds,originalscreenshot+encodeelapsed<=8secondsrequired. The8secondcriterionischeckedatcompletion;watchdog20secondsisthehardprocessbound. Onerrorchildscreencapisdestroyed.
- BinaryJPEGoverAndroidshellwasobservedwithnewlineconversion. Base64transportpreservesbytehashandcanonicalencoding;CRLFallowedonlyattextframingboundaries. DecodedJPEGlength,SHA256,SOI/EOI,metadata,timeandlocalFFmpegdecodeallmustpass.
- Localresultstoresstart/endwalltimeandmonotonicelapsed;clockstep>20msfails. Noautomaticrecapture. Helperexitmustbe0;failure/partialbytesretainedasinvalid.

`device-thumbnail.mjs` starts `adb shell -T` (notPTY),checksreadynonce/limits,sendsonecommand,verifiesoutputstrictly,thenawaitsprocessclosure. Setupcostisreportedseparatelyfromfresh-imagecost. JAR remainsinprivateQAtempstaging;notinstalledasapackage. CorrectbaselineCloudGimeAPKneverreplaced.

## Prepared Host gate

`CaptureGate.Ready.ps1` wraps the already-tested sharedCore and separateDryRun/Liveentrypoints. It validatesroot/nonce/hash,initializes.NETandprocess/hashPowerShellproviders **before emittingready**. Thatwarmupdataisnotreusedasauthority:entryre-readsactualmutableprocess/hash/rental/progress/evidenceatsubmission.

ObservedSSHstdininteractivityfailedwiththeoriginalreadlinebridge. Thefinalbridgeusesoneprivateinbox`gate-inbox-<exchangeNonce>.json`,uploadedviaexistinghost-key-pinnedSSH/SCP. Envelopefields:exchangeNonce,bytes,SHA256,canonicalBase64. Boundedread16KiB,payload<=8KiB;readerwaitsforparseablecompletecontent,verifiesallfields/hash,andinvokesthefixedentryonce. Thisisnotanarbitraryremotecommandserver. Filecannotgrantnative/rentalauthority;liveCorevalidationandstreamercontextremainmandatory.

Per-sessionbounds:10secondsfromreadyforinbox,15secondshardlifetimeafterwarmup,client30secondstotal. No automaticresubmit;ambiguouslivereplymustbereconciled. LocaltempinboxfilesandremoteQAinboxesareoperationalmetadata,retainedforaudit;inheritOSdirectoryprotection. Theyare**notcryptographicallysignedbackend/rendererattestations**. Hashprovidesintegrity,notindependentauthority. ExistingWindowsadmin/SYSTEMstagingandSSHhost-keyboundaryistrusted. Partialfilewritesretryonlyreadingtheoneinbox,notreissuingcapture.

`ready-capture-gate.mjs` verifiesreadyexchangeNonce/role/dryRun/limits,tracksmonotonicdeadline,allowsoneupload,validatesprocessclosureandexacttwo-responsecontract. It distinguishesa validrejectedDryRunfromprotocolfailure;callerchecks`result.eligible`. Wronghash/nonce/idlecasesontargetPC02allrejectandexitterminatewithoutrequest.

## Measurements and retained negatives

### Image-only samples, physical RMX5367

PreviousPNGfulltransfer4.66–5.20s. Threeacceptedon-devicecompressedlobbycaptures:
-1344.754ms,total;1283.431msdevicework;68,961JPEGbytes.
-1369.402ms,total;1307.712msdevicework;69,413bytes.
-1331.099ms,total;1271.704msdevicework;69,723bytes.

Helper+hashstartup~0.76–0.79sbeforefreshtrigger. Rawframe1080×2376,10,264,336bytes;returnedthumbnail480×1056. Allhashes/decodespassed. Theseareactualscreencontentmeasurements,notarandomizedbenchmarkagainstearliernetworkconditions.

### Full path (realphonecapture,actualAPIread,actualPC02process/hashgate)

Allrecordscontainoriginalimagecapturestart,monotonicclockandHostoffsetinterval,simulatedreview1000ms,backendreadstart,gateobservedage,processresult,requestWrittenfalse.

-Afterprewarminggate,lobby3runsvisualage3586.825/3611.635/3590.706ms;all<=4750mswith250mssafety.
-WhilelocalQAappplayedvideo,3runsvisualage3533.780/3491.557/3598.559ms. Thumbnail1325.694–1402.999ms;backend98–108ms;post-uploadgate~1.34–1.44s.
-Exactv4prebillinghelper:thumbnail1396.041ms,visualage3568.595ms,backendage1169.595ms,clockinterval15ms;gateeligible,norequestwritten.

Backendreallyhadnorental;billing/rental/rendererstateusedbyDryRunwasexplicitlysynthetic. Localvideo ismediaplayerfixture,notCloudGimeWebRTCdecode. Playerstateconfirmedrunningduringthetest. TemporaryQAAPKwasuninstalledafterward;lobbyrestoredandbaselineAPKhashunchanged.

Earlierretainedresults:
1.`exec-out`requestneverreachedhelper→NO_IMAGE;notvalidcapture.
2.ShellbinaryJPEGreceivednewlineconversion→SHA/lengthrejected;exactrawoutputsretained.
3.FreshthumbnailwithcoldHostgate:onevisualage5466msfailure;twootherroundsfailedoldsyntheticcontextexpiry. Refreshedfixturedoesnotrepairhistoricalfailedresults.
4.PreparedstdinHostbridge:INBOX/inputtimeout,retained.
5.Preparedinboxgatewithoutwarmup:3eligiblebutfirstvisualage4919ms,**failed250mssafetymargin**. Nooverheadthresholdrelaxed.
6.Finalprewarmmethodpassedseparatefixedthree-roundbatchesasabove. Thisisdevelopmentqualification,notheld-outcloudgamingperformanceevidence.

## Review and live-runner boundary

`validateThumbnailApproval` requiresactualreviewtimeafterimageverification,beforedeadline,captureNonceexactmatchandJPEGhashmatch. Freshnessoriginis**triggerbeforecapture**,notreviewtimeordeliverycompletion. It rejectsoldapprovals,wrongimagesandfive-secondexpiry. Noautomaticvisualapprovalintroduced.

`ready-visual-path.mjs` qualifiesonlycontrol-tooltimingunder1ssimulatedreview. Outputs`realReviewerQualified=false`,`newRentalAllowed=false`,`streamQualityAccepted=false`. `requireReadyVisualTimingScreen` cannot be used asrentalpermission. A separatelyauthorizedpilotstillneedsactualfreshreviewandallbackend/Hostgates.

Preparedbut**unlaunched**runner:
`06 TEST/20261007-scoped-pilot-v4[-run]`.
-Older v3 remainsblockedanditsfailedreadinessdecisionunchanged.
-v4requiresnewtimingscreenbeforedeployment,mandatoryactualstartupreceipt,thenexactnewprebillingdryrunwithphonecapturebeforeclaim.
-Matchesbinary/helperhashesandretainsdaemon-awarerollback.
-Refreshescontrolclockafterstreamready,preparesliveGatebeforethumbnail,emitscapturereadinesswithnonce/hash,acceptsactualreviewonlywithin4.5sclientwindow(theHost5sbudgetstillauthoritative).
-Checksbackendbindingafterreview,submitsoneinbox,noretry,closesgateinfinallyonanyerror.
-DoesnotinstallthethumbnailJARautomaticallyifmissing;failedhashpreflightmuststopbeforeclaim. A pre-existingverifiedJARpathisrecordedinitsbuildmanifest.

No livegate submission,rendererreview latency under realassistantinteraction,frame-tracecompletenessduringanactualrental,orfreezereductionwasmeasured. Theliveentryremainsa distinctfilesystem/authorityadapterfromdryrunandmustnotbedeemedqualifiedsolelybytimingsamples.

## Safety verification

160NodeQA tests at checkpoint,0failed/skippedwithalloldevidenceenabled. Testscoverimageprotocol/hash/Base64/truncation/format/timestamps,wrongnonce,gateone-shot/extraoutput/uploadfailure/exit/oversizedenvelope,captureapprovalbindingandnegative/marginalhistoricalresults.

Actualphone:wrongcapture nonce→exit2/noimage;idle→exit2~10.595s/noimage. ActualPC02:wronginboxnonce→INBOX_SCOPE;hash→INBOX_HASH;noinbox→INBOX_TIMEOUT;nonzeroexitandnoprocessremaining. SHAverificationonlybeforeeachcaptureandpergate;noinstalledpackageorservicechangeexcepttemporaryseparateQAfixtureinstall/removalexplicitlyreported.

Finalaudit:Host/APKbaseline,PC02ready/maintenance,noowner/claim,noSDK/media/clock/gatehelperprocess,noQAenvironment,nocapture/requestfiles. HPscreenONunlocked,lobbyholdscreen,ADBfresh;QAfixturepackageremoved. HelperJARandadminQAfilesretainedbutnotrunning. No newrentalorproductiondeployment.

Evidence:`06 TEST/20261007-on-device-thumbnail` containsbuild/selftest,allfailedrawframes,acceptedJPEGs,full-pathbatches,localvideoresults,realnegativecases,v4integrationrun,QAresultsandfinalaudit. Figurecomparisonsareobservationsfromtheseruns,notguaranteedlatencyonotherdevices/networks.
